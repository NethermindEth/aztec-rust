//! Authentication witness tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_authwit.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_authwit -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::too_many_lines,
    dead_code,
    unused_imports
)]

use crate::common::*;

// Imports not available from common
use aztec_rs::authwit::{lookup_validity, AuthWitValidity, SetPublicAuthWitInteraction};
use aztec_rs::constants::protocol_contract_address;
use aztec_rs::hash::{compute_inner_auth_wit_hash, MessageHashOrIntent};
use aztec_rs::tx::AuthWitness;

// ---------------------------------------------------------------------------
// Contract interaction helpers (test-specific)
// ---------------------------------------------------------------------------

/// Wait for the next block to ensure post-TX state is committed.
async fn wait_for_next_block(wallet: &SharedTestWallet) {
    let current = wallet.pxe().node().get_block_number().await.unwrap_or(0);
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let now = wallet.pxe().node().get_block_number().await.unwrap_or(0);
        if now > current + 1 {
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Shared test state (mirrors beforeAll)
// ---------------------------------------------------------------------------

struct TestState {
    wallet: SharedTestWallet,
    account1: AztecAddress,
    account2: AztecAddress,
    auth_address: AztecAddress,
    auth_artifact: ContractArtifact,
    proxy_address: AztecAddress,
    proxy_artifact: ContractArtifact,
}

static SHARED_STATE: OnceCell<Option<TestState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static TestState> {
    let state = SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await;
    state.as_ref()
}

async fn init_shared_state() -> Option<TestState> {
    let (wallet, account1) = create_wallet(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1]).await?;
    let account2 =
        AztecAddress(Fr::from_hex(TEST_ACCOUNT_1.address).expect("valid account2 address"));

    let auth_artifact = load_auth_wit_test_artifact();
    let proxy_artifact = load_generic_proxy_artifact();

    // Deploy AuthWitTest contract from account1
    let (auth_address, _, _) =
        deploy_contract(&*wallet, auth_artifact.clone(), vec![], account1).await;

    // Deploy GenericProxy contract from account1
    let (proxy_address, _, _) =
        deploy_contract(&*wallet, proxy_artifact.clone(), vec![], account1).await;

    Some(TestState {
        wallet,
        account1,
        account2,
        auth_address,
        auth_artifact,
        proxy_address,
        proxy_artifact,
    })
}

// ---------------------------------------------------------------------------
// Tests: e2e_authwit — Private > arbitrary data
// ---------------------------------------------------------------------------

/// TS: Private > arbitrary data > happy path
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_authwit_arbitrary_data_happy_path() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Use a unique salt so different test runs don't collide on nullifiers.
    let inner_hash = compute_inner_auth_wit_hash(&[
        Fr::from_hex("0xdead").expect("valid hex"),
        Fr::from(next_unique_salt()),
    ]);

    let intent = MessageHashOrIntent::InnerHash {
        consumer: s.auth_address,
        inner_hash,
    };
    let witness = s
        .wallet
        .create_auth_wit(s.account1, intent.clone())
        .await
        .expect("create authwit");

    // Check validity for account1: private=true, public=false
    let validity = lookup_validity(&*s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity for account1");
    assert_eq!(
        validity,
        AuthWitValidity {
            is_valid_in_private: true,
            is_valid_in_public: false,
        },
        "authwit should be valid in private for account1"
    );

    // Check NOT valid for account2: private=false, public=false
    let validity2 = lookup_validity(&*s.wallet, &s.account2, &intent, &witness)
        .await
        .expect("lookup validity for account2");
    assert_eq!(
        validity2,
        AuthWitValidity {
            is_valid_in_private: false,
            is_valid_in_public: false,
        },
        "authwit should NOT be valid for account2"
    );

    // Consume via proxy
    let consume_action = build_call(
        &s.auth_artifact,
        s.auth_address,
        "consume",
        vec![abi_address(s.account1), AbiValue::Field(inner_hash)],
    );
    let proxy_call = build_proxy_call(&s.proxy_artifact, s.proxy_address, &consume_action);

    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![proxy_call],
                ..Default::default()
            },
            SendOptions {
                from: s.account1,
                auth_witnesses: vec![witness.clone()],
                ..Default::default()
            },
        )
        .await
        .expect("consume authwit via proxy");

    wait_for_next_block(&s.wallet).await;

    // Check validity after consumption: private=false, public=false
    let validity_after = lookup_validity(&*s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity after consumption");
    assert_eq!(
        validity_after,
        AuthWitValidity {
            is_valid_in_private: false,
            is_valid_in_public: false,
        },
        "authwit should be invalid after consumption"
    );

    // Try to consume again — duplicate nullifier
    let consume_action2 = build_call(
        &s.auth_artifact,
        s.auth_address,
        "consume",
        vec![abi_address(s.account1), AbiValue::Field(inner_hash)],
    );
    let proxy_call2 = build_proxy_call(&s.proxy_artifact, s.proxy_address, &consume_action2);

    let err = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![proxy_call2],
                ..Default::default()
            },
            SendOptions {
                from: s.account1,
                auth_witnesses: vec![witness],
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: duplicate nullifier");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("duplicate nullifier")
            || err_str.contains("nullifier already exists")
            || err_str.contains("nullifier collision")
            || err_str.contains("existing nullifier"),
        "expected duplicate nullifier error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// Tests: e2e_authwit — Public > arbitrary data
// ---------------------------------------------------------------------------

/// TS: Public > arbitrary data > happy path
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_authwit_arbitrary_data_happy_path() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Use a unique salt so different test runs don't collide on the
    // AuthRegistry's public storage (pre-funded accounts persist state).
    let inner_hash = compute_inner_auth_wit_hash(&[
        Fr::from_hex("0xdead").expect("valid hex"),
        Fr::from_hex("0x01").expect("valid hex"),
        Fr::from(next_unique_salt()),
    ]);

    let intent = MessageHashOrIntent::InnerHash {
        consumer: s.account2,
        inner_hash,
    };
    let witness = s
        .wallet
        .create_auth_wit(s.account1, intent.clone())
        .await
        .expect("create authwit");

    // Check validity: private=true, public=false
    let validity = lookup_validity(&*s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity before set_public");
    assert_eq!(
        validity,
        AuthWitValidity {
            is_valid_in_private: true,
            is_valid_in_public: false,
        },
    );

    // Set public authwit (authorized=true)
    let set_public =
        SetPublicAuthWitInteraction::create(&*s.wallet, s.account1, intent.clone(), true)
            .await
            .expect("create set_public");
    set_public
        .send(SendOptions::default())
        .await
        .expect("send set_public");

    wait_for_next_block(&s.wallet).await;

    // Check validity: private=true, public=true
    let validity_after_set = lookup_validity(&*s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity after set_public");
    assert_eq!(
        validity_after_set,
        AuthWitValidity {
            is_valid_in_private: true,
            is_valid_in_public: true,
        },
    );

    // Consume via AuthRegistry.consume from account2
    let (wallet2, _) = create_wallet(TEST_ACCOUNT_1, &[TEST_ACCOUNT_0])
        .await
        .expect("create wallet for account2");

    let consume_call = FunctionCall {
        to: protocol_contract_address::auth_registry(),
        selector: FunctionSelector::from_signature("consume((Field),Field)"),
        args: vec![abi_address(s.account1), AbiValue::Field(inner_hash)],
        function_type: FunctionType::Public,
        is_static: false,
        hide_msg_sender: false,
    };
    wallet2
        .send_tx(
            ExecutionPayload {
                calls: vec![consume_call],
                ..Default::default()
            },
            SendOptions {
                from: s.account2,
                ..Default::default()
            },
        )
        .await
        .expect("consume public authwit from account2");

    // Wait for the consume TX's block to be fully committed before reading.
    wait_for_next_block(&wallet2).await;
    wait_for_next_block(&s.wallet).await;

    // Check validity: private=true, public=false (consumed in public)
    let validity_after_consume = lookup_validity(&*s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity after consume");
    assert_eq!(
        validity_after_consume,
        AuthWitValidity {
            is_valid_in_private: true,
            is_valid_in_public: false,
        },
    );
}

/// TS: Public > arbitrary data > failure case > cancel before usage
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_authwit_cancel_before_usage() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let inner_hash = compute_inner_auth_wit_hash(&[
        Fr::from_hex("0xdead").expect("valid hex"),
        Fr::from_hex("0x02").expect("valid hex"),
        Fr::from(next_unique_salt()),
    ]);

    let intent = MessageHashOrIntent::InnerHash {
        consumer: s.auth_address,
        inner_hash,
    };
    let witness = s
        .wallet
        .create_auth_wit(s.account1, intent.clone())
        .await
        .expect("create authwit");

    // Check validity: private=true, public=false
    let validity = lookup_validity(&*s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity");
    assert_eq!(
        validity,
        AuthWitValidity {
            is_valid_in_private: true,
            is_valid_in_public: false,
        },
    );

    // Set public authwit (authorized=true)
    let set_public =
        SetPublicAuthWitInteraction::create(&*s.wallet, s.account1, intent.clone(), true)
            .await
            .expect("create set_public");
    set_public
        .send(SendOptions::default())
        .await
        .expect("send set_public");

    wait_for_next_block(&s.wallet).await;

    // Check validity: private=true, public=true
    let validity_set = lookup_validity(&*s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity after set_public");
    assert_eq!(
        validity_set,
        AuthWitValidity {
            is_valid_in_private: true,
            is_valid_in_public: true,
        },
    );

    // Cancel public authwit (authorized=false)
    let cancel = SetPublicAuthWitInteraction::create(&*s.wallet, s.account1, intent.clone(), false)
        .await
        .expect("create cancel");
    cancel
        .send(SendOptions::default())
        .await
        .expect("send cancel");

    wait_for_next_block(&s.wallet).await;

    // Check validity: private=true, public=false
    let validity_cancel = lookup_validity(&*s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity after cancel");
    assert_eq!(
        validity_cancel,
        AuthWitValidity {
            is_valid_in_private: true,
            is_valid_in_public: false,
        },
    );

    // Try to consume via AuthRegistry — should fail with unauthorized
    let (wallet2, _) = create_wallet(TEST_ACCOUNT_1, &[TEST_ACCOUNT_0])
        .await
        .expect("create wallet for account2");

    // Try to consume via AuthRegistry — should fail.
    // Use send_tx (which includes node-side public simulation) rather than
    // simulate_tx (which only simulates the private part locally).
    let consume_call = FunctionCall {
        to: protocol_contract_address::auth_registry(),
        selector: FunctionSelector::from_signature("consume((Field),Field)"),
        args: vec![abi_address(s.account1), AbiValue::Field(inner_hash)],
        function_type: FunctionType::Public,
        is_static: false,
        hide_msg_sender: false,
    };
    let err = wallet2
        .send_tx(
            ExecutionPayload {
                calls: vec![consume_call],
                ..Default::default()
            },
            SendOptions {
                from: s.account2,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: authwit was cancelled");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("unauthorized")
            || err_str.contains("not authorized")
            || err_str.contains("assertion failed")
            || err_str.contains("reverted"),
        "expected unauthorized/reverted error, got: {err}"
    );
}
