//! Create private and public L2 to L1 messages in one transaction.

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
    let l1_chain_id = node_info.l1_chain_id;
    let rollup_version = node_info.rollup_version;
    let eth_client = EthClient::new(&ethereum_url());
    let eth_account = parse_eth_address(&eth_client.get_account().await?);

    let (test_address, test_artifact, _) =
        deploy_contract(&wallet, load_test_contract_artifact(), vec![], owner).await?;
    let recipient_field = eth_address_as_field(&eth_account);
    let private_content = Fr::random();
    let public_content = Fr::random();

    let batch = BatchCall::new(
        &wallet,
        vec![
            ExecutionPayload {
                calls: vec![build_call(
                    &test_artifact,
                    test_address,
                    "create_l2_to_l1_message_arbitrary_recipient_private",
                    vec![
                        AbiValue::Field(private_content),
                        AbiValue::Field(recipient_field),
                    ],
                )],
                ..Default::default()
            },
            ExecutionPayload {
                calls: vec![build_call(
                    &test_artifact,
                    test_address,
                    "create_l2_to_l1_message_arbitrary_recipient_public",
                    vec![
                        AbiValue::Field(public_content),
                        AbiValue::Field(recipient_field),
                    ],
                )],
                ..Default::default()
            },
        ],
    );

    let tx_hash = batch
        .send(SendOptions {
            from: owner,
            ..Default::default()
        })
        .await?
        .tx_hash;

    println!("Test contract:      {test_address}");
    println!("Recipient:          {eth_account:?}");
    println!("L1 chain ID:        {l1_chain_id}");
    println!("Rollup version:     {rollup_version}");
    println!("Batch tx hash:      {tx_hash}");

    Ok(())
}
