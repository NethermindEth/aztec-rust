//! End-to-end tests with two PXE instances — 1:1 mirror of upstream
//! `end-to-end/src/e2e_2_pxes.test.ts`.
//!
//! All tests in this file require both a live Aztec node AND ACVM integration
//! (Phase 1) because they deploy contracts and execute transactions.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_2_pxes -- --ignored
//! ```

#![allow(clippy::expect_used, clippy::print_stderr, dead_code, unused_variables)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionType};
use aztec_rs::account::{SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::{Pxe, RegisterContractRequest};
use aztec_rs::tx::FunctionCall;
use aztec_rs::types::{
    AztecAddress, CompleteAddress, ContractInstance, ContractInstanceWithAddress, Fr, PublicKeys,
};
use aztec_rs::wallet::{BaseWallet, SendOptions, Wallet};

// ---------------------------------------------------------------------------
// Fixtures (mirrors upstream fixtures/token_utils.ts + contract imports)
// ---------------------------------------------------------------------------

fn load_token_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/token_contract.json");
    ContractArtifact::from_json(json).expect("parse token_contract.json")
}

fn load_compiled_token_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/token_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse token_contract_compiled.json")
}

fn load_compiled_child_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/child_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse child_contract_compiled.json")
}

// ---------------------------------------------------------------------------
// Setup helpers (mirrors upstream fixtures/utils.ts)
// ---------------------------------------------------------------------------

type TestWallet = BaseWallet<EmbeddedPxe<HttpNodeClient>, HttpNodeClient, SingleAccountProvider>;

#[derive(Clone, Copy)]
struct ImportedTestAccount {
    alias: &'static str,
    address: &'static str,
    secret_key: &'static str,
    partial_address: &'static str,
}

const TEST_ACCOUNT_0: ImportedTestAccount = ImportedTestAccount {
    alias: "test0",
    address: "0x0a60414ee907527880b7a53d4dacdeb9ef768bb98d9d8d1e7200725c13763331",
    secret_key: "0x2153536ff6628eee01cf4024889ff977a18d9fa61d0e414422f7681cf085c281",
    partial_address: "0x140c3a658e105092549c8402f0647fe61d87aba4422b484dfac5d4a87462eeef",
};

const TEST_ACCOUNT_1: ImportedTestAccount = ImportedTestAccount {
    alias: "test1",
    address: "0x00cedf87a800bd88274762d77ffd93e97bc881d1fc99570d62ba97953597914d",
    secret_key: "0x0aebd1b4be76efa44f5ee655c20bf9ea60f7ae44b9a7fd1fd9f189c7a0b0cdae",
    partial_address: "0x0325ee1689daec508c6adef0df4a1e270ac1fcf971fed1f893b2d98ad12d6bb8",
};

const TEST_ACCOUNT_2: ImportedTestAccount = ImportedTestAccount {
    alias: "test2",
    address: "0x1dd551228da3a56b5da5f5d73728e08d8114f59897c27136f1bcdd4c05028905",
    secret_key: "0x0f6addf0da06c33293df974a565b03d1ab096090d907d98055a8b7f4954e120c",
    partial_address: "0x17604ccd69bd09d8df02c4a345bc4232e5d24b568536c55407b3e4e4e3354c4c",
};

fn node_url() -> String {
    std::env::var("AZTEC_NODE_URL").unwrap_or_else(|_| "http://localhost:8080".to_owned())
}

fn serial_guard() -> MutexGuard<'static, ()> {
    static E2E_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    E2E_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

fn next_unique_salt() -> u64 {
    static NEXT_SALT: OnceLock<AtomicU64> = OnceLock::new();
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    NEXT_SALT
        .get_or_init(|| AtomicU64::new(seed))
        .fetch_add(1, Ordering::Relaxed)
}

async fn create_pxe() -> Option<EmbeddedPxe<HttpNodeClient>> {
    let node = create_aztec_node_client(node_url());
    if let Err(err) = node.get_node_info().await {
        eprintln!("skipping: node not reachable: {err}");
        return None;
    }
    let kv = Arc::new(InMemoryKvStore::new());
    match EmbeddedPxe::create(node, kv).await {
        Ok(pxe) => Some(pxe),
        Err(err) => {
            eprintln!("skipping: failed to create PXE: {err}");
            None
        }
    }
}

