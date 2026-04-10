use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::abi::{AbiType, ContractArtifact, EventSelector};
use crate::error::Error;
use crate::fee::{Gas, GasSettings};
use crate::node::LogId;
use crate::tx::{AuthWitness, Capsule, ExecutionPayload, FunctionCall, TxHash};
use crate::types::{AztecAddress, ContractInstanceWithAddress, Fr};

// Re-export ChainInfo and MessageHashOrIntent from aztec-core::hash
// so existing consumers of aztec-wallet continue to find them here.
pub use aztec_core::hash::{ChainInfo, MessageHashOrIntent};

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// A value with an optional human-readable alias.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Aliased<T> {
    /// Human-readable alias.
    pub alias: String,
    /// The aliased value.
    pub item: T,
}

/// Metadata about a registered contract instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractMetadata {
    /// The contract instance, if registered.
    pub instance: Option<ContractInstanceWithAddress>,
    /// Whether the contract has been initialized.
    pub is_contract_initialized: bool,
    /// Whether the contract class has been published on-chain.
    pub is_contract_published: bool,
    /// Whether the contract has been upgraded.
    pub is_contract_updated: bool,
    /// Updated contract class ID after an upgrade.
    pub updated_contract_class_id: Option<Fr>,
}

/// Metadata about a registered contract class.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractClassMetadata {
    /// Whether the artifact has been registered locally.
    pub is_artifact_registered: bool,
    /// Whether the class has been published on-chain.
    pub is_contract_class_publicly_registered: bool,
}

/// Options for transaction simulation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateOptions {
    /// Address of the simulating account.
    pub from: AztecAddress,
    /// Skip validation checks during simulation.
    #[serde(default)]
    pub skip_validation: bool,
    /// Whether to skip fee enforcement during simulation.
    #[serde(default = "default_skip_fee_enforcement")]
    pub skip_fee_enforcement: bool,
    /// Additional authorization witnesses.
    #[serde(default)]
    pub auth_witnesses: Vec<AuthWitness>,
    /// Private data capsules for the simulation.
    #[serde(default)]
    pub capsules: Vec<Capsule>,
    /// Additional note-discovery scopes.
    #[serde(default)]
    pub additional_scopes: Vec<AztecAddress>,
    /// Gas settings for the simulation.
    pub gas_settings: Option<GasSettings>,
    /// Pre-resolved fee execution payload to merge into the transaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_execution_payload: Option<ExecutionPayload>,
    /// If true, estimate gas and include suggested gas settings in the result.
    #[serde(default)]
    pub estimate_gas: bool,
    /// Padding factor for gas estimation (default: 0.1 = 10%).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_gas_padding: Option<f64>,
}

/// Options for transaction sending.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendOptions {
    /// Address of the sending account.
    pub from: AztecAddress,
    /// Additional authorization witnesses.
    #[serde(default)]
    pub auth_witnesses: Vec<AuthWitness>,
    /// Private data capsules.
    #[serde(default)]
    pub capsules: Vec<Capsule>,
    /// Additional note-discovery scopes.
    #[serde(default)]
    pub additional_scopes: Vec<AztecAddress>,
    /// Gas settings for the transaction.
    pub gas_settings: Option<GasSettings>,
    /// Pre-resolved fee execution payload to merge into the transaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_execution_payload: Option<ExecutionPayload>,
}

/// Profiling mode for transaction analysis.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileMode {
    /// Count constraint gates.
    Gates,
    /// Count execution steps.
    ExecutionSteps,
    /// Full profiling (gates + execution steps).
    Full,
}

/// Options for transaction profiling.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileOptions {
    /// Address of the profiling account.
    pub from: AztecAddress,
    /// Additional authorization witnesses.
    #[serde(default)]
    pub auth_witnesses: Vec<AuthWitness>,
    /// Private data capsules.
    #[serde(default)]
    pub capsules: Vec<Capsule>,
    /// Additional note-discovery scopes.
    #[serde(default)]
    pub additional_scopes: Vec<AztecAddress>,
    /// Profiling mode.
    pub profile_mode: Option<ProfileMode>,
    /// Whether proof generation should be skipped while profiling.
    #[serde(default = "default_skip_proof_generation")]
    pub skip_proof_generation: bool,
    /// Gas settings for profiling.
    pub gas_settings: Option<GasSettings>,
    /// Pre-resolved fee execution payload to merge into the transaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_execution_payload: Option<ExecutionPayload>,
}

