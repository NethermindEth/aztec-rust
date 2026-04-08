use crate::abi::{AbiValue, ContractArtifact};
use crate::deployment::{ContractDeployer, DeployMethod};
use crate::error::Error;
use crate::tx::{AuthWitness, Capsule, ExecutionPayload, FunctionCall, HashedValues};
use crate::types::{AztecAddress, PublicKeys};
use crate::wallet::{
    ProfileOptions, SendOptions, SendResult, SimulateOptions, TxProfileResult, TxSimulationResult,
    Wallet,
};

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

/// A handle to a deployed contract at a specific address.
///
/// Provides dynamic method lookup and call construction driven by the
/// contract artifact (ABI). Use [`Contract::at`] to create a handle.
pub struct Contract<W> {
    /// The deployed contract's address.
    pub address: AztecAddress,
    /// The contract's ABI artifact.
    pub artifact: ContractArtifact,
    wallet: W,
}

impl<W: Wallet> Contract<W> {
    /// Create a contract handle at the given address.
    pub const fn at(address: AztecAddress, artifact: ContractArtifact, wallet: W) -> Self {
        Self {
            address,
            artifact,
            wallet,
        }
    }

    /// Create a deployment interaction for the given artifact and constructor args.
    ///
    /// Uses default (empty) public keys. For custom public keys, use
    /// [`deploy_with_public_keys`](Self::deploy_with_public_keys).
    pub fn deploy<'a>(
        wallet: &'a W,
        artifact: ContractArtifact,
        args: Vec<AbiValue>,
        constructor_name: Option<&str>,
    ) -> Result<DeployMethod<'a, W>, Error> {
        let mut deployer = ContractDeployer::new(artifact, wallet);
        if let Some(name) = constructor_name {
            deployer = deployer.with_constructor_name(name);
        }
        deployer.deploy(args)
    }

    /// Create a deployment interaction with custom public keys.
    pub fn deploy_with_public_keys<'a>(
        public_keys: PublicKeys,
        wallet: &'a W,
        artifact: ContractArtifact,
        args: Vec<AbiValue>,
        constructor_name: Option<&str>,
    ) -> Result<DeployMethod<'a, W>, Error> {
        let mut deployer = ContractDeployer::new(artifact, wallet).with_public_keys(public_keys);
        if let Some(name) = constructor_name {
            deployer = deployer.with_constructor_name(name);
        }
        deployer.deploy(args)
    }

    /// Return a new Contract handle using a different wallet.
    pub fn with_wallet<W2: Wallet>(self, wallet: W2) -> Contract<W2> {
        Contract {
            address: self.address,
            artifact: self.artifact,
            wallet,
        }
    }

    /// Look up a function by name and build a call interaction.
    ///
    /// The function's type (`Private`, `Public`, `Utility`) and `is_static`
    /// flag are taken from the artifact metadata. The selector must be present
    /// in the artifact; if missing, an error is returned.
    pub fn method(
        &self,
        name: &str,
        args: Vec<AbiValue>,
    ) -> Result<ContractFunctionInteraction<'_, W>, Error> {
        let func = self.artifact.find_function(name)?;
        let expected = func.parameters.len();
        let got = args.len();
        if got != expected {
            return Err(Error::Abi(format!(
                "function '{name}' expects {expected} argument(s), got {got}"
            )));
        }
        let selector = func.selector.ok_or_else(|| {
            Error::Abi(format!(
                "function '{}' in artifact '{}' has no selector",
                name, self.artifact.name
            ))
        })?;
        let call = FunctionCall {
            to: self.address,
            selector,
            args,
            function_type: func.function_type.clone(),
            is_static: func.is_static,
            hide_msg_sender: false,
        };
        Ok(ContractFunctionInteraction {
            wallet: &self.wallet,
            call,
            capsules: vec![],
            auth_witnesses: vec![],
            extra_hashed_args: vec![],
        })
    }
}

// ---------------------------------------------------------------------------
// Fee payload merging
// ---------------------------------------------------------------------------

