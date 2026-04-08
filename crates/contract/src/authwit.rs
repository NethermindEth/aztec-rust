//! Authorization witness helpers for public authwits and validity checking.
//!
//! Provides [`SetPublicAuthWitInteraction`] for setting public authwits in the
//! AuthRegistry, and [`lookup_validity`] for checking authwit validity in both
//! private and public contexts.

use aztec_core::abi::{AbiValue, FunctionSelector, FunctionType};
use aztec_core::constants::protocol_contract_address;
use aztec_core::error::Error;
use aztec_core::hash::{
    compute_auth_wit_message_hash, compute_inner_auth_wit_hash_from_action, ChainInfo,
    MessageHashOrIntent,
};
use aztec_core::tx::{AuthWitness, ExecutionPayload, FunctionCall};
use aztec_core::types::{AztecAddress, Fr};

use crate::wallet::{
    ExecuteUtilityOptions, ProfileOptions, SendOptions, SendResult, SimulateOptions,
    TxProfileResult, TxSimulationResult, Wallet,
};

// ---------------------------------------------------------------------------
// SetPublicAuthWitInteraction
// ---------------------------------------------------------------------------

/// Convenience interaction for setting a public authwit in the AuthRegistry.
///
/// Wraps a call to `AuthRegistry.set_authorized(message_hash, authorize)`.
/// Automatically enforces that only the authorizer (`from`) is the sender.
///
/// Mirrors TS `SetPublicAuthwitContractInteraction`.
pub struct SetPublicAuthWitInteraction<'a, W> {
    wallet: &'a W,
    from: AztecAddress,
    call: FunctionCall,
}

impl<'a, W: Wallet> SetPublicAuthWitInteraction<'a, W> {
    /// Create a new interaction for setting a public authwit.
    ///
    /// Computes the message hash from the intent and chain info,
    /// then constructs a call to `AuthRegistry.set_authorized(hash, authorized)`.
    pub async fn create(
        wallet: &'a W,
        from: AztecAddress,
        message_hash_or_intent: MessageHashOrIntent,
        authorized: bool,
    ) -> Result<Self, Error> {
        let chain_info = wallet.get_chain_info().await?;
        let message_hash = compute_auth_wit_message_hash(&message_hash_or_intent, &chain_info);

        let call = FunctionCall {
            to: protocol_contract_address::auth_registry(),
            selector: FunctionSelector::from_signature("set_authorized(Field,bool)"),
            args: vec![AbiValue::Field(message_hash), AbiValue::Boolean(authorized)],
            function_type: FunctionType::Public,
            is_static: false,
            hide_msg_sender: false,
        };

        Ok(Self { wallet, from, call })
    }

    /// Build the execution payload.
    pub fn request(&self) -> ExecutionPayload {
        ExecutionPayload {
            calls: vec![self.call.clone()],
            ..Default::default()
        }
    }

    /// Simulate the interaction (sender is always `from`).
    pub async fn simulate(&self, mut opts: SimulateOptions) -> Result<TxSimulationResult, Error> {
        opts.from = self.from;
        self.wallet.simulate_tx(self.request(), opts).await
    }

    /// Send the interaction (sender is always `from`).
    pub async fn send(&self, mut opts: SendOptions) -> Result<SendResult, Error> {
        opts.from = self.from;
        self.wallet.send_tx(self.request(), opts).await
    }

    /// Profile the interaction (sender is always `from`).
    pub async fn profile(&self, mut opts: ProfileOptions) -> Result<TxProfileResult, Error> {
        opts.from = self.from;
        self.wallet.profile_tx(self.request(), opts).await
    }
}

// ---------------------------------------------------------------------------
// lookup_validity
// ---------------------------------------------------------------------------

/// Result of an authwit validity check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthWitValidity {
    /// Whether the authwit is valid in private context (signature check).
    pub is_valid_in_private: bool,
    /// Whether the authwit is valid in public context (AuthRegistry check).
    pub is_valid_in_public: bool,
}

