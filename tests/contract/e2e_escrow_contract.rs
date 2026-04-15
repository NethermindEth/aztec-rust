//! Escrow contract tests -- 1:1 mirror of upstream
//! `end-to-end/src/e2e_escrow_contract.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_escrow_contract -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::cast_possible_wrap,
    clippy::too_many_lines,
    dead_code,
    unused_imports
)]

use crate::common::*;

use aztec_rs::contract::Contract;
use aztec_rs::crypto::derive_keys;
use aztec_rs::hash::{compute_partial_address, compute_salted_initialization_hash};

// ---------------------------------------------------------------------------
// Setup helper: deploy escrow with its own key pair
// ---------------------------------------------------------------------------

struct EscrowTestSetup {
    owner_wallet: TestWallet,
    recipient_wallet: TestWallet,
    owner: AztecAddress,
    recipient: AztecAddress,
    escrow_address: AztecAddress,
    escrow_artifact: ContractArtifact,
    token_address: AztecAddress,
    token_artifact: ContractArtifact,
}

/// Mirrors upstream `beforeEach`:
///
/// 1. `setup(2)` — wallet with 2 accounts (owner, recipient).
/// 2. Generate `escrowSecretKey = Fr::random()`, derive its public keys.
/// 3. `EscrowContract.deployWithPublicKeys(escrowPublicKeys, wallet, owner)` —
///    build the deployment with the escrow's custom key pair.
/// 4. `await wallet.registerContract(escrowInstance, EscrowContract.artifact, escrowSecretKey)` —
///    register the contract class + instance on the PXE and bind the secret key so
///    the PXE can decrypt notes emitted by the escrow for itself.
/// 5. `deployment.send({ from: owner, additionalScopes: [escrowInstance.address] })` —
///    the constructor initialises private storage that needs the escrow's own
///    nullifier key in scope.
/// 6. Deploy the Token contract with `(owner, 'TokenName', 'TokenSymbol', 18)` and
///    `mintTokensToPrivate(token, owner, escrow.address, 100n)`.
///
/// Returns `None` if the escrow compiled artifact is missing or the live node
/// is unreachable.
#[allow(clippy::cognitive_complexity)]
async fn setup_escrow() -> Option<EscrowTestSetup> {
    let escrow_artifact = load_escrow_compiled_artifact()?;

    // Upstream uses a single wallet with 2 registered accounts.  The Rust
    // `BaseWallet<SingleAccountProvider>` is pinned to a single signer, so we
    // mirror that by running two wallets that each know about the other
    // account (same PXE state, different signing key).
    let (owner_wallet, owner) =
        setup_wallet_with_accounts(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1]).await?;
    let (recipient_wallet, recipient) =
        setup_wallet_with_accounts(TEST_ACCOUNT_1, &[TEST_ACCOUNT_0]).await?;

    owner_wallet.pxe().register_sender(&recipient).await.ok();
    recipient_wallet.pxe().register_sender(&owner).await.ok();

    // (2) Escrow key pair
    let escrow_secret = Fr::random();
    let derived = derive_keys(&escrow_secret);
    let escrow_public_keys = derived.public_keys.clone();

    // (3) Build the deployment.
    let deploy_method = Contract::deploy_with_public_keys(
        escrow_public_keys.clone(),
        &owner_wallet,
        escrow_artifact.clone(),
        vec![AbiValue::Field(Fr::from(owner))],
        None,
    )
    .expect("deploy_with_public_keys builder");

    // Deterministic deploy options so `.get_instance(...)` and the eventual
    // `.send(...)` agree on the address/deployer.
    let deploy_opts = DeployOptions {
        contract_address_salt: Some(Fr::from(next_unique_salt())),
        from: Some(owner),
        ..Default::default()
    };

    // (4) Pre-compute the escrow instance (mirrors `deployment.getInstance()`)
    // and register it on the owner's PXE *before* sending.  Upstream calls
    // `wallet.registerContract(instance, artifact, escrowSecretKey)`, which on
    // the TS PXE does three things:
    //   (a) register the contract class + instance,
    //   (b) add the secret to the keystore, and
    //   (c) add the contract's *complete address* (public_keys +
    //       partial_address) to the address store so oracles like
    //       `tryGetPublicKeysAndPartialAddress` can resolve it during the
    //       constructor's private execution.
    //
    // In this SDK those three operations are exposed independently — we have
    // to perform all three explicitly to match upstream behaviour.
    let escrow_instance = deploy_method
        .get_instance(&deploy_opts)
        .expect("compute escrow instance");
    let escrow_address = escrow_instance.address;

    let escrow_salted_init_hash = compute_salted_initialization_hash(
        escrow_instance.inner.salt,
        escrow_instance.inner.initialization_hash,
        escrow_instance.inner.deployer,
    );
    let escrow_partial_address = compute_partial_address(
        escrow_instance.inner.original_contract_class_id,
        escrow_salted_init_hash,
    );
    let escrow_complete = CompleteAddress {
        address: escrow_address,
        public_keys: escrow_public_keys.clone(),
        partial_address: escrow_partial_address,
    };

    // (a) + (b) + (c) on the owner's PXE
    owner_wallet
        .pxe()
        .key_store()
        .add_account(&escrow_secret)
        .await
        .expect("register escrow secret on owner PXE");
    owner_wallet
        .pxe()
        .address_store()
        .add(&escrow_complete)
        .await
        .expect("register escrow complete address on owner PXE");
    register_contract_on_pxe(owner_wallet.pxe(), &escrow_artifact, &escrow_instance).await;

    // Upstream's single PXE is shared by both accounts.  To replicate that
    // shared state, mirror the registration on the recipient's PXE so any
    // call/simulate from `recipient` can also resolve the escrow.
    recipient_wallet
        .pxe()
        .key_store()
        .add_account(&escrow_secret)
        .await
        .expect("register escrow secret on recipient PXE");
    recipient_wallet
        .pxe()
        .address_store()
        .add(&escrow_complete)
        .await
        .expect("register escrow complete address on recipient PXE");
    register_contract_on_pxe(recipient_wallet.pxe(), &escrow_artifact, &escrow_instance).await;

    // (5) Send the deployment.  `additional_scopes: [escrow_address]` matches
    // upstream's `additionalScopes: [escrowInstance.address]`: the constructor
    // initialises private storage that needs the escrow's own nullifier key in
    // scope.
    let _deploy_result = deploy_method
        .send(
            &deploy_opts,
            SendOptions {
                from: owner,
                additional_scopes: vec![escrow_address],
                ..Default::default()
            },
        )
        .await
        .expect("deploy escrow");

    // (6) Deploy the Token contract — `deploy_token` constructs it with
    // `(admin, "TestToken", "TT", 18)`, behaviourally equivalent to upstream's
    // `(owner, "TokenName", "TokenSymbol", 18)`.
    let (token_address, token_artifact, token_instance) =
        deploy_token(&owner_wallet, owner, 0).await;
    register_contract_on_pxe(recipient_wallet.pxe(), &token_artifact, &token_instance).await;

    // Mint 100 tokens to the escrow privately.
    mint_tokens_to_private(
        &owner_wallet,
        token_address,
        &token_artifact,
        owner,
        escrow_address,
        100,
    )
    .await;

    Some(EscrowTestSetup {
        owner_wallet,
        recipient_wallet,
        owner,
        recipient,
        escrow_address,
        escrow_artifact,
        token_address,
        token_artifact,
    })
}

