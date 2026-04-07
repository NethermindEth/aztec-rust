use serde::{Deserialize, Serialize};

use crate::abi::{AbiValue, ContractArtifact, FunctionType};
use crate::contract::ContractFunctionInteraction;
use crate::error::Error;
use crate::tx::{Capsule, ExecutionPayload, FunctionCall};
use crate::types::{AztecAddress, ContractInstance, ContractInstanceWithAddress, Fr, PublicKeys};
use crate::wallet::{SendOptions, SendResult, SimulateOptions, TxSimulationResult, Wallet};

use aztec_core::abi::FunctionSelector;
use aztec_core::constants::{
    contract_class_registry_bytecode_capsule_slot, protocol_contract_address,
    MAX_PACKED_PUBLIC_BYTECODE_SIZE_IN_FIELDS, MAX_PROCESSABLE_L2_GAS,
};
use aztec_core::fee::Gas;
use aztec_core::hash::{
    buffer_as_fields, compute_artifact_hash, compute_contract_address_from_instance,
    compute_contract_class_id, compute_initialization_hash,
    compute_private_functions_root_from_artifact, compute_public_bytecode_commitment,
};

// ---------------------------------------------------------------------------
// DeployOptions
// ---------------------------------------------------------------------------

/// Options controlling contract deployment behavior.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct DeployOptions {
    /// Salt for deterministic address computation.
    pub contract_address_salt: Option<Fr>,
    /// Skip publishing the contract class on-chain.
    #[serde(default)]
    pub skip_class_publication: bool,
    /// Skip publishing the contract instance on-chain.
    #[serde(default)]
    pub skip_instance_publication: bool,
    /// Skip calling the initialization function.
    #[serde(default)]
    pub skip_initialization: bool,
    /// Skip registering the contract with the wallet.
    #[serde(default)]
    pub skip_registration: bool,
    /// Use the universal deployer for this deployment.
    #[serde(default)]
    pub universal_deploy: bool,
    /// Explicit deployer address. Required when `universal_deploy` is false.
    #[serde(default)]
    pub from: Option<AztecAddress>,
}

// ---------------------------------------------------------------------------
// SuggestedGasLimits / get_gas_limits
// ---------------------------------------------------------------------------

/// Suggested gas limits from simulation, with optional padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuggestedGasLimits {
    /// Main execution phase gas limits.
    pub gas_limits: Gas,
    /// Teardown phase gas limits.
    pub teardown_gas_limits: Gas,
}

/// Compute gas limits from a simulation result.
///
/// Applies a padding factor (default 10%) to the simulated gas usage
/// to provide a safety margin.
pub fn get_gas_limits(
    simulation_result: &TxSimulationResult,
    pad: Option<f64>,
) -> SuggestedGasLimits {
    let pad_factor = 1.0 + pad.unwrap_or(0.1);

    let gas_used = simulation_result
        .gas_used
        .as_ref()
        .cloned()
        .unwrap_or_default();

    let padded_da = (gas_used.da_gas as f64 * pad_factor).ceil() as u64;
    let padded_l2 = (gas_used.l2_gas as f64 * pad_factor).ceil() as u64;

    SuggestedGasLimits {
        gas_limits: Gas {
            da_gas: padded_da,
            l2_gas: padded_l2.min(MAX_PROCESSABLE_L2_GAS),
        },
        teardown_gas_limits: Gas::default(),
    }
}

// ---------------------------------------------------------------------------
// publish_contract_class
// ---------------------------------------------------------------------------

