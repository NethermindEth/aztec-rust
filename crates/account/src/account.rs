use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::abi::{AbiValue, ContractArtifact};
use crate::error::Error;
use crate::fee::GasSettings;
use crate::tx::{AuthWitness, Capsule, ExecutionPayload, HashedValues, TxContext};
use crate::types::{AztecAddress, CompleteAddress, ContractInstanceWithAddress, Fr, Salt};
use crate::wallet::{
    ChainInfo, MessageHashOrIntent, SendOptions, SendResult, SimulateOptions, TxSimulationResult,
    Wallet,
};

use aztec_contract::deployment::{
    get_contract_instance_from_instantiation_params, ContractInstantiationParams,
};
use aztec_fee::FeePaymentMethod;

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Options for account entrypoint execution.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrypointOptions {
    /// Override the fee payer for this transaction.
    pub fee_payer: Option<AztecAddress>,
    /// Gas settings for the entrypoint call.
    pub gas_settings: Option<GasSettings>,
}

/// A transaction execution request produced by an account's entrypoint.
///
/// This mirrors the upstream Aztec `TxExecutionRequest` shape used by PXE.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxExecutionRequest {
    /// The origin account address.
    pub origin: AztecAddress,
    /// Selector of the entrypoint function to execute.
    pub function_selector: crate::abi::FunctionSelector,
    /// Hash of the first call's encoded arguments.
    pub first_call_args_hash: Fr,
    /// Transaction context (chain info + gas settings).
    pub tx_context: TxContext,
    /// Hashed arguments for all calls in the transaction.
    pub args_of_calls: Vec<HashedValues>,
    /// Authorization witnesses.
    pub auth_witnesses: Vec<AuthWitness>,
    /// Capsules (private data) for the transaction.
    pub capsules: Vec<Capsule>,
    /// Salt used to randomize the tx request hash.
    pub salt: Fr,
    /// Optional fee payer override (defaults to origin if absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_payer: Option<AztecAddress>,
}

/// Specification for the initialization function of an account contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializationSpec {
    /// Name of the initializer function in the contract artifact.
    pub constructor_name: String,
    /// Arguments to pass to the initializer function.
    pub constructor_args: Vec<AbiValue>,
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Provides authorization witnesses for transaction authentication.
///
/// This trait is the foundation of the Aztec account model. Implementations
/// produce [`AuthWitness`] values that prove the caller is authorized to
/// execute a given intent.
#[async_trait]
pub trait AuthorizationProvider: Send + Sync {
    /// Create an authorization witness for the given intent.
    async fn create_auth_wit(
        &self,
        intent: MessageHashOrIntent,
        chain_info: &ChainInfo,
    ) -> Result<AuthWitness, Error>;
}

/// An Aztec account -- combines an entrypoint, auth-witness provider, and
/// complete address.
///
/// The `Account` trait is the main abstraction for account-based transaction
/// creation. It wraps execution payloads through the account's entrypoint
/// contract, adding authentication and fee payment.
#[async_trait]
pub trait Account: Send + Sync + AuthorizationProvider {
    /// Get the complete address of this account.
    fn complete_address(&self) -> &CompleteAddress;

    /// Get the account address.
    fn address(&self) -> AztecAddress;

    /// Create a full transaction execution request from a payload.
    ///
    /// This processes the payload through the account's entrypoint,
    /// adding authentication and gas handling.
    async fn create_tx_execution_request(
        &self,
        exec: ExecutionPayload,
        gas_settings: GasSettings,
        chain_info: &ChainInfo,
        options: EntrypointOptions,
    ) -> Result<TxExecutionRequest, Error>;

    /// Wrap an execution payload through the account's entrypoint.
    ///
    /// Similar to [`Account::create_tx_execution_request`] but returns an
    /// [`ExecutionPayload`] rather than a full request.
    async fn wrap_execution_payload(
        &self,
        exec: ExecutionPayload,
        options: EntrypointOptions,
    ) -> Result<ExecutionPayload, Error>;
}

/// Defines an account contract type (e.g., Schnorr, ECDSA).
///
/// Implementations provide the contract artifact, initialization parameters,
/// and the ability to produce [`Account`] and [`AuthorizationProvider`]
/// instances for a given address.
#[async_trait]
pub trait AccountContract: Send + Sync {
    /// Get the contract artifact for this account type.
    async fn contract_artifact(&self) -> Result<ContractArtifact, Error>;

    /// Get the initialization function name and arguments, if any.
    async fn initialization_function_and_args(&self) -> Result<Option<InitializationSpec>, Error>;

    /// Create an [`Account`] instance for the given address.
    fn account(&self, address: CompleteAddress) -> Box<dyn Account>;

    /// Create an [`AuthorizationProvider`] for the given address.
    fn auth_witness_provider(&self, address: CompleteAddress) -> Box<dyn AuthorizationProvider>;
}

// ---------------------------------------------------------------------------
// get_account_contract_address
// ---------------------------------------------------------------------------