/// Options for utility function execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteUtilityOptions {
    /// Address scope for note discovery.
    pub scope: AztecAddress,
    /// Additional authorization witnesses.
    #[serde(default)]
    pub auth_witnesses: Vec<AuthWitness>,
}

/// Result of a transaction simulation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxSimulationResult {
    /// Return values from the simulated execution.
    pub return_values: serde_json::Value,
    /// Gas consumed during simulation.
    pub gas_used: Option<Gas>,
}

/// Result of a transaction profiling.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxProfileResult {
    /// Return values from the profiled execution.
    pub return_values: serde_json::Value,
    /// Gas consumed during profiling.
    pub gas_used: Option<Gas>,
    /// Detailed profiling data.
    pub profile_data: serde_json::Value,
}

/// Result of a utility function execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UtilityExecutionResult {
    /// Return values from the utility function call.
    pub result: serde_json::Value,
    /// Optional simulation stats payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<serde_json::Value>,
}

/// Result of sending a transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendResult {
    /// Hash of the submitted transaction.
    pub tx_hash: TxHash,
}

/// Metadata definition for event decoding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventMetadataDefinition {
    /// Selector identifying the event type.
    pub event_selector: EventSelector,
    /// ABI type describing the event's fields.
    pub abi_type: AbiType,
    /// Ordered field names for decoding.
    pub field_names: Vec<String>,
}

/// Filter for querying private events from a wallet.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateEventFilter {
    /// Contract to filter events from.
    pub contract_address: AztecAddress,
    /// Note-discovery scopes.
    #[serde(default)]
    pub scopes: Vec<AztecAddress>,
    /// Filter by transaction hash.
    pub tx_hash: Option<TxHash>,
    /// Start block (inclusive).
    pub from_block: Option<u64>,
    /// End block (inclusive).
    pub to_block: Option<u64>,
    /// Cursor for pagination.
    pub after_log: Option<LogId>,
}

/// Metadata attached to a decoded private event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateEventMetadata {
    /// Hash of the transaction that emitted the event.
    pub tx_hash: TxHash,
    /// Block number, if available.
    pub block_number: Option<u64>,
    /// Log index within the block.
    pub log_index: Option<u64>,
}

/// A private event retrieved from the wallet.
///
/// Event data is kept as opaque JSON; callers can deserialize into a
/// concrete type as needed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateEvent {
    /// Decoded event data (opaque JSON).
    pub event: serde_json::Value,
    /// Event metadata (tx hash, block, index).
    pub metadata: PrivateEventMetadata,
}

impl Default for SimulateOptions {
    fn default() -> Self {
        Self {
            from: AztecAddress(Fr::zero()),
            skip_validation: false,
            skip_fee_enforcement: default_skip_fee_enforcement(),
            auth_witnesses: vec![],
            capsules: vec![],
            additional_scopes: vec![],
            gas_settings: None,
            fee_execution_payload: None,
            estimate_gas: false,
            estimated_gas_padding: None,
        }
    }
}

impl Default for SendOptions {
    fn default() -> Self {
        Self {
            from: AztecAddress(Fr::zero()),
            auth_witnesses: vec![],
            capsules: vec![],
            additional_scopes: vec![],
            gas_settings: None,
            fee_execution_payload: None,
        }
    }
}

impl Default for ProfileOptions {
    fn default() -> Self {
        Self {
            from: AztecAddress(Fr::zero()),
            auth_witnesses: vec![],
            capsules: vec![],
            additional_scopes: vec![],
            profile_mode: None,
            skip_proof_generation: default_skip_proof_generation(),
            gas_settings: None,
            fee_execution_payload: None,
        }
    }
}