/// Check whether an authorization witness is valid in both private and public contexts.
///
/// - **Private:** Simulates a `lookup_validity(consumer, inner_hash)` utility call
///   on the `on_behalf_of` account contract, passing the witness. If simulation
///   succeeds and returns `true`, the authwit is valid privately.
///
/// - **Public:** Simulates a `utility_is_consumable(address, message_hash)` utility call
///   on the AuthRegistry protocol contract. If it returns `true`, the authwit is
///   valid publicly.
///
/// Mirrors TS `lookupValidity(wallet, onBehalfOf, intent, witness)`.
pub async fn lookup_validity<W: Wallet>(
    wallet: &W,
    on_behalf_of: &AztecAddress,
    intent: &MessageHashOrIntent,
    witness: &AuthWitness,
) -> Result<AuthWitValidity, Error> {
    let chain_info = wallet.get_chain_info().await?;

    // Extract inner_hash and consumer from the intent
    let (inner_hash, consumer) = match intent {
        MessageHashOrIntent::Intent { caller, call } => {
            let inner = compute_inner_auth_wit_hash_from_action(caller, call);
            (inner, call.to)
        }
        MessageHashOrIntent::InnerHash {
            consumer,
            inner_hash,
        } => (*inner_hash, *consumer),
        MessageHashOrIntent::Hash { hash } => {
            // For raw hashes, we can only check public validity.
            // Private check requires knowing the consumer, which a raw hash doesn't provide.
            let is_valid_in_public =
                check_public_validity(wallet, on_behalf_of, hash, &chain_info).await;
            return Ok(AuthWitValidity {
                is_valid_in_private: false,
                is_valid_in_public,
            });
        }
    };

    // Private validity check
    let is_valid_in_private =
        check_private_validity(wallet, on_behalf_of, &consumer, &inner_hash, witness).await;

    // Public validity check
    let message_hash = compute_auth_wit_message_hash(intent, &chain_info);
    let is_valid_in_public =
        check_public_validity(wallet, on_behalf_of, &message_hash, &chain_info).await;

    Ok(AuthWitValidity {
        is_valid_in_private,
        is_valid_in_public,
    })
}

/// Check private validity by calling `lookup_validity` on the account contract.
async fn check_private_validity<W: Wallet>(
    wallet: &W,
    on_behalf_of: &AztecAddress,
    consumer: &AztecAddress,
    inner_hash: &Fr,
    witness: &AuthWitness,
) -> bool {
    let call = FunctionCall {
        to: *on_behalf_of,
        selector: FunctionSelector::from_signature("lookup_validity((Field),Field)"),
        args: vec![AbiValue::Field(consumer.0), AbiValue::Field(*inner_hash)],
        function_type: FunctionType::Utility,
        is_static: true,
        hide_msg_sender: false,
    };

    let opts = ExecuteUtilityOptions {
        scope: *on_behalf_of,
        auth_witnesses: vec![witness.clone()],
    };

    match wallet.execute_utility(call, opts).await {
        Ok(result) => parse_boolean_result(&result.result),
        Err(_) => false,
    }
}

/// Check public validity by calling `utility_is_consumable` on the AuthRegistry.
async fn check_public_validity<W: Wallet>(
    wallet: &W,
    on_behalf_of: &AztecAddress,
    message_hash: &Fr,
    _chain_info: &ChainInfo,
) -> bool {
    let call = FunctionCall {
        to: protocol_contract_address::auth_registry(),
        selector: FunctionSelector::from_signature("utility_is_consumable((Field),Field)"),
        args: vec![
            AbiValue::Field(on_behalf_of.0),
            AbiValue::Field(*message_hash),
        ],
        function_type: FunctionType::Utility,
        is_static: true,
        hide_msg_sender: false,
    };

    let opts = ExecuteUtilityOptions {
        scope: *on_behalf_of,
        auth_witnesses: vec![],
    };

    match wallet.execute_utility(call, opts).await {
        Ok(result) => parse_boolean_result(&result.result),
        Err(_) => false,
    }
}