/// Assert `balance_of_private(owner) == expected` on the token contract.
/// Mirrors upstream `expectTokenBalance(wallet, token, owner, expected, logger)`.
async fn expect_token_balance_private(
    wallet: &TestWallet,
    token_address: AztecAddress,
    token_artifact: &ContractArtifact,
    owner: AztecAddress,
    expected: u64,
) {
    let balance = call_utility_u128(
        wallet,
        token_artifact,
        token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(owner))],
        owner,
    )
    .await;
    assert_eq!(
        balance,
        u128::from(expected),
        "expected balance {expected} for {owner}, got {balance}"
    );
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: `withdraws funds from the escrow contract`
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn withdraws_funds_from_the_escrow_contract() {
    let _guard = serial_guard();
    let Some(s) = setup_escrow().await else {
        return;
    };

    // expect owner=0, recipient=0, escrow=100
    expect_token_balance_private(
        &s.owner_wallet,
        s.token_address,
        &s.token_artifact,
        s.owner,
        0,
    )
    .await;
    expect_token_balance_private(
        &s.recipient_wallet,
        s.token_address,
        &s.token_artifact,
        s.recipient,
        0,
    )
    .await;
    expect_token_balance_private(
        &s.owner_wallet,
        s.token_address,
        &s.token_artifact,
        s.escrow_address,
        100,
    )
    .await;

    // escrow.withdraw(token.address, 30, recipient) from owner.
    //
    // `additional_scopes: [escrow_address]` is required here: the escrow's
    // `withdraw` reads its own private notes, which requires the escrow's
    // nullifier key.  The Rust PXE enforces scope isolation and will deny the
    // key-validation request unless the escrow is in scope.  Upstream TS does
    // not need this because its PXE treats registered-contract keys as
    // implicitly in scope.
    let withdraw_call = build_call(
        &s.escrow_artifact,
        s.escrow_address,
        "withdraw",
        vec![
            AbiValue::Field(Fr::from(s.token_address)),
            AbiValue::Integer(30),
            AbiValue::Field(Fr::from(s.recipient)),
        ],
    );
    s.owner_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![withdraw_call],
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                additional_scopes: vec![s.escrow_address],
                ..Default::default()
            },
        )
        .await
        .expect("withdraw");

    // expect owner=0, recipient=30, escrow=70
    expect_token_balance_private(
        &s.owner_wallet,
        s.token_address,
        &s.token_artifact,
        s.owner,
        0,
    )
    .await;
    expect_token_balance_private(
        &s.recipient_wallet,
        s.token_address,
        &s.token_artifact,
        s.recipient,
        30,
    )
    .await;
    expect_token_balance_private(
        &s.owner_wallet,
        s.token_address,
        &s.token_artifact,
        s.escrow_address,
        70,
    )
    .await;
}