impl Default for ExecuteUtilityOptions {
    fn default() -> Self {
        Self {
            scope: AztecAddress(Fr::zero()),
            auth_witnesses: vec![],
        }
    }
}

const fn default_skip_fee_enforcement() -> bool {
    true
}

const fn default_skip_proof_generation() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Wallet trait
// ---------------------------------------------------------------------------

/// Main private execution interface for end users.
///
/// This trait is the primary abstraction for interacting with the Aztec network
/// through a wallet implementation. It provides methods for account management,
/// contract registration, transaction simulation, sending, and event retrieval.
#[async_trait]
pub trait Wallet: Send + Sync {
    /// Get chain identification information.
    async fn get_chain_info(&self) -> Result<ChainInfo, Error>;

    /// Get the list of accounts managed by this wallet.
    async fn get_accounts(&self) -> Result<Vec<Aliased<AztecAddress>>, Error>;

    /// Get the address book entries.
    async fn get_address_book(&self) -> Result<Vec<Aliased<AztecAddress>>, Error>;

    /// Register a sender address with an optional alias.
    async fn register_sender(
        &self,
        address: AztecAddress,
        alias: Option<String>,
    ) -> Result<AztecAddress, Error>;

    /// Get metadata about a registered contract.
    async fn get_contract_metadata(&self, address: AztecAddress)
        -> Result<ContractMetadata, Error>;

    /// Get metadata about a registered contract class.
    async fn get_contract_class_metadata(
        &self,
        class_id: Fr,
    ) -> Result<ContractClassMetadata, Error>;

    /// Register a contract instance (and optionally its artifact) with the wallet.
    async fn register_contract(
        &self,
        instance: ContractInstanceWithAddress,
        artifact: Option<ContractArtifact>,
        secret_key: Option<Fr>,
    ) -> Result<ContractInstanceWithAddress, Error>;

    /// Get private events matching the given filter.
    async fn get_private_events(
        &self,
        event_metadata: &EventMetadataDefinition,
        filter: PrivateEventFilter,
    ) -> Result<Vec<PrivateEvent>, Error>;

    /// Simulate a transaction without sending it.
    async fn simulate_tx(
        &self,
        exec: ExecutionPayload,
        opts: SimulateOptions,
    ) -> Result<TxSimulationResult, Error>;

    /// Execute a utility (view) function.
    async fn execute_utility(
        &self,
        call: FunctionCall,
        opts: ExecuteUtilityOptions,
    ) -> Result<UtilityExecutionResult, Error>;

    /// Profile a transaction for gas estimation and performance data.
    async fn profile_tx(
        &self,
        exec: ExecutionPayload,
        opts: ProfileOptions,
    ) -> Result<TxProfileResult, Error>;

    /// Send a transaction to the network.
    async fn send_tx(&self, exec: ExecutionPayload, opts: SendOptions)
        -> Result<SendResult, Error>;

    /// Wait until a deployed contract instance is queryable from the node.
    async fn wait_for_contract(&self, address: AztecAddress) -> Result<(), Error>;

    /// Wait until the block containing a transaction is proven on L1.
    async fn wait_for_tx_proven(&self, tx_hash: TxHash) -> Result<(), Error>;

    /// Create an authorization witness.
    async fn create_auth_wit(
        &self,
        from: AztecAddress,
        message_hash_or_intent: MessageHashOrIntent,
    ) -> Result<AuthWitness, Error>;
}

// ---------------------------------------------------------------------------
// MockWallet
// ---------------------------------------------------------------------------

fn lock_mutex<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, Error> {
    mutex
        .lock()
        .map_err(|e| Error::InvalidData(format!("mutex poisoned: {e}")))
}

/// In-memory test wallet implementation.
///
/// Provides configurable behavior for testing contract interactions
/// without a real wallet backend. Simulation and send calls return
/// configurable default results.
pub struct MockWallet {
    chain_info: ChainInfo,
    accounts: Mutex<Vec<Aliased<AztecAddress>>>,
    address_book: Mutex<Vec<Aliased<AztecAddress>>>,
    contracts: Mutex<HashMap<AztecAddress, ContractMetadata>>,
    contract_classes: Mutex<HashMap<Fr, ContractClassMetadata>>,
    simulate_result: TxSimulationResult,
    send_result: SendResult,
}