/// Computes the address of an account contract before deployment.
///
/// This derives keys from `secret`, gets the contract artifact and initialization spec,
/// then computes the deterministic address using salt and derived public keys.
///
/// Uses the same shared instance-construction path as deployment to avoid
/// duplicating class-id / init-hash / address derivation logic.
pub async fn get_account_contract_address(
    account_contract: &dyn AccountContract,
    secret: Fr,
    salt: impl Into<Salt>,
) -> Result<AztecAddress, Error> {
    let salt: Fr = salt.into();
    let derived = aztec_crypto::derive_keys(&secret);
    let public_keys = derived.public_keys;

    let init_spec = account_contract.initialization_function_and_args().await?;
    let artifact = account_contract.contract_artifact().await?;

    let constructor_name = init_spec
        .as_ref()
        .map(|spec| spec.constructor_name.as_str());
    let constructor_args = init_spec
        .as_ref()
        .map(|spec| spec.constructor_args.clone())
        .unwrap_or_default();

    let instance = get_contract_instance_from_instantiation_params(
        &artifact,
        ContractInstantiationParams {
            constructor_name,
            constructor_args,
            salt,
            public_keys,
            deployer: AztecAddress::zero(),
        },
    )?;

    Ok(instance.address)
}

// ---------------------------------------------------------------------------
// AccountWithSecretKey
// ---------------------------------------------------------------------------

/// An account paired with its secret key.
///
/// Returned by [`AccountManager::account`] and provides both the account
/// interface and the secret key needed for signing operations.
pub struct AccountWithSecretKey {
    /// The underlying account implementation.
    pub account: Box<dyn Account>,
    /// The secret key associated with this account.
    pub secret_key: Fr,
    /// Deployment salt for this account contract.
    pub salt: Salt,
}

impl std::fmt::Debug for AccountWithSecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountWithSecretKey")
            .field("secret_key", &self.secret_key)
            .field("salt", &self.salt)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl AuthorizationProvider for AccountWithSecretKey {
    async fn create_auth_wit(
        &self,
        intent: MessageHashOrIntent,
        chain_info: &ChainInfo,
    ) -> Result<AuthWitness, Error> {
        self.account.create_auth_wit(intent, chain_info).await
    }
}

#[async_trait]
impl Account for AccountWithSecretKey {
    fn complete_address(&self) -> &CompleteAddress {
        self.account.complete_address()
    }

    fn address(&self) -> AztecAddress {
        self.account.address()
    }

    async fn create_tx_execution_request(
        &self,
        exec: ExecutionPayload,
        gas_settings: GasSettings,
        chain_info: &ChainInfo,
        options: EntrypointOptions,
    ) -> Result<TxExecutionRequest, Error> {
        self.account
            .create_tx_execution_request(exec, gas_settings, chain_info, options)
            .await
    }

    async fn wrap_execution_payload(
        &self,
        exec: ExecutionPayload,
        options: EntrypointOptions,
    ) -> Result<ExecutionPayload, Error> {
        self.account.wrap_execution_payload(exec, options).await
    }
}

// ---------------------------------------------------------------------------
// DeployAccountMethod
// ---------------------------------------------------------------------------

/// Options specific to account deployment.
pub struct DeployAccountOptions {
    /// Skip publishing the contract class (if already published).
    pub skip_class_publication: bool,
    /// Skip publishing the contract instance.
    pub skip_instance_publication: bool,
    /// Skip calling the constructor.
    pub skip_initialization: bool,
    /// Skip registering the contract with the wallet.
    pub skip_registration: bool,
    /// Explicit deployer override for third-party deployment.
    pub from: Option<AztecAddress>,
    /// Fee payment method.
    pub fee: Option<std::sync::Arc<dyn aztec_fee::FeePaymentMethod>>,
    /// Fee entrypoint options override.
    pub fee_entrypoint_options: Option<crate::entrypoint::DefaultAccountEntrypointOptions>,
}

impl Default for DeployAccountOptions {
    fn default() -> Self {
        Self {
            skip_class_publication: true,
            skip_instance_publication: true,
            skip_initialization: false,
            skip_registration: false,
            from: None,
            fee: None,
            fee_entrypoint_options: None,
        }
    }
}

impl std::fmt::Debug for DeployAccountOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeployAccountOptions")
            .field("skip_class_publication", &self.skip_class_publication)
            .field("skip_instance_publication", &self.skip_instance_publication)
            .field("skip_initialization", &self.skip_initialization)
            .field("from", &self.from)
            .field("has_fee", &self.fee.is_some())
            .finish_non_exhaustive()
    }
}

/// A deployment method for account contracts.
///
/// Wraps the generic [`DeployMethod`] from `aztec-contract` with account-specific
/// fee-payment wrapping via [`AccountEntrypointMetaPaymentMethod`].
pub struct DeployAccountMethod<'a, W> {
    wallet: &'a W,
    account: std::sync::Arc<dyn Account>,
    inner: aztec_contract::deployment::DeployMethod<'a, W>,
    inner_instance: ContractInstanceWithAddress,
}

impl<W> std::fmt::Debug for DeployAccountMethod<'_, W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeployAccountMethod")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

