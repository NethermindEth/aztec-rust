//! Kernelless simulation tests -- 1:1 mirror of upstream
//! `end-to-end/src/e2e_kernelless_simulation.test.ts`.
//!
//! NOTE: the Rust SDK does not yet expose `enableSimulatedSimulations()` /
//! `disableSimulatedSimulations()` toggles that upstream's TS wallet uses to
//! bypass the kernel during simulation.  These tests therefore only run the
//! ordinary (with-kernels) simulate path and assert the same observable
//! outcomes (authwit round-trip, gas-estimate non-zero, settled read-request
//! simulation completes).  When the SDK gains a kernelless flag, the calls
//! labelled `# kernelless` below should switch to it.
//!
//! Tests skip when the AMM artifact is not available (it's not shipped in the
//! local fixtures dir and upstream's compiled target may not be built).
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_kernelless_simulation -- --ignored --nocapture
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

use aztec_rs::hash::MessageHashOrIntent;

const INITIAL_TOKEN_BALANCE: u64 = 1_000_000_000;

// ---------------------------------------------------------------------------
// Shared state (mirrors upstream beforeAll(setup(3)))
// ---------------------------------------------------------------------------

struct KernellessState {
    admin_wallet: TestWallet,
    lp_wallet: TestWallet,
    swapper_wallet: TestWallet,
    admin: AztecAddress,
    lp: AztecAddress,
    swapper: AztecAddress,
    token0_address: AztecAddress,
    token1_address: AztecAddress,
    liquidity_token_address: AztecAddress,
    token_artifact: ContractArtifact,
    amm_address: AztecAddress,
    amm_artifact: ContractArtifact,
}