/// TS: `refuses to withdraw funds as a non-owner`
///
/// `expect(escrowContract.methods.withdraw(..).simulate({ from: recipient })).rejects.toThrow()`
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn refuses_to_withdraw_funds_as_a_non_owner() {
    let _guard = serial_guard();
    let Some(s) = setup_escrow().await else {
        return;
    };

    let call = build_call(
        &s.escrow_artifact,
        s.escrow_address,
        "withdraw",
        vec![
            AbiValue::Field(Fr::from(s.token_address)),
            AbiValue::Integer(30),
            AbiValue::Field(Fr::from(s.recipient)),
        ],
    );

    // Upstream: `await expect(...simulate({ from: recipient })).rejects.toThrow()` —
    // any error counts.  The non-owner fails ownership check or key-scope check
    // depending on which constraint fires first; we just assert the simulation
    // is rejected.
    let err = s
        .recipient_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.recipient,
                additional_scopes: vec![s.escrow_address],
                ..Default::default()
            },
        )
        .await
        .expect_err("non-owner withdraw must fail");
    eprintln!("non-owner withdraw rejected as expected: {err}");
}

/// TS: `moves funds using multiple keys on the same tx (#1010)`
///
/// `new BatchCall(wallet, [token.transfer(recipient, 10), escrow.withdraw(token, 20, recipient)]).send({ from: owner })`
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn moves_funds_using_multiple_keys_on_the_same_tx() {
    let _guard = serial_guard();
    let Some(s) = setup_escrow().await else {
        return;
    };

    // Mint 50 to owner privately so the first call in the batch has funds.
    mint_tokens_to_private(
        &s.owner_wallet,
        s.token_address,
        &s.token_artifact,
        s.owner,
        s.owner,
        50,
    )
    .await;

    expect_token_balance_private(
        &s.owner_wallet,
        s.token_address,
        &s.token_artifact,
        s.owner,
        50,
    )
    .await;

    let transfer_call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(s.recipient)),
            AbiValue::Integer(10),
        ],
    );
    let withdraw_call = build_call(
        &s.escrow_artifact,
        s.escrow_address,
        "withdraw",
        vec![
            AbiValue::Field(Fr::from(s.token_address)),
            AbiValue::Integer(20),
            AbiValue::Field(Fr::from(s.recipient)),
        ],
    );

    // Rust equivalent of TS `new BatchCall(wallet, [transfer, withdraw]).send(...)`.
    // The second call needs the escrow's keys; pass its address in
    // `additional_scopes` (see note on test 1).
    s.owner_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![transfer_call, withdraw_call],
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                additional_scopes: vec![s.escrow_address],
                ..Default::default()
            },
        )
        .await
        .expect("batched transfer + withdraw");

    // recipient gets 10 (transfer) + 20 (withdraw) = 30
    expect_token_balance_private(
        &s.recipient_wallet,
        s.token_address,
        &s.token_artifact,
        s.recipient,
        30,
    )
    .await;
}
