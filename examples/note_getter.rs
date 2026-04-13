//! Compare note getter utility and simulation paths.

#![allow(clippy::print_stdout)]

mod common;

use common::*;

fn first_field(value: &serde_json::Value) -> String {
    value
        .pointer("/returnValues/0")
        .or_else(|| value.pointer("/0"))
        .and_then(|value| value.as_str())
        .unwrap_or("<missing>")
        .to_owned()
}

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let Some((wallet, owner)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return Err(aztec_rs::Error::InvalidData(format!(
            "node not reachable at {}",
            node_url()
        )));
    };

    let (contract_address, artifact, _) =
        deploy_contract(&wallet, load_test_contract_artifact(), vec![], owner).await?;

    send_call(
        &wallet,
        build_call(
            &artifact,
            contract_address,
            "call_create_note",
            vec![
                AbiValue::Integer(5),
                abi_address(owner),
                AbiValue::Field(Fr::from(7u64)),
                AbiValue::Boolean(false),
            ],
        ),
        owner,
    )
    .await?;

    let view_result = wallet
        .execute_utility(
            build_call(
                &artifact,
                contract_address,
                "call_view_notes",
                vec![
                    abi_address(owner),
                    AbiValue::Field(Fr::from(7u64)),
                    AbiValue::Boolean(false),
                ],
            ),
            ExecuteUtilityOptions {
                scope: owner,
                ..Default::default()
            },
        )
        .await?;
    let get_result = wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &artifact,
                    contract_address,
                    "call_get_notes",
                    vec![
                        abi_address(owner),
                        AbiValue::Field(Fr::from(7u64)),
                        AbiValue::Boolean(false),
                    ],
                )],
                ..Default::default()
            },
            SimulateOptions {
                from: owner,
                ..Default::default()
            },
        )
        .await?;

    println!("Contract address:   {contract_address}");
    println!("Utility result:     {}", view_result.result);
    println!("Sim result:         {}", get_result.return_values);
    println!("Utility first:      {}", first_field(&view_result.result));
    println!(
        "Sim first:          {}",
        first_field(&get_result.return_values)
    );

    Ok(())
}