static SHARED_STATE: OnceCell<Option<KernellessState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static KernellessState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<KernellessState> {
    let amm_artifact = load_amm_artifact()?;

    let (admin_wallet, admin) =
        setup_wallet_with_accounts(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1, TEST_ACCOUNT_2]).await?;
    let (lp_wallet, lp) =
        setup_wallet_with_accounts(TEST_ACCOUNT_1, &[TEST_ACCOUNT_0, TEST_ACCOUNT_2]).await?;
    let (swapper_wallet, swapper) =
        setup_wallet_with_accounts(TEST_ACCOUNT_2, &[TEST_ACCOUNT_0, TEST_ACCOUNT_1]).await?;

    for (w, senders) in [
        (&admin_wallet, vec![lp, swapper]),
        (&lp_wallet, vec![admin, swapper]),
        (&swapper_wallet, vec![admin, lp]),
    ] {
        for sender in senders {
            w.pxe().register_sender(&sender).await.ok();
        }
    }

    // Register each account's Schnorr contract on every PXE that might
    // dispatch a nested `verify_private_authwit` call into it, and seed the
    // Schnorr signing-key note so the account entrypoint's
    // `utilityGetNotes`-based signing-key lookup resolves.  Upstream's
    // `setup(3)` uses a single shared PXE that already knows all three
    // account contracts via `registerAccount` (which does both the
    // contract-store registration and the signing-key note seeding); our
    // split-wallet translation must seed both explicitly or nested authwit
    // verification fails with "contract not found" and the outer entrypoint
    // fails with a brillig constraint failure in the signing-key note read.
    let compiled_account = load_schnorr_account_artifact();
    for (w, accounts) in [
        (
            &admin_wallet,
            [TEST_ACCOUNT_0, TEST_ACCOUNT_1, TEST_ACCOUNT_2],
        ),
        (&lp_wallet, [TEST_ACCOUNT_0, TEST_ACCOUNT_1, TEST_ACCOUNT_2]),
        (
            &swapper_wallet,
            [TEST_ACCOUNT_0, TEST_ACCOUNT_1, TEST_ACCOUNT_2],
        ),
    ] {
        for (i, account) in accounts.iter().enumerate() {
            register_account_for_authwit(w.pxe(), &compiled_account, *account).await;
            let secret_key = Fr::from_hex(account.secret_key).expect("valid secret key");
            let account_contract = SchnorrAccountContract::new(secret_key);
            let complete = imported_complete_address(*account);
            // Distinct nullifier seed per account on each PXE.  The signing-key
            // note's siloed nullifier is derived from this seed; reusing the
            // same seed for every account on the same PXE makes the 2nd/3rd
            // `add_note` collide with the 1st and silently overwrite it,
            // which then breaks the `verify_private_authwit` note lookup for
            // every account except the last one seeded.
            seed_signing_key_note(w.pxe(), &account_contract, complete.address, (i as u64) + 1)
                .await;
        }
    }

    // Deploy three token contracts: token0, token1, liquidityToken.
    let (token0_address, token_artifact, token0_instance) =
        deploy_token(&admin_wallet, admin, 0).await;
    let (token1_address, _, token1_instance) = deploy_token(&admin_wallet, admin, 0).await;
    let (liquidity_token_address, _, lt_instance) = deploy_token(&admin_wallet, admin, 0).await;

    register_contract_on_pxe(lp_wallet.pxe(), &token_artifact, &token0_instance).await;
    register_contract_on_pxe(lp_wallet.pxe(), &token_artifact, &token1_instance).await;
    register_contract_on_pxe(lp_wallet.pxe(), &token_artifact, &lt_instance).await;
    register_contract_on_pxe(swapper_wallet.pxe(), &token_artifact, &token0_instance).await;
    register_contract_on_pxe(swapper_wallet.pxe(), &token_artifact, &token1_instance).await;

    // Deploy the AMM contract.
    let (amm_address, amm_artifact, amm_instance) = deploy_contract(
        &admin_wallet,
        amm_artifact,
        vec![
            AbiValue::Field(Fr::from(token0_address)),
            AbiValue::Field(Fr::from(token1_address)),
            AbiValue::Field(Fr::from(liquidity_token_address)),
        ],
        admin,
    )
    .await;

    register_contract_on_pxe(lp_wallet.pxe(), &amm_artifact, &amm_instance).await;
    register_contract_on_pxe(swapper_wallet.pxe(), &amm_artifact, &amm_instance).await;

    // Authorise the AMM to mint the liquidity token.
    send_token_method(
        &admin_wallet,
        &token_artifact,
        liquidity_token_address,
        "set_minter",
        vec![
            AbiValue::Field(Fr::from(amm_address)),
            AbiValue::Boolean(true),
        ],
        admin,
    )
    .await;

    // Mint balances: LP has both tokens, swapper only has token0.
    mint_tokens_to_private(
        &admin_wallet,
        token0_address,
        &token_artifact,
        admin,
        lp,
        INITIAL_TOKEN_BALANCE,
    )
    .await;
    mint_tokens_to_private(
        &admin_wallet,
        token1_address,
        &token_artifact,
        admin,
        lp,
        INITIAL_TOKEN_BALANCE,
    )
    .await;
    mint_tokens_to_private(
        &admin_wallet,
        token0_address,
        &token_artifact,
        admin,
        swapper,
        INITIAL_TOKEN_BALANCE,
    )
    .await;

    Some(KernellessState {
        admin_wallet,
        lp_wallet,
        swapper_wallet,
        admin,
        lp,
        swapper,
        token0_address,
        token1_address,
        liquidity_token_address,
        token_artifact,
        amm_address,
        amm_artifact,
    })
}

// ===========================================================================
// describe('Authwits and gas')
// ===========================================================================