impl<W: Wallet> DeployAccountMethod<'_, W> {
    /// Build the full deployment execution payload.
    ///
    /// For self-deployed accounts (deployer == zero), fee payment goes after
    /// the deployment payload because the account contract must exist before
    /// it can execute its own entrypoint. For third-party deploys, fee goes first.
    pub async fn request(&self, opts: &DeployAccountOptions) -> Result<ExecutionPayload, Error> {
        let deploy_opts = self.to_deploy_options(opts);
        let deploy_payload = self.inner.request(&deploy_opts).await?;
        let is_self_deploy = opts.from == Some(AztecAddress::zero());

        let fee_payload = match (&opts.fee, is_self_deploy) {
            (Some(method), true) => {
                let wrapped = crate::meta_payment::AccountEntrypointMetaPaymentMethod::new(
                    self.account.clone(),
                    Some(method.clone()),
                    opts.fee_entrypoint_options.clone(),
                );
                Some(wrapped.get_fee_execution_payload().await?)
            }
            (Some(method), false) => Some(method.get_fee_execution_payload().await?),
            (None, true) => {
                let wrapped = crate::meta_payment::AccountEntrypointMetaPaymentMethod::new(
                    self.account.clone(),
                    None,
                    Some(crate::entrypoint::DefaultAccountEntrypointOptions {
                        cancellable: false,
                        tx_nonce: None,
                        fee_payment_method_options:
                            crate::entrypoint::AccountFeePaymentMethodOptions::PreexistingFeeJuice,
                    }),
                );
                Some(wrapped.get_fee_execution_payload().await?)
            }
            (None, false) => None,
        };

        match fee_payload {
            Some(fee) if is_self_deploy => ExecutionPayload::merge(vec![deploy_payload, fee]),
            Some(fee) => ExecutionPayload::merge(vec![fee, deploy_payload]),
            None => Ok(deploy_payload),
        }
    }

    /// Simulate the account deployment without sending.
    pub async fn simulate(
        &self,
        opts: &DeployAccountOptions,
        sim_opts: SimulateOptions,
    ) -> Result<TxSimulationResult, Error> {
        let payload = self.request(opts).await?;
        self.wallet.simulate_tx(payload, sim_opts).await
    }

    /// Send the account deployment transaction.
    pub async fn send(
        &self,
        opts: &DeployAccountOptions,
        send_opts: SendOptions,
    ) -> Result<DeployResult, Error> {
        let deploy_opts = self.to_deploy_options(opts);
        let instance = self.inner.get_instance(&deploy_opts)?;
        let payload = self.request(opts).await?;
        let send_result = self.wallet.send_tx(payload, send_opts).await?;
        Ok(DeployResult {
            send_result,
            instance,
        })
    }

    /// Get the contract instance that will be deployed.
    pub fn instance(&self) -> &ContractInstanceWithAddress {
        &self.inner_instance
    }

    fn to_deploy_options(
        &self,
        opts: &DeployAccountOptions,
    ) -> aztec_contract::deployment::DeployOptions {
        aztec_contract::deployment::DeployOptions {
            contract_address_salt: Some(self.inner_instance.inner.salt),
            skip_class_publication: opts.skip_class_publication,
            skip_instance_publication: opts.skip_instance_publication,
            skip_initialization: opts.skip_initialization,
            skip_registration: opts.skip_registration,
            universal_deploy: true,
            from: None,
        }
    }
}

/// The result of an account deployment transaction.
#[derive(Clone, Debug)]
pub struct DeployResult {
    /// The underlying send result (tx hash).
    pub send_result: SendResult,
    /// The deployed contract instance with its derived address.
    pub instance: ContractInstanceWithAddress,
}

// ---------------------------------------------------------------------------
// AccountManager
// ---------------------------------------------------------------------------

/// Manages account creation, address computation, and deployment.
///
/// `AccountManager` is the main entry point for working with Aztec accounts.
/// It combines a wallet backend, a secret key, and an account contract type
/// to provide deterministic address computation, account instance creation,
/// and deployment method generation.
pub struct AccountManager<W> {
    wallet: W,
    secret_key: Fr,
    account_contract: Box<dyn AccountContract>,
    has_initializer: bool,
    instance: ContractInstanceWithAddress,
    salt: Salt,
}

impl<W: Wallet> AccountManager<W> {
    /// Create a new account manager.
    ///
    /// Validates the account contract by fetching its artifact and
    /// initializer metadata. Contract instance hashing and key derivation
    /// are not implemented yet, so the stored instance remains an explicit
    /// placeholder until the deployment/key path lands.
    pub async fn create(
        wallet: W,
        secret_key: Fr,
        account_contract: Box<dyn AccountContract>,
        salt: Option<impl Into<Salt>>,
    ) -> Result<Self, Error> {
        let salt: Salt = salt.map(Into::into).unwrap_or_else(Salt::random);

        let artifact = account_contract.contract_artifact().await?;
        let init_spec = account_contract.initialization_function_and_args().await?;

        if let Some(spec) = &init_spec {
            let initializer = artifact.find_function(&spec.constructor_name)?;
            if !initializer.is_initializer {
                return Err(Error::Abi(format!(
                    "function '{}' in account artifact '{}' is not marked as an initializer",
                    spec.constructor_name, artifact.name
                )));
            }

            let expected = initializer.parameters.len();
            let actual = spec.constructor_args.len();
            if actual != expected {
                return Err(Error::Abi(format!(
                    "initializer '{}' in account artifact '{}' expects {expected} argument(s), got {actual}",
                    spec.constructor_name, artifact.name
                )));
            }
        }

        // Derive keys and compute full contract instance with real address.
        let derived = aztec_crypto::derive_keys(&secret_key);
        let public_keys = derived.public_keys;

        let constructor_name = init_spec
            .as_ref()
            .map(|spec| spec.constructor_name.as_str());
        let constructor_args = init_spec
            .as_ref()
            .map(|spec| spec.constructor_args.clone())
            .unwrap_or_default();

        let instance = get_contract_instance_from_instantiation_params(
            &artifact,
            ContractInstantiationParams {
                constructor_name,
                constructor_args,
                salt,
                public_keys,
                deployer: AztecAddress::zero(),
            },
        )?;

        Ok(Self {
            wallet,
            secret_key,
            account_contract,
            has_initializer: init_spec.is_some(),
            instance,
            salt,
        })
    }

