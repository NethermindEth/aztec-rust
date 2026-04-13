//! Send an L1 to L2 message and consume it on L2.

#![allow(clippy::print_stdout, clippy::wildcard_imports)]

mod common;

use common::*;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let Some((wallet, owner)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return Err(aztec_rs::Error::InvalidData(format!(
            "node not reachable at {}",
            node_url()
        )));
    };

    let node_info = wallet.pxe().node().get_node_info().await?;
    let rollup_version = node_info.rollup_version;
    let l1_addresses = L1ContractAddresses::from_json(&node_info.l1_contract_addresses)
        .ok_or_else(|| aztec_rs::Error::InvalidData("missing L1 addresses".to_owned()))?;
    let eth_client = EthClient::new(&ethereum_url());

    let (test_address, test_artifact, _) =
        deploy_contract(&wallet, load_test_contract_artifact(), vec![], owner).await?;
    let (secret, secret_hash) = messaging::generate_claim_secret();
    let content = Fr::random();

    let sent = l1_client::send_l1_to_l2_message(
        &eth_client,
        &l1_addresses.inbox,
        &test_address,
        rollup_version,
        &content,
        &secret_hash,
    )
    .await?;
    let ready =
        wait_for_l1_to_l2_message_ready_by_advancing(&wallet, owner, &sent.msg_hash, 30).await?;
    if !ready {
        return Err(aztec_rs::Error::Timeout(format!(
            "L1-to-L2 message {} was not ready after advancing 30 L2 blocks",
            sent.msg_hash
        )));
    }

    let l1_sender = eth_client.get_account().await?;
    let consume_hash = send_call(
        &wallet,
        build_call(
            &test_artifact,
            test_address,
            "consume_message_from_arbitrary_sender_private",
            vec![
                AbiValue::Field(content),
                AbiValue::Field(secret),
                AbiValue::Field(eth_address_as_field(&parse_eth_address(&l1_sender))),
                AbiValue::Field(sent.global_leaf_index),
            ],
        ),
        owner,
    )
    .await?;

    println!("Test contract:      {test_address}");
    println!("L1->L2 message:     {}", sent.msg_hash);
    println!("Leaf index:         {}", sent.global_leaf_index);
    println!("Consume tx hash:    {consume_hash}");

    Ok(())
}