/// Merge an optional fee execution payload into the main payload.
pub(crate) fn merge_fee_payload(
    mut payload: ExecutionPayload,
    fee: &Option<ExecutionPayload>,
) -> Result<ExecutionPayload, Error> {
    if let Some(fee_payload) = fee {
        payload.calls.extend(fee_payload.calls.clone());
        payload
            .auth_witnesses
            .extend(fee_payload.auth_witnesses.clone());
        payload.capsules.extend(fee_payload.capsules.clone());
        payload
            .extra_hashed_args
            .extend(fee_payload.extra_hashed_args.clone());
        if let Some(payer) = fee_payload.fee_payer {
            payload.fee_payer = Some(payer);
        }
    }
    Ok(payload)
}

// ---------------------------------------------------------------------------
// ContractFunctionInteraction
// ---------------------------------------------------------------------------

/// A pending interaction with a single contract function.
///
/// Created by [`Contract::method`]. Use [`request`](Self::request) to get the
/// raw execution payload, [`simulate`](Self::simulate) to dry-run, or
/// [`send`](Self::send) to submit to the network.
pub struct ContractFunctionInteraction<'a, W> {
    wallet: &'a W,
    call: FunctionCall,
    capsules: Vec<Capsule>,
    auth_witnesses: Vec<AuthWitness>,
    extra_hashed_args: Vec<HashedValues>,
}

impl<W> std::fmt::Debug for ContractFunctionInteraction<'_, W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContractFunctionInteraction")
            .field("call", &self.call)
            .finish_non_exhaustive()
    }
}

impl<'a, W: Wallet> ContractFunctionInteraction<'a, W> {
    /// Create a new interaction for a single function call.
    pub fn new(wallet: &'a W, call: FunctionCall) -> Self {
        Self {
            wallet,
            call,
            capsules: vec![],
            auth_witnesses: vec![],
            extra_hashed_args: vec![],
        }
    }

    /// Create a new interaction with capsules attached.
    pub fn new_with_capsules(wallet: &'a W, call: FunctionCall, capsules: Vec<Capsule>) -> Self {
        Self {
            wallet,
            call,
            capsules,
            auth_witnesses: vec![],
            extra_hashed_args: vec![],
        }
    }

    /// Return a new interaction with additional auth witnesses and capsules.
    pub fn with(mut self, auth_witnesses: Vec<AuthWitness>, capsules: Vec<Capsule>) -> Self {
        self.auth_witnesses.extend(auth_witnesses);
        self.capsules.extend(capsules);
        self
    }

    /// Returns the underlying [`FunctionCall`] for use in authwit hash computation.
    pub fn get_function_call(&self) -> &FunctionCall {
        &self.call
    }

    /// Build an [`ExecutionPayload`] containing this single call.
    pub fn request(&self) -> Result<ExecutionPayload, Error> {
        Ok(ExecutionPayload {
            calls: vec![self.call.clone()],
            capsules: self.capsules.clone(),
            auth_witnesses: self.auth_witnesses.clone(),
            extra_hashed_args: self.extra_hashed_args.clone(),
            ..ExecutionPayload::default()
        })
    }

    /// Simulate the call without sending it.
    pub async fn simulate(&self, opts: SimulateOptions) -> Result<TxSimulationResult, Error> {
        let payload = merge_fee_payload(self.request()?, &opts.fee_execution_payload)?;
        self.wallet.simulate_tx(payload, opts).await
    }

    /// Profile the gate count / execution steps for this call.
    pub async fn profile(&self, opts: ProfileOptions) -> Result<TxProfileResult, Error> {
        let payload = merge_fee_payload(self.request()?, &opts.fee_execution_payload)?;
        self.wallet.profile_tx(payload, opts).await
    }

    /// Send the call as a transaction.
    pub async fn send(&self, opts: SendOptions) -> Result<SendResult, Error> {
        let payload = merge_fee_payload(self.request()?, &opts.fee_execution_payload)?;
        self.wallet.send_tx(payload, opts).await
    }
}

// ---------------------------------------------------------------------------
// BatchCall
// ---------------------------------------------------------------------------

/// A batch of interactions aggregated into a single transaction.
///
/// Merges multiple [`ExecutionPayload`]s into one, preserving all calls,
/// auth witnesses, capsules, and extra hashed args from each payload.
/// The fee payer is taken from the last payload that specifies one.
pub struct BatchCall<'a, W> {
    wallet: &'a W,
    payloads: Vec<ExecutionPayload>,
}

