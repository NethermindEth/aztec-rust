# Contract Layer

`aztec-contract` hosts the high-level surface for talking to deployed contracts: call construction, deployment, authwits, and public events.

## Context

Given a wallet (`W: Wallet`), applications want to:

- Point at a deployed contract and build calls against its ABI.
- Deploy a new contract (class + instance) deterministically.
- Set and look up public authorization witnesses.
- Read public events.

All of these live in this crate.

## Design

The crate is structured around four modules, each generic over a `Wallet`:

| Module       | Primary types                                                                                             |
| ------------ | --------------------------------------------------------------------------------------------------------- |
| `contract`   | `Contract<W>`, `ContractFunctionInteraction<'a, W>`, `BatchCall<'a, W>`                                   |
| `deployment` | `ContractDeployer<'a, W>`, `DeployMethod<'a, W>`, `DeployResult`, `DeployOptions`, helpers                |
| `authwit`    | `SetPublicAuthWitInteraction<'a, W>`, `AuthWitValidity`, `lookup_validity`                                |
| `events`     | `PublicEvent<T>`, `PublicEventFilter`, `get_public_events`                                                |

## Implementation Notes

### Call Construction

`Contract::method(name, args)` performs:

1. Artifact lookup by function name.
2. Arity validation against the `AbiParameter` list.
3. Construction of a `FunctionCall` with type (`Private` / `Public` / `Utility`) and `is_static` flag from metadata.

The returned `ContractFunctionInteraction` is a builder with `simulate(opts)`, `profile(opts)`, `send(opts)`, `with(authwits, capsules)`, and `request()` producing the raw `ExecutionPayload`.

### Deployment

`ContractDeployer` follows a builder pattern:

```rust,ignore
ContractDeployer::new(artifact, &wallet)
    .with_constructor_name("constructor")
    .with_public_keys(public_keys)
    .deploy(args)?
    .send(DeployOptions::default())
    .await?
```

Lower-level primitives — `publish_contract_class`, `publish_instance`, `get_contract_instance_from_instantiation_params` — let callers split deployment steps (e.g. to share a class across instances).

Addresses are deterministic in the constructor + salt + public keys tuple; `get_contract_instance_from_instantiation_params` returns the computed address without sending a tx.

### Events

Public events are pulled from the node via `get_public_events`, with filtering by address + event selector + block range.
Private events come from the wallet (`Wallet::get_private_events`) since they require PXE-side decryption.

### Authwits

`SetPublicAuthWitInteraction` targets the AuthRegistry protocol contract.
`lookup_validity` returns the still-consumable state of a witness from the current chain head.

## Edge Cases

- **Utility vs public**: calls with `FunctionType::Utility` MUST NOT be scheduled as on-chain calls — use `Wallet::execute_utility` or `Contract::method(...).simulate(...)`.
- **Deployment replay**: deploying with an identical salt + args re-derives the same address; `DeployResult` contains the `TxReceipt` so callers can distinguish inclusion success from idempotent no-ops.
- **Missing selector**: artifacts occasionally omit selectors; `Contract::method` surfaces this with an `Error::InvalidData` rather than silently computing one.

## Security Considerations

- Artifact integrity SHOULD be verified against the expected class hash before use; once registered in PXE / on-chain, the class id is the authoritative reference.
- Public event payloads are untrusted until decoded against the matching ABI type; the decoder enforces shape.
- Authwits bound to the wrong intent or origin are rejected at consumption — but applications SHOULD validate shape before issuing a wit to avoid user confusion.

## References

- [`aztec-contract` reference](../reference/aztec-contract.md)
- [`aztec-wallet` reference](../reference/aztec-wallet.md) — `Wallet` trait these APIs wrap.
- [Concepts: Contracts](../concepts/contracts.md)