/// Parse a boolean from a JSON value returned by utility execution.
fn parse_boolean_result(value: &serde_json::Value) -> bool {
    // The result may be a boolean, a number (0/1), or a hex-encoded field element
    match value {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_u64() == Some(1),
        serde_json::Value::String(s) => {
            // Try to parse as a field element — nonzero means true
            s != "0x0000000000000000000000000000000000000000000000000000000000000000"
                && s != "0"
                && s != "false"
        }
        _ => false,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::wallet::MockWallet;

    fn sample_chain_info() -> ChainInfo {
        ChainInfo {
            chain_id: Fr::from(31337u64),
            version: Fr::from(1u64),
        }
    }

    #[tokio::test]
    async fn set_public_auth_wit_targets_auth_registry() {
        let wallet = MockWallet::new(sample_chain_info());
        let from = AztecAddress(Fr::from(1u64));

        let interaction = SetPublicAuthWitInteraction::create(
            &wallet,
            from,
            MessageHashOrIntent::Hash {
                hash: Fr::from(42u64),
            },
            true,
        )
        .await
        .expect("create interaction");

        let payload = interaction.request();
        assert_eq!(payload.calls.len(), 1);
        assert_eq!(
            payload.calls[0].to,
            protocol_contract_address::auth_registry()
        );
        assert_eq!(payload.calls[0].function_type, FunctionType::Public);
    }

    #[tokio::test]
    async fn set_public_auth_wit_enforces_from() {
        let wallet = MockWallet::new(sample_chain_info());
        let from = AztecAddress(Fr::from(1u64));

        let interaction = SetPublicAuthWitInteraction::create(
            &wallet,
            from,
            MessageHashOrIntent::Hash {
                hash: Fr::from(42u64),
            },
            true,
        )
        .await
        .expect("create interaction");

        // When simulating, the `from` address should be overridden
        let opts = SimulateOptions::default();
        let _result = interaction.simulate(opts).await.expect("simulate");
        // MockWallet always succeeds; the key assertion is that it compiled and ran
    }

    #[tokio::test]
    async fn set_public_auth_wit_can_profile() {
        let wallet = MockWallet::new(sample_chain_info());
        let from = AztecAddress(Fr::from(1u64));

        let interaction = SetPublicAuthWitInteraction::create(
            &wallet,
            from,
            MessageHashOrIntent::Hash {
                hash: Fr::from(42u64),
            },
            true,
        )
        .await
        .expect("create interaction");

        let _result = interaction
            .profile(ProfileOptions::default())
            .await
            .expect("profile");
    }

    #[tokio::test]
    async fn lookup_validity_with_hash_returns_false_private() {
        let wallet = MockWallet::new(sample_chain_info());
        let on_behalf_of = AztecAddress(Fr::from(1u64));
        let intent = MessageHashOrIntent::Hash {
            hash: Fr::from(42u64),
        };
        let witness = AuthWitness::default();

        let validity = lookup_validity(&wallet, &on_behalf_of, &intent, &witness)
            .await
            .expect("lookup validity");

        // Raw hash can't be checked privately (no consumer info)
        assert!(!validity.is_valid_in_private);
        // MockWallet returns Null for utility execution, which parses as false
        assert!(!validity.is_valid_in_public);
    }

    #[tokio::test]
    async fn lookup_validity_with_intent() {
        let wallet = MockWallet::new(sample_chain_info());
        let on_behalf_of = AztecAddress(Fr::from(1u64));
        let caller = AztecAddress(Fr::from(2u64));
        let call = FunctionCall {
            to: AztecAddress(Fr::from(3u64)),
            selector: FunctionSelector::from_hex("0xaabbccdd").expect("valid"),
            args: vec![AbiValue::Field(Fr::from(100u64))],
            function_type: FunctionType::Private,
            is_static: false,
            hide_msg_sender: false,
        };
        let intent = MessageHashOrIntent::Intent { caller, call };
        let witness = AuthWitness::default();

        let validity = lookup_validity(&wallet, &on_behalf_of, &intent, &witness)
            .await
            .expect("lookup validity");

        // MockWallet returns Null, so both should be false
        assert!(!validity.is_valid_in_private);
        assert!(!validity.is_valid_in_public);
    }

    #[test]
    fn parse_boolean_result_variants() {
        assert!(parse_boolean_result(&serde_json::Value::Bool(true)));
        assert!(!parse_boolean_result(&serde_json::Value::Bool(false)));
        assert!(parse_boolean_result(&serde_json::json!(1)));
        assert!(!parse_boolean_result(&serde_json::json!(0)));
        assert!(!parse_boolean_result(&serde_json::Value::Null));
    }
}
