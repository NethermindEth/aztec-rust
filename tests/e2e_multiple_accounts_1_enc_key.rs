//! Multiple accounts with one encryption key tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_multiple_accounts_1_enc_key.test.ts`.
//!
//! Tests that the PXE can handle multiple accounts sharing the same encryption
//! key but with different signing keys.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_multiple_accounts_1_enc_key -- --ignored --nocapture
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

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionSelector, FunctionType};
use aztec_rs::account::{
    AccountContract, EntrypointOptions, SchnorrAccountContract, SingleAccountProvider,
};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::{complete_address_from_secret_key_and_partial_address, derive_keys};
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::{Pxe, RegisterContractRequest};
use aztec_rs::tx::{AuthWitness, ExecutionPayload, FunctionCall};
use aztec_rs::types::{
    AztecAddress, CompleteAddress, ContractInstance, ContractInstanceWithAddress, Fr,
    GrumpkinScalar, PublicKeys,
};
use aztec_rs::wallet::{
    AccountProvider, Aliased, BaseWallet, ChainInfo, ExecuteUtilityOptions, MessageHashOrIntent,
    SendOptions, Wallet,
};

use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_compiled_token_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/token_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse token_contract_compiled.json")
}

fn load_schnorr_account_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/schnorr_account_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse schnorr_account_contract_compiled.json")
}

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

type TestWallet = BaseWallet<EmbeddedPxe<HttpNodeClient>, HttpNodeClient, MultiAccountProvider>;

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

fn node_url() -> String {
    std::env::var("AZTEC_NODE_URL").unwrap_or_else(|_| "http://localhost:8080".to_owned())
}

fn serial_guard() -> MutexGuard<'static, ()> {
    static E2E_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    E2E_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[allow(clippy::cast_possible_truncation)]
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

fn imported_complete_address(account: ImportedTestAccount) -> CompleteAddress {
    let expected_address =
        AztecAddress(Fr::from_hex(account.address).expect("valid test account address"));
    let secret_key = Fr::from_hex(account.secret_key).expect("valid test account secret key");
    let partial_address =
        Fr::from_hex(account.partial_address).expect("valid test account partial address");
    let complete =
        complete_address_from_secret_key_and_partial_address(&secret_key, &partial_address)
            .expect("derive complete address");
    assert_eq!(
        complete.address, expected_address,
        "imported fixture address does not match derived complete address for {}",
        account.alias
    );
    complete
}

// ---------------------------------------------------------------------------
// MultiAccountProvider — manages N accounts in a single wallet
// ---------------------------------------------------------------------------

struct MultiAccountEntry {
    complete_address: CompleteAddress,
    secret_key: Fr,
    signing_key: GrumpkinScalar,
    alias: String,
}

struct MultiAccountProvider {
    entries: Vec<MultiAccountEntry>,
}

impl MultiAccountProvider {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn add(
        &mut self,
        complete_address: CompleteAddress,
        secret_key: Fr,
        signing_key: GrumpkinScalar,
        alias: impl Into<String>,
    ) {
        self.entries.push(MultiAccountEntry {
            complete_address,
            secret_key,
            signing_key,
            alias: alias.into(),
        });
    }

    fn find(&self, address: &AztecAddress) -> Result<&MultiAccountEntry, aztec_rs::Error> {
        self.entries
            .iter()
            .find(|e| e.complete_address.address == *address)
            .ok_or_else(|| aztec_rs::Error::InvalidData(format!("account not found: {address}")))
    }
}

#[async_trait]
impl AccountProvider for MultiAccountProvider {
    async fn create_tx_execution_request(
        &self,
        from: &AztecAddress,
        exec: ExecutionPayload,
        gas_settings: aztec_rs::fee::GasSettings,
        chain_info: &ChainInfo,
        fee_payer: Option<AztecAddress>,
    ) -> Result<aztec_rs::pxe::TxExecutionRequest, aztec_rs::Error> {
        let entry = self.find(from)?;
        let contract =
            SchnorrAccountContract::new_with_signing_key(entry.secret_key, entry.signing_key);
        let account = contract.account(entry.complete_address.clone());
        let options = EntrypointOptions {
            fee_payer,
            gas_settings: Some(gas_settings.clone()),
        };
        let tx_request = account
            .create_tx_execution_request(exec, gas_settings, chain_info, options)
            .await?;
        let data = serde_json::to_value(&tx_request)
            .map_err(|e| aztec_rs::Error::InvalidData(e.to_string()))?;
        Ok(aztec_rs::pxe::TxExecutionRequest { data })
    }

