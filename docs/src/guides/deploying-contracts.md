# Deploying Contracts

Deploy a compiled Noir / Aztec contract via the builder API in [`aztec-contract`](../reference/aztec-contract.md).

## Runnable Examples

- `examples/deploy_contract.rs` — minimal deployment.
- `examples/deploy_options.rs` — all builder options.
- `examples/account_deploy.rs` — deploying an account contract.
- `examples/contract_update.rs` — updating a contract class.

## Typical Flow

```rust,ignore
use aztec_rs::abi::{AbiValue, ContractArtifact};
use aztec_rs::deployment::{ContractDeployer, DeployOptions};
use aztec_rs::wallet::SendOptions;

let artifact: ContractArtifact = /* load_artifact(...) */;
let deployer = ContractDeployer::new(artifact, &wallet)
    .with_constructor_name("constructor")
    .with_public_keys(public_keys);

let method = deployer.deploy(vec![
    AbiValue::Field(initial_supply.into()),
])?;

let result = method
    .send(&DeployOptions::default(), SendOptions::default())
    .await?;
println!("deployed at: {}", result.instance.address);
```

## What the Deployer Does

1. **Class registration** — publishes the class hash + artifact metadata once per class (idempotent for the same class id).
2. **Instance publication** — creates the deterministic instance at an address derived from `(class_id, constructor_args, salt, public_keys)`.
3. **Constructor call** — executes the initializer function inside the deployment tx.

## Deterministic Addresses

```rust,ignore
use aztec_rs::deployment::{
    ContractInstantiationParams, get_contract_instance_from_instantiation_params,
};

let params = ContractInstantiationParams { /* ... */ };
let instance = get_contract_instance_from_instantiation_params(&artifact, params)?;
let expected_address = instance.address;
```

Use this to predict an address before sending the deploy tx (useful for authwits that reference the deployed contract).

## Low-Level Split

`publish_contract_class(wallet, &artifact).await?` and `publish_instance(wallet, &instance)?` build the low-level interactions for class and instance publication separately when the class is shared across many deploys.

## Edge Cases

- Re-deploying with identical `(salt, args)` yields the same address; the deployer recognises this and returns the existing instance rather than erroring.
- Deploying through an account that does not yet have Fee Juice requires a non-native fee strategy — see [Fee Payments](./fee-payments.md).

## Full Runnable Example

Source: [`examples/deploy_contract.rs`](https://github.com/NethermindEth/aztec-rs/blob/main/examples/deploy_contract.rs).

```rust,ignore
{{#include ../../../examples/deploy_contract.rs}}
```

## References

- [Concepts: Contracts](../concepts/contracts.md)
- [`aztec-contract` reference](../reference/aztec-contract.md)
