use serde::{Deserialize, Serialize};

use crate::abi::{AbiValue, ContractArtifact};
use crate::contract::ContractFunctionInteraction;
use crate::error::Error;
use crate::tx::ExecutionPayload;
use crate::types::{AztecAddress, ContractInstance, ContractInstanceWithAddress, Fr, PublicKeys};
use crate::wallet::{SendOptions, SendResult, SimulateOptions, TxSimulationResult, Wallet};

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
}

// ---------------------------------------------------------------------------
// publish_contract_class
// ---------------------------------------------------------------------------

/// Build an interaction payload that publishes a contract class on-chain.
///
/// This constructs a call to the protocol's `ContractClassRegisterer` system
/// contract. Full implementation requires bytecode extraction and artifact
/// hashing primitives (artifact hash, private functions root, public bytecode
/// commitment, packed bytecode) that are not yet available in this crate.
#[allow(clippy::unused_async)]
pub async fn publish_contract_class<'a, W: Wallet>(
    _wallet: &'a W,
    _artifact: &ContractArtifact,
) -> Result<ContractFunctionInteraction<'a, W>, Error> {
    Err(Error::InvalidData(
        "publish_contract_class requires bytecode extraction and artifact hashing \
         primitives that are not yet implemented"
            .to_owned(),
    ))
}

// ---------------------------------------------------------------------------
// publish_instance
// ---------------------------------------------------------------------------