    /// Get the complete address of this account.
    ///
    /// Derives the full key set from the secret key, computes the partial
    /// address from the contract instance, and returns the complete address.
    #[allow(clippy::unused_async)]
    pub async fn complete_address(&self) -> Result<CompleteAddress, Error> {
        use aztec_core::hash::{
            compute_address, compute_partial_address, compute_salted_initialization_hash,
        };
        use aztec_crypto::derive_keys;

        let derived = derive_keys(&self.secret_key);

        let salted_init_hash = compute_salted_initialization_hash(
            self.instance.inner.salt,
            self.instance.inner.initialization_hash,
            self.instance.inner.deployer,
        );
        let partial_address = compute_partial_address(
            self.instance.inner.original_contract_class_id,
            salted_init_hash,
        );

        let address = compute_address(&derived.public_keys, &partial_address)?;

        Ok(CompleteAddress {
            address,
            public_keys: derived.public_keys,
            partial_address,
        })
    }

    /// Get the address of this account instance.
    pub const fn address(&self) -> AztecAddress {
        self.instance.address
    }

    /// Get the contract instance for this account.
    pub const fn instance(&self) -> &ContractInstanceWithAddress {
        &self.instance
    }

    /// Get the salt used for this account.
    pub const fn salt(&self) -> Salt {
        self.salt
    }

    /// Get the secret key for this account.
    pub const fn secret_key(&self) -> Fr {
        self.secret_key
    }

    /// Return whether this account contract has an initializer function.
    pub const fn has_initializer(&self) -> bool {
        self.has_initializer
    }

    /// Get an [`Account`] implementation backed by this manager's account
    /// contract, paired with the secret key.
    pub async fn account(&self) -> Result<AccountWithSecretKey, Error> {
        let complete_addr = self.complete_address().await?;
        Ok(AccountWithSecretKey {
            account: self.account_contract.account(complete_addr),
            secret_key: self.secret_key,
            salt: self.salt,
        })
    }

    /// Get a deployment method for this account's contract.
    ///
    /// Fetches the artifact and initialization spec from the account contract,
    /// creates a generic `ContractDeployer` + `DeployMethod`, and wraps it
    /// with account-specific fee-payment logic in a [`DeployAccountMethod`].
    pub async fn deploy_method(&self) -> Result<DeployAccountMethod<'_, W>, Error> {
        let artifact = self.account_contract.contract_artifact().await?;
        let init_spec = self
            .account_contract
            .initialization_function_and_args()
            .await?;

        let complete_addr = self.complete_address().await?;
        let account: std::sync::Arc<dyn Account> =
            std::sync::Arc::from(self.account_contract.account(complete_addr));

        let constructor_args = init_spec
            .as_ref()
            .map(|s| s.constructor_args.clone())
            .unwrap_or_default();

        let mut deployer =
            aztec_contract::deployment::ContractDeployer::new(artifact, &self.wallet)
                .with_public_keys(self.instance.inner.public_keys.clone());
        if let Some(spec) = &init_spec {
            deployer = deployer.with_constructor_name(spec.constructor_name.clone());
        }

        let inner = deployer.deploy(constructor_args)?;

        // Get the instance using the account's salt.
        let deploy_opts = aztec_contract::deployment::DeployOptions {
            contract_address_salt: Some(self.salt),
            ..Default::default()
        };
        let inner_instance = inner.get_instance(&deploy_opts)?;

