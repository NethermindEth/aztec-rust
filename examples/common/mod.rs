#![allow(clippy::expect_used, clippy::panic, dead_code, unused_imports)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use aztec_rs::abi::{
    AbiParameter, AbiType, AbiValue, ContractArtifact, EventSelector, FunctionSelector,
    FunctionType,
};
pub use aztec_rs::account::{
    Account, AccountContract, AccountManager, DeployAccountOptions, EntrypointOptions,
    SchnorrAccountContract, SingleAccountProvider,
};
pub use aztec_rs::authwit::{lookup_validity, AuthWitValidity, SetPublicAuthWitInteraction};
pub use aztec_rs::contract::{BatchCall, Contract};
pub use aztec_rs::crypto::{
    complete_address_from_secret_key_and_partial_address, derive_keys, DerivedKeys,
};
pub use aztec_rs::deployment::{
    get_gas_limits, publish_contract_class, ContractDeployer, ContractInstantiationParams,
    DeployOptions,
};
pub use aztec_rs::embedded_pxe::stores::note_store::StoredNote;
pub use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
pub use aztec_rs::events::{get_public_events, PrivateEventFilter, PublicEventFilter};
pub use aztec_rs::fee::{
    FeeJuicePaymentMethodWithClaim, FeePaymentMethod, Gas, GasFees, GasSettings,
    NativeFeePaymentMethod, SponsoredFeePaymentMethod,
};
pub use aztec_rs::hash::{
    compute_auth_wit_message_hash, compute_contract_class_id_from_artifact,
    compute_inner_auth_wit_hash, poseidon2_hash_with_separator, MessageHashOrIntent,
};
pub use aztec_rs::l1_client::{self, EthClient, L1ContractAddresses};
pub use aztec_rs::messaging::{self, L1Actor, L1ToL2Message, L2Actor};
pub use aztec_rs::node::{
    create_aztec_node_client, wait_for_node, AztecNode, HttpNodeClient, WaitOpts,
};
pub use aztec_rs::pxe::{Pxe, RegisterContractRequest};
pub use aztec_rs::tx::{AuthWitness, ExecutionPayload, FunctionCall, TxHash, TxStatus};
pub use aztec_rs::types::{
    AztecAddress, CompleteAddress, ContractInstance, ContractInstanceWithAddress, EthAddress, Fr,
    GrumpkinScalar, PublicKeys,
};
pub use aztec_rs::wallet::{
    AccountProvider, Aliased, BaseWallet, ChainInfo, EventMetadataDefinition,
    ExecuteUtilityOptions, ProfileMode, ProfileOptions, SendOptions, SimulateOptions, Wallet,
};

pub type TestWallet =
    BaseWallet<EmbeddedPxe<HttpNodeClient>, HttpNodeClient, SingleAccountProvider>;
pub type SharedTestWallet = Arc<TestWallet>;

#[derive(Clone, Copy)]
pub struct ImportedTestAccount {
    pub alias: &'static str,
    pub address: &'static str,
    pub secret_key: &'static str,
    pub partial_address: &'static str,
}

pub const TEST_ACCOUNT_0: ImportedTestAccount = ImportedTestAccount {
    alias: "test0",
    address: "0x0a60414ee907527880b7a53d4dacdeb9ef768bb98d9d8d1e7200725c13763331",
    secret_key: "0x2153536ff6628eee01cf4024889ff977a18d9fa61d0e414422f7681cf085c281",
    partial_address: "0x140c3a658e105092549c8402f0647fe61d87aba4422b484dfac5d4a87462eeef",
};

pub const TEST_ACCOUNT_1: ImportedTestAccount = ImportedTestAccount {
    alias: "test1",
    address: "0x00cedf87a800bd88274762d77ffd93e97bc881d1fc99570d62ba97953597914d",
    secret_key: "0x0aebd1b4be76efa44f5ee655c20bf9ea60f7ae44b9a7fd1fd9f189c7a0b0cdae",
    partial_address: "0x0325ee1689daec508c6adef0df4a1e270ac1fcf971fed1f893b2d98ad12d6bb8",
};

