//! Emit and read public and private events.

#![allow(clippy::print_stdout)]

mod common;

use common::*;

fn example_event0_metadata() -> EventMetadataDefinition {
    EventMetadataDefinition {
        event_selector: event_selector_from_signature("ExampleEvent0(Field,Field)"),
        abi_type: AbiType::Struct {
            name: "ExampleEvent0".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "value0".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                },
                AbiParameter {
                    name: "value1".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                },
            ],
        },
        field_names: vec!["value0".to_owned(), "value1".to_owned()],
    }
}

fn example_event1_metadata() -> EventMetadataDefinition {
    EventMetadataDefinition {
        event_selector: event_selector_from_signature("ExampleEvent1((Field),u8)"),
        abi_type: AbiType::Struct {
            name: "ExampleEvent1".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "value2".to_owned(),
                    typ: AbiType::Struct {
                        name: "AztecAddress".to_owned(),
                        fields: vec![AbiParameter {
                            name: "inner".to_owned(),
                            typ: AbiType::Field,
                            visibility: None,
                        }],
                    },
                    visibility: None,
                },
                AbiParameter {
                    name: "value3".to_owned(),
                    typ: AbiType::Integer {
                        sign: "unsigned".to_owned(),
                        width: 8,
                    },
                    visibility: None,
                },
            ],
        },
        field_names: vec!["value2".to_owned(), "value3".to_owned()],
    }
}

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let Some((wallet, account1)) =
        setup_wallet_with_accounts(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1]).await
    else {
        return Err(aztec_rs::Error::InvalidData(format!(
            "node not reachable at {}",
            node_url()
        )));
    };
    let account2 = imported_complete_address(TEST_ACCOUNT_1).address;
    wallet.pxe().register_sender(&account2).await?;

    let (contract_address, artifact, _) =
        deploy_contract(&wallet, load_test_log_artifact(), vec![], account1).await?;

    let private_tx = send_call(
        &wallet,
        build_call(
            &artifact,
            contract_address,
            "emit_encrypted_events",
            vec![
                abi_address(account2),
                AbiValue::Array(vec![
                    AbiValue::Field(Fr::from(1u64)),
                    AbiValue::Field(Fr::from(2u64)),
                    AbiValue::Field(Fr::from(3u64)),
                    AbiValue::Field(Fr::from(4u64)),
                ]),
            ],
        ),
        account1,
    )
    .await?;

    let public_tx = send_call(
        &wallet,
        build_call(
            &artifact,
            contract_address,
            "emit_unencrypted_events",
            vec![AbiValue::Array(vec![
                AbiValue::Field(Fr::from(11u64)),
                AbiValue::Field(Fr::from(12u64)),
                AbiValue::Field(Fr::from(13u64)),
                AbiValue::Field(Fr::from(14u64)),
            ])],
        ),
        account1,
    )
    .await?;

    let private_block = wallet
        .node()
        .get_tx_receipt(&private_tx)
        .await?
        .block_number
        .expect("block number");
    let public_block = wallet
        .node()
        .get_tx_receipt(&public_tx)
        .await?
        .block_number
        .expect("block number");

    let private_events = wallet
        .get_private_events(
            &example_event0_metadata(),
            PrivateEventFilter {
                contract_address,
                from_block: Some(private_block),
                to_block: Some(private_block + 1),
                scopes: vec![account1, account2],
                ..Default::default()
            },
        )
        .await?;

    let public_events = get_public_events(
        wallet.node(),
        &example_event1_metadata(),
        PublicEventFilter {
            from_block: Some(public_block),
            to_block: Some(public_block + 1),
            ..Default::default()
        },
    )
    .await?;

    println!("Contract address:   {contract_address}");
    println!("Private tx hash:    {private_tx}");
    println!("Public tx hash:     {public_tx}");
    println!("Private events:     {}", private_events.len());
    println!("Public events:      {}", public_events.events.len());

    Ok(())
}