    async fn create_auth_wit(
        &self,
        from: &AztecAddress,
        intent: MessageHashOrIntent,
        chain_info: &ChainInfo,
    ) -> Result<AuthWitness, aztec_rs::Error> {
        let entry = self.find(from)?;
        let contract =
            SchnorrAccountContract::new_with_signing_key(entry.secret_key, entry.signing_key);
        let resolved = match &intent {
            MessageHashOrIntent::Hash { .. } => intent,
            MessageHashOrIntent::Intent { .. } | MessageHashOrIntent::InnerHash { .. } => {
                let hash = aztec_rs::hash::compute_auth_wit_message_hash(&intent, chain_info);
                MessageHashOrIntent::Hash { hash }
            }
        };
        let account = contract.account(entry.complete_address.clone());
        let mut witness = account
            .create_auth_wit(resolved.clone(), chain_info)
            .await?;
        if let MessageHashOrIntent::Hash { hash } = &resolved {
            witness.request_hash = *hash;
        }
        Ok(witness)
    }

    async fn get_complete_address(
        &self,
        address: &AztecAddress,
    ) -> Result<Option<CompleteAddress>, aztec_rs::Error> {
        Ok(self
            .entries
            .iter()
            .find(|e| e.complete_address.address == *address)
            .map(|e| e.complete_address.clone()))
    }