        Ok(DeployAccountMethod {
            wallet: &self.wallet,
            account,
            inner,
            inner_instance,
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
    use crate::abi::{AbiParameter, AbiType, FunctionArtifact, FunctionSelector, FunctionType};
    use crate::fee::Gas;
    use crate::types::PublicKeys;
    use crate::wallet::MockWallet;

    // -- Test helpers: mock account types -----------------------------------

    struct MockAuthProvider {
        addr: CompleteAddress,
    }

    #[async_trait]
    impl AuthorizationProvider for MockAuthProvider {
        async fn create_auth_wit(
            &self,
            _intent: MessageHashOrIntent,
            _chain_info: &ChainInfo,
        ) -> Result<AuthWitness, Error> {
            Ok(AuthWitness {
                fields: vec![self.addr.address.0],
                ..Default::default()
            })
        }
    }

    struct MockAccount {
        addr: CompleteAddress,
    }

    #[async_trait]
    impl AuthorizationProvider for MockAccount {
        async fn create_auth_wit(
            &self,
            _intent: MessageHashOrIntent,
            _chain_info: &ChainInfo,
        ) -> Result<AuthWitness, Error> {
            Ok(AuthWitness {
                fields: vec![self.addr.address.0],
                ..Default::default()
            })
        }
    }

    #[async_trait]
    impl Account for MockAccount {
        fn complete_address(&self) -> &CompleteAddress {
            &self.addr
        }

        fn address(&self) -> AztecAddress {
            self.addr.address
        }

        async fn create_tx_execution_request(
            &self,
            exec: ExecutionPayload,
            gas_settings: GasSettings,
            chain_info: &ChainInfo,
            _options: EntrypointOptions,
        ) -> Result<TxExecutionRequest, Error> {
            Ok(TxExecutionRequest {
                origin: self.addr.address,
                function_selector: FunctionSelector::from_hex("0x12345678")
                    .expect("valid selector"),
                first_call_args_hash: Fr::from(11u64),
                tx_context: TxContext {
                    chain_id: chain_info.chain_id,
                    version: chain_info.version,
                    gas_settings,
                },
                args_of_calls: exec.extra_hashed_args,
                auth_witnesses: exec.auth_witnesses,
                capsules: exec.capsules,
                salt: Fr::from(7u64),
                fee_payer: exec.fee_payer,
            })
        }

        async fn wrap_execution_payload(
            &self,
            exec: ExecutionPayload,
            options: EntrypointOptions,
        ) -> Result<ExecutionPayload, Error> {
            Ok(ExecutionPayload {
                fee_payer: options.fee_payer.or(exec.fee_payer),
                ..exec
            })
        }
    }

    struct MockAccountContract;

    #[async_trait]
    impl AccountContract for MockAccountContract {
        async fn contract_artifact(&self) -> Result<ContractArtifact, Error> {
            Ok(ContractArtifact {
                name: "MockAccount".to_owned(),
                functions: vec![FunctionArtifact {
                    name: "constructor".to_owned(),
                    function_type: FunctionType::Private,
                    is_initializer: true,
                    is_static: false,
                    parameters: vec![AbiParameter {
                        name: "owner".to_owned(),
                        typ: AbiType::Field,
                        visibility: None,
                    }],
                    return_types: vec![],
                    selector: Some(
                        FunctionSelector::from_hex("0x12345678").expect("valid selector"),
                    ),
                    bytecode: None,
                    verification_key_hash: None,
                    verification_key: None,
                    custom_attributes: None,
                    is_unconstrained: None,
                    debug_symbols: None,
                    error_types: None,
                    is_only_self: None,
                }],
                outputs: None,
                file_map: None,
                context_inputs_sizes: None,
            })
        }

        async fn initialization_function_and_args(
            &self,
        ) -> Result<Option<InitializationSpec>, Error> {
            Ok(Some(InitializationSpec {
                constructor_name: "constructor".to_owned(),
                constructor_args: vec![AbiValue::Field(Fr::from(42u64))],
            }))
        }

        fn account(&self, address: CompleteAddress) -> Box<dyn Account> {
            Box::new(MockAccount { addr: address })
        }

        fn auth_witness_provider(
            &self,
            address: CompleteAddress,
        ) -> Box<dyn AuthorizationProvider> {
            Box::new(MockAuthProvider { addr: address })
        }
    }

    struct NoInitializerAccountContract;

    #[async_trait]
    impl AccountContract for NoInitializerAccountContract {
        async fn contract_artifact(&self) -> Result<ContractArtifact, Error> {
            Ok(ContractArtifact {
                name: "NoInitializerAccount".to_owned(),
                functions: vec![],
                outputs: None,
                file_map: None,
                context_inputs_sizes: None,
            })
        }

        async fn initialization_function_and_args(
            &self,
        ) -> Result<Option<InitializationSpec>, Error> {
            Ok(None)
        }

        fn account(&self, address: CompleteAddress) -> Box<dyn Account> {
            Box::new(MockAccount { addr: address })
        }

        fn auth_witness_provider(
            &self,
            address: CompleteAddress,
        ) -> Box<dyn AuthorizationProvider> {
            Box::new(MockAuthProvider { addr: address })
        }
    }

    struct BadInitializerNameAccountContract;

    #[async_trait]
    impl AccountContract for BadInitializerNameAccountContract {
        async fn contract_artifact(&self) -> Result<ContractArtifact, Error> {
            MockAccountContract.contract_artifact().await
        }

        async fn initialization_function_and_args(
            &self,
        ) -> Result<Option<InitializationSpec>, Error> {
            Ok(Some(InitializationSpec {
                constructor_name: "missing".to_owned(),
                constructor_args: vec![AbiValue::Field(Fr::from(42u64))],
            }))
        }

        fn account(&self, address: CompleteAddress) -> Box<dyn Account> {
            Box::new(MockAccount { addr: address })
        }

        fn auth_witness_provider(
            &self,
            address: CompleteAddress,
        ) -> Box<dyn AuthorizationProvider> {
            Box::new(MockAuthProvider { addr: address })
        }
    }

    struct BadInitializerArgsAccountContract;

    #[async_trait]
    impl AccountContract for BadInitializerArgsAccountContract {
        async fn contract_artifact(&self) -> Result<ContractArtifact, Error> {
            MockAccountContract.contract_artifact().await
        }

        async fn initialization_function_and_args(
            &self,
        ) -> Result<Option<InitializationSpec>, Error> {
            Ok(Some(InitializationSpec {
                constructor_name: "constructor".to_owned(),
                constructor_args: vec![],
            }))
        }

        fn account(&self, address: CompleteAddress) -> Box<dyn Account> {
            Box::new(MockAccount { addr: address })
        }

        fn auth_witness_provider(
            &self,
            address: CompleteAddress,
        ) -> Box<dyn AuthorizationProvider> {
            Box::new(MockAuthProvider { addr: address })
        }
    }

    fn sample_chain_info() -> ChainInfo {
        ChainInfo {
            chain_id: Fr::from(31337u64),
            version: Fr::from(1u64),
        }
    }

    fn sample_complete_address() -> CompleteAddress {
        CompleteAddress {
            address: AztecAddress(Fr::from(99u64)),
            public_keys: PublicKeys::default(),
            partial_address: Fr::from(1u64),
        }
    }

    // -- Trait object safety -----------------------------------------------

    #[test]
    fn authorization_provider_is_object_safe() {
        fn _assert(_: &dyn AuthorizationProvider) {}
    }

    #[test]
    fn account_is_object_safe() {
        fn _assert(_: &dyn Account) {}
    }

    #[test]
    fn account_contract_is_object_safe() {
        fn _assert(_: &dyn AccountContract) {}
    }

    // -- Send + Sync -------------------------------------------------------

    #[test]
    fn mock_account_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockAccount>();
        assert_send_sync::<MockAuthProvider>();
        assert_send_sync::<MockAccountContract>();
    }

    #[test]
    fn account_with_secret_key_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<AccountWithSecretKey>();
    }