/// Mirrors upstream `setup(1)` + `setupPXEAndGetWallet`.
/// Creates a wallet backed by a fresh PXE and a Schnorr account.
fn imported_complete_address(account: ImportedTestAccount) -> CompleteAddress {
    let expected_address =
        AztecAddress(Fr::from_hex(account.address).expect("valid test account address"));
    let secret_key = Fr::from_hex(account.secret_key).expect("valid test account secret key");
    let partial_address =
        Fr::from_hex(account.partial_address).expect("valid test account partial address");
    let complete =
        complete_address_from_secret_key_and_partial_address(&secret_key, &partial_address)
            .expect("derive complete address from test account fixture");
    let derived_address = complete.address;
    assert_eq!(
        derived_address, expected_address,
        "imported fixture address does not match derived complete address for {}",
        account.alias
    );
    complete
}

async fn setup_wallet(account: ImportedTestAccount) -> Option<(TestWallet, AztecAddress)> {
    let url = node_url();
    let node = create_aztec_node_client(&url);
    if let Err(err) = node.get_node_info().await {
        eprintln!("skipping: node not reachable: {err}");
        return None;
    }

    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = match EmbeddedPxe::create(node.clone(), kv).await {
        Ok(pxe) => pxe,
        Err(err) => {
            eprintln!("skipping: failed to create PXE: {err}");
            return None;
        }
    };

    let secret_key = Fr::from_hex(account.secret_key).expect("valid test account secret key");
    let complete = imported_complete_address(account);

    if let Err(err) = pxe.key_store().add_account(&secret_key).await {
        eprintln!(
            "skipping: failed to seed key store for {}: {err}",
            account.alias
        );
        return None;
    }
    if let Err(err) = pxe.address_store().add(&complete).await {
        eprintln!(
            "skipping: failed to seed address store for {}: {err}",
            account.alias
        );
        return None;
    }

    let account_contract = SchnorrAccountContract::new(secret_key);

    let provider =
        SingleAccountProvider::new(complete.clone(), Box::new(account_contract), account.alias);

    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, complete.address))
}

async fn setup_two_wallets() -> Option<((TestWallet, AztecAddress), (TestWallet, AztecAddress))> {
    let Some((wallet_a, account_a_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return None;
    };
    let Some((wallet_b, account_b_address)) = setup_wallet(TEST_ACCOUNT_1).await else {
        return None;
    };

    wallet_a
        .pxe()
        .register_sender(&account_b_address)
        .await
        .expect("A registers B");
    wallet_b
        .pxe()
        .register_sender(&account_a_address)
        .await
        .expect("B registers A");

    Some(((wallet_a, account_a_address), (wallet_b, account_b_address)))
}

fn make_instance(artifact: &ContractArtifact, salt: u64) -> ContractInstanceWithAddress {
    let class_id = aztec_rs::hash::compute_contract_class_id_from_artifact(artifact)
        .expect("compute class id");
    let inner = ContractInstance {
        version: 1,
        salt: Fr::from(salt),
        deployer: AztecAddress(Fr::zero()),
        current_contract_class_id: class_id,
        original_contract_class_id: class_id,
        initialization_hash: Fr::zero(),
        public_keys: PublicKeys::default(),
    };
    let address =
        aztec_rs::hash::compute_contract_address_from_instance(&inner).expect("compute address");
    ContractInstanceWithAddress { address, inner }
}

async fn register_contract_on_pxe(
    pxe: &impl Pxe,
    artifact: &ContractArtifact,
    instance: &ContractInstanceWithAddress,
) {
    pxe.register_contract_class(artifact)
        .await
        .expect("register class");
    pxe.register_contract(RegisterContractRequest {
        instance: instance.clone(),
        artifact: Some(artifact.clone()),
    })
    .await
    .expect("register contract");
}

// ---------------------------------------------------------------------------
// Token interaction helpers (mirrors upstream fixtures/token_utils.ts)
// ---------------------------------------------------------------------------

async fn deploy_token(
    wallet: &TestWallet,
    admin: AztecAddress,
    initial_balance: u64,
) -> (AztecAddress, ContractArtifact, ContractInstanceWithAddress) {
    let artifact = load_compiled_token_artifact();
    let deploy = Contract::deploy(
        wallet,
        artifact.clone(),
        vec![
            AbiValue::Field(Fr::from(admin)),
            AbiValue::String("TestToken".to_string()),
            AbiValue::String("TT".to_string()),
            AbiValue::Integer(18),
        ],
        None,
    )
    .expect("token deploy builder");
    let deploy_result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: admin,
                ..Default::default()
            },
        )
        .await
        .expect("deploy token");

    let instance = deploy_result.instance;
    let token_address = instance.address;

    if initial_balance > 0 {
        send_token_method(
            wallet,
            &artifact,
            token_address,
            "mint_to_private",
            vec![
                AbiValue::Field(Fr::from(admin)),
                AbiValue::Integer(initial_balance as i128),
            ],
            admin,
        )
        .await;
    }

    (token_address, artifact, instance)
}