impl<W> std::fmt::Debug for BatchCall<'_, W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BatchCall")
            .field("payload_count", &self.payloads.len())
            .finish_non_exhaustive()
    }
}

impl<'a, W: Wallet> BatchCall<'a, W> {
    /// Create a new batch from a list of execution payloads.
    pub const fn new(wallet: &'a W, payloads: Vec<ExecutionPayload>) -> Self {
        Self { wallet, payloads }
    }

    /// Merge all payloads into a single [`ExecutionPayload`].
    pub fn request(&self) -> Result<ExecutionPayload, Error> {
        let mut merged = ExecutionPayload::default();

        for payload in &self.payloads {
            merged.calls.extend(payload.calls.clone());
            merged.auth_witnesses.extend(payload.auth_witnesses.clone());
            merged.capsules.extend(payload.capsules.clone());
            merged
                .extra_hashed_args
                .extend(payload.extra_hashed_args.clone());

            if payload.fee_payer.is_some() {
                merged.fee_payer = payload.fee_payer;
            }
        }

        Ok(merged)
    }

    /// Simulate the batch without sending.
    pub async fn simulate(&self, opts: SimulateOptions) -> Result<TxSimulationResult, Error> {
        let payload = merge_fee_payload(self.request()?, &opts.fee_execution_payload)?;
        self.wallet.simulate_tx(payload, opts).await
    }

    /// Profile the batch as a single transaction.
    pub async fn profile(&self, opts: ProfileOptions) -> Result<TxProfileResult, Error> {
        let payload = merge_fee_payload(self.request()?, &opts.fee_execution_payload)?;
        self.wallet.profile_tx(payload, opts).await
    }