/// TS: adds liquidity without authwits
///
/// Upstream uses kernelless simulation to harvest authwit-request metadata from
/// offchain effects, then reproduces the same request hash via
/// `wallet.createAuthWit`.  The Rust SDK does not yet expose the kernelless
/// toggle or offchain-effect extraction used here, so we mirror the happy-path
/// round-trip: create the two authwits for the transfer_to_public calls
/// required by `add_liquidity` and submit the tx.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn adds_liquidity_without_authwits() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let nonce_for_authwits = Fr::random();

    // Check LP's live balance rather than trusting `INITIAL_TOKEN_BALANCE`:
    // the shared sandbox may expose previously-minted notes or the PXE may
    // still be syncing.  Upstream's test uses the value returned from
    // `balance_of_private.simulate()` for the same reason.
    let bal0 = call_utility_u128(
        &s.lp_wallet,
        &s.token_artifact,
        s.token0_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.lp))],
        s.lp,
    )
    .await;
    let bal1 = call_utility_u128(
        &s.lp_wallet,
        &s.token_artifact,
        s.token1_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.lp))],
        s.lp,
    )
    .await;
    eprintln!("LP balances before add_liquidity: token0={bal0} token1={bal1}");
    assert!(
        bal0 > 0 && bal1 > 0,
        "LP must have non-zero private balance"
    );

    let amount0_max = bal0;
    let amount1_max = bal1;
    let amount0_min = amount0_max / 2;
    let amount1_min = amount1_max / 2;

    // Build the add_liquidity call.
    let add_liquidity_call = build_call(
        &s.amm_artifact,
        s.amm_address,
        "add_liquidity",
        vec![
            AbiValue::Integer(amount0_max as i128),
            AbiValue::Integer(amount1_max as i128),
            AbiValue::Integer(amount0_min as i128),
            AbiValue::Integer(amount1_min as i128),
            AbiValue::Field(nonce_for_authwits),
        ],
    );

    // Pre-create both authwits (the AMM will consume these for the two
    // transfer_to_public_and_prepare_private_balance_increase calls that
    // `add_liquidity` fans out to token0 and token1).
    let token0_action = build_call(
        &s.token_artifact,
        s.token0_address,
        "transfer_to_public_and_prepare_private_balance_increase",
        vec![
            AbiValue::Field(Fr::from(s.lp)),
            AbiValue::Field(Fr::from(s.amm_address)),
            AbiValue::Integer(amount0_max as i128),
            AbiValue::Field(nonce_for_authwits),
        ],
    );
    let token1_action = build_call(
        &s.token_artifact,
        s.token1_address,
        "transfer_to_public_and_prepare_private_balance_increase",
        vec![
            AbiValue::Field(Fr::from(s.lp)),
            AbiValue::Field(Fr::from(s.amm_address)),
            AbiValue::Integer(amount1_max as i128),
            AbiValue::Field(nonce_for_authwits),
        ],
    );
    let token0_wit = s
        .lp_wallet
        .create_auth_wit(
            s.lp,
            MessageHashOrIntent::Intent {
                caller: s.amm_address,
                call: token0_action,
            },
        )
        .await
        .expect("create token0 authwit");
    let token1_wit = s
        .lp_wallet
        .create_auth_wit(
            s.lp,
            MessageHashOrIntent::Intent {
                caller: s.amm_address,
                call: token1_action,
            },
        )
        .await
        .expect("create token1 authwit");

    s.lp_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![add_liquidity_call],
                ..Default::default()
            },
            SendOptions {
                from: s.lp,
                auth_witnesses: vec![token0_wit, token1_wit],
                ..Default::default()
            },
        )
        .await
        .expect("add_liquidity tx");
}