/// Build an interaction payload that publishes a contract class on-chain.
#[allow(clippy::unused_async)]
pub async fn publish_contract_class<'a, W: Wallet>(
    wallet: &'a W,
    artifact: &ContractArtifact,
) -> Result<ContractFunctionInteraction<'a, W>, Error> {
    // 1. Compute class preimage components.
    let artifact_hash = compute_artifact_hash(artifact);
    let private_functions_root = compute_private_functions_root_from_artifact(artifact)?;

    // Extract and encode packed public bytecode.
    let packed_bytecode = extract_packed_bytecode(artifact);
    let public_bytecode_commitment = compute_public_bytecode_commitment(&packed_bytecode);

    // 2. Encode packed bytecode as field elements for capsule.
    let bytecode_fields = if packed_bytecode.is_empty() {
        vec![]
    } else {
        buffer_as_fields(&packed_bytecode, MAX_PACKED_PUBLIC_BYTECODE_SIZE_IN_FIELDS)
    };

    // 3. Build function call to the Contract Class Registry.
    let registerer_address = protocol_contract_address::contract_class_registry();

    let call = FunctionCall {
        to: registerer_address,
        selector: FunctionSelector::from_signature("publish(Field,Field,Field)"),
        args: vec![
            AbiValue::Field(artifact_hash),
            AbiValue::Field(private_functions_root),
            AbiValue::Field(public_bytecode_commitment),
        ],
        function_type: FunctionType::Private,
        is_static: false,
    };

    // 4. Create capsule with bytecode data.
    let capsules = if bytecode_fields.is_empty() {
        vec![]
    } else {
        vec![Capsule {
            contract_address: registerer_address,
            storage_slot: contract_class_registry_bytecode_capsule_slot(),
            data: bytecode_fields,
        }]
    };

    // 5. Return interaction with capsule attached.
    Ok(ContractFunctionInteraction::new_with_capsules(
        wallet, call, capsules,
    ))
}

/// Extract packed public bytecode from an artifact.
fn extract_packed_bytecode(artifact: &ContractArtifact) -> Vec<u8> {
    let mut bytecode = Vec::new();
    for func in &artifact.functions {
        if func.function_type == FunctionType::Public {
            if let Some(ref bc) = func.bytecode {
                bytecode.extend_from_slice(&decode_artifact_bytes(bc));
            }
        }
    }
    bytecode
}

// ---------------------------------------------------------------------------
// publish_instance
// ---------------------------------------------------------------------------

/// Build an interaction payload that publishes a contract instance on-chain.
pub fn publish_instance<'a, W: Wallet>(
    wallet: &'a W,
    instance: &ContractInstanceWithAddress,
) -> Result<ContractFunctionInteraction<'a, W>, Error> {
    let is_universal_deploy = instance.inner.deployer == AztecAddress(Fr::zero());

    let deployer_address = protocol_contract_address::contract_instance_registry();

    let call = FunctionCall {
        to: deployer_address,
        selector: FunctionSelector::from_signature(
            "publish_for_public_execution(Field,(Field),Field,(((Field,Field,bool)),((Field,Field,bool)),((Field,Field,bool)),((Field,Field,bool))),bool)"
        ),
        args: vec![
            AbiValue::Field(instance.inner.salt),
            AbiValue::Tuple(vec![AbiValue::Field(
                instance.inner.current_contract_class_id,
            )]),
            AbiValue::Field(instance.inner.initialization_hash),
            public_keys_to_abi_value(&instance.inner.public_keys),
            AbiValue::Boolean(is_universal_deploy),
        ],
        function_type: FunctionType::Private,
        is_static: false,
    };

    Ok(ContractFunctionInteraction::new(wallet, call))
}

fn point_to_abi_value(point: &aztec_core::types::Point) -> AbiValue {
    AbiValue::Tuple(vec![
        AbiValue::Field(point.x),
        AbiValue::Field(point.y),
        AbiValue::Boolean(point.is_infinite),
    ])
}

fn public_keys_to_abi_value(public_keys: &PublicKeys) -> AbiValue {
    AbiValue::Tuple(vec![
        point_to_abi_value(&public_keys.master_nullifier_public_key),
        point_to_abi_value(&public_keys.master_incoming_viewing_public_key),
        point_to_abi_value(&public_keys.master_outgoing_viewing_public_key),
        point_to_abi_value(&public_keys.master_tagging_public_key),
    ])
}