pub const TEST_ACCOUNT_2: ImportedTestAccount = ImportedTestAccount {
    alias: "test2",
    address: "0x1dd551228da3a56b5da5f5d73728e08d8114f59897c27136f1bcdd4c05028905",
    secret_key: "0x0f6addf0da06c33293df974a565b03d1ab096090d907d98055a8b7f4954e120c",
    partial_address: "0x17604ccd69bd09d8df02c4a345bc4232e5d24b568536c55407b3e4e4e3354c4c",
};

pub fn node_url() -> String {
    std::env::var("AZTEC_NODE_URL").unwrap_or_else(|_| "http://localhost:8080".to_owned())
}

pub fn ethereum_url() -> String {
    std::env::var("ETHEREUM_HOST").unwrap_or_else(|_| "http://localhost:8545".to_owned())
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

pub fn next_unique_salt() -> Fr {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_SALT: OnceLock<AtomicU64> = OnceLock::new();
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    Fr::from(
        NEXT_SALT
            .get_or_init(|| AtomicU64::new(seed))
            .fetch_add(1, Ordering::Relaxed),
    )
}

pub fn imported_complete_address(account: ImportedTestAccount) -> CompleteAddress {
    let expected_address =
        AztecAddress(Fr::from_hex(account.address).expect("valid test account address"));
    let secret_key = Fr::from_hex(account.secret_key).expect("valid test account secret key");
    let partial_address =
        Fr::from_hex(account.partial_address).expect("valid test account partial address");
    let complete =
        complete_address_from_secret_key_and_partial_address(&secret_key, &partial_address)
            .expect("derive complete address");
    assert_eq!(complete.address, expected_address);
    complete
}

pub fn load_artifact_from_candidates(
    display_name: &str,
    candidates: &[PathBuf],
) -> ContractArtifact {
    for path in candidates {
        if let Ok(json) = fs::read_to_string(path) {
            return ContractArtifact::from_nargo_json(&json)
                .unwrap_or_else(|e| panic!("parse {display_name} from {}: {e}", path.display()));
        }
    }

    let searched = candidates
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    panic!("could not locate {display_name}; searched: {searched}");
}

pub fn try_load_artifact_from_candidates(candidates: &[PathBuf]) -> Option<ContractArtifact> {
    for path in candidates {
        if let Ok(json) = fs::read_to_string(path) {
            return ContractArtifact::from_nargo_json(&json).ok();
        }
    }
    None
}

pub fn load_token_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/token_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse token contract")
}

pub fn load_schnorr_account_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/schnorr_account_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse schnorr account artifact")
}

pub fn load_test_contract_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/test_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse test contract artifact")
}

pub fn load_stateful_test_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/stateful_test_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse stateful test artifact")
}

pub fn load_test_log_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/test_log_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse test log artifact")
}

pub fn load_scope_test_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/scope_test_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse scope test artifact")
}

pub fn load_note_getter_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/note_getter_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse note getter artifact")
}

pub fn load_auth_wit_test_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/auth_wit_test_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse authwit artifact")
}

pub fn load_generic_proxy_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/generic_proxy_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse generic proxy artifact")
}

pub fn load_updatable_artifact() -> Option<ContractArtifact> {
    try_load_artifact_from_candidates(&[
        repo_root().join("fixtures/updatable_contract_compiled.json")
    ])
}

pub fn load_updated_artifact() -> Option<ContractArtifact> {
    try_load_artifact_from_candidates(
        &[repo_root().join("fixtures/updated_contract_compiled.json")],
    )
}

pub fn load_sponsored_fpc_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/sponsored_fpc_contract_compiled.json"),
        root.join(
            "../aztec-packages/noir-projects/noir-contracts/target/sponsored_fpc_contract-SponsoredFPC.json",
        ),
    ])
}

pub fn load_fee_juice_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/fee_juice_contract_compiled.json"),
        root.join(
            "../aztec-packages/noir-projects/noir-contracts/target/fee_juice_contract-FeeJuice.json",
        ),
    ])
}

