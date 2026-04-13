//! Use two Schnorr accounts that share one encryption key but have different signing keys.

#![allow(
    clippy::print_stdout,
    clippy::wildcard_imports,
    // `compute_account_config` cannot fail for the canned inputs used in this example.
    clippy::expect_used,
)]

mod common;

use common::*;

#[allow(dead_code)]
struct AccountConfig {
    alias: &'static str,
    secret: Fr,
    signing_key: GrumpkinScalar,
    complete_address: CompleteAddress,
}

fn compute_account_config(
    alias: &'static str,
    secret: Fr,
    signing_key: GrumpkinScalar,
    salt: Fr,
    compiled_artifact: &ContractArtifact,
) -> AccountConfig {
    let contract = SchnorrAccountContract::new_with_signing_key(secret, signing_key);
    let signing_pk = contract.signing_public_key();
    let derived = derive_keys(&secret);
    let instance = aztec_rs::deployment::get_contract_instance_from_instantiation_params(
        compiled_artifact,
        ContractInstantiationParams {
            constructor_name: Some("constructor"),
            constructor_args: vec![AbiValue::Field(signing_pk.x), AbiValue::Field(signing_pk.y)],
            salt,
            public_keys: derived.public_keys.clone(),
            deployer: AztecAddress::zero(),
        },
    )
    .expect("build instance");
    let salted = aztec_rs::hash::compute_salted_initialization_hash(
        instance.inner.salt,
        instance.inner.initialization_hash,
        instance.inner.deployer,
    );
    let partial_address =
        aztec_rs::hash::compute_partial_address(instance.inner.original_contract_class_id, salted);

    AccountConfig {
        alias,
        secret,
        signing_key,
        complete_address: CompleteAddress {
            address: instance.address,
            public_keys: derived.public_keys,
            partial_address,
        },
    }
}

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let node = create_aztec_node_client(node_url());
    wait_for_node(&node).await?;

    let shared_secret = Fr::random();
    let compiled_account = load_schnorr_account_artifact();
    let account_a = compute_account_config(
        "shared-a",
        shared_secret,
        aztec_rs::crypto::derive_signing_key(&Fr::random()),
        next_unique_salt(),
        &compiled_account,
    );
    let account_b = compute_account_config(
        "shared-b",
        shared_secret,
        aztec_rs::crypto::derive_signing_key(&Fr::random()),
        next_unique_salt(),
        &compiled_account,
    );

    println!("Shared secret:      {shared_secret}");
    println!("Account A:          {}", account_a.complete_address.address);
    println!("Account B:          {}", account_b.complete_address.address);
    println!(
        "Shared viewing key: {}",
        account_a
            .complete_address
            .public_keys
            .master_incoming_viewing_public_key
            .x
            == account_b
                .complete_address
                .public_keys
                .master_incoming_viewing_public_key
                .x
    );
    println!(
        "Different addresses:{}",
        account_a.complete_address.address != account_b.complete_address.address
    );

    Ok(())
}
