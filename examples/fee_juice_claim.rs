//! Bridge `FeeJuice` from L1, claim it, and use it as a fee payment payload.

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
    let bob = imported_complete_address(TEST_ACCOUNT_1).address;

    let node_info = wallet.pxe().node().get_node_info().await?;
    let l1_addresses = L1ContractAddresses::from_json(&node_info.l1_contract_addresses)
        .ok_or_else(|| aztec_rs::Error::InvalidData("missing L1 contract addresses".to_owned()))?;
    let eth_client = EthClient::new(&ethereum_url());
    let bridge = l1_client::prepare_fee_juice_on_l1(&eth_client, &l1_addresses, &owner).await?;
    let ready =
        wait_for_l1_to_l2_message_ready_by_advancing(&wallet, owner, &bridge.message_hash, 30)
            .await?;
    if !ready {
        return Err(aztec_rs::Error::Timeout(format!(
            "L1-to-L2 message {} was not ready after advancing 30 L2 blocks",
            bridge.message_hash
        )));
    }

    let fee_payload = FeeJuicePaymentMethodWithClaim::new(
        owner,
        aztec_rs::fee::L2AmountClaim {
            claim_amount: bridge.claim_amount,
            claim_secret: bridge.claim_secret,
            message_leaf_index: bridge.message_leaf_index,
        },
    )
    .get_fee_execution_payload()
    .await?;

    let (token_address, token_artifact, _) = deploy_token(&wallet, owner, 0).await?;
    send_token_method(
        &wallet,
        &token_artifact,
        token_address,
        "mint_to_public",
        vec![AbiValue::Field(Fr::from(owner)), AbiValue::Integer(1_000)],
        owner,
    )
    .await?;

    let tx_hash = wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &token_artifact,
                    token_address,
                    "transfer_in_public",
                    vec![
                        AbiValue::Field(Fr::from(owner)),
                        AbiValue::Field(Fr::from(bob)),
                        AbiValue::Integer(10),
                        AbiValue::Integer(0),
                    ],
                )],
                ..Default::default()
            },
            SendOptions {
                from: owner,
                fee_execution_payload: Some(fee_payload),
                ..Default::default()
            },
        )
        .await?
        .tx_hash;

    println!("Message hash:       {}", bridge.message_hash);
    println!("Claim amount:       {}", bridge.claim_amount);
    println!("Leaf index:         {}", bridge.message_leaf_index);
    println!("Token address:      {token_address}");
    println!("Transfer tx hash:   {tx_hash}");

    Ok(())
}