fn decode_artifact_bytes(encoded: &str) -> Vec<u8> {
    if let Some(hex) = encoded.strip_prefix("0x") {
        return hex::decode(hex).unwrap_or_else(|_| encoded.as_bytes().to_vec());
    }

    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap_or_else(|_| encoded.as_bytes().to_vec())
}

// ---------------------------------------------------------------------------
// DeployResult
// ---------------------------------------------------------------------------

/// The result of a deployment transaction.
#[derive(Clone, Debug)]
pub struct DeployResult {
    /// The underlying send result (tx hash).
    pub send_result: SendResult,
    /// The deployed contract instance with its derived address.
    pub instance: ContractInstanceWithAddress,
}

// ---------------------------------------------------------------------------
// ContractDeployer
// ---------------------------------------------------------------------------

/// Builder for deploying new contract instances.
///
/// Created with a contract artifact and wallet reference. Use
/// [`ContractDeployer::deploy`] to produce a [`DeployMethod`] for a
/// specific set of constructor arguments.
pub struct ContractDeployer<'a, W> {
    artifact: ContractArtifact,
    wallet: &'a W,
    public_keys: PublicKeys,
    constructor_name: Option<String>,
}

impl<W> std::fmt::Debug for ContractDeployer<'_, W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContractDeployer")
            .field("artifact", &self.artifact.name)
            .field("constructor_name", &self.constructor_name)
            .finish_non_exhaustive()
    }
}

impl<'a, W: Wallet> ContractDeployer<'a, W> {
    /// Create a new deployer for the given artifact and wallet.
    pub fn new(artifact: ContractArtifact, wallet: &'a W) -> Self {
        Self {
            artifact,
            wallet,
            public_keys: PublicKeys::default(),
            constructor_name: None,
        }
    }

    /// Set the public keys for the deployed instance.
    #[must_use]
    pub const fn with_public_keys(mut self, keys: PublicKeys) -> Self {
        self.public_keys = keys;
        self
    }

    /// Set the constructor function name (defaults to `"constructor"`).
    #[must_use]
    pub fn with_constructor_name(mut self, name: impl Into<String>) -> Self {
        self.constructor_name = Some(name.into());
        self
    }

    /// Create a [`DeployMethod`] for the given constructor arguments.
    ///
    /// Validates the selected initializer, when present. Contracts with no
    /// initializer are allowed as long as no constructor arguments are passed.
    pub fn deploy(self, args: Vec<AbiValue>) -> Result<DeployMethod<'a, W>, Error> {
        let constructor_name = if let Some(name) = self.constructor_name {
            let func = self.artifact.find_function(&name)?;
            if !func.is_initializer {
                return Err(Error::Abi(format!(
                    "function '{name}' in artifact '{}' is not an initializer",
                    self.artifact.name,
                )));
            }

            let expected = func.parameters.len();
            let got = args.len();
            if got != expected {
                return Err(Error::Abi(format!(
                    "constructor '{name}' expects {expected} argument(s), got {got}",
                )));
            }

            Some(name)
        } else if let Some(func) = self
            .artifact
            .functions
            .iter()
            .find(|func| func.is_initializer)
        {
            let expected = func.parameters.len();
            let got = args.len();
            if got != expected {
                return Err(Error::Abi(format!(
                    "constructor '{}' expects {expected} argument(s), got {got}",
                    func.name
                )));
            }

            Some(func.name.clone())
        } else if args.is_empty() {
            None
        } else {
            return Err(Error::Abi(format!(
                "artifact '{}' has no initializer but got {} constructor argument(s)",
                self.artifact.name,
                args.len()
            )));
        };

        Ok(DeployMethod {
            wallet: self.wallet,
            artifact: self.artifact,
            args,
            public_keys: self.public_keys,
            constructor_name,
            default_salt: Fr::random(),
        })
    }
}

// ---------------------------------------------------------------------------
// DeployMethod
// ---------------------------------------------------------------------------