pub fn make_signing_key_note(
    account_contract: &SchnorrAccountContract,
    owner: AztecAddress,
    nullifier_seed: u64,
) -> StoredNote {
    let signing_pk = account_contract.signing_public_key();
    let mut hex = format!("0xdeadbeef{nullifier_seed:0>56x}");
    hex.truncate(66);
    StoredNote {
        contract_address: owner,
        owner,
        storage_slot: Fr::from(1u64),
        randomness: Fr::zero(),
        note_nonce: Fr::from(1u64),
        note_hash: Fr::from(1u64),
        siloed_nullifier: Fr::from_hex(&hex).expect("unique nullifier"),
        note_data: vec![signing_pk.x, signing_pk.y],
        nullified: false,
        is_pending: false,
        nullification_block_number: None,
        leaf_index: None,
        block_number: None,
        tx_index_in_block: None,
        note_index_in_tx: None,
        scopes: vec![owner],
    }
}

pub async fn seed_signing_key_note(
    pxe: &EmbeddedPxe<HttpNodeClient>,
    account_contract: &SchnorrAccountContract,
    owner: AztecAddress,
    nullifier_seed: u64,
) {
    let note = make_signing_key_note(account_contract, owner, nullifier_seed);
    pxe.note_store()
        .add_note(&note)
        .await
        .expect("seed signing key");
}

pub async fn register_protocol_contracts(pxe: &EmbeddedPxe<HttpNodeClient>) {
    if let Some(artifact) = load_fee_juice_artifact() {
        let fee_juice_address = aztec_rs::constants::protocol_contract_address::fee_juice();
        let class_id = compute_contract_class_id_from_artifact(&artifact).unwrap_or(Fr::zero());
        let _ = pxe
            .contract_store()
            .add_artifact(&class_id, &artifact)
            .await;
        let instance = ContractInstanceWithAddress {
            address: fee_juice_address,
            inner: ContractInstance {
                version: 1,
                salt: Fr::zero(),
                deployer: AztecAddress::zero(),
                current_contract_class_id: class_id,
                original_contract_class_id: class_id,
                initialization_hash: Fr::zero(),
                public_keys: PublicKeys::default(),
            },
        };
        let _ = pxe.contract_store().add_instance(&instance).await;
    }
}

pub async fn setup_wallet(account: ImportedTestAccount) -> Option<(TestWallet, AztecAddress)> {
    let node = create_aztec_node_client(node_url());
    if node.get_node_info().await.is_err() {
        return None;
    }

    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = EmbeddedPxe::create(node.clone(), kv).await.ok()?;

    let secret_key = Fr::from_hex(account.secret_key).expect("valid secret key");
    let complete = imported_complete_address(account);
    pxe.key_store().add_account(&secret_key).await.ok()?;
    pxe.address_store().add(&complete).await.ok()?;

    let account_contract = SchnorrAccountContract::new(secret_key);
    let compiled_account_artifact = load_schnorr_account_artifact();
    let dynamic_artifact = account_contract.contract_artifact().await.ok()?;
    let dynamic_class_id = compute_contract_class_id_from_artifact(&dynamic_artifact).ok()?;

    pxe.contract_store()
        .add_artifact(&dynamic_class_id, &compiled_account_artifact)
        .await
        .ok()?;
    let account_instance = ContractInstanceWithAddress {
        address: complete.address,
        inner: ContractInstance {
            version: 1,
            salt: Fr::zero(),
            deployer: AztecAddress::zero(),
            current_contract_class_id: dynamic_class_id,
            original_contract_class_id: dynamic_class_id,
            initialization_hash: Fr::zero(),
            public_keys: complete.public_keys.clone(),
        },
    };
    pxe.contract_store()
        .add_instance(&account_instance)
        .await
        .ok()?;

    seed_signing_key_note(&pxe, &account_contract, complete.address, 1).await;
    register_protocol_contracts(&pxe).await;

    let provider =
        SingleAccountProvider::new(complete.clone(), Box::new(account_contract), account.alias);
    Some((BaseWallet::new(pxe, node, provider), complete.address))
}