    // -- Supporting type serde ---------------------------------------------

    #[test]
    fn entrypoint_options_default() {
        let opts = EntrypointOptions::default();
        assert!(opts.fee_payer.is_none());
        assert!(opts.gas_settings.is_none());
    }

    #[test]
    fn entrypoint_options_roundtrip() {
        let opts = EntrypointOptions {
            fee_payer: Some(AztecAddress(Fr::from(1u64))),
            gas_settings: Some(GasSettings {
                gas_limits: Some(Gas {
                    da_gas: 100,
                    l2_gas: 200,
                }),
                ..GasSettings::default()
            }),
        };
        let json = serde_json::to_string(&opts).expect("serialize EntrypointOptions");
        let decoded: EntrypointOptions =
            serde_json::from_str(&json).expect("deserialize EntrypointOptions");
        assert_eq!(decoded, opts);
    }

    #[test]
    fn tx_execution_request_roundtrip() {
        let req = TxExecutionRequest {
            origin: AztecAddress(Fr::from(1u64)),
            function_selector: FunctionSelector::from_hex("0xaabbccdd").expect("selector"),
            first_call_args_hash: Fr::from(3u64),
            tx_context: TxContext {
                chain_id: Fr::from(31337u64),
                version: Fr::from(1u64),
                gas_settings: GasSettings::default(),
            },
            args_of_calls: vec![HashedValues::from_args(vec![Fr::from(7u64)])],
            auth_witnesses: vec![AuthWitness {
                fields: vec![Fr::from(42u64)],
                ..Default::default()
            }],
            capsules: vec![],
            salt: Fr::from(9u64),
            fee_payer: None,
        };
        let json = serde_json::to_string(&req).expect("serialize TxExecutionRequest");
        let decoded: TxExecutionRequest =
            serde_json::from_str(&json).expect("deserialize TxExecutionRequest");
        assert_eq!(decoded, req);
    }

    #[test]
    fn initialization_spec_fields() {
        let spec = InitializationSpec {
            constructor_name: "constructor".to_owned(),
            constructor_args: vec![AbiValue::Field(Fr::from(1u64)), AbiValue::Boolean(true)],
        };
        assert_eq!(spec.constructor_name, "constructor");
        assert_eq!(spec.constructor_args.len(), 2);
    }

    // -- AuthorizationProvider tests ---------------------------------------

    #[tokio::test]
    async fn mock_auth_provider_creates_witness() {
        let provider = MockAuthProvider {
            addr: sample_complete_address(),
        };
        let chain_info = sample_chain_info();
        let wit = provider
            .create_auth_wit(
                MessageHashOrIntent::Hash {
                    hash: Fr::from(1u64),
                },
                &chain_info,
            )
            .await
            .expect("create auth wit");
        assert_eq!(wit.fields.len(), 1);
        assert_eq!(wit.fields[0], Fr::from(99u64));
    }

    // -- Account tests -----------------------------------------------------

    #[tokio::test]
    async fn mock_account_address() {
        let account = MockAccount {
            addr: sample_complete_address(),
        };
        assert_eq!(account.address(), AztecAddress(Fr::from(99u64)));
        assert_eq!(
            account.complete_address().address,
            AztecAddress(Fr::from(99u64))
        );
    }

    #[tokio::test]
    async fn mock_account_creates_execution_request() {
        let account = MockAccount {
            addr: sample_complete_address(),
        };
        let chain_info = sample_chain_info();

        let payload = ExecutionPayload {
            calls: vec![],
            auth_witnesses: vec![AuthWitness {
                fields: vec![Fr::from(1u64)],
                ..Default::default()
            }],
            capsules: vec![],
            extra_hashed_args: vec![],
            fee_payer: None,
        };

        let gas_settings = GasSettings {
            gas_limits: Some(Gas {
                da_gas: 100,
                l2_gas: 200,
            }),
            ..GasSettings::default()
        };

        let options = EntrypointOptions {
            fee_payer: Some(AztecAddress(Fr::from(5u64))),
            gas_settings: None,
        };

        let req = account
            .create_tx_execution_request(payload, gas_settings.clone(), &chain_info, options)
            .await
            .expect("create tx execution request");

        assert_eq!(req.origin, AztecAddress(Fr::from(99u64)));
        assert_eq!(req.auth_witnesses.len(), 1);
        assert_eq!(req.tx_context.gas_settings, gas_settings);
        assert_eq!(req.tx_context.chain_id, chain_info.chain_id);
    }