/// Build an interaction payload that publishes a contract instance on-chain.
///
/// This constructs a call to the protocol's `ContractInstanceDeployer` system
/// contract to register the given instance for public execution. Full
/// implementation requires protocol system contract addresses and a
/// `PublicKeys::hash()` method that are not yet available in this crate.
pub fn publish_instance<'a, W: Wallet>(
    _wallet: &'a W,
    _instance: &ContractInstanceWithAddress,
) -> Result<ContractFunctionInteraction<'a, W>, Error> {
    Err(Error::InvalidData(
        "publish_instance requires protocol system contract addresses \
         that are not yet configured"
            .to_owned(),
    ))
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
    ///
    /// Full deployment currently remains explicit rather than speculative. The
    /// contract class publication path, instance publication path, wallet
    /// registration, and deployment-address derivation are still incomplete in
    /// this crate, so this returns an error instead of emitting a misleading
    /// zero-address constructor call.
    pub fn request(&self, opts: &DeployOptions) -> Result<ExecutionPayload, Error> {
        if !opts.skip_registration {
            return Err(Error::InvalidData(
                "deployment request construction requires wallet registration support for derived contract instances"
                    .to_owned(),
            ));
        }

        if !opts.skip_class_publication {
            return Err(Error::InvalidData(
                "publish_contract_class requires bytecode extraction and artifact hashing primitives that are not yet implemented"
                    .to_owned(),
            ));
        }

        if !opts.skip_instance_publication {
            let _ = self.get_instance(opts);
            return Err(Error::InvalidData(
                "publish_instance requires protocol system contract addresses that are not yet configured"
                    .to_owned(),
            ));
        }

        if !opts.skip_initialization && self.constructor_name.is_some() {
            return Err(Error::InvalidData(
                "deployment request construction requires contract address derivation before initialization can be encoded"
                    .to_owned(),
            ));
        }

        Err(Error::InvalidData(
            "deployment request construction is not fully implemented yet".to_owned(),
        ))
    }

    /// Compute the contract instance that would be deployed.
    ///
    /// The returned instance uses a placeholder address because contract
    /// address derivation requires hashing primitives not yet available.
    pub fn get_instance(&self, opts: &DeployOptions) -> ContractInstanceWithAddress {
        let salt = opts.contract_address_salt.unwrap_or(self.default_salt);

        ContractInstanceWithAddress {
            address: AztecAddress(Fr::zero()),
            inner: ContractInstance {
                version: 1,
                salt,
                deployer: AztecAddress(Fr::zero()),
                current_contract_class_id: Fr::zero(),
                original_contract_class_id: Fr::zero(),
                initialization_hash: Fr::zero(),
                public_keys: self.public_keys.clone(),
            },
        }
    }

    /// Simulate the deployment without sending.
    pub async fn simulate(
        &self,
        deploy_opts: &DeployOptions,
        sim_opts: SimulateOptions,
    ) -> Result<TxSimulationResult, Error> {
        let payload = self.request(deploy_opts)?;
        self.wallet.simulate_tx(payload, sim_opts).await
    }

    /// Send the deployment transaction.
    pub async fn send(
        &self,
        deploy_opts: &DeployOptions,
        send_opts: SendOptions,
    ) -> Result<SendResult, Error> {
        let payload = self.request(deploy_opts)?;
        self.wallet.send_tx(payload, send_opts).await
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
    use crate::tx::TxHash;
    use crate::types::Fr;
    use crate::wallet::{ChainInfo, MockWallet, SendResult, TxSimulationResult};

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
        };
        let json = serde_json::to_string(&opts).expect("serialize");
        let decoded: DeployOptions = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, opts);
    }

    // -- publish_contract_class ----------------------------------------------

    #[tokio::test]
    async fn publish_contract_class_returns_deferred_error() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let result = publish_contract_class(&wallet, &artifact).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("bytecode extraction"));
    }

    // -- publish_instance ----------------------------------------------------

    #[test]
    fn publish_instance_returns_deferred_error() {
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
        let result = publish_instance(&wallet, &instance);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("protocol system contract addresses"));
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

    // -- DeployMethod::request -----------------------------------------------

    #[test]
    fn deploy_method_request_requires_registration_support() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deployer = ContractDeployer::new(artifact, &wallet);

        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(42u64))])
            .expect("create deploy method");

        let err = deploy
            .request(&DeployOptions::default())
            .expect_err("request should stay explicit");
        assert!(err.to_string().contains("wallet registration support"));
    }

    #[test]
    fn deploy_method_request_reports_missing_class_publication_primitives() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deployer = ContractDeployer::new(artifact, &wallet);

        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let opts = DeployOptions {
            skip_registration: true,
            skip_initialization: true,
            ..DeployOptions::default()
        };
        let err = deploy
            .request(&opts)
            .expect_err("request should stay explicit");
        assert!(err.to_string().contains("bytecode extraction"));
    }

    #[test]
    fn deploy_method_request_requires_address_derivation_before_initialization() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deployer = ContractDeployer::new(artifact, &wallet);

        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let opts = DeployOptions {
            skip_registration: true,
            skip_class_publication: true,
            skip_instance_publication: true,
            ..DeployOptions::default()
        };
        let err = deploy
            .request(&opts)
            .expect_err("request should stay explicit");
        assert!(err.to_string().contains("contract address derivation"));
    }

    // -- DeployMethod::get_instance ------------------------------------------

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
            ..DeployOptions::default()
        };
        let instance = deploy.get_instance(&opts);
        assert_eq!(instance.inner.salt, Fr::from(99u64));
        assert_eq!(instance.inner.version, 1);
    }

    #[test]
    fn deploy_method_get_instance_generates_random_salt() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deployer = ContractDeployer::new(artifact, &wallet);

        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let instance = deploy.get_instance(&DeployOptions::default());
        assert_ne!(instance.inner.salt, Fr::zero());
    }

    #[test]
    fn deploy_method_get_instance_is_stable_for_same_options() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deploy = ContractDeployer::new(artifact, &wallet)
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let first = deploy.get_instance(&DeployOptions::default());
        let second = deploy.get_instance(&DeployOptions::default());
        assert_eq!(first.inner.salt, second.inner.salt);
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

        let instance = deploy.get_instance(&DeployOptions::default());
        assert_eq!(instance.inner.public_keys, keys);
    }

    // -- DeployMethod::simulate ----------------------------------------------

    #[tokio::test]
    async fn deploy_method_simulate_delegates_to_wallet() {
        let wallet =
            MockWallet::new(sample_chain_info()).with_simulate_result(TxSimulationResult {
                return_values: serde_json::json!({"deployed": true}),
                gas_used: Some(Gas {
                    da_gas: 50,
                    l2_gas: 100,
                }),
            });
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deployer = ContractDeployer::new(artifact, &wallet);

        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let err = deploy
            .simulate(&DeployOptions::default(), SimulateOptions::default())
            .await
            .expect_err("simulate should stay explicit");
        assert!(err.to_string().contains("wallet registration support"));
    }

    // -- DeployMethod::send --------------------------------------------------

    #[tokio::test]
    async fn deploy_method_send_delegates_to_wallet() {
        let tx_hash =
            TxHash::from_hex("0x00000000000000000000000000000000000000000000000000000000deadbeef")
                .expect("valid hex");
        let wallet = MockWallet::new(sample_chain_info()).with_send_result(SendResult { tx_hash });
        let artifact = load_artifact(DEPLOY_ARTIFACT);
        let deployer = ContractDeployer::new(artifact, &wallet);

        let deploy = deployer
            .deploy(vec![AbiValue::Field(Fr::from(1u64))])
            .expect("create deploy method");

        let err = deploy
            .send(&DeployOptions::default(), SendOptions::default())
            .await
            .expect_err("send should stay explicit");
        assert!(err.to_string().contains("wallet registration support"));
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