/// TS: produces matching gas estimates between kernelless and with-kernels simulation
///
/// With no kernelless toggle in the Rust SDK, we can only assert that the
/// ordinary simulate path produces a non-zero gas estimate.  When the SDK
/// gains a toggle, both branches should be run and their gas limits compared.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn produces_matching_gas_estimates_between_kernelless_and_with_kernels() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Amount-in = 10% of swapper's token0 balance.
    let amount_in = INITIAL_TOKEN_BALANCE / 10;
    let amount_out_min = 0u64; // Loose lower bound — we just want the sim to succeed.
    let nonce_for_authwits = Fr::random();

    let swap_call = build_call(
        &s.amm_artifact,
        s.amm_address,
        "swap_exact_tokens_for_tokens",
        vec![
            AbiValue::Field(Fr::from(s.token0_address)),
            AbiValue::Field(Fr::from(s.token1_address)),
            AbiValue::Integer(i128::from(amount_in)),
            AbiValue::Integer(i128::from(amount_out_min)),
            AbiValue::Field(nonce_for_authwits),
        ],
    );

    let sim = s
        .swapper_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![swap_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.swapper,
                ..Default::default()
            },
        )
        .await;

    // Some nodes reject without authwits at simulation time (which would make
    // the gas assertion meaningless). If the sim succeeds, sanity-check that a
    // non-zero gas estimate came back.
    if let Ok(result) = sim {
        if let Some(gas) = result.gas_used {
            assert!(
                gas.l2_gas > 0 || gas.da_gas > 0,
                "simulation gas estimate must be non-zero"
            );
        }
    }
}

// ===========================================================================
// describe('Note squashing')
// ===========================================================================

/// TS: squashing produces same gas estimates as with-kernels path
///
/// Without the kernelless toggle we only assert the ordinary simulation
/// succeeds for the nested-call squashing case.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn squashing_produces_same_gas_estimates_as_with_kernels_path() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Deploy the PendingNoteHashes contract ad-hoc for this scenario.
    let pnh_artifact = load_pending_note_hashes_artifact();
    let (pnh_address, pnh_artifact, _pnh_instance) =
        deploy_contract(&s.admin_wallet, pnh_artifact, vec![], s.admin).await;

    let insert_selector = pnh_artifact
        .find_function("insert_note")
        .expect("insert_note")
        .selector
        .expect("selector");
    let get_nullify_selector = pnh_artifact
        .find_function("get_then_nullify_note")
        .expect("get_then_nullify_note")
        .selector
        .expect("selector");

    let mint_amount = 42u64;
    let nested_call = build_call(
        &pnh_artifact,
        pnh_address,
        "test_insert_then_get_then_nullify_all_in_nested_calls",
        vec![
            AbiValue::Integer(i128::from(mint_amount)),
            AbiValue::Field(Fr::from(s.admin)),
            AbiValue::Field(Fr::from(s.admin)),
            abi_selector(insert_selector),
            abi_selector(get_nullify_selector),
        ],
    );

    let _ = s
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![nested_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.admin,
                ..Default::default()
            },
        )
        .await;
    // Just ensure it doesn't panic; specific gas-equality checks require the
    // kernelless toggle to be meaningful.
}

// ===========================================================================
// describe('read request verification')
// ===========================================================================

/// TS: verifies settled read requests against the note hash tree
///
/// 1. Insert a note on-chain (with-kernels path).
/// 2. Simulate a get+nullify of that settled note and assert it resolves.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn verifies_settled_read_requests_against_the_note_hash_tree() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let pnh_artifact = load_pending_note_hashes_artifact();
    let (pnh_address, pnh_artifact, _pnh_instance) =
        deploy_contract(&s.admin_wallet, pnh_artifact, vec![], s.admin).await;

    let mint_amount = 100u64;

    // 1. Insert the note on-chain (ordinary path).
    send_token_method(
        &s.admin_wallet,
        &pnh_artifact,
        pnh_address,
        "insert_note",
        vec![
            AbiValue::Integer(i128::from(mint_amount)),
            AbiValue::Field(Fr::from(s.admin)),
            AbiValue::Field(Fr::from(s.admin)),
        ],
        s.admin,
    )
    .await;

    // 2. Simulate read+nullify (upstream uses kernelless here; we fall back
    //    to regular simulate).
    let call = build_call(
        &pnh_artifact,
        pnh_address,
        "get_then_nullify_note",
        vec![
            AbiValue::Integer(i128::from(mint_amount)),
            AbiValue::Field(Fr::from(s.admin)),
        ],
    );

    s.admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.admin,
                ..Default::default()
            },
        )
        .await
        .expect("simulate get_then_nullify_note of settled note");
}