    #[tokio::test]
    async fn mock_account_wraps_payload() {
        let account = MockAccount {
            addr: sample_complete_address(),
        };

        let payload = ExecutionPayload {
            calls: vec![],
            auth_witnesses: vec![],
            capsules: vec![],
            extra_hashed_args: vec![],
            fee_payer: Some(AztecAddress(Fr::from(3u64))),
        };

        let options = EntrypointOptions::default();

        let wrapped = account
            .wrap_execution_payload(payload, options)
            .await
            .expect("wrap execution payload");

        assert_eq!(wrapped.fee_payer, Some(AztecAddress(Fr::from(3u64))));
    }

    #[tokio::test]
    async fn mock_account_wrap_overrides_fee_payer() {
        let account = MockAccount {
            addr: sample_complete_address(),
        };

        let payload = ExecutionPayload {
            calls: vec![],
            auth_witnesses: vec![],
            capsules: vec![],
            extra_hashed_args: vec![],
            fee_payer: Some(AztecAddress(Fr::from(3u64))),
        };

        let options = EntrypointOptions {
            fee_payer: Some(AztecAddress(Fr::from(7u64))),
            gas_settings: None,
        };

        let wrapped = account
            .wrap_execution_payload(payload, options)
            .await
            .expect("wrap execution payload");

        assert_eq!(wrapped.fee_payer, Some(AztecAddress(Fr::from(7u64))));
    }

    #[tokio::test]
    async fn account_with_secret_key_delegates_to_inner_account() {
        let account = AccountWithSecretKey {
            account: Box::new(MockAccount {
                addr: sample_complete_address(),
            }),
            secret_key: Fr::from(42u64),
            salt: Fr::from(7u64),
        };

        let chain_info = sample_chain_info();
        let wit = account
            .create_auth_wit(
                MessageHashOrIntent::Hash {
                    hash: Fr::from(1u64),
                },
                &chain_info,
            )
            .await
            .expect("create auth wit");

        assert_eq!(account.address(), AztecAddress(Fr::from(99u64)));
        assert_eq!(account.complete_address().partial_address, Fr::from(1u64));
        assert_eq!(account.secret_key, Fr::from(42u64));
        assert_eq!(account.salt, Fr::from(7u64));
        assert_eq!(wit.fields, vec![Fr::from(99u64)]);
    }

    // -- AccountContract tests ---------------------------------------------

    #[tokio::test]
    async fn mock_account_contract_artifact() {
        let contract = MockAccountContract;
        let artifact = contract
            .contract_artifact()
            .await
            .expect("get contract artifact");
        assert_eq!(artifact.name, "MockAccount");
        assert_eq!(artifact.functions.len(), 1);
        assert!(artifact.functions[0].is_initializer);
    }

    #[tokio::test]
    async fn mock_account_contract_init_spec() {
        let contract = MockAccountContract;
        let spec = contract
            .initialization_function_and_args()
            .await
            .expect("get init spec")
            .expect("should have init spec");
        assert_eq!(spec.constructor_name, "constructor");
        assert_eq!(spec.constructor_args.len(), 1);
    }

    #[tokio::test]
    async fn mock_account_contract_produces_account() {
        let contract = MockAccountContract;
        let addr = sample_complete_address();
        let account = contract.account(addr);
        assert_eq!(account.address(), AztecAddress(Fr::from(99u64)));
    }

    #[tokio::test]
    async fn mock_account_contract_produces_auth_provider() {
        let contract = MockAccountContract;
        let addr = sample_complete_address();
        let provider = contract.auth_witness_provider(addr);
        let chain_info = sample_chain_info();

        let wit = provider
            .create_auth_wit(
                MessageHashOrIntent::Hash {
                    hash: Fr::from(1u64),
                },
                &chain_info,
            )
            .await
            .expect("create auth wit");
        assert_eq!(wit.fields.len(), 1);
    }

    // -- AccountManager tests ----------------------------------------------

    #[tokio::test]
    async fn account_manager_create() {
        let wallet = MockWallet::new(sample_chain_info());
        let manager = AccountManager::create(
            wallet,
            Fr::from(42u64),
            Box::new(MockAccountContract),
            Some(Fr::from(7u64)),
        )
        .await
        .expect("create account manager");

        assert_eq!(manager.salt(), Fr::from(7u64));
        assert_eq!(manager.secret_key(), Fr::from(42u64));
    }

    #[tokio::test]
    async fn account_manager_default_salt() {
        let wallet = MockWallet::new(sample_chain_info());
        let manager = AccountManager::create(
            wallet,
            Fr::from(1u64),
            Box::new(MockAccountContract),
            None::<Fr>,
        )
        .await
        .expect("create account manager");

        assert_ne!(manager.salt(), Fr::zero());
    }