pub async fn setup_wallet_with_accounts(
    primary: ImportedTestAccount,
    extra: &[ImportedTestAccount],
) -> Option<(TestWallet, AztecAddress)> {
    let (wallet, address) = setup_wallet(primary).await?;
    for account in extra {
        let sk = Fr::from_hex(account.secret_key).expect("valid extra secret key");
        let ca = imported_complete_address(*account);
        wallet.pxe().key_store().add_account(&sk).await.ok()?;
        wallet.pxe().address_store().add(&ca).await.ok()?;
    }
    Some((wallet, address))
}

pub async fn create_wallet(
    primary: ImportedTestAccount,
    extra: &[ImportedTestAccount],
) -> Option<(SharedTestWallet, AztecAddress)> {
    let (wallet, address) = setup_wallet(primary).await?;
    let wallet = Arc::new(wallet);
    for account in extra {
        let sk = Fr::from_hex(account.secret_key).expect("valid extra secret key");
        let ca = imported_complete_address(*account);
        wallet.pxe().key_store().add_account(&sk).await.ok()?;
        wallet.pxe().address_store().add(&ca).await.ok()?;
        wallet.pxe().register_sender(&ca.address).await.ok()?;

        let contract = SchnorrAccountContract::new(sk);
        let dynamic_artifact = contract.contract_artifact().await.ok()?;
        let class_id = compute_contract_class_id_from_artifact(&dynamic_artifact).ok()?;
        wallet
            .pxe()
            .contract_store()
            .add_artifact(&class_id, &load_schnorr_account_artifact())
            .await
            .ok()?;
        wallet
            .pxe()
            .contract_store()
            .add_instance(&ContractInstanceWithAddress {
                address: ca.address,
                inner: ContractInstance {
                    version: 1,
                    salt: Fr::zero(),
                    deployer: AztecAddress::zero(),
                    current_contract_class_id: class_id,
                    original_contract_class_id: class_id,
                    initialization_hash: Fr::zero(),
                    public_keys: ca.public_keys.clone(),
                },
            })
            .await
            .ok()?;
    }
    Some((wallet, address))
}

pub async fn setup_registered_schnorr_wallet(
    secret_key: Fr,
    complete: CompleteAddress,
    instance: ContractInstanceWithAddress,
    alias: impl Into<String>,
) -> Result<
    (
        TestWallet,
        AztecAddress,
        ContractInstanceWithAddress,
        CompleteAddress,
    ),
    aztec_rs::Error,
> {
    let node = create_aztec_node_client(node_url());
    wait_for_node(&node).await?;

    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = EmbeddedPxe::create(node.clone(), kv).await?;

    let contract = SchnorrAccountContract::new(secret_key);
    let compiled_account_artifact = load_schnorr_account_artifact();
    let class_id = instance.inner.current_contract_class_id;

    pxe.key_store().add_account(&secret_key).await?;
    pxe.address_store().add(&complete).await?;
    pxe.contract_store()
        .add_artifact(&class_id, &compiled_account_artifact)
        .await?;
    pxe.contract_store().add_instance(&instance).await?;
    seed_signing_key_note(&pxe, &contract, complete.address, 1).await;
    register_protocol_contracts(&pxe).await;

    let provider = SingleAccountProvider::new(complete.clone(), Box::new(contract), alias.into());
    Ok((
        BaseWallet::new(pxe, node, provider),
        complete.address,
        instance,
        complete,
    ))
}

pub async fn register_contract_on_pxe(
    pxe: &impl Pxe,
    artifact: &ContractArtifact,
    instance: &ContractInstanceWithAddress,
) -> Result<(), aztec_rs::Error> {
    pxe.register_contract_class(artifact).await?;
    pxe.register_contract(RegisterContractRequest {
        instance: instance.clone(),
        artifact: Some(artifact.clone()),
    })
    .await
}

pub async fn deploy_contract(
    wallet: &impl Wallet,
    artifact: ContractArtifact,
    constructor_args: Vec<AbiValue>,
    from: AztecAddress,
) -> Result<(AztecAddress, ContractArtifact, ContractInstanceWithAddress), aztec_rs::Error> {
    let result = Contract::deploy(wallet, artifact.clone(), constructor_args, None)?
        .send(
            &DeployOptions {
                contract_address_salt: Some(next_unique_salt()),
                ..Default::default()
            },
            SendOptions {
                from,
                ..Default::default()
            },
        )
        .await?;
    Ok((result.instance.address, artifact, result.instance))
}

