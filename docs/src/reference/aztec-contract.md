# `aztec-contract`

Contract interaction, deployment, authorization witnesses, and event reading.

Source: `crates/contract/src/`.

## Module Map

| Module       | Highlights                                                                                      |
| ------------ | ----------------------------------------------------------------------------------------------- |
| `contract`   | `Contract<W>`, `ContractFunctionInteraction<'a, W>`, `BatchCall<'a, W>`                         |
| `deployment` | `ContractDeployer<'a, W>`, `DeployMethod<'a, W>`, `DeployOptions`, `DeployResult`, `publish_contract_class`, `publish_instance`, `get_contract_instance_from_instantiation_params`, `get_gas_limits`, `SuggestedGasLimits`, `ContractInstantiationParams` |
| `authwit`    | `SetPublicAuthWitInteraction<'a, W>`, `AuthWitValidity`, `lookup_validity`                      |
| `events`     | `PublicEvent<T>`, `PublicEventMetadata`, `PublicEventFilter`, `GetPublicEventsResult<T>`, `get_public_events` |

## Contract Handles

```rust,ignore
use aztec_contract::contract::Contract;
use aztec_contract::abi::{AbiValue, ContractArtifact};

let artifact = load_artifact_from_file("fixtures/token_contract_compiled.json")?;
let handle = Contract::at(token_address, artifact, wallet.clone());

// Build a call — method name + ABI-encoded args
let call = handle.method("transfer", vec![
    AbiValue::Struct(/* recipient */),
    AbiValue::Field(amount.into()),
])?;

// Simulate / profile / send
let sim = call.simulate(Default::default()).await?;
let sent = call.send(Default::default()).await?;
```

`Contract::method` performs ABI lookup and argument arity validation against the artifact.
The returned `ContractFunctionInteraction` offers:

- `request()` — the `ExecutionPayload` (for inspection or hand-off to the wallet).
- `simulate(opts)` — returns a `TxSimulationResult`.
- `profile(opts)` — returns a `TxProfileResult` (gate counts / steps).
- `send(opts)` — submits the tx, returns a `SendResult`.
- `with(auth_witnesses, capsules)` — attach authwits + capsules before submission.

`BatchCall` bundles several calls into one transaction, exposing the same simulate / profile / send API.

## Deployment

```rust,ignore
use aztec_contract::deployment::{ContractDeployer, DeployOptions};
use aztec_wallet::SendOptions;

let deployer = ContractDeployer::new(artifact, &wallet)
    .with_constructor_name("constructor")
    .with_public_keys(public_keys);

let deploy = deployer.deploy(constructor_args)?;
let result = deploy
    .send(&DeployOptions::default(), SendOptions::default())
    .await?;
println!("contract address = {}", result.instance.address);
```

Low-level helpers:

- `publish_contract_class(wallet, &artifact)` — builds the class-publication interaction.
- `publish_instance(wallet, &instance)` — builds the instance-publication interaction.
- `get_contract_instance_from_instantiation_params(&artifact, params)` — computes a deterministic `ContractInstanceWithAddress` (address, class id, init hash) without sending a tx.
- `get_gas_limits(...)` — suggests `SuggestedGasLimits` given an instantiation.

## Events

```rust,ignore
use aztec_contract::events::{get_public_events, PublicEventFilter};

let filter = PublicEventFilter::new(token_address, from_block, to_block)
    .with_event::<MyEvent>();

let GetPublicEventsResult { events, .. } = get_public_events::<MyEvent>(&node, filter).await?;
for PublicEvent { metadata, data } in events {
    /* ... */
}
```

Private events are read from the PXE via [`Pxe::get_private_events`](./aztec-pxe-client.md).

## Authwits

`SetPublicAuthWitInteraction` is a helper for the canonical "set a public authwit" flow via the AuthRegistry protocol contract.
`lookup_validity(wallet, witness)` returns `AuthWitValidity`, which indicates whether a given witness is still consumable.

## Full API

Bundled rustdoc: [`api/aztec_contract/`](../api/aztec_contract/index.html).
Local regeneration:

```bash
cargo doc -p aztec-contract --open
```

## See Also

- [`aztec-wallet`](./aztec-wallet.md) — the `Wallet` trait these APIs are generic over.
- [`aztec-core`](./aztec-core.md) — `ContractArtifact`, `AbiValue`, `FunctionCall`, `ExecutionPayload`.