    #[tokio::test]
    async fn account_manager_rejects_missing_initializer_function() {
        let wallet = MockWallet::new(sample_chain_info());
        let result = AccountManager::create(
            wallet,
            Fr::from(1u64),
            Box::new(BadInitializerNameAccountContract),
            None::<Fr>,
        )
        .await;
        assert!(result.is_err());
        let err = result.err().expect("initializer lookup should fail");

        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn account_manager_rejects_initializer_argument_mismatch() {
        let wallet = MockWallet::new(sample_chain_info());
        let result = AccountManager::create(
            wallet,
            Fr::from(1u64),
            Box::new(BadInitializerArgsAccountContract),
            None::<Fr>,
        )
        .await;
        assert!(result.is_err());
        let err = result
            .err()
            .expect("initializer arg validation should fail");

        assert!(err.to_string().contains("expects 1 argument(s), got 0"));
    }

    #[tokio::test]
    async fn account_manager_address_accessors() {
        let wallet = MockWallet::new(sample_chain_info());
        let manager = AccountManager::create(
            wallet,
            Fr::from(1u64),
            Box::new(MockAccountContract),
            None::<Fr>,
        )
        .await
        .expect("create account manager");

        // Address is now computed from real key derivation and artifact hashing
        assert_ne!(manager.address(), AztecAddress(Fr::zero()));

        // complete_address() now works (derives keys from secret key)
        let complete = manager
            .complete_address()
            .await
            .expect("complete address derivation");
        assert!(!complete.public_keys.is_empty());
        assert!(!complete.address.0.is_zero());

        let instance = manager.instance();
        assert_eq!(instance.inner.version, 1);
    }

    #[tokio::test]
    async fn account_manager_account() {
        let wallet = MockWallet::new(sample_chain_info());
        let manager = AccountManager::create(
            wallet,
            Fr::from(42u64),
            Box::new(MockAccountContract),
            None::<Fr>,
        )
        .await
        .expect("create account manager");

        // account() now works since complete_address() is implemented
        let account = manager.account().await.expect("account construction");
        assert_eq!(account.secret_key, Fr::from(42u64));
    }

    #[tokio::test]
    async fn account_manager_account_creates_auth_wit() {
        let wallet = MockWallet::new(sample_chain_info());
        let manager = AccountManager::create(
            wallet,
            Fr::from(42u64),
            Box::new(MockAccountContract),
            None::<Fr>,
        )
        .await
        .expect("create account manager");

        let account = manager.account().await.expect("account construction");
        // Verify the account can create auth witnesses
        let chain_info = sample_chain_info();
        let wit = account
            .create_auth_wit(
                MessageHashOrIntent::Hash {
                    hash: Fr::from(999u64),
                },
                &chain_info,
            )
            .await
            .expect("create auth wit");
        assert!(!wit.fields.is_empty());
    }

    // -- DeployAccountMethod tests -----------------------------------------

    #[tokio::test]
    async fn deploy_method_request_builds_payload() {
        let wallet = MockWallet::new(sample_chain_info());
        let manager = AccountManager::create(
            wallet,
            Fr::from(1u64),
            Box::new(MockAccountContract),
            None::<Fr>,
        )
        .await
        .expect("create account manager");

        let deploy = manager.deploy_method().await.expect("build deploy method");
        let opts = DeployAccountOptions {
            skip_registration: true,
            ..Default::default()
        };
        let payload = deploy.request(&opts).await.expect("request should succeed");
        // Should have at least some calls (class publication, instance publication, constructor)
        assert!(!payload.calls.is_empty());
    }

    #[tokio::test]
    async fn deploy_method_requires_initializer() {
        let wallet = MockWallet::new(sample_chain_info());
        let manager = AccountManager::create(
            wallet,
            Fr::from(1u64),
            Box::new(NoInitializerAccountContract),
            None::<Fr>,
        )
        .await
        .expect("create account manager");

        assert!(!manager.has_initializer());
        let deploy = manager
            .deploy_method()
            .await
            .expect("deploy method should still build");
        let payload = deploy
            .request(&DeployAccountOptions {
                skip_registration: true,
                ..Default::default()
            })
            .await
            .expect("request should succeed without initializer");
        assert!(payload.calls.is_empty());
    }

    #[tokio::test]
    async fn deploy_method_instance() {
        let wallet = MockWallet::new(sample_chain_info());
        let manager = AccountManager::create(
            wallet,
            Fr::from(1u64),
            Box::new(MockAccountContract),
            Some(Fr::from(99u64)),
        )
        .await
        .expect("create account manager");

        let deploy = manager.deploy_method().await.expect("build deploy method");
        assert_eq!(deploy.instance().inner.salt, Fr::from(99u64));
        // Address should be non-zero (real derivation)
        assert_ne!(deploy.instance().address, AztecAddress(Fr::zero()));
    }

    #[tokio::test]
    async fn deploy_method_instance_matches_manager_address() {
        let wallet = MockWallet::new(sample_chain_info());
        let manager = AccountManager::create(
            wallet,
            Fr::from(1u64),
            Box::new(MockAccountContract),
            Some(Fr::from(99u64)),
        )
        .await
        .expect("create account manager");

        let deploy = manager.deploy_method().await.expect("build deploy method");
        // The deployed instance address should match manager's stored instance address
        assert_eq!(deploy.instance().address, manager.instance().address);
    }

    #[tokio::test]
    async fn deploy_method_skip_flags_pass_through() {
        let wallet = MockWallet::new(sample_chain_info());
        let manager = AccountManager::create(
            wallet,
            Fr::from(1u64),
            Box::new(MockAccountContract),
            None::<Fr>,
        )
        .await
        .expect("create account manager");

        let deploy = manager.deploy_method().await.expect("build deploy method");
        let opts = DeployAccountOptions {
            skip_registration: true,
            skip_class_publication: true,
            skip_instance_publication: true,
            skip_initialization: true,
            ..Default::default()
        };
        let payload = deploy.request(&opts).await.expect("request");
        // With everything skipped, should produce empty or fee-only payload
        // The fee wrapper still produces calls via meta payment
        // This mainly tests that skip flags don't crash
        let _ = payload;
    }
}