    async fn get_accounts(&self) -> Result<Vec<Aliased<AztecAddress>>, aztec_rs::Error> {
        Ok(self
            .entries
            .iter()
            .map(|e| Aliased {
                alias: e.alias.clone(),
                item: e.complete_address.address,
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Account configuration
// ---------------------------------------------------------------------------

struct AccountConfig {
    secret: Fr,
    signing_key: GrumpkinScalar,
    salt: Fr,
    address: AztecAddress,
    complete_address: CompleteAddress,
    public_keys: PublicKeys,
}

/// Compute account config using the COMPILED artifact (same as deployment uses).
fn compute_account_config(
    secret: Fr,
    signing_key: GrumpkinScalar,
    salt: Fr,
    compiled_artifact: &ContractArtifact,
) -> AccountConfig {
    let contract = SchnorrAccountContract::new_with_signing_key(secret, signing_key);
    let signing_pk = contract.signing_public_key();

    let derived = derive_keys(&secret);
    let public_keys = derived.public_keys.clone();

    let instance = aztec_rs::deployment::get_contract_instance_from_instantiation_params(
        compiled_artifact,
        aztec_rs::deployment::ContractInstantiationParams {
            constructor_name: Some("constructor"),
            constructor_args: vec![AbiValue::Field(signing_pk.x), AbiValue::Field(signing_pk.y)],
            salt,
            public_keys: public_keys.clone(),
            deployer: AztecAddress::zero(),
        },
    )
    .expect("build instance");

    let address = instance.address;

    let complete_address = CompleteAddress {
        address,
        public_keys: public_keys.clone(),
        partial_address: {
            let salted = aztec_rs::hash::compute_salted_initialization_hash(
                instance.inner.salt,
                instance.inner.initialization_hash,
                instance.inner.deployer,
            );
            aztec_rs::hash::compute_partial_address(
                instance.inner.original_contract_class_id,
                salted,
            )
        },
    };

    AccountConfig {
        secret,
        signing_key,
        salt,
        address,
        complete_address,
        public_keys,
    }
}

// ---------------------------------------------------------------------------
// Contract interaction helpers
// ---------------------------------------------------------------------------

fn build_call(
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
) -> FunctionCall {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|_| panic!("function '{method_name}' not found in artifact"));
    let selector = func.selector.expect("selector");
    FunctionCall {
        to: contract_address,
        selector,
        args,
        function_type: func.function_type.clone(),
        is_static: func.is_static,
        hide_msg_sender: false,
    }
}

async fn send_token_method(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    token_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    from: AztecAddress,
    fee_payer: AztecAddress,
) {
    let call = build_call(artifact, token_address, method_name, args);
    wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                fee_payer: Some(fee_payer),
                ..Default::default()
            },
            SendOptions {
                from,
                additional_scopes: if from != fee_payer {
                    vec![fee_payer]
                } else {
                    vec![]
                },
                ..Default::default()
            },
        )
        .await
        .unwrap_or_else(|e| panic!("{method_name} failed: {e}"));
}

#[allow(clippy::cast_possible_truncation)]
async fn expect_token_balance(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    token_address: AztecAddress,
    owner: AztecAddress,
    expected: u128,
) {
    let func = artifact
        .find_function("balance_of_private")
        .expect("balance_of_private");
    let call = FunctionCall {
        to: token_address,
        selector: func.selector.expect("selector"),
        args: vec![AbiValue::Field(Fr::from(owner))],
        function_type: FunctionType::Utility,
        is_static: false,
        hide_msg_sender: false,
    };

    let result = wallet
        .execute_utility(
            call,
            ExecuteUtilityOptions {
                scope: owner,
                auth_witnesses: vec![],
            },
        )
        .await
        .unwrap_or_else(|e| panic!("balance_of_private: {e}"));

    let balance = result
        .result
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .and_then(|s| Fr::from_hex(s).ok())
        .map_or(0u64, |f| f.to_usize() as u64);

    assert_eq!(
        u128::from(balance),
        expected,
        "unexpected private balance for {owner}: got {balance}, expected {expected}"
    );
    eprintln!("  balance of {owner}: {balance} (expected {expected}) OK");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// TS: spends notes from multiple account under the same encryption key
///
/// Setup: 3 accounts sharing the same encryption key but different signing keys.
/// Performs transfers 0->1, 0->2, 1->2 and validates balances after each.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn spends_notes_from_multiple_accounts_same_enc_key() {
    let _guard = serial_guard();

    // --- 1. Connect to node, create single PXE ---
    let url = node_url();
    let node = create_aztec_node_client(&url);
    if let Err(err) = node.get_node_info().await {
        eprintln!("skipping: node not reachable: {err}");
        return;
    }

    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = match EmbeddedPxe::create(node.clone(), kv).await {
        Ok(pxe) => pxe,
        Err(err) => {
            eprintln!("skipping: failed to create PXE: {err}");
            return;
        }
    };

    // --- 2. Register admin account (TEST_ACCOUNT_0) for deployment ---
    let admin_secret =
        Fr::from_hex(TEST_ACCOUNT_0.secret_key).expect("valid test account secret key");
    let admin_complete = imported_complete_address(TEST_ACCOUNT_0);
    let admin_address = admin_complete.address;
    let admin_signing_key = aztec_rs::crypto::derive_signing_key(&admin_secret);

    pxe.key_store()
        .add_account(&admin_secret)
        .await
        .expect("seed admin key store");
    pxe.address_store()
        .add(&admin_complete)
        .await
        .expect("seed admin address store");

    // Register compiled Schnorr account artifact for admin.
    let compiled_account = load_schnorr_account_artifact();
    let admin_contract = SchnorrAccountContract::new(admin_secret);
    let admin_dynamic_artifact = admin_contract
        .contract_artifact()
        .await
        .expect("dynamic artifact");
    let admin_class_id =
        aztec_rs::hash::compute_contract_class_id_from_artifact(&admin_dynamic_artifact)
            .expect("compute class id");
    pxe.contract_store()
        .add_artifact(&admin_class_id, &compiled_account)
        .await
        .expect("register compiled account artifact");
    let admin_instance = ContractInstanceWithAddress {
        address: admin_address,
        inner: ContractInstance {
            version: 1,
            salt: Fr::from(0u64),
            deployer: AztecAddress::zero(),
            current_contract_class_id: admin_class_id,
            original_contract_class_id: admin_class_id,
            initialization_hash: Fr::zero(),
            public_keys: admin_complete.public_keys.clone(),
        },
    };
    pxe.contract_store()
        .add_instance(&admin_instance)
        .await
        .expect("register admin account instance");

    // Seed admin signing key note.
    let admin_signing_pk = admin_contract.signing_public_key();
    let admin_note = aztec_rs::embedded_pxe::stores::note_store::StoredNote {
        contract_address: admin_address,
        owner: admin_address,
        storage_slot: Fr::from(1u64),
        randomness: Fr::zero(),
        note_nonce: Fr::from(1u64),
        note_hash: Fr::from(1u64),
        siloed_nullifier: Fr::from_hex(
            "0xdeadbeef00000000000000000000000000000000000000000000000000000001",
        )
        .expect("unique nullifier"),
        note_data: vec![admin_signing_pk.x, admin_signing_pk.y],
        nullified: false,
        is_pending: false,
        nullification_block_number: None,
        leaf_index: None,
        block_number: None,
        tx_index_in_block: None,
        note_index_in_tx: None,
        scopes: vec![admin_address],
    };
    pxe.note_store()
        .add_note(&admin_note)
        .await
        .expect("seed admin signing key note");

    eprintln!("admin wallet ready: {admin_address}");

    // --- 3. Generate shared secret and 3 account configs ---
    let secret = Fr::random();
    let num_accounts = 3;
    let mut accounts: Vec<AccountConfig> = Vec::new();

    for i in 0..num_accounts {
        let signing_key = GrumpkinScalar::random();
        let salt = Fr::from(next_unique_salt());
        let config = compute_account_config(secret, signing_key, salt, &compiled_account);
        eprintln!("account[{i}]: {}", config.address);
        accounts.push(config);
    }

    // --- 4. Register new accounts in PXE ---
    pxe.key_store()
        .add_account(&secret)
        .await
        .expect("seed shared secret");
    for (i, acct) in accounts.iter().enumerate() {
        pxe.address_store()
            .add(&acct.complete_address)
            .await
            .unwrap_or_else(|e| panic!("register address {i}: {e}"));
    }

    // Register senders for tag discovery.
    for acct in &accounts {
        if let Err(e) = pxe.register_sender(&acct.address).await {
            eprintln!("register sender: {e}");
        }
    }

    // --- 5. Build wallet with admin + 3 accounts ---
    let mut provider = MultiAccountProvider::new();
    // Add admin account.
    provider.add(
        admin_complete.clone(),
        admin_secret,
        admin_signing_key,
        "admin",
    );
    // Add the 3 new accounts.
    for (i, acct) in accounts.iter().enumerate() {
        provider.add(
            acct.complete_address.clone(),
            acct.secret,
            acct.signing_key,
            format!("account{i}"),
        );
    }
    let wallet = BaseWallet::new(pxe, node, provider);
    eprintln!("wallet ready (admin + 3 accounts)");

    // --- 6. Deploy 3 account contracts ---
    for (i, acct) in accounts.iter().enumerate() {
        eprintln!("deploying account contract {i}...");
        let contract = SchnorrAccountContract::new_with_signing_key(acct.secret, acct.signing_key);
        let signing_pk = contract.signing_public_key();

        let deploy = Contract::deploy_with_public_keys(
            acct.public_keys.clone(),
            &wallet,
            compiled_account.clone(),
            vec![AbiValue::Field(signing_pk.x), AbiValue::Field(signing_pk.y)],
            Some("constructor"),
        )
        .expect("deploy builder");

        let result = deploy
            .send(
                &DeployOptions {
                    contract_address_salt: Some(acct.salt),
                    universal_deploy: true,
                    ..Default::default()
                },
                SendOptions {
                    from: admin_address,
                    additional_scopes: vec![acct.address],
                    ..Default::default()
                },
            )
            .await
            .unwrap_or_else(|e| panic!("deploy account {i}: {e}"));

        assert_eq!(
            result.instance.address, acct.address,
            "deployed address mismatch for account {i}"
        );
        eprintln!("  account[{i}] deployed at {}", acct.address);

        // Seed signing key note for the new account.
        let note = aztec_rs::embedded_pxe::stores::note_store::StoredNote {
            contract_address: acct.address,
            owner: acct.address,
            storage_slot: Fr::from(1u64),
            randomness: Fr::zero(),
            note_nonce: Fr::from(1u64),
            note_hash: Fr::from(10u64 + i as u64),
            siloed_nullifier: Fr::from(0xdead_beef_0000_0000u64 + i as u64),
            note_data: vec![signing_pk.x, signing_pk.y],
            nullified: false,
            is_pending: false,
            nullification_block_number: None,
            leaf_index: None,
            block_number: None,
            tx_index_in_block: None,
            note_index_in_tx: None,
            scopes: vec![acct.address],
        };
        wallet
            .pxe()
            .note_store()
            .add_note(&note)
            .await
            .unwrap_or_else(|e| panic!("seed signing key note {i}: {e}"));
    }

    // --- 7. Deploy token contract ---
    let initial_balance: u128 = 987;
    eprintln!("deploying token contract...");
    let token_artifact = load_compiled_token_artifact();
    let deploy = Contract::deploy(
        &wallet,
        token_artifact.clone(),
        vec![
            AbiValue::Field(Fr::from(admin_address)),
            AbiValue::String("TestToken".to_owned()),
            AbiValue::String("TT".to_owned()),
            AbiValue::Integer(18),
        ],
        None,
    )
    .expect("deploy token builder");
    let token_result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: admin_address,
                ..Default::default()
            },
        )
        .await
        .expect("deploy Token");
    let token_address = token_result.instance.address;
    eprintln!("Token deployed at {token_address}");