async fn deploy_child(
    wallet: &TestWallet,
    from: AztecAddress,
) -> (AztecAddress, ContractArtifact, ContractInstanceWithAddress) {
    let artifact = load_compiled_child_artifact();
    let deploy =
        Contract::deploy(wallet, artifact.clone(), vec![], None).expect("child deploy builder");
    let result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from,
                ..Default::default()
            },
        )
        .await
        .expect("deploy child");

    (result.instance.address, artifact, result.instance)
}

async fn send_token_method(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    token_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    from: AztecAddress,
) {
    let func = artifact
        .find_function(method_name)
        .expect("function not found");
    let selector = func.selector.expect("selector");
    let call = FunctionCall {
        to: token_address,
        selector,
        args,
        function_type: func.function_type.clone(),
        is_static: false,
        hide_msg_sender: false,
    };

    wallet
        .send_tx(
            aztec_rs::tx::ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from,
                ..Default::default()
            },
        )
        .await
        .expect("send tx");
}

async fn mint_tokens_to_private(
    wallet: &TestWallet,
    token_address: AztecAddress,
    artifact: &ContractArtifact,
    from: AztecAddress,
    to: AztecAddress,
    amount: u64,
) {
    send_token_method(
        wallet,
        artifact,
        token_address,
        "mint_to_private",
        vec![
            AbiValue::Field(Fr::from(to)),
            AbiValue::Integer(amount as i128),
        ],
        from,
    )
    .await;
}

async fn expect_token_balance(
    wallet: &TestWallet,
    token_address: AztecAddress,
    artifact: &ContractArtifact,
    owner: AztecAddress,
    expected: u64,
) {
    let func = artifact
        .find_function("balance_of_private")
        .expect("balance_of_private");
    let selector = func.selector.expect("selector");
    let call = FunctionCall {
        to: token_address,
        selector,
        args: vec![AbiValue::Field(Fr::from(owner))],
        function_type: FunctionType::Utility,
        is_static: false,
        hide_msg_sender: false,
    };

    let result = wallet
        .execute_utility(
            call,
            aztec_rs::wallet::ExecuteUtilityOptions {
                scope: owner,
                auth_witnesses: vec![],
            },
        )
        .await
        .expect("execute balance_of_private");

    let balance = result
        .result
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .and_then(|s| Fr::from_hex(s).ok())
        .map(|f| f.to_usize() as u64)
        .unwrap_or(0);

    assert_eq!(
        balance, expected,
        "expected balance {expected} for {owner}, got {balance}"
    );
}

// ===========================================================================
// describe('e2e_2_pxes')
//
// All 5 tests below are 1:1 mirrors of the upstream TS e2e_2_pxes.test.ts.
// They all require ACVM integration (Phase 1) to actually run.
// ===========================================================================