impl MockWallet {
    /// Create a new mock wallet with the given chain info and sensible defaults.
    pub fn new(chain_info: ChainInfo) -> Self {
        Self {
            chain_info,
            accounts: Mutex::new(vec![]),
            address_book: Mutex::new(vec![]),
            contracts: Mutex::new(HashMap::new()),
            contract_classes: Mutex::new(HashMap::new()),
            simulate_result: TxSimulationResult {
                return_values: serde_json::Value::Null,
                gas_used: None,
            },
            send_result: SendResult {
                tx_hash: TxHash::zero(),
            },
        }
    }

    /// Set the default result returned by `simulate_tx`.
    #[must_use]
    pub fn with_simulate_result(mut self, result: TxSimulationResult) -> Self {
        self.simulate_result = result;
        self
    }

    /// Set the default result returned by `send_tx`.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn with_send_result(mut self, result: SendResult) -> Self {
        self.send_result = result;
        self
    }

    /// Add an account to the mock wallet.
    pub fn add_account(&self, address: AztecAddress, alias: Option<String>) -> Result<(), Error> {
        lock_mutex(&self.accounts)?.push(Aliased {
            alias: alias.unwrap_or_default(),
            item: address,
        });
        Ok(())
    }

    /// Register a contract class in the mock wallet.
    pub fn register_contract_class(
        &self,
        class_id: Fr,
        metadata: ContractClassMetadata,
    ) -> Result<(), Error> {
        lock_mutex(&self.contract_classes)?.insert(class_id, metadata);
        Ok(())
    }
}

#[async_trait]
impl Wallet for MockWallet {
    async fn get_chain_info(&self) -> Result<ChainInfo, Error> {
        Ok(self.chain_info.clone())
    }

    async fn get_accounts(&self) -> Result<Vec<Aliased<AztecAddress>>, Error> {
        Ok(lock_mutex(&self.accounts)?.clone())
    }

    async fn get_address_book(&self) -> Result<Vec<Aliased<AztecAddress>>, Error> {
        Ok(lock_mutex(&self.address_book)?.clone())
    }

    async fn register_sender(
        &self,
        address: AztecAddress,
        alias: Option<String>,
    ) -> Result<AztecAddress, Error> {
        lock_mutex(&self.address_book)?.push(Aliased {
            alias: alias.unwrap_or_default(),
            item: address,
        });
        Ok(address)
    }

    async fn get_contract_metadata(
        &self,
        address: AztecAddress,
    ) -> Result<ContractMetadata, Error> {
        lock_mutex(&self.contracts)?
            .get(&address)
            .cloned()
            .ok_or_else(|| Error::InvalidData(format!("contract not registered: {address}")))
    }

    async fn get_contract_class_metadata(
        &self,
        class_id: Fr,
    ) -> Result<ContractClassMetadata, Error> {
        lock_mutex(&self.contract_classes)?
            .get(&class_id)
            .cloned()
            .ok_or_else(|| Error::InvalidData(format!("contract class not registered: {class_id}")))
    }

    async fn register_contract(
        &self,
        instance: ContractInstanceWithAddress,
        artifact: Option<ContractArtifact>,
        _secret_key: Option<Fr>,
    ) -> Result<ContractInstanceWithAddress, Error> {
        let metadata = ContractMetadata {
            instance: Some(instance.clone()),
            is_contract_initialized: false,
            is_contract_published: false,
            is_contract_updated: false,
            updated_contract_class_id: None,
        };
        lock_mutex(&self.contracts)?.insert(instance.address, metadata);

        if artifact.is_some() {
            lock_mutex(&self.contract_classes)?
                .entry(instance.inner.current_contract_class_id)
                .or_insert(ContractClassMetadata {
                    is_artifact_registered: true,
                    is_contract_class_publicly_registered: false,
                });
        }

        Ok(instance)
    }