    // --- 8. Mint to account[0] ---
    eprintln!("minting {initial_balance} to account[0]...");
    send_token_method(
        &wallet,
        &token_artifact,
        token_address,
        "mint_to_private",
        vec![
            AbiValue::Field(Fr::from(accounts[0].address)),
            AbiValue::Integer(initial_balance as i128),
        ],
        admin_address,
        admin_address,
    )
    .await;
    eprintln!("minting complete");

    // --- 9. Verify initial balances ---
    let acct_addrs: Vec<AztecAddress> = accounts.iter().map(|a| a.address).collect();
    eprintln!("verifying initial balances...");
    expect_token_balance(
        &wallet,
        &token_artifact,
        token_address,
        acct_addrs[0],
        initial_balance,
    )
    .await;
    expect_token_balance(&wallet, &token_artifact, token_address, acct_addrs[1], 0).await;
    expect_token_balance(&wallet, &token_artifact, token_address, acct_addrs[2], 0).await;

    // --- 10. Transfer 0 -> 1 ---
    let transfer_amount_1: u128 = 654;
    eprintln!("\ntransfer {transfer_amount_1} from account[0] to account[1]...");
    send_token_method(
        &wallet,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(acct_addrs[1])),
            AbiValue::Integer(transfer_amount_1 as i128),
        ],
        acct_addrs[0],
        admin_address,
    )
    .await;

    let expected_0 = initial_balance - transfer_amount_1;
    let expected_1 = transfer_amount_1;
    let expected_2: u128 = 0;
    eprintln!("verifying balances after transfer 1...");
    expect_token_balance(
        &wallet,
        &token_artifact,
        token_address,
        acct_addrs[0],
        expected_0,
    )
    .await;
    expect_token_balance(
        &wallet,
        &token_artifact,
        token_address,
        acct_addrs[1],
        expected_1,
    )
    .await;
    expect_token_balance(
        &wallet,
        &token_artifact,
        token_address,
        acct_addrs[2],
        expected_2,
    )
    .await;

    // --- 11. Transfer 0 -> 2 ---
    let transfer_amount_2: u128 = 123;
    eprintln!("\ntransfer {transfer_amount_2} from account[0] to account[2]...");
    send_token_method(
        &wallet,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(acct_addrs[2])),
            AbiValue::Integer(transfer_amount_2 as i128),
        ],
        acct_addrs[0],
        admin_address,
    )
    .await;

    let expected_0 = expected_0 - transfer_amount_2;
    let expected_2 = transfer_amount_2;
    eprintln!("verifying balances after transfer 2...");
    expect_token_balance(
        &wallet,
        &token_artifact,
        token_address,
        acct_addrs[0],
        expected_0,
    )
    .await;
    expect_token_balance(
        &wallet,
        &token_artifact,
        token_address,
        acct_addrs[1],
        expected_1,
    )
    .await;
    expect_token_balance(
        &wallet,
        &token_artifact,
        token_address,
        acct_addrs[2],
        expected_2,
    )
    .await;

    // --- 12. Transfer 1 -> 2 ---
    let transfer_amount_3: u128 = 210;
    eprintln!("\ntransfer {transfer_amount_3} from account[1] to account[2]...");
    send_token_method(
        &wallet,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(acct_addrs[2])),
            AbiValue::Integer(transfer_amount_3 as i128),
        ],
        acct_addrs[1],
        admin_address,
    )
    .await;

    let expected_1 = expected_1 - transfer_amount_3;
    let expected_2 = expected_2 + transfer_amount_3;
    eprintln!("verifying balances after transfer 3...");
    expect_token_balance(
        &wallet,
        &token_artifact,
        token_address,
        acct_addrs[0],
        expected_0,
    )
    .await;
    expect_token_balance(
        &wallet,
        &token_artifact,
        token_address,
        acct_addrs[1],
        expected_1,
    )
    .await;
    expect_token_balance(
        &wallet,
        &token_artifact,
        token_address,
        acct_addrs[2],
        expected_2,
    )
    .await;

    eprintln!("\nall transfers and balances verified successfully!");
}
