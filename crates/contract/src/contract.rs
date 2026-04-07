use crate::abi::{AbiValue, ContractArtifact};
use crate::error::Error;
use crate::tx::{ExecutionPayload, FunctionCall};
use crate::types::AztecAddress;
use crate::wallet::{SendOptions, SendResult, SimulateOptions, TxSimulationResult, Wallet};

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
        };
        Ok(ContractFunctionInteraction {
            wallet: &self.wallet,
            call,
            capsules: vec![],
        })
    }
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
    capsules: Vec<crate::tx::Capsule>,
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
        }
    }

    /// Create a new interaction with capsules attached.
    pub fn new_with_capsules(
        wallet: &'a W,
        call: FunctionCall,
        capsules: Vec<crate::tx::Capsule>,
    ) -> Self {
        Self {
            wallet,
            call,
            capsules,
        }
    }

    /// Build an [`ExecutionPayload`] containing this single call.
    pub fn request(&self) -> Result<ExecutionPayload, Error> {
        Ok(ExecutionPayload {
            calls: vec![self.call.clone()],
            capsules: self.capsules.clone(),
            ..ExecutionPayload::default()
        })
    }

    /// Simulate the call without sending it.
    pub async fn simulate(&self, opts: SimulateOptions) -> Result<TxSimulationResult, Error> {
        self.wallet.simulate_tx(self.request()?, opts).await
    }

    /// Send the call as a transaction.
    pub async fn send(&self, opts: SendOptions) -> Result<SendResult, Error> {
        self.wallet.send_tx(self.request()?, opts).await
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
        self.wallet.simulate_tx(self.request()?, opts).await
    }

    /// Send the batch as a single transaction.
    pub async fn send(&self, opts: SendOptions) -> Result<SendResult, Error> {
        self.wallet.send_tx(self.request()?, opts).await
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
            extra_hashed_args: vec![HashedValues {
                values: vec![Fr::from(20u64)],
            }],
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
}
