//! Full contract interaction workflow using a real Schnorr wallet.
//!
//! Requires a running Aztec local network:
//!
//! ```bash
//! aztec start --local-network   # starts node on port 8080
//! cargo run --example contract_call
//! ```
//!
//! **Note:** In Aztec v4.x, the PXE is embedded in the client SDK — there is
//! no standalone PXE RPC endpoint. The local network exposes `node_*` methods
//! only. This example demonstrates all local and node-level operations (account
//! derivation, artifact validation, deploy address computation, payload
//! building, batching, augmentation, gas estimation, wait configuration).
//! PXE-dependent operations (simulate, profile, send) require an embedded PXE
//! integration (future SDK work) and are attempted but may fail gracefully.

#![allow(clippy::print_stdout, clippy::too_many_lines)]

use std::time::Duration;

use aztec_rs::abi::{abi_checker, AbiValue, ContractArtifact, ContractStorageLayout, FieldLayout};
use aztec_rs::account::{AccountManager, SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::{BatchCall, Contract};
use aztec_rs::deployment::{get_gas_limits, DeployOptions};
use aztec_rs::node::{create_aztec_node_client, wait_for_node, AztecNode, WaitOpts};
use aztec_rs::tx::{AuthWitness, Capsule, ExecutionPayload, TxStatus};
use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
use aztec_rs::wallet::{
    create_embedded_wallet, ProfileMode, ProfileOptions, SendOptions, SimulateOptions,
    TxSimulationResult,
};

async fn make_wallet(
    node_url: &str,
    secret_key: Fr,
    complete_address: CompleteAddress,
    alias: &str,
) -> Result<impl aztec_rs::wallet::Wallet, aztec_rs::Error> {
    create_embedded_wallet(
        node_url,
        SingleAccountProvider::new(
            complete_address,
            Box::new(SchnorrAccountContract::new(secret_key)),
            alias,
        ),
    )
    .await
}

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let node_url =
        std::env::var("AZTEC_NODE_URL").unwrap_or_else(|_| "http://localhost:8080".into());

    // -- Connect to the network ------------------------------------------------

    let node = create_aztec_node_client(&node_url);
    let info = wait_for_node(&node).await?;
    println!(
        "Node: v={}, chain={}, block={}",
        info.node_version,
        info.l1_chain_id,
        node.get_block_number().await?
    );

    // -- Derive account from secret key ----------------------------------------

    let secret_key = Fr::from(0xdead_beef_u64);
    let account_contract = SchnorrAccountContract::new(secret_key);
    println!(
        "Signing pubkey: {:?}",
        account_contract.signing_public_key()
    );

    let bootstrap = make_wallet(
        &node_url,
        secret_key,
        CompleteAddress::default(),
        "bootstrap",
    )
    .await?;
    let manager = AccountManager::create(
        bootstrap,
        secret_key,
        Box::new(account_contract),
        Some(Fr::from(1u64)),
    )
    .await?;

    let complete_address = manager.complete_address().await?;
    let from = complete_address.address;
    println!("Account: {from}");

    // -- Load and validate artifact --------------------------------------------

    let artifact = ContractArtifact::from_json(include_str!("../fixtures/token_contract.json"))?;
    println!(
        "Artifact: {} ({} functions)",
        artifact.name,
        artifact.functions.len()
    );

    let errors = abi_checker(&artifact);
    if !errors.is_empty() {
        for e in &errors {
            println!("  ABI error: {e}");
        }
    }

    // -- Storage layout --------------------------------------------------------

    let mut storage = ContractStorageLayout::new();
    storage.insert(
        "balances".into(),
        FieldLayout {
            slot: Fr::from(1u64),
        },
    );
    storage.insert(
        "total_supply".into(),
        FieldLayout {
            slot: Fr::from(2u64),
        },
    );
    if let Ok(json) = serde_json::to_string(&storage) {
        println!("Storage: {json}");
    }

    // -- Compute deploy address (local, no PXE) --------------------------------

    let deploy_wallet =
        make_wallet(&node_url, secret_key, complete_address.clone(), "deploy").await?;
    let deploy_method = Contract::deploy(
        &deploy_wallet,
        artifact.clone(),
        vec![
            AbiValue::Field(from.0),
            AbiValue::String("TestToken".into()),
            AbiValue::String("TT".into()),
            AbiValue::Integer(18),
        ],
        None,
    )?;

    let deploy_opts = DeployOptions {
        contract_address_salt: Some(Fr::from(42u64)),
        universal_deploy: true,
        skip_registration: true,
        ..Default::default()
    };
    let instance = deploy_method.get_instance(&deploy_opts)?;
    println!("Deploy target: {}", instance.address);
    println!("  class_id: {}", instance.inner.current_contract_class_id);

    // -- Build payloads locally (no PXE) ---------------------------------------

    let wallet = make_wallet(&node_url, secret_key, complete_address.clone(), "main").await?;
    let contract = Contract::at(instance.address, artifact.clone(), wallet);

    let transfer_payload = contract
        .method(
            "transfer",
            vec![
                AbiValue::Field(from.0),
                AbiValue::Field(Fr::from(2u64)),
                AbiValue::Integer(100),
                AbiValue::Field(Fr::from(0u64)),
            ],
        )?
        .request()?;
    println!("Transfer payload: {} call(s)", transfer_payload.calls.len());

    // Augment with auth witnesses and capsules.
    let augmented = contract.method("total_supply", vec![])?.with(
        vec![AuthWitness {
            request_hash: Fr::from(123u64),
            fields: vec![Fr::from(10u64)],
        }],
        vec![Capsule {
            contract_address: instance.address,
            storage_slot: Fr::from(1u64),
            data: vec![Fr::from(42u64)],
        }],
    );
    let call = augmented.get_function_call();
    println!(
        "total_supply: selector={}, is_static={}",
        call.selector, call.is_static
    );
    let payload = augmented.request()?;
    println!(
        "  augmented: {} calls, {} auth_witnesses, {} capsules",
        payload.calls.len(),
        payload.auth_witnesses.len(),
        payload.capsules.len()
    );

    // Batch multiple payloads.
    let p1 = contract
        .method(
            "transfer",
            vec![
                AbiValue::Field(from.0),
                AbiValue::Field(Fr::from(3u64)),
                AbiValue::Integer(200),
                AbiValue::Field(Fr::from(0u64)),
            ],
        )?
        .request()?;
    let p2 = contract
        .method(
            "transfer",
            vec![
                AbiValue::Field(from.0),
                AbiValue::Field(Fr::from(4u64)),
                AbiValue::Integer(300),
                AbiValue::Field(Fr::from(0u64)),
            ],
        )?
        .request()?;
    let batch_wallet =
        make_wallet(&node_url, secret_key, complete_address.clone(), "batch").await?;
    let batch = BatchCall::new(&batch_wallet, vec![p1, p2]);
    let merged = batch.request()?;
    println!("Batch: {} calls merged", merged.calls.len());

    // Fee payload.
    let fee_payload = ExecutionPayload {
        fee_payer: Some(AztecAddress(Fr::from(99u64))),
        ..Default::default()
    };
    println!("Fee payer: {:?}", fee_payload.fee_payer);

    // Gas estimation from a simulation result.
    let mock_sim = TxSimulationResult {
        return_values: serde_json::Value::Null,
        gas_used: Some(aztec_rs::fee::Gas {
            da_gas: 1000,
            l2_gas: 5000,
        }),
    };
    let suggested = get_gas_limits(&mock_sim, Some(0.1));
    println!("Suggested gas: {:?}", suggested.gas_limits);

    // Wait configuration.
    let wait = WaitOpts {
        wait_for_status: TxStatus::Proven,
        timeout: Duration::from_secs(600),
        interval: Duration::from_millis(500),
        dont_throw_on_revert: true,
        ..Default::default()
    };
    println!(
        "WaitOpts: {:?}, timeout={:?}",
        wait.wait_for_status, wait.timeout
    );

    // -- PXE operations (require a running PXE) --------------------------------

    println!("\n--- PXE operations ---");

    // Deploy: simulate, profile, send.
    let sim = deploy_method
        .simulate(
            &deploy_opts,
            SimulateOptions {
                from,
                ..Default::default()
            },
        )
        .await;
    match sim {
        Ok(s) => println!("Deploy sim: gas={:?}", s.gas_used),
        Err(e) => {
            println!("Deploy sim skipped (no PXE RPC endpoint): {e}");
            println!("In Aztec v4.x, PXE is embedded in the client — no remote pxe_* methods.");
            println!("\nDone (local + node operations only).");
            return Ok(());
        }
    }

    let profile = deploy_method
        .profile(
            &deploy_opts,
            ProfileOptions {
                from,
                profile_mode: Some(ProfileMode::Full),
                ..Default::default()
            },
        )
        .await?;
    println!("Deploy profile: gas={:?}", profile.gas_used);

    let deployed = deploy_method
        .send(
            &deploy_opts,
            SendOptions {
                from,
                ..Default::default()
            },
        )
        .await?;
    println!(
        "Deployed: tx={}, addr={}",
        deployed.send_result.tx_hash, deployed.instance.address
    );

    // Interact with the deployed contract.
    let contract = Contract::at(
        instance.address,
        artifact,
        make_wallet(&node_url, secret_key, complete_address.clone(), "interact").await?,
    );

    // Simulate with gas estimation.
    let sim = contract
        .method("balance_of_public", vec![AbiValue::Field(from.0)])?
        .simulate(SimulateOptions {
            from,
            estimate_gas: true,
            estimated_gas_padding: Some(0.15),
            ..Default::default()
        })
        .await?;
    println!(
        "balance_of_public: val={}, gas={:?}",
        sim.return_values, sim.gas_used
    );

    let suggested = get_gas_limits(&sim, Some(0.1));
    println!("Suggested gas: {:?}", suggested.gas_limits);

    // Profile (gates).
    let profile = contract
        .method(
            "transfer",
            vec![
                AbiValue::Field(from.0),
                AbiValue::Field(Fr::from(2u64)),
                AbiValue::Integer(100),
                AbiValue::Field(Fr::from(0u64)),
            ],
        )?
        .profile(ProfileOptions {
            from,
            profile_mode: Some(ProfileMode::Gates),
            ..Default::default()
        })
        .await?;
    println!("transfer profile (gates): {:?}", profile.gas_used);

    // Send transfer.
    let tx = contract
        .method(
            "transfer",
            vec![
                AbiValue::Field(from.0),
                AbiValue::Field(Fr::from(2u64)),
                AbiValue::Integer(50),
                AbiValue::Field(Fr::from(0u64)),
            ],
        )?
        .send(SendOptions {
            from,
            ..Default::default()
        })
        .await?;
    println!("transfer sent: {}", tx.tx_hash);

    // Batch send.
    let batch_tx = batch
        .send(SendOptions {
            from,
            ..Default::default()
        })
        .await?;
    println!("Batch sent: {}", batch_tx.tx_hash);

    // Send with fee payload.
    let tx_with_fee = contract
        .method("total_supply", vec![])?
        .send(SendOptions {
            from,
            fee_execution_payload: Some(fee_payload),
            ..Default::default()
        })
        .await?;
    println!("Sent with fee: {}", tx_with_fee.tx_hash);

    // Swap wallet.
    let contract2 = contract.with_wallet(batch_wallet);
    let sim = contract2
        .method("total_supply", vec![])?
        .simulate(SimulateOptions {
            from,
            ..Default::default()
        })
        .await?;
    println!("with_wallet sim: {}", sim.return_values);

    // wait_for_tx(&node, &tx.tx_hash, wait).await?
    // wait_for_proven(&node, &receipt, WaitForProvenOpts::default()).await?

    println!("\nDone.");
    Ok(())
}
