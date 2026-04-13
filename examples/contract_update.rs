//! Publish a new class and schedule a contract update.

#![allow(clippy::print_stdout, clippy::wildcard_imports)]

mod common;

use common::*;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let Some(updatable_artifact) = load_updatable_artifact() else {
        println!("Missing updatable contract artifact.");
        return Ok(());
    };
    let Some(updated_artifact) = load_updated_artifact() else {
        println!("Missing updated contract artifact.");
        return Ok(());
    };
    let Some((wallet, owner)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return Err(aztec_rs::Error::InvalidData(format!(
            "node not reachable at {}",
            node_url()
        )));
    };

    let deploy = Contract::deploy(
        &wallet,
        updatable_artifact.clone(),
        vec![AbiValue::Field(Fr::from(1u64))],
        Some("initialize"),
    )?;
    let deployed = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(next_unique_salt()),
                ..Default::default()
            },
            SendOptions {
                from: owner,
                ..Default::default()
            },
        )
        .await?;

    let publish_tx = match publish_contract_class(&wallet, &updated_artifact)
        .await?
        .send(SendOptions {
            from: owner,
            ..Default::default()
        })
        .await
    {
        Ok(result) => Some(result.tx_hash),
        Err(err) => {
            let err_str = err.to_string().to_lowercase();
            if err_str.contains("existing nullifier")
                || err_str.contains("duplicate")
                || err_str.contains("dropped")
            {
                None
            } else {
                return Err(err);
            }
        }
    };
    let updated_class_id = compute_contract_class_id_from_artifact(&updated_artifact)?;

    let mut class_id_struct = std::collections::BTreeMap::new();
    class_id_struct.insert("inner".to_owned(), AbiValue::Field(updated_class_id));
    let update_tx = send_call(
        &wallet,
        build_call(
            &updatable_artifact,
            deployed.instance.address,
            "update_to",
            vec![AbiValue::Struct(class_id_struct)],
        ),
        owner,
    )
    .await?;

    println!("Contract address:   {}", deployed.instance.address);
    println!("Published class:    {updated_class_id}");
    println!(
        "Publish tx hash:    {}",
        publish_tx.map_or_else(|| "<already published>".to_owned(), |hash| hash.to_string())
    );
    println!("Update tx hash:     {update_tx}");

    Ok(())
}
