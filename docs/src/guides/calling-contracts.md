# Calling Contracts

Build, simulate, and send contract function calls via [`aztec-contract`](../reference/aztec-contract.md).

## Runnable Examples

- `examples/simulate_profile_send.rs` — simulate → profile → send for a single call.
- `examples/private_token_transfer.rs` — private function call end to end.
- `examples/public_storage.rs` — public function + public storage read.
- `examples/note_getter.rs` — reading owned notes from the PXE.

## Typical Flow

```rust,ignore
use aztec_rs::abi::{AbiValue, ContractArtifact};
use aztec_rs::contract::Contract;

let artifact: ContractArtifact = /* load_artifact(...) */;
let handle = Contract::at(token_address, artifact, wallet.clone());

// Build a call — arity and types are validated against the artifact.
let call = handle.method("transfer", vec![
    AbiValue::Field(sender.into()),
    AbiValue::Field(recipient.into()),
    AbiValue::Integer(amount),
])?;

// Optional: simulate + profile
let sim   = call.simulate(Default::default()).await?;
let prof  = call.profile(Default::default()).await?;

// Submit; SendOptions picks the fee payment method.
let sent  = call.send(SendOptions { from: sender, ..Default::default() }).await?;
println!("tx hash: {}", sent.tx_hash);
```

## Choosing the Right Path

| Function kind      | Method                                   |
| ------------------ | ---------------------------------------- |
| Private            | `handle.method(...).send(...)`           |
| Public             | `handle.method(...).send(...)` (same)    |
| Utility (off-chain)| `Wallet::execute_utility(call, opts)` or `handle.method(...).simulate(...)` |

`Wallet::execute_utility` does not produce a transaction; it runs the utility inside the PXE and returns the decoded values + logs.

## Batch Calls

`BatchCall` bundles several calls into a single transaction.
Use it when multiple calls share authorization or must be atomic.

## Attaching Authwits / Capsules

```rust,ignore
let call = handle.method("spend_on_behalf", args)?
    .with(vec![authwit], vec![capsule]);
```

`ContractFunctionInteraction::with(auth_witnesses, capsules)` attaches additional authorization or capsule data before simulation / send.

## Full Runnable Example

Source: [`examples/simulate_profile_send.rs`](https://github.com/NethermindEth/aztec-rs/blob/main/examples/simulate_profile_send.rs).

```rust,ignore
{{#include ../../../examples/simulate_profile_send.rs}}
```

## References

- [`aztec-contract` reference](../reference/aztec-contract.md)
- [`aztec-wallet` reference](../reference/aztec-wallet.md) — `SendOptions`, `SimulateOptions`, `ProfileOptions`.
- [Events guide](./events.md)