    async fn get_private_events(
        &self,
        _event_metadata: &EventMetadataDefinition,
        _filter: PrivateEventFilter,
    ) -> Result<Vec<PrivateEvent>, Error> {
        Ok(vec![])
    }

    async fn simulate_tx(
        &self,
        _exec: ExecutionPayload,
        _opts: SimulateOptions,
    ) -> Result<TxSimulationResult, Error> {
        Ok(self.simulate_result.clone())
    }

    async fn execute_utility(
        &self,
        _call: FunctionCall,
        _opts: ExecuteUtilityOptions,
    ) -> Result<UtilityExecutionResult, Error> {
        Ok(UtilityExecutionResult {
            result: serde_json::Value::Null,
            stats: None,
        })
    }

    async fn profile_tx(
        &self,
        _exec: ExecutionPayload,
        _opts: ProfileOptions,
    ) -> Result<TxProfileResult, Error> {
        Ok(TxProfileResult {
            return_values: serde_json::Value::Null,
            gas_used: None,
            profile_data: serde_json::Value::Null,
        })
    }

    async fn send_tx(
        &self,
        _exec: ExecutionPayload,
        _opts: SendOptions,
    ) -> Result<SendResult, Error> {
        Ok(self.send_result.clone())
    }

    async fn wait_for_contract(&self, _address: AztecAddress) -> Result<(), Error> {
        Ok(())
    }

    async fn wait_for_tx_proven(&self, _tx_hash: TxHash) -> Result<(), Error> {
        Ok(())
    }

    async fn create_auth_wit(
        &self,
        _from: AztecAddress,
        _message_hash_or_intent: MessageHashOrIntent,
    ) -> Result<AuthWitness, Error> {
        Ok(AuthWitness::default())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::abi::{AbiParameter, FunctionSelector, FunctionType};
    use crate::types::{ContractInstance, PublicKeys};

    fn sample_chain_info() -> ChainInfo {
        ChainInfo {
            chain_id: Fr::from(31337u64),
            version: Fr::from(1u64),
        }
    }

    fn sample_instance() -> ContractInstanceWithAddress {
        ContractInstanceWithAddress {
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
        }
    }

    // -- Trait object safety --

    #[test]
    fn wallet_is_object_safe() {
        fn _assert_object_safe(_: &dyn Wallet) {}
    }

    // -- Send + Sync --

    #[test]
    fn mock_wallet_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockWallet>();
    }

    // -- Supporting type serde --

    #[test]
    fn chain_info_roundtrip() {
        let info = sample_chain_info();
        let json = serde_json::to_string(&info).expect("serialize ChainInfo");
        let decoded: ChainInfo = serde_json::from_str(&json).expect("deserialize ChainInfo");
        assert_eq!(decoded, info);
    }