/// TS: it('transfers funds from user A to B via PXE A followed by transfer
///        from B to A via PXE B')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfers_funds_from_user_a_to_b_via_pxe_a_followed_by_transfer_from_b_to_a_via_pxe_b() {
    let _serial = serial_guard();
    let Some(((wallet_a, account_a_address), (wallet_b, account_b_address))) =
        setup_two_wallets().await
    else {
        return;
    };

    let initial_balance = 987u64;
    let transfer_amount1 = 654u64;
    let transfer_amount2 = 323u64;

    let (token_address, token_artifact, token_instance) =
        deploy_token(&wallet_a, account_a_address, initial_balance).await;

    // Add token to PXE B (PXE A already has it because it was deployed through it)
    register_contract_on_pxe(wallet_b.pxe(), &token_artifact, &token_instance).await;

    // Check initial balances are as expected
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        initial_balance,
    )
    .await;
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        account_b_address,
        0,
    )
    .await;

    // Transfer funds from A to B via PXE A
    send_token_method(
        &wallet_a,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(account_b_address)),
            AbiValue::Integer(transfer_amount1 as i128),
        ],
        account_a_address,
    )
    .await;

    // Check balances are as expected
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        initial_balance - transfer_amount1,
    )
    .await;
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        account_b_address,
        transfer_amount1,
    )
    .await;

    // Transfer funds from B to A via PXE B
    send_token_method(
        &wallet_b,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(account_a_address)),
            AbiValue::Integer(transfer_amount2 as i128),
        ],
        account_b_address,
    )
    .await;

    // Check balances are as expected
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        initial_balance - transfer_amount1 + transfer_amount2,
    )
    .await;
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        account_b_address,
        transfer_amount1 - transfer_amount2,
    )
    .await;
}

/// TS: it('user calls a public function on a contract deployed by a different
///        user using a different PXE')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn user_calls_a_public_function_on_a_contract_deployed_by_a_different_user_using_a_different_pxe(
) {
    let _serial = serial_guard();
    let Some(((wallet_a, account_a_address), (wallet_b, account_b_address))) =
        setup_two_wallets().await
    else {
        return;
    };

    // Deploy Child contract via PXE A
    let (child_address, child_artifact, child_instance) =
        deploy_child(&wallet_a, account_a_address).await;

    // Add Child to PXE B
    register_contract_on_pxe(wallet_b.pxe(), &child_artifact, &child_instance).await;

    let new_value_to_set = Fr::from(256u64);

    // Call pub_inc_value via PXE B
    send_token_method(
        &wallet_b,
        &child_artifact,
        child_address,
        "pub_inc_value",
        vec![AbiValue::Field(new_value_to_set)],
        account_b_address,
    )
    .await;

    // Verify public storage via node
    let stored_value_on_b = wallet_b
        .pxe()
        .node()
        .get_public_storage_at(0, &child_address, &Fr::from(1u64))
        .await
        .expect("get public storage B");
    assert_eq!(stored_value_on_b, new_value_to_set);

    let stored_value_on_a = wallet_a
        .pxe()
        .node()
        .get_public_storage_at(0, &child_address, &Fr::from(1u64))
        .await
        .expect("get public storage A");
    assert_eq!(stored_value_on_a, new_value_to_set);
}

/// TS: it('private state is "zero" when PXE does not have the account secret key')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_state_is_zero_when_pxe_does_not_have_the_account_secret_key() {
    let _serial = serial_guard();
    let Some(((wallet_a, account_a_address), (wallet_b, account_b_address))) =
        setup_two_wallets().await
    else {
        return;
    };

    let user_a_balance = 100u64;
    let user_b_balance = 150u64;

    let (token_address, token_artifact, token_instance) =
        deploy_token(&wallet_a, account_a_address, user_a_balance).await;

    // Add token to PXE B
    register_contract_on_pxe(wallet_b.pxe(), &token_artifact, &token_instance).await;

    // Mint tokens to user B
    mint_tokens_to_private(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        account_b_address,
        user_b_balance,
    )
    .await;

    // Check that user A balance is 100 on server A
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        user_a_balance,
    )
    .await;
    // Check that user B balance is 150 on server B
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        account_b_address,
        user_b_balance,
    )
    .await;

    // CHECK THAT PRIVATE BALANCES ARE 0 WHEN ACCOUNT'S SECRET KEYS ARE NOT REGISTERED
    // Check that user A balance is 0 on server B
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        account_a_address,
        0,
    )
    .await;
    // Check that user B balance is 0 on server A
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_b_address,
        0,
    )
    .await;
}

