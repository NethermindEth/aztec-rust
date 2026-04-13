# `aztec-core`

Primitives and shared types consumed by every other crate.
No network or runtime dependencies — pure data + validation.

Source: `crates/core/src/`.

## Module Map

| Module          | Highlights                                                                                 |
| --------------- | ------------------------------------------------------------------------------------------ |
| `types`         | `Fr`, `Fq` (BN254 / Grumpkin scalar + base), `Point`, `AztecAddress`, `EthAddress`, `PublicKeys`, `CompleteAddress`, `ContractInstance`, `ContractInstanceWithAddress`, `Salt` |
| `abi`           | `AbiType`, `AbiValue`, `AbiParameter`, `ContractArtifact`, `FunctionArtifact`, `FunctionType`, `FunctionSelector`, `EventSelector`, `NoteSelector`, `AuthorizationSelector`, `ContractStorageLayout`, plus `encode_arguments`, `decode_from_abi`, `abi_checker` |
| `tx`            | `Tx`, `TxHash`, `TxStatus`, `TxExecutionResult`, `TxReceipt`, `TxContext`, `TypedTx`, `FunctionCall`, `AuthWitness`, `Capsule`, `HashedValues`, `ExecutionPayload`, `compute_tx_request_hash` |
| `fee`           | `Gas`, `GasFees`, `GasSettings` (with Aztec default limits)                                |
| `hash`          | `poseidon2_hash`, `poseidon2_hash_with_separator`, `poseidon2_hash_bytes`, authwit hash    |
| `grumpkin`      | `generator`, `point_add`, `scalar_mul`, `point_from_x`, `has_positive_y`                   |
| `kernel_types`  | `NoteHash`, `ScopedNoteHash`, `PrivateKernelTailCircuitPublicInputs`, gas + tx shapes used by the kernel |
| `validation`    | `validate_calldata`, `validate_contract_class_logs`                                        |
| `constants`     | `protocol_contract_address::{fee_juice, public_checks, auth_registry, contract_instance_deployer, contract_class_registerer, ...}`, `domain_separator` helpers, default gas limits |
| `error`         | `Error` enum (see [Errors](./errors.md))                                                   |

## Field Arithmetic

`Fr` is the BN254 scalar field — used for addresses, hashes, note values, field arguments.
`Fq` is the BN254 base field (also Grumpkin's scalar field) — used for Grumpkin keys.
Both offer:

- `zero()`, `one()`, `random()`.
- `from_hex(&str)`, `to_be_bytes() -> [u8; 32]`.
- Serde: serialized as hex strings.
- `From<u64>`, `From<u128>`, `From<bool>`, `From<[u8; 32]>`.
- `Fq::hi()` / `Fq::lo()` return the upper / lower 128 bits as `Fr`.

## ABI & Artifacts

`ContractArtifact` is the in-memory representation of a compiled Aztec contract JSON.
Use the decoder / encoder to move between typed values and field-encoded calldata:

```rust,ignore
use aztec_core::abi::{decode_from_abi, encode_arguments, AbiValue};
let fields = encode_arguments(&abi_params, &[AbiValue::Field(fr)]);
let decoded = decode_from_abi(&return_types, &field_output)?;
```

Selectors:

- `FunctionSelector` — derived from the function signature.
- `EventSelector`, `NoteSelector`, `AuthorizationSelector` — Poseidon2-derived tags.

## Transactions

`TypedTx` is the full, validated transaction shape used by the PXE and node client.
`Tx` holds the wire-format variant that gets submitted.
`TxReceipt` + `TxStatus` + `TxExecutionResult` form the lifecycle surface consumed by wallets.

## Hashing

`poseidon2_hash` is a rate-3 / capacity-1 sponge matching barretenberg's implementation.
Use `poseidon2_hash_with_separator` to bind a domain tag into the sponge.

## Full API

Bundled rustdoc: [`api/aztec_core/`](../api/aztec_core/index.html).
Local regeneration:

```bash
cargo doc -p aztec-core --open
```

## See Also

- [Errors](./errors.md)
- [`aztec-crypto`](./aztec-crypto.md) — higher-level crypto built on top of this crate.