    /// Send the batch as a single transaction.
    pub async fn send(&self, opts: SendOptions) -> Result<SendResult, Error> {
        let payload = merge_fee_payload(self.request()?, &opts.fee_execution_payload)?;
        self.wallet.send_tx(payload, opts).await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::abi::{AbiValue, FunctionType};
    use crate::fee::Gas;
    use crate::tx::TxHash;
    use crate::types::Fr;
    use crate::wallet::{ChainInfo, MockWallet, SendResult, TxSimulationResult};

    const TOKEN_ARTIFACT: &str = r#"
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
            { "name": "to", "type": { "kind": "field" } },
            { "name": "amount", "type": { "kind": "integer", "sign": "unsigned", "width": 64 } }
          ],
          "return_types": [],
          "selector": "0xd6f42325"
        },
        {
          "name": "balance_of",
          "function_type": "utility",
          "is_initializer": false,
          "is_static": true,
          "parameters": [
            { "name": "owner", "type": { "kind": "field" } }
          ],
          "return_types": [
            { "kind": "integer", "sign": "unsigned", "width": 64 }
          ],
          "selector": "0x12345678"
        },
        {
          "name": "total_supply",
          "function_type": "public",
          "is_initializer": false,
          "is_static": true,
          "parameters": [],
          "return_types": [
            { "kind": "integer", "sign": "unsigned", "width": 64 }
          ],
          "selector": "0xabcdef01"
        }
      ]
    }
    "#;

    const NO_SELECTOR_ARTIFACT: &str = r#"
    {
      "name": "NoSelector",
      "functions": [
        {
          "name": "foo",
          "function_type": "public",
          "is_initializer": false,
          "is_static": false,
          "parameters": [],
          "return_types": []
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

    fn sample_address() -> AztecAddress {
        AztecAddress(Fr::from(42u64))
    }

    fn load_token_artifact() -> ContractArtifact {
        ContractArtifact::from_json(TOKEN_ARTIFACT).expect("parse token artifact")
    }

    // -- Contract::at --

    #[test]
    fn contract_at_creates_handle() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_token_artifact();
        let addr = sample_address();

        let contract = Contract::at(addr, artifact, wallet);
        assert_eq!(contract.address, addr);
        assert_eq!(contract.artifact.name, "TokenContract");
    }

    // -- Contract::method --

    #[test]
    fn method_finds_function_and_builds_call() {
        let wallet = MockWallet::new(sample_chain_info());
        let contract = Contract::at(sample_address(), load_token_artifact(), wallet);

        let interaction = contract
            .method(
                "transfer",
                vec![
                    AbiValue::Field(Fr::from(1u64)),
                    AbiValue::Field(Fr::from(2u64)),
                    AbiValue::Integer(100),
                ],
            )
            .expect("find transfer");

        assert_eq!(interaction.call.to, sample_address());
        assert_eq!(interaction.call.function_type, FunctionType::Private);
        assert!(!interaction.call.is_static);
        assert_eq!(interaction.call.args.len(), 3);
        assert_eq!(interaction.call.selector.to_string(), "0xd6f42325");
    }

    #[test]
    fn method_preserves_private_type() {
        let wallet = MockWallet::new(sample_chain_info());
        let contract = Contract::at(sample_address(), load_token_artifact(), wallet);

        let interaction = contract
            .method("constructor", vec![AbiValue::Field(Fr::from(1u64))])
            .expect("find constructor");
        assert_eq!(interaction.call.function_type, FunctionType::Private);
        assert!(!interaction.call.is_static);
    }

    #[test]
    fn method_preserves_utility_static() {
        let wallet = MockWallet::new(sample_chain_info());
        let contract = Contract::at(sample_address(), load_token_artifact(), wallet);

        let interaction = contract
            .method("balance_of", vec![AbiValue::Field(Fr::from(1u64))])
            .expect("find balance_of");
        assert_eq!(interaction.call.function_type, FunctionType::Utility);
        assert!(interaction.call.is_static);
    }

    #[test]
    fn method_preserves_public_static() {
        let wallet = MockWallet::new(sample_chain_info());
        let contract = Contract::at(sample_address(), load_token_artifact(), wallet);

        let interaction = contract
            .method("total_supply", vec![])
            .expect("find total_supply");
        assert_eq!(interaction.call.function_type, FunctionType::Public);
        assert!(interaction.call.is_static);
    }

    #[test]
    fn method_not_found_returns_error() {
        let wallet = MockWallet::new(sample_chain_info());
        let contract = Contract::at(sample_address(), load_token_artifact(), wallet);

        let result = contract.method("nonexistent", vec![]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("nonexistent"),
            "error should mention function name: {err}"
        );
    }

    #[test]
    fn method_without_selector_returns_error() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact =
            ContractArtifact::from_json(NO_SELECTOR_ARTIFACT).expect("parse no-selector artifact");
        let contract = Contract::at(sample_address(), artifact, wallet);

        let result = contract.method("foo", vec![]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("no selector"),
            "error should mention missing selector: {err}"
        );
    }

    #[test]
    fn method_argument_count_mismatch_returns_error() {
        let wallet = MockWallet::new(sample_chain_info());
        let contract = Contract::at(sample_address(), load_token_artifact(), wallet);

        let result = contract.method(
            "transfer",
            vec![
                AbiValue::Field(Fr::from(1u64)),
                AbiValue::Field(Fr::from(2u64)),
            ],
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("expects 3 argument(s), got 2"),
            "error should mention argument mismatch: {err}"
        );
    }

    // -- ContractFunctionInteraction::request --

    #[test]
    fn request_wraps_single_call() {
        let wallet = MockWallet::new(sample_chain_info());
        let contract = Contract::at(sample_address(), load_token_artifact(), wallet);

        let interaction = contract
            .method(
                "transfer",
                vec![
                    AbiValue::Field(Fr::from(1u64)),
                    AbiValue::Field(Fr::from(2u64)),
                    AbiValue::Integer(100),
                ],
            )
            .expect("find transfer");
        let payload = interaction.request().expect("build payload");

        assert_eq!(payload.calls.len(), 1);
        assert_eq!(payload.calls[0].to, sample_address());
        assert_eq!(payload.calls[0].selector.to_string(), "0xd6f42325");
        assert!(payload.auth_witnesses.is_empty());
        assert!(payload.capsules.is_empty());
        assert!(payload.extra_hashed_args.is_empty());
        assert!(payload.fee_payer.is_none());
    }

    // -- ContractFunctionInteraction::simulate --

    #[tokio::test]
    async fn simulate_delegates_to_wallet() {
        let wallet =
            MockWallet::new(sample_chain_info()).with_simulate_result(TxSimulationResult {
                return_values: serde_json::json!({"balance": 1000}),
                gas_used: Some(Gas {
                    da_gas: 10,
                    l2_gas: 20,
                }),
            });
        let contract = Contract::at(sample_address(), load_token_artifact(), wallet);

        let result = contract
            .method("balance_of", vec![AbiValue::Field(Fr::from(1u64))])
            .expect("find balance_of")
            .simulate(SimulateOptions::default())
            .await
            .expect("simulate");

        assert_eq!(result.return_values, serde_json::json!({"balance": 1000}));
        assert_eq!(result.gas_used.as_ref().map(|g| g.l2_gas), Some(20));
    }

    // -- ContractFunctionInteraction::send --

    #[tokio::test]
    async fn send_delegates_to_wallet() {
        let tx_hash =
            TxHash::from_hex("0x00000000000000000000000000000000000000000000000000000000deadbeef")
                .expect("valid hex");
        let wallet = MockWallet::new(sample_chain_info()).with_send_result(SendResult { tx_hash });
        let contract = Contract::at(sample_address(), load_token_artifact(), wallet);

        let result = contract
            .method(
                "transfer",
                vec![
                    AbiValue::Field(Fr::from(1u64)),
                    AbiValue::Field(Fr::from(2u64)),
                    AbiValue::Integer(100),
                ],
            )
            .expect("find transfer")
            .send(SendOptions::default())
            .await
            .expect("send");

        assert_eq!(result.tx_hash, tx_hash);
    }

    // -----------------------------------------------------------------------
    // BatchCall tests
    // -----------------------------------------------------------------------

    use crate::abi::FunctionSelector;
    use crate::tx::{AuthWitness, Capsule, HashedValues};

    fn make_call(addr: u64, selector: &str) -> FunctionCall {
        FunctionCall {
            to: AztecAddress(Fr::from(addr)),
            selector: FunctionSelector::from_hex(selector).expect("valid selector"),
            args: vec![AbiValue::Field(Fr::from(addr))],
            function_type: FunctionType::Private,
            is_static: false,
            hide_msg_sender: false,
        }
    }

    fn make_payload(addr: u64, selector: &str) -> ExecutionPayload {
        ExecutionPayload {
            calls: vec![make_call(addr, selector)],
            ..ExecutionPayload::default()
        }
    }

    // -- BatchCall::request --

    #[test]
    fn batch_call_empty() {
        let wallet = MockWallet::new(sample_chain_info());
        let batch = BatchCall::new(&wallet, vec![]);
        let payload = batch.request().expect("empty batch");
        assert!(payload.calls.is_empty());
        assert!(payload.auth_witnesses.is_empty());
        assert!(payload.capsules.is_empty());
        assert!(payload.extra_hashed_args.is_empty());
        assert!(payload.fee_payer.is_none());
    }

    #[test]
    fn batch_call_single_payload() {
        let wallet = MockWallet::new(sample_chain_info());
        let p = make_payload(1, "0xaabbccdd");
        let batch = BatchCall::new(&wallet, vec![p]);
        let payload = batch.request().expect("single payload");
        assert_eq!(payload.calls.len(), 1);
        assert_eq!(payload.calls[0].to, AztecAddress(Fr::from(1u64)));
    }

    #[test]
    fn batch_call_merges_multiple_payloads() {
        let wallet = MockWallet::new(sample_chain_info());
        let p1 = ExecutionPayload {
            calls: vec![make_call(1, "0xaabbccdd")],
            auth_witnesses: vec![AuthWitness {
                fields: vec![Fr::from(10u64)],
                ..Default::default()
            }],
            capsules: vec![Capsule {
                contract_address: AztecAddress(Fr::from(10u64)),
                storage_slot: Fr::from(1u64),
                data: vec![Fr::from(1u64)],
            }],
            extra_hashed_args: vec![HashedValues::from_args(vec![Fr::from(20u64)])],
            fee_payer: None,
        };
        let p2 = ExecutionPayload {
            calls: vec![make_call(2, "0x11223344")],
            auth_witnesses: vec![AuthWitness {
                fields: vec![Fr::from(30u64)],
                ..Default::default()
            }],
            capsules: vec![],
            extra_hashed_args: vec![],
            fee_payer: Some(AztecAddress(Fr::from(99u64))),
        };

        let batch = BatchCall::new(&wallet, vec![p1, p2]);
        let payload = batch.request().expect("merge payloads");

        assert_eq!(payload.calls.len(), 2);
        assert_eq!(payload.calls[0].to, AztecAddress(Fr::from(1u64)));
        assert_eq!(payload.calls[1].to, AztecAddress(Fr::from(2u64)));
        assert_eq!(payload.auth_witnesses.len(), 2);
        assert_eq!(payload.capsules.len(), 1);
        assert_eq!(payload.extra_hashed_args.len(), 1);
        assert_eq!(payload.fee_payer, Some(AztecAddress(Fr::from(99u64))));
    }

    #[test]
    fn batch_call_fee_payer_uses_last_non_none() {
        let wallet = MockWallet::new(sample_chain_info());
        let p1 = ExecutionPayload {
            fee_payer: Some(AztecAddress(Fr::from(1u64))),
            ..ExecutionPayload::default()
        };
        let p2 = ExecutionPayload {
            fee_payer: None,
            ..ExecutionPayload::default()
        };
        let p3 = ExecutionPayload {
            fee_payer: Some(AztecAddress(Fr::from(3u64))),
            ..ExecutionPayload::default()
        };

        let batch = BatchCall::new(&wallet, vec![p1, p2, p3]);
        let payload = batch.request().expect("merge payloads");
        assert_eq!(payload.fee_payer, Some(AztecAddress(Fr::from(3u64))));
    }

    // -- BatchCall::simulate --

    #[tokio::test]
    async fn batch_call_simulate_delegates_to_wallet() {
        let wallet =
            MockWallet::new(sample_chain_info()).with_simulate_result(TxSimulationResult {
                return_values: serde_json::json!({"batch": true}),
                gas_used: Some(Gas {
                    da_gas: 10,
                    l2_gas: 20,
                }),
            });

        let batch = BatchCall::new(
            &wallet,
            vec![make_payload(1, "0xaabbccdd"), make_payload(2, "0x11223344")],
        );

        let result = batch
            .simulate(SimulateOptions::default())
            .await
            .expect("simulate batch");

        assert_eq!(result.return_values, serde_json::json!({"batch": true}));
        assert_eq!(result.gas_used.as_ref().map(|g| g.l2_gas), Some(20));
    }

    // -- BatchCall::send --

    #[tokio::test]
    async fn batch_call_send_delegates_to_wallet() {
        let tx_hash =
            TxHash::from_hex("0x00000000000000000000000000000000000000000000000000000000deadbeef")
                .expect("valid hex");
        let wallet = MockWallet::new(sample_chain_info()).with_send_result(SendResult { tx_hash });

        let batch = BatchCall::new(
            &wallet,
            vec![make_payload(1, "0xaabbccdd"), make_payload(2, "0x11223344")],
        );

        let result = batch
            .send(SendOptions::default())
            .await
            .expect("send batch");

        assert_eq!(result.tx_hash, tx_hash);
    }

    // -- BatchCall debug --

    #[test]
    fn batch_call_debug() {
        let wallet = MockWallet::new(sample_chain_info());
        let batch = BatchCall::new(&wallet, vec![make_payload(1, "0xaabbccdd")]);
        let dbg = format!("{batch:?}");
        assert!(dbg.contains("payload_count: 1"));
    }

    // -- ContractFunctionInteraction::with --

    #[test]
    fn with_adds_capsules_and_auth_witnesses() {
        let wallet = MockWallet::new(sample_chain_info());
        let contract = Contract::at(sample_address(), load_token_artifact(), wallet);

        let aw = AuthWitness {
            fields: vec![Fr::from(99u64)],
            ..Default::default()
        };
        let cap = Capsule {
            contract_address: AztecAddress(Fr::from(10u64)),
            storage_slot: Fr::from(1u64),
            data: vec![Fr::from(42u64)],
        };

        let interaction = contract
            .method("total_supply", vec![])
            .expect("find total_supply")
            .with(vec![aw.clone()], vec![cap.clone()]);

        let payload = interaction.request().expect("build payload");
        assert_eq!(payload.auth_witnesses.len(), 1);
        assert_eq!(payload.auth_witnesses[0].fields, aw.fields);
        assert_eq!(payload.capsules.len(), 1);
        assert_eq!(payload.capsules[0].storage_slot, cap.storage_slot);
    }

    #[test]
    fn get_function_call_returns_call() {
        let wallet = MockWallet::new(sample_chain_info());
        let contract = Contract::at(sample_address(), load_token_artifact(), wallet);

        let interaction = contract
            .method("total_supply", vec![])
            .expect("find total_supply");

        let call = interaction.get_function_call();
        assert_eq!(call.to, sample_address());
        assert_eq!(call.selector.to_string(), "0xabcdef01");
    }

    #[test]
    fn request_includes_auth_witnesses() {
        let wallet = MockWallet::new(sample_chain_info());
        let contract = Contract::at(sample_address(), load_token_artifact(), wallet);

        let aw = AuthWitness {
            fields: vec![Fr::from(1u64), Fr::from(2u64)],
            ..Default::default()
        };

        let interaction = contract
            .method("total_supply", vec![])
            .expect("find total_supply")
            .with(vec![aw], vec![]);

        let payload = interaction.request().expect("build payload");
        assert_eq!(payload.auth_witnesses.len(), 1);
        assert_eq!(payload.auth_witnesses[0].fields.len(), 2);
    }

    // -- profile tests --

    #[tokio::test]
    async fn profile_delegates_to_wallet() {
        let wallet = MockWallet::new(sample_chain_info());
        let contract = Contract::at(sample_address(), load_token_artifact(), wallet);

        let result = contract
            .method("total_supply", vec![])
            .expect("find total_supply")
            .profile(ProfileOptions::default())
            .await
            .expect("profile");

        assert_eq!(result.return_values, serde_json::Value::Null);
    }

    #[tokio::test]
    async fn batch_profile_delegates_to_wallet() {
        let wallet = MockWallet::new(sample_chain_info());

        let batch = BatchCall::new(
            &wallet,
            vec![make_payload(1, "0xaabbccdd"), make_payload(2, "0x11223344")],
        );

        let result = batch
            .profile(ProfileOptions::default())
            .await
            .expect("profile batch");

        assert_eq!(result.return_values, serde_json::Value::Null);
    }

    // -- Fee payload merging --

    #[test]
    fn send_options_with_fee_payload_merged() {
        let fee_payload = ExecutionPayload {
            calls: vec![make_call(99, "0x11111111")],
            fee_payer: Some(AztecAddress(Fr::from(99u64))),
            ..ExecutionPayload::default()
        };
        let main_payload = ExecutionPayload {
            calls: vec![make_call(1, "0xaabbccdd")],
            ..ExecutionPayload::default()
        };
        let merged = merge_fee_payload(main_payload, &Some(fee_payload)).expect("merge");
        assert_eq!(merged.calls.len(), 2);
        assert_eq!(merged.fee_payer, Some(AztecAddress(Fr::from(99u64))));
    }

    #[test]
    fn simulate_options_with_gas_estimation_flags() {
        let opts = SimulateOptions {
            estimate_gas: true,
            estimated_gas_padding: Some(0.1),
            ..SimulateOptions::default()
        };
        assert!(opts.estimate_gas);
        assert_eq!(opts.estimated_gas_padding, Some(0.1));
    }

    // -- Contract::deploy --

    #[test]
    fn contract_deploy_creates_deploy_method() {
        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_token_artifact();
        let result = Contract::deploy(
            &wallet,
            artifact,
            vec![AbiValue::Field(Fr::from(1u64))],
            None,
        );
        assert!(result.is_ok(), "deploy should succeed");
    }

    #[test]
    fn contract_deploy_with_public_keys() {
        use crate::types::PublicKeys;

        let wallet = MockWallet::new(sample_chain_info());
        let artifact = load_token_artifact();
        let keys = PublicKeys::default();
        let result = Contract::deploy_with_public_keys(
            keys,
            &wallet,
            artifact,
            vec![AbiValue::Field(Fr::from(1u64))],
            None,
        );
        assert!(result.is_ok(), "deploy_with_public_keys should succeed");
    }

    #[test]
    fn contract_with_wallet_changes_wallet() {
        let wallet1 = MockWallet::new(sample_chain_info());
        let wallet2 = MockWallet::new(ChainInfo {
            chain_id: Fr::from(999u64),
            version: Fr::from(2u64),
        });
        let addr = sample_address();
        let contract = Contract::at(addr, load_token_artifact(), wallet1);
        let contract2 = contract.with_wallet(wallet2);
        assert_eq!(contract2.address, addr);
        assert_eq!(contract2.artifact.name, "TokenContract");
    }
}