/// TS: it('permits sending funds to a user before they have registered the contract')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn permits_sending_funds_to_a_user_before_they_have_registered_the_contract() {
    let _serial = serial_guard();
    let Some(((wallet_a, account_a_address), (wallet_b, account_b_address))) =
        setup_two_wallets().await
    else {
        return;
    };

    let initial_balance = 987u64;
    let transfer_amount1 = 654u64;

    let (token_address, token_artifact, token_instance) =
        deploy_token(&wallet_a, account_a_address, initial_balance).await;

    // Check initial balances are as expected
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        initial_balance,
    )
    .await;
    // don't check userB yet

    // Transfer funds from A to B via PXE A
    send_token_method(
        &wallet_a,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(account_b_address)),
            AbiValue::Integer(transfer_amount1 as i128),
        ],
        account_a_address,
    )
    .await;

    // now add the contract and check balances
    register_contract_on_pxe(wallet_b.pxe(), &token_artifact, &token_instance).await;
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        initial_balance - transfer_amount1,
    )
    .await;
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        account_b_address,
        transfer_amount1,
    )
    .await;
}

/// TS: it('permits sending funds to a user, and spending them, before they
///        have registered the contract')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn permits_sending_funds_to_a_user_and_spending_them_before_they_have_registered_the_contract(
) {
    let _serial = serial_guard();
    let Some(((wallet_a, account_a_address), (wallet_b, account_b_address))) =
        setup_two_wallets().await
    else {
        return;
    };

    let initial_balance = 987u64;
    let transfer_amount1 = 654u64;
    let transfer_amount2 = 323u64;

    // setup an account that is shared across PXEs
    let shared_secret =
        Fr::from_hex(TEST_ACCOUNT_2.secret_key).expect("valid test account secret key");
    let shared_account = imported_complete_address(TEST_ACCOUNT_2);
    wallet_a
        .pxe()
        .key_store()
        .add_account(&shared_secret)
        .await
        .expect("seed shared key on A");
    wallet_a
        .pxe()
        .address_store()
        .add(&shared_account)
        .await
        .expect("seed shared address on A");
    let shared_account_address = shared_account.address;

    // Register the shared account on walletB
    wallet_b
        .pxe()
        .key_store()
        .add_account(&shared_secret)
        .await
        .expect("seed shared key on B");
    wallet_b
        .pxe()
        .address_store()
        .add(&shared_account)
        .await
        .expect("seed shared address on B");

    // deploy the contract on PXE A
    let (token_address, token_artifact, token_instance) =
        deploy_token(&wallet_a, account_a_address, initial_balance).await;

    // Transfer funds from A to Shared Wallet via PXE A
    send_token_method(
        &wallet_a,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(shared_account_address)),
            AbiValue::Integer(transfer_amount1 as i128),
        ],
        account_a_address,
    )
    .await;

    // Now send funds from Shared Wallet to B via PXE A
    send_token_method(
        &wallet_a,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(account_b_address)),
            AbiValue::Integer(transfer_amount2 as i128),
        ],
        shared_account_address,
    )
    .await;

    // check balances from PXE-A's perspective
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        initial_balance - transfer_amount1,
    )
    .await;
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        shared_account_address,
        transfer_amount1 - transfer_amount2,
    )
    .await;

    // now add the contract and check balances from PXE-B's perspective.
    // The process should be:
    // PXE-B had previously deferred the notes from A -> Shared, and Shared -> B
    // PXE-B adds the contract
    // PXE-B reprocesses the deferred notes, and sees the nullifier for A -> Shared
    register_contract_on_pxe(wallet_b.pxe(), &token_artifact, &token_instance).await;
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        account_b_address,
        transfer_amount2,
    )
    .await;
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        shared_account_address,
        transfer_amount1 - transfer_amount2,
    )
    .await;
}