    #[test]
    fn aliased_with_alias_roundtrip() {
        let aliased = Aliased {
            alias: "alice".to_owned(),
            item: AztecAddress(Fr::from(1u64)),
        };
        let json = serde_json::to_string(&aliased).expect("serialize");
        let decoded: Aliased<AztecAddress> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, aliased);
    }

    #[test]
    fn aliased_without_alias_roundtrip() {
        let aliased: Aliased<AztecAddress> = Aliased {
            alias: String::new(),
            item: AztecAddress(Fr::from(1u64)),
        };
        let json = serde_json::to_string(&aliased).expect("serialize");
        let decoded: Aliased<AztecAddress> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, aliased);
    }

    #[test]
    fn simulate_options_default() {
        let opts = SimulateOptions::default();
        assert_eq!(opts.from, AztecAddress(Fr::zero()));
        assert!(!opts.skip_validation);
        assert!(opts.skip_fee_enforcement);
        assert!(opts.gas_settings.is_none());
        assert!(opts.auth_witnesses.is_empty());
        assert!(opts.capsules.is_empty());
        assert!(opts.additional_scopes.is_empty());
    }

    #[test]
    fn send_options_default() {
        let opts = SendOptions::default();
        assert_eq!(opts.from, AztecAddress(Fr::zero()));
        assert!(opts.gas_settings.is_none());
        assert!(opts.auth_witnesses.is_empty());
        assert!(opts.capsules.is_empty());
        assert!(opts.additional_scopes.is_empty());
    }

    #[test]
    fn profile_options_default() {
        let opts = ProfileOptions::default();
        assert_eq!(opts.from, AztecAddress(Fr::zero()));
        assert!(opts.profile_mode.is_none());
        assert!(opts.skip_proof_generation);
        assert!(opts.gas_settings.is_none());
    }

    #[test]
    fn execute_utility_options_default() {
        let opts = ExecuteUtilityOptions::default();
        assert_eq!(opts.scope, AztecAddress(Fr::zero()));
        assert!(opts.auth_witnesses.is_empty());
    }

    #[test]
    fn send_result_roundtrip() {
        let result = SendResult {
            tx_hash: TxHash::zero(),
        };
        let json = serde_json::to_string(&result).expect("serialize");
        let decoded: SendResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, result);
    }

    #[test]
    fn private_event_filter_default() {
        let filter = PrivateEventFilter::default();
        assert_eq!(filter.contract_address, AztecAddress(Fr::zero()));
        assert!(filter.scopes.is_empty());
        assert!(filter.tx_hash.is_none());
        assert!(filter.from_block.is_none());
        assert!(filter.to_block.is_none());
        assert!(filter.after_log.is_none());
    }

    #[test]
    fn message_hash_roundtrip() {
        let msg = MessageHashOrIntent::Hash {
            hash: Fr::from(42u64),
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: MessageHashOrIntent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn message_intent_roundtrip() {
        let msg = MessageHashOrIntent::Intent {
            caller: AztecAddress(Fr::from(1u64)),
            call: FunctionCall {
                to: AztecAddress(Fr::from(2u64)),
                selector: FunctionSelector::from_hex("0xaabbccdd").expect("valid selector"),
                args: vec![],
                function_type: FunctionType::Private,
                is_static: false,
                hide_msg_sender: false,
            },
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: MessageHashOrIntent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, msg);
    }

    #[test]
    fn contract_metadata_roundtrip() {
        let meta = ContractMetadata {
            instance: Some(sample_instance()),
            is_contract_initialized: false,
            is_contract_published: false,
            is_contract_updated: false,
            updated_contract_class_id: None,
        };
        let json = serde_json::to_string(&meta).expect("serialize");
        let decoded: ContractMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, meta);
    }

    #[test]
    fn contract_class_metadata_roundtrip() {
        let meta = ContractClassMetadata {
            is_artifact_registered: true,
            is_contract_class_publicly_registered: true,
        };
        let json = serde_json::to_string(&meta).expect("serialize");
        let decoded: ContractClassMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, meta);
    }

    #[test]
    fn event_metadata_definition_roundtrip() {
        let def = EventMetadataDefinition {
            event_selector: EventSelector(Fr::from(1u64)),
            abi_type: AbiType::Struct {
                name: "Transfer".to_owned(),
                fields: vec![
                    AbiParameter {
                        name: "amount".to_owned(),
                        typ: AbiType::Field,
                        visibility: None,
                    },
                    AbiParameter {
                        name: "sender".to_owned(),
                        typ: AbiType::Field,
                        visibility: None,
                    },
                ],
            },
            field_names: vec!["amount".to_owned(), "sender".to_owned()],
        };
        let json = serde_json::to_string(&def).expect("serialize");
        let decoded: EventMetadataDefinition = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, def);
    }

    // -- MockWallet: chain info --

    #[tokio::test]
    async fn mock_wallet_get_chain_info() {
        let wallet = MockWallet::new(sample_chain_info());
        let info = wallet.get_chain_info().await.expect("get chain info");
        assert_eq!(info.chain_id, Fr::from(31337u64));
        assert_eq!(info.version, Fr::from(1u64));
    }

    // -- MockWallet: accounts --

    #[tokio::test]
    async fn mock_wallet_accounts_initially_empty() {
        let wallet = MockWallet::new(sample_chain_info());
        let accounts = wallet.get_accounts().await.expect("get accounts");
        assert!(accounts.is_empty());
    }

    #[tokio::test]
    async fn mock_wallet_add_and_get_accounts() {
        let wallet = MockWallet::new(sample_chain_info());
        wallet
            .add_account(AztecAddress(Fr::from(1u64)), Some("alice".into()))
            .expect("add account");
        wallet
            .add_account(AztecAddress(Fr::from(2u64)), None)
            .expect("add account");

        let accounts = wallet.get_accounts().await.expect("get accounts");
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].alias, "alice");
        assert!(accounts[1].alias.is_empty());
    }

    // -- MockWallet: address book --

    #[tokio::test]
    async fn mock_wallet_register_sender() {
        let wallet = MockWallet::new(sample_chain_info());
        let addr = AztecAddress(Fr::from(99u64));
        let result = wallet
            .register_sender(addr, Some("bob".into()))
            .await
            .expect("register sender");
        assert_eq!(result, addr);

        let book = wallet.get_address_book().await.expect("get address book");
        assert_eq!(book.len(), 1);
        assert_eq!(book[0].item, addr);
        assert_eq!(book[0].alias, "bob");
    }

    // -- MockWallet: contract registration --

    #[tokio::test]
    async fn mock_wallet_register_and_get_contract() {
        let wallet = MockWallet::new(sample_chain_info());
        let instance = sample_instance();

        let registered = wallet
            .register_contract(instance.clone(), None, None)
            .await
            .expect("register contract");
        assert_eq!(registered.address, instance.address);

        let metadata = wallet
            .get_contract_metadata(instance.address)
            .await
            .expect("get contract metadata");
        assert_eq!(
            metadata.instance.expect("registered instance").address,
            instance.address
        );
        assert!(!metadata.is_contract_initialized);
        assert!(!metadata.is_contract_published);
        assert!(!metadata.is_contract_updated);
        assert!(metadata.updated_contract_class_id.is_none());
    }

    #[tokio::test]
    async fn mock_wallet_unregistered_contract_fails() {
        let wallet = MockWallet::new(sample_chain_info());
        let result = wallet
            .get_contract_metadata(AztecAddress(Fr::from(999u64)))
            .await;
        assert!(result.is_err());
    }

    // -- MockWallet: contract class --

    #[tokio::test]
    async fn mock_wallet_contract_class_metadata() {
        let wallet = MockWallet::new(sample_chain_info());
        let class_id = Fr::from(100u64);

        wallet
            .register_contract_class(
                class_id,
                ContractClassMetadata {
                    is_artifact_registered: true,
                    is_contract_class_publicly_registered: true,
                },
            )
            .expect("register class");

        let meta = wallet
            .get_contract_class_metadata(class_id)
            .await
            .expect("get class metadata");
        assert!(meta.is_artifact_registered);
        assert!(meta.is_contract_class_publicly_registered);
    }

    #[tokio::test]
    async fn mock_wallet_unregistered_class_fails() {
        let wallet = MockWallet::new(sample_chain_info());
        let result = wallet.get_contract_class_metadata(Fr::from(999u64)).await;
        assert!(result.is_err());
    }

    // -- MockWallet: simulate --

    #[tokio::test]
    async fn mock_wallet_simulate_default() {
        let wallet = MockWallet::new(sample_chain_info());
        let result = wallet
            .simulate_tx(ExecutionPayload::default(), SimulateOptions::default())
            .await
            .expect("simulate tx");
        assert_eq!(result.return_values, serde_json::Value::Null);
        assert!(result.gas_used.is_none());
    }

    #[tokio::test]
    async fn mock_wallet_simulate_custom_result() {
        let wallet =
            MockWallet::new(sample_chain_info()).with_simulate_result(TxSimulationResult {
                return_values: serde_json::json!([42]),
                gas_used: Some(Gas {
                    da_gas: 100,
                    l2_gas: 200,
                }),
            });

        let result = wallet
            .simulate_tx(ExecutionPayload::default(), SimulateOptions::default())
            .await
            .expect("simulate tx");
        assert_eq!(result.return_values, serde_json::json!([42]));
        assert_eq!(result.gas_used.as_ref().map(|g| g.l2_gas), Some(200));
    }

    // -- MockWallet: send --

    #[tokio::test]
    async fn mock_wallet_send_default() {
        let wallet = MockWallet::new(sample_chain_info());
        let result = wallet
            .send_tx(ExecutionPayload::default(), SendOptions::default())
            .await
            .expect("send tx");
        assert_eq!(result.tx_hash, TxHash::zero());
    }

    #[tokio::test]
    async fn mock_wallet_send_custom_result() {
        let tx_hash =
            TxHash::from_hex("0x00000000000000000000000000000000000000000000000000000000deadbeef")
                .expect("valid hex");
        let wallet = MockWallet::new(sample_chain_info()).with_send_result(SendResult { tx_hash });

        let result = wallet
            .send_tx(ExecutionPayload::default(), SendOptions::default())
            .await
            .expect("send tx");
        assert_eq!(result.tx_hash, tx_hash);
    }

    // -- MockWallet: utility execution --

    #[tokio::test]
    async fn mock_wallet_execute_utility() {
        let wallet = MockWallet::new(sample_chain_info());
        let call = FunctionCall {
            to: AztecAddress(Fr::from(1u64)),
            selector: FunctionSelector::from_hex("0xaabbccdd").expect("valid selector"),
            args: vec![],
            function_type: FunctionType::Utility,
            is_static: true,
            hide_msg_sender: false,
        };
        let result = wallet
            .execute_utility(call, ExecuteUtilityOptions::default())
            .await
            .expect("execute utility");
        assert_eq!(result.result, serde_json::Value::Null);
        assert!(result.stats.is_none());
    }

    // -- MockWallet: profile --

    #[tokio::test]
    async fn mock_wallet_profile_tx() {
        let wallet = MockWallet::new(sample_chain_info());
        let result = wallet
            .profile_tx(ExecutionPayload::default(), ProfileOptions::default())
            .await
            .expect("profile tx");
        assert_eq!(result.return_values, serde_json::Value::Null);
        assert!(result.gas_used.is_none());
    }

    // -- MockWallet: private events --

    #[tokio::test]
    async fn mock_wallet_private_events_empty() {
        let wallet = MockWallet::new(sample_chain_info());
        let events = wallet
            .get_private_events(
                &EventMetadataDefinition {
                    event_selector: EventSelector(Fr::from(1u64)),
                    abi_type: AbiType::Struct {
                        name: "AmountOnly".to_owned(),
                        fields: vec![AbiParameter {
                            name: "amount".to_owned(),
                            typ: AbiType::Field,
                            visibility: None,
                        }],
                    },
                    field_names: vec!["amount".to_owned()],
                },
                PrivateEventFilter {
                    contract_address: AztecAddress(Fr::from(1u64)),
                    scopes: vec![AztecAddress(Fr::from(2u64))],
                    ..PrivateEventFilter::default()
                },
            )
            .await
            .expect("get private events");
        assert!(events.is_empty());
    }

    // -- MockWallet: auth witness --

    #[tokio::test]
    async fn mock_wallet_create_auth_wit() {
        let wallet = MockWallet::new(sample_chain_info());
        let wit = wallet
            .create_auth_wit(
                AztecAddress(Fr::from(1u64)),
                MessageHashOrIntent::Hash {
                    hash: Fr::from(42u64),
                },
            )
            .await
            .expect("create auth wit");
        assert!(wit.fields.is_empty());
    }

    // -- ProfileMode serialization --

    #[test]
    fn profile_mode_serialization() {
        for mode in [
            ProfileMode::Gates,
            ProfileMode::ExecutionSteps,
            ProfileMode::Full,
        ] {
            let json = serde_json::to_string(&mode).expect("serialize");
            let decoded: ProfileMode = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(decoded, mode);
        }
        assert_eq!(
            serde_json::to_string(&ProfileMode::Gates).unwrap(),
            "\"gates\""
        );
        assert_eq!(
            serde_json::to_string(&ProfileMode::ExecutionSteps).unwrap(),
            "\"execution-steps\""
        );
        assert_eq!(
            serde_json::to_string(&ProfileMode::Full).unwrap(),
            "\"full\""
        );
    }
}