pub fn build_call(
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
) -> FunctionCall {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|_| panic!("function '{method_name}' not found in artifact"));
    FunctionCall {
        to: contract_address,
        selector: func.selector.expect("selector"),
        args,
        function_type: func.function_type.clone(),
        is_static: func.is_static,
        hide_msg_sender: false,
    }
}

pub async fn send_call(
    wallet: &impl Wallet,
    call: FunctionCall,
    from: AztecAddress,
) -> Result<TxHash, aztec_rs::Error> {
    let result = wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from,
                ..Default::default()
            },
        )
        .await?;
    Ok(result.tx_hash)
}

pub async fn advance_block(wallet: &impl Wallet, from: AztecAddress) {
    let _ = wallet
        .send_tx(
            ExecutionPayload::default(),
            SendOptions {
                from,
                ..Default::default()
            },
        )
        .await;
}

pub async fn wait_for_l1_to_l2_message_ready_by_advancing(
    wallet: &TestWallet,
    from: AztecAddress,
    msg_hash: &Fr,
    max_blocks: usize,
) -> Result<bool, aztec_rs::Error> {
    for _ in 0..max_blocks {
        advance_block(wallet, from).await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if aztec_rs::cross_chain::is_l1_to_l2_message_ready(wallet.pxe().node(), msg_hash).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub async fn send_token_method(
    wallet: &impl Wallet,
    artifact: &ContractArtifact,
    token_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    from: AztecAddress,
) -> Result<TxHash, aztec_rs::Error> {
    send_call(
        wallet,
        build_call(artifact, token_address, method_name, args),
        from,
    )
    .await
}

pub async fn call_utility_u64(
    wallet: &impl Wallet,
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    scope: AztecAddress,
) -> Result<u64, aztec_rs::Error> {
    let func = artifact.find_function(method_name)?;
    let result = wallet
        .execute_utility(
            FunctionCall {
                to: contract_address,
                selector: func.selector.expect("selector"),
                args,
                function_type: FunctionType::Utility,
                is_static: false,
                hide_msg_sender: false,
            },
            ExecuteUtilityOptions {
                scope,
                auth_witnesses: vec![],
            },
        )
        .await?;

    Ok(result
        .result
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|value| value.as_str())
        .and_then(|value| Fr::from_hex(value).ok())
        .map_or(0u64, |value| value.to_usize() as u64))
}

pub async fn call_utility_u128(
    wallet: &impl Wallet,
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    scope: AztecAddress,
) -> Result<u128, aztec_rs::Error> {
    Ok(u128::from(
        call_utility_u64(wallet, artifact, contract_address, method_name, args, scope).await?,
    ))
}

pub fn derive_storage_slot_in_map(base_slot: u64, key: &AztecAddress) -> Fr {
    const DOM_SEP_PUBLIC_STORAGE_MAP_SLOT: u32 = 4_015_149_901;
    poseidon2_hash_with_separator(
        &[Fr::from(base_slot), Fr::from(*key)],
        DOM_SEP_PUBLIC_STORAGE_MAP_SLOT,
    )
}

pub async fn read_public_storage(
    wallet: &TestWallet,
    contract: AztecAddress,
    slot: Fr,
) -> Result<Fr, aztec_rs::Error> {
    wallet
        .pxe()
        .node()
        .get_public_storage_at(0, &contract, &slot)
        .await
}

pub async fn read_public_u128(
    wallet: &TestWallet,
    contract: AztecAddress,
    slot: Fr,
) -> Result<u128, aztec_rs::Error> {
    let raw = read_public_storage(wallet, contract, slot).await?;
    let bytes = raw.to_be_bytes();
    Ok(u128::from_be_bytes(
        bytes[16..32].try_into().expect("16 bytes"),
    ))
}

pub fn abi_address(address: AztecAddress) -> AbiValue {
    let mut fields = BTreeMap::new();
    fields.insert("inner".to_owned(), AbiValue::Field(Fr::from(address)));
    AbiValue::Struct(fields)
}

pub fn abi_selector(selector: FunctionSelector) -> AbiValue {
    let mut fields = BTreeMap::new();
    fields.insert(
        "inner".to_owned(),
        AbiValue::Integer(u32::from_be_bytes(selector.0).into()),
    );
    AbiValue::Struct(fields)
}

pub fn parse_eth_address(hex_str: &str) -> EthAddress {
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let mut bytes = [0u8; 20];
    let nibbles: Vec<u8> = hex_str
        .chars()
        .filter_map(|c| c.to_digit(16).map(|d| d as u8))
        .collect();
    let len = nibbles.len() / 2;
    let start = 20usize.saturating_sub(len);
    for i in 0..len.min(20) {
        bytes[start + i] = (nibbles[i * 2] << 4) | nibbles[i * 2 + 1];
    }
    EthAddress(bytes)
}

pub fn eth_address_as_field(addr: &EthAddress) -> Fr {
    let mut bytes = [0u8; 32];
    bytes[12..32].copy_from_slice(&addr.0);
    Fr::from(bytes)
}

pub fn build_proxy_call(
    proxy_artifact: &ContractArtifact,
    proxy_address: AztecAddress,
    action: &FunctionCall,
) -> FunctionCall {
    let method_name = format!("forward_private_{}", action.args.len());
    build_call(
        proxy_artifact,
        proxy_address,
        &method_name,
        vec![
            abi_address(action.to),
            abi_selector(action.selector),
            AbiValue::Array(action.args.clone()),
        ],
    )
}

pub fn public_balance_slot(address: &AztecAddress) -> Fr {
    derive_storage_slot_in_map(5, address)
}

pub async fn public_balance(
    wallet: &TestWallet,
    token: AztecAddress,
    account: &AztecAddress,
) -> Result<u128, aztec_rs::Error> {
    read_public_u128(wallet, token, public_balance_slot(account)).await
}

pub async fn mint_tokens_to_private(
    wallet: &impl Wallet,
    token_address: AztecAddress,
    artifact: &ContractArtifact,
    from: AztecAddress,
    to: AztecAddress,
    amount: u64,
) -> Result<TxHash, aztec_rs::Error> {
    send_token_method(
        wallet,
        artifact,
        token_address,
        "mint_to_private",
        vec![
            AbiValue::Field(Fr::from(to)),
            AbiValue::Integer(i128::from(amount)),
        ],
        from,
    )
    .await
}

pub async fn deploy_token(
    wallet: &impl Wallet,
    admin: AztecAddress,
    initial_private_balance: u64,
) -> Result<(AztecAddress, ContractArtifact, ContractInstanceWithAddress), aztec_rs::Error> {
    let artifact = load_token_artifact();
    let (token_address, artifact, instance) = deploy_contract(
        wallet,
        artifact,
        vec![
            AbiValue::Field(Fr::from(admin)),
            AbiValue::String("TestToken".to_owned()),
            AbiValue::String("TT".to_owned()),
            AbiValue::Integer(18),
        ],
        admin,
    )
    .await?;

    if initial_private_balance > 0 {
        mint_tokens_to_private(
            wallet,
            token_address,
            &artifact,
            admin,
            admin,
            initial_private_balance,
        )
        .await?;
    }

    Ok((token_address, artifact, instance))
}

pub async fn private_token_balance(
    wallet: &impl Wallet,
    artifact: &ContractArtifact,
    token_address: AztecAddress,
    owner: AztecAddress,
) -> Result<u64, aztec_rs::Error> {
    call_utility_u64(
        wallet,
        artifact,
        token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(owner))],
        owner,
    )
    .await
}

pub fn event_selector_from_signature(signature: &str) -> EventSelector {
    EventSelector(FunctionSelector::from_signature(signature).to_field())
}

pub fn tx_hash_block_number(receipt: &serde_json::Value) -> Option<u64> {
    receipt.get("blockNumber").and_then(|value| value.as_u64())
}