/// A pending contract deployment interaction.
///
/// Created by [`ContractDeployer::deploy`]. Supports building the deployment
/// payload, computing the target instance, simulating, and sending.
pub struct DeployMethod<'a, W> {
    wallet: &'a W,
    artifact: ContractArtifact,
    args: Vec<AbiValue>,
    public_keys: PublicKeys,
    constructor_name: Option<String>,
    default_salt: Fr,
}

impl<W> std::fmt::Debug for DeployMethod<'_, W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeployMethod")
            .field("artifact", &self.artifact.name)
            .field("constructor_name", &self.constructor_name)
            .field("args_count", &self.args.len())
            .finish_non_exhaustive()
    }
}

impl<W: Wallet> DeployMethod<'_, W> {
    /// Build the deployment [`ExecutionPayload`].
    pub async fn request(&self, opts: &DeployOptions) -> Result<ExecutionPayload, Error> {
        let instance = self.get_instance(opts)?;
        let mut payloads: Vec<ExecutionPayload> = Vec::new();

        // 1. Register contract with wallet (unless skipped).
        if !opts.skip_registration {
            self.wallet
                .register_contract(instance.clone(), Some(self.artifact.clone()), None)
                .await?;
        }

        // 2. Publish contract class (unless skipped).
        if !opts.skip_class_publication {
            let class_id = instance.inner.current_contract_class_id;
            let already_registered = self
                .wallet
                .get_contract_class_metadata(class_id)
                .await
                .map(|m| m.is_contract_class_publicly_registered)
                .unwrap_or(false);

            if !already_registered {
                let class_interaction = publish_contract_class(self.wallet, &self.artifact).await?;
                payloads.push(class_interaction.request()?);
            }
        }

        // 3. Publish instance (unless skipped).
        if !opts.skip_instance_publication {
            let instance_interaction = publish_instance(self.wallet, &instance)?;
            payloads.push(instance_interaction.request()?);
        }

        // 4. Call constructor (unless skipped or no constructor).
        if !opts.skip_initialization {
            if let Some(ref constructor_name) = self.constructor_name {
                let func = self.artifact.find_function(constructor_name)?;
                let selector = func.selector.unwrap_or_else(|| {
                    FunctionSelector::from_name_and_parameters(&func.name, &func.parameters)
                });

                let call = FunctionCall {
                    to: instance.address,
                    selector,
                    args: self.args.clone(),
                    function_type: func.function_type.clone(),
                    is_static: false,
                };

                payloads.push(ExecutionPayload {
                    calls: vec![call],
                    auth_witnesses: vec![],
                    capsules: vec![],
                    extra_hashed_args: vec![],
                    fee_payer: None,
                });
            }
        }

        // 5. Merge all payloads.
        ExecutionPayload::merge(payloads)
    }

    /// Compute the contract instance that would be deployed.
    pub fn get_instance(&self, opts: &DeployOptions) -> Result<ContractInstanceWithAddress, Error> {
        let salt = opts.contract_address_salt.unwrap_or(self.default_salt);

        // Compute contract class ID from artifact.
        let artifact_hash = compute_artifact_hash(&self.artifact);
        let private_functions_root = compute_private_functions_root_from_artifact(&self.artifact)?;
        let packed_bytecode = extract_packed_bytecode(&self.artifact);
        let public_bytecode_commitment = compute_public_bytecode_commitment(&packed_bytecode);
        let class_id = compute_contract_class_id(
            artifact_hash,
            private_functions_root,
            public_bytecode_commitment,
        );

        // Compute initialization hash.
        let init_fn = self
            .constructor_name
            .as_ref()
            .map(|name| self.artifact.find_function(name))
            .transpose()?;

        let init_hash = compute_initialization_hash(init_fn, &self.args)?;

        // Determine deployer.
        let deployer = if opts.universal_deploy {
            AztecAddress(Fr::zero())
        } else {
            opts.from.unwrap_or(AztecAddress(Fr::zero()))
        };

        let instance = ContractInstance {
            version: 1,
            salt,
            deployer,
            current_contract_class_id: class_id,
            original_contract_class_id: class_id,
            initialization_hash: init_hash,
            public_keys: self.public_keys.clone(),
        };

        // Compute the deterministic address.
        let address = compute_contract_address_from_instance(&instance)?;

        Ok(ContractInstanceWithAddress {
            address,
            inner: instance,
        })
    }

    /// Simulate the deployment without sending.
    pub async fn simulate(
        &self,
        deploy_opts: &DeployOptions,
        sim_opts: SimulateOptions,
    ) -> Result<TxSimulationResult, Error> {
        let payload = self.request(deploy_opts).await?;
        self.wallet.simulate_tx(payload, sim_opts).await
    }

    /// Send the deployment transaction.
    pub async fn send(
        &self,
        deploy_opts: &DeployOptions,
        send_opts: SendOptions,
    ) -> Result<DeployResult, Error> {
        let instance = self.get_instance(deploy_opts)?;
        let payload = self.request(deploy_opts).await?;
        let send_result = self.wallet.send_tx(payload, send_opts).await?;
        Ok(DeployResult {
            send_result,
            instance,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::abi::AbiValue;
    use crate::fee::Gas;
    use crate::types::Fr;
    use crate::wallet::{ChainInfo, MockWallet, TxSimulationResult};

    const DEPLOY_ARTIFACT: &str = r#"
    {
      "name": "TokenContract",
      "functions": [
        {
          "name": "constructor",
          "function_type": "private",
          "is_initializer": true,
          "is_static": false,
          "parameters": [
            { "name": "admin", "type": { "kind": "field" } }
          ],
          "return_types": [],
          "selector": "0xe5fb6c81"
        },
        {
          "name": "transfer",
          "function_type": "private",
          "is_initializer": false,
          "is_static": false,
          "parameters": [
            { "name": "from", "type": { "kind": "field" } },
            { "name": "to", "type": { "kind": "field" } }
          ],
          "return_types": [],
          "selector": "0xd6f42325"
        }
      ]
    }
    "#;

    const NO_INITIALIZER_ARTIFACT: &str = r#"
    {
      "name": "NoInitContract",
      "functions": [
        {
          "name": "do_stuff",
          "function_type": "public",
          "is_initializer": false,
          "is_static": false,
          "parameters": [],
          "return_types": [],
          "selector": "0xaabbccdd"
        }
      ]
    }
    "#;

    fn sample_chain_info() -> ChainInfo {
        ChainInfo {
            chain_id: Fr::from(31337u64),
            version: Fr::from(1u64),
        }
    }

    fn load_artifact(json: &str) -> ContractArtifact {
        ContractArtifact::from_json(json).expect("parse artifact")
    }

    // -- DeployOptions -------------------------------------------------------

    #[test]
    fn deploy_options_default() {
        let opts = DeployOptions::default();
        assert!(opts.contract_address_salt.is_none());
        assert!(!opts.skip_class_publication);
        assert!(!opts.skip_instance_publication);
        assert!(!opts.skip_initialization);
        assert!(!opts.skip_registration);
        assert!(!opts.universal_deploy);
        assert!(opts.from.is_none());
    }

    #[test]
    fn deploy_options_roundtrip() {
        let opts = DeployOptions {
            contract_address_salt: Some(Fr::from(42u64)),
            skip_class_publication: true,
            skip_instance_publication: false,
            skip_initialization: false,
            skip_registration: true,
            universal_deploy: false,
            from: None,
        };
        let json = serde_json::to_string(&opts).expect("serialize");
        let decoded: DeployOptions = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, opts);
    }

    // -- get_gas_limits -------------------------------------------------------

    #[test]
    fn get_gas_limits_default_pad() {
        let result = TxSimulationResult {
            return_values: serde_json::Value::Null,
            gas_used: Some(Gas {
                da_gas: 1000,
                l2_gas: 2000,
            }),
        };
        let limits = get_gas_limits(&result, None);
        assert_eq!(limits.gas_limits.da_gas, 1100);
        assert_eq!(limits.gas_limits.l2_gas, 2200);
    }

    #[test]
    fn get_gas_limits_custom_pad() {
        let result = TxSimulationResult {
            return_values: serde_json::Value::Null,
            gas_used: Some(Gas {
                da_gas: 1000,
                l2_gas: 2000,
            }),
        };
        let limits = get_gas_limits(&result, Some(0.5));
        assert_eq!(limits.gas_limits.da_gas, 1500);
        assert_eq!(limits.gas_limits.l2_gas, 3000);
    }

    #[test]
    fn get_gas_limits_zero_gas() {
        let result = TxSimulationResult {
            return_values: serde_json::Value::Null,
            gas_used: Some(Gas {
                da_gas: 0,
                l2_gas: 0,
            }),
        };
        let limits = get_gas_limits(&result, None);
        assert_eq!(limits.gas_limits.da_gas, 0);
        assert_eq!(limits.gas_limits.l2_gas, 0);
    }

    // -- publish_contract_class -----------------------------------------------

    #[tokio::test]
    async fn publish_contract_class_targets_registerer() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let interaction = publish_contract_class(&wallet, &artifact)
            .await
            .expect("publish class");
        let payload = interaction.request().expect("build payload");
        assert_eq!(payload.calls.len(), 1);
        assert_eq!(
            payload.calls[0].to,
            protocol_contract_address::contract_class_registerer()
        );
    }

    // -- publish_instance ----------------------------------------------------

    #[test]
    fn publish_instance_targets_deployer() {
        let wallet = MockWallet::new(sample_chain_info());
        let instance = ContractInstanceWithAddress {
            address: AztecAddress(Fr::from(1u64)),
            inner: ContractInstance {
                version: 1,
                salt: Fr::from(42u64),
                deployer: AztecAddress(Fr::from(2u64)),
                current_contract_class_id: Fr::from(100u64),
                original_contract_class_id: Fr::from(100u64),
                initialization_hash: Fr::from(0u64),
                public_keys: PublicKeys::default(),
            },
        };
        let interaction = publish_instance(&wallet, &instance).expect("publish instance");
        let payload = interaction.request().expect("build payload");
        assert_eq!(payload.calls.len(), 1);
        assert_eq!(
            payload.calls[0].to,
            protocol_contract_address::contract_instance_deployer()
        );
    }

    #[test]
    fn publish_instance_universal_deploy_flag() {
        let wallet = MockWallet::new(sample_chain_info());

        // Non-universal (deployer is non-zero)
        let instance_non_universal = ContractInstanceWithAddress {
            address: AztecAddress(Fr::from(1u64)),
            inner: ContractInstance {
                version: 1,
                salt: Fr::from(42u64),
                deployer: AztecAddress(Fr::from(2u64)),
                current_contract_class_id: Fr::from(100u64),
                original_contract_class_id: Fr::from(100u64),
                initialization_hash: Fr::zero(),
                public_keys: PublicKeys::default(),
            },
        };
        let interaction = publish_instance(&wallet, &instance_non_universal).expect("non-uni");
        let payload = interaction.request().expect("payload");
        // Last arg should be false (non-universal)
        assert_eq!(payload.calls[0].args[4], AbiValue::Boolean(false));

        // Universal (deployer is zero)
        let instance_universal = ContractInstanceWithAddress {
            address: AztecAddress(Fr::from(1u64)),
            inner: ContractInstance {
                version: 1,
                salt: Fr::from(42u64),
                deployer: AztecAddress(Fr::zero()),
                current_contract_class_id: Fr::from(100u64),
                original_contract_class_id: Fr::from(100u64),
                initialization_hash: Fr::zero(),
                public_keys: PublicKeys::default(),
            },
        };
        let interaction2 = publish_instance(&wallet, &instance_universal).expect("uni");
        let payload2 = interaction2.request().expect("payload2");
        assert_eq!(payload2.calls[0].args[4], AbiValue::Boolean(true));
    }

    // -- ContractDeployer ----------------------------------------------------

    #[test]
    fn contract_deployer_creates_deploy_method() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);

        let deployer = ContractDeployer::new(artifact, &wallet);
        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let dbg = format!("{deploy:?}");
        assert!(dbg.contains("TokenContract"));
        assert!(dbg.contains("constructor"));
    }

    #[test]
    fn contract_deployer_with_builder_methods() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);

        let deployer = ContractDeployer::new(artifact, &wallet)
            .with_public_keys(PublicKeys::default())
            .with_constructor_name("constructor");

        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let dbg = format!("{deploy:?}");
        assert!(dbg.contains("constructor"));
    }

    #[test]
    fn contract_deployer_rejects_missing_function() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);

        let deployer =
            ContractDeployer::new(artifact, &wallet).with_constructor_name("nonexistent");

        let result = deployer.deploy(vec![]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn contract_deployer_rejects_non_initializer() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);

        let deployer = ContractDeployer::new(artifact, &wallet).with_constructor_name("transfer");

        let result = deployer.deploy(vec![
            AbiValue::Field(Fr::from(1u64)),
            AbiValue::Field(Fr::from(2u64)),
        ]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not an initializer"));
    }

    #[test]
    fn contract_deployer_rejects_arg_count_mismatch() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);

        let deployer = ContractDeployer::new(artifact, &wallet);
        let result = deployer.deploy(vec![]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("expects 1 argument(s), got 0"));
    }

    #[test]
    fn contract_deployer_no_initializer_artifact() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(NO_INITIALIZER_ARTIFACT);

        let deploy = ContractDeployer::new(artifact, &wallet)
            .deploy(vec![])
            .expect("create deploy method without initializer");
        let dbg = format!("{deploy:?}");
        assert!(dbg.contains("NoInitContract"));
        assert!(dbg.contains("None"));
    }

    // -- DeployMethod::get_instance ------------------------------------------

    #[test]
    fn deploy_method_get_instance_computes_real_address() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deployer = ContractDeployer::new(artifact, &wallet);

        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let opts = DeployOptions {
            contract_address_salt: Some(Fr::from(99u64)),
            universal_deploy: true,
            ..DeployOptions::default()
        };
        let instance = deploy.get_instance(&opts).expect("get instance");

        // Address should no longer be zero.
        assert_ne!(instance.address, AztecAddress(Fr::zero()));
        assert_eq!(instance.inner.salt, Fr::from(99u64));
        assert_eq!(instance.inner.version, 1);
        assert_eq!(instance.inner.deployer, AztecAddress(Fr::zero()));
        // Class ID should be non-zero.
        assert_ne!(instance.inner.current_contract_class_id, Fr::zero());
        assert_eq!(
            instance.inner.current_contract_class_id,
            instance.inner.original_contract_class_id
        );
    }

    #[test]
    fn deploy_method_get_instance_uses_provided_salt() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deployer = ContractDeployer::new(artifact, &wallet);

        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let opts = DeployOptions {
            contract_address_salt: Some(Fr::from(99u64)),
            universal_deploy: true,
            ..DeployOptions::default()
        };
        let instance = deploy.get_instance(&opts).expect("get instance");
        assert_eq!(instance.inner.salt, Fr::from(99u64));
    }

    #[test]
    fn deploy_method_get_instance_generates_random_salt() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deployer = ContractDeployer::new(artifact, &wallet);

        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let opts = DeployOptions {
            universal_deploy: true,
            ..DeployOptions::default()
        };
        let instance = deploy.get_instance(&opts).expect("get instance");
        assert_ne!(instance.inner.salt, Fr::zero());
    }

    #[test]
    fn deploy_method_get_instance_is_stable_for_same_options() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deploy = ContractDeployer::new(artifact, &wallet)
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let opts = DeployOptions {
            universal_deploy: true,
            ..DeployOptions::default()
        };
        let first = deploy.get_instance(&opts).expect("first");
        let second = deploy.get_instance(&opts).expect("second");
        assert_eq!(first.inner.salt, second.inner.salt);
        assert_eq!(first.address, second.address);
    }

    #[test]
    fn deploy_method_get_instance_preserves_public_keys() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);

        let keys = PublicKeys {
            master_nullifier_public_key: crate::types::Point {
                x: Fr::from(1u64),
                y: Fr::from(2u64),
                is_infinite: false,
            },
            ..PublicKeys::default()
        };

        let deployer = ContractDeployer::new(artifact, &wallet).with_public_keys(keys.clone());
        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let opts = DeployOptions {
            universal_deploy: true,
            ..DeployOptions::default()
        };
        let instance = deploy.get_instance(&opts).expect("get instance");
        assert_eq!(instance.inner.public_keys, keys);
    }

    #[test]
    fn deploy_method_get_instance_universal_deploy() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deploy = ContractDeployer::new(artifact, &wallet)
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let opts = DeployOptions {
            contract_address_salt: Some(Fr::from(1u64)),
            universal_deploy: true,
            ..DeployOptions::default()
        };
        let instance = deploy.get_instance(&opts).expect("get instance");
        assert_eq!(instance.inner.deployer, AztecAddress(Fr::zero()));
    }

    #[test]
    fn deploy_method_get_instance_with_explicit_from() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deploy = ContractDeployer::new(artifact, &wallet)
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let deployer_addr = AztecAddress(Fr::from(42u64));
        let opts = DeployOptions {
            contract_address_salt: Some(Fr::from(1u64)),
            universal_deploy: false,
            from: Some(deployer_addr),
            ..DeployOptions::default()
        };
        let instance = deploy.get_instance(&opts).expect("get instance");
        assert_eq!(instance.inner.deployer, deployer_addr);
    }

    // -- DeployMethod::request -----------------------------------------------

    #[tokio::test]
    async fn deploy_method_request_full_flow() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deployer = ContractDeployer::new(artifact, &wallet);

        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(42u64))])
            .expect("create deploy method");

        let opts = DeployOptions {
            contract_address_salt: Some(Fr::from(1u64)),
            universal_deploy: true,
            skip_registration: true,
            ..DeployOptions::default()
        };

        let payload = deploy.request(&opts).await.expect("request");

        // Should have calls for: class publication, instance publication, constructor
        assert!(
            payload.calls.len() >= 2,
            "expected at least 2 calls, got {}",
            payload.calls.len()
        );
    }

    #[tokio::test]
    async fn deploy_method_request_skips_class_publication() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deployer = ContractDeployer::new(artifact, &wallet);

        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(42u64))])
            .expect("create deploy method");

        let opts = DeployOptions {
            contract_address_salt: Some(Fr::from(1u64)),
            universal_deploy: true,
            skip_registration: true,
            skip_class_publication: true,
            ..DeployOptions::default()
        };

        let payload = deploy.request(&opts).await.expect("request");

        // Should have calls for: instance publication + constructor (no class publication)
        let has_registerer_call = payload
            .calls
            .iter()
            .any(|c| c.to == protocol_contract_address::contract_class_registerer());
        assert!(
            !has_registerer_call,
            "should not contain class publication call"
        );
    }

    #[tokio::test]
    async fn deploy_method_request_skips_initialization() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deployer = ContractDeployer::new(artifact, &wallet);

        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(42u64))])
            .expect("create deploy method");

        let opts = DeployOptions {
            contract_address_salt: Some(Fr::from(1u64)),
            universal_deploy: true,
            skip_registration: true,
            skip_class_publication: true,
            skip_instance_publication: true,
            skip_initialization: true,
            ..DeployOptions::default()
        };

        let payload = deploy.request(&opts).await.expect("request");
        assert!(payload.calls.is_empty());
    }

    // -- Debug impls ---------------------------------------------------------

    #[test]
    fn contract_deployer_debug() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deployer = ContractDeployer::new(artifact, &wallet);
        let dbg = format!("{deployer:?}");
        assert!(dbg.contains("TokenContract"));
    }
}
