# `aztec-pxe`

Embedded, in-process PXE runtime.
Implements [`aztec_pxe_client::Pxe`](./aztec-pxe-client.md) by composing local stores, an ACVM executor, the private kernel prover, and an `AztecNode` backend.

Source: `crates/pxe/src/`.

## Start From User Tasks

Use `aztec-pxe` when your process should run its own Private Execution Environment instead of talking to an external PXE service.
Most application code gets one through `aztec_rs::wallet::create_embedded_wallet`; use this crate directly when you need custom storage, custom prover settings, or lower-level tests.

| Task | API | Example |
| ---- | --- | ------- |
| Spin up a short-lived PXE | `EmbeddedPxe::create_ephemeral` | Tests and examples |
| Persist local PXE state | `EmbeddedPxe::create` + `SledKvStore` | Desktop or server process with durable notes |
| Customize sync/proving | `EmbeddedPxe::create_with_config` | Advanced local runtime setup |
| Inspect local stores | `note_store`, `contract_store`, `key_store` accessors | Debugging note discovery |
| Accept a PXE in library code | `aztec_pxe_client::Pxe` | Avoid depending on `EmbeddedPxe` directly |

## Top-Level Types

```rust,ignore
pub struct EmbeddedPxe<N: AztecNode> { /* ... */ }

pub struct EmbeddedPxeConfig {
    pub prover_config:     BbProverConfig,
    pub block_sync_config: BlockSyncConfig,
}
```

`EmbeddedPxe` is generic over the node backend so tests can substitute mock nodes.
It implements `Pxe` and `Send + Sync + 'static`.

## Construction

```rust,ignore
use aztec_pxe::{EmbeddedPxe, EmbeddedPxeConfig, InMemoryKvStore};

// Non-persistent, suitable for tests and short-lived processes:
let pxe = EmbeddedPxe::create_ephemeral(node.clone()).await?;

// Backed by any KvStore (InMemoryKvStore, SledKvStore):
let kv = std::sync::Arc::new(InMemoryKvStore::new());
let pxe = EmbeddedPxe::create(node.clone(), kv).await?;

// With a custom prover or sync config:
let pxe = EmbeddedPxe::create_with_config(
    node.clone(),
    kv,
    EmbeddedPxeConfig::default(),
).await?;
```

For the complete wallet path that creates this for you, run:

```bash
cargo run --example wallet_minimal
```

Accessors expose individual stores for advanced use:
`node()`, `contract_store()`, `key_store()`, `address_store()`, `note_store()`,
`anchor_block_store()`, `private_event_store()`.

## Module Map

| Module        | Highlights                                                                                              |
| ------------- | ------------------------------------------------------------------------------------------------------- |
| `embedded_pxe`| `EmbeddedPxe`, `EmbeddedPxeConfig`, composition root and `Pxe` impl                                     |
| `stores`      | `AnchorBlockStore`, `NoteStore`, `PrivateEventStore`, `RecipientTaggingStore`, `SenderTaggingStore`, `KvStore`, `InMemoryKvStore`, `SledKvStore`, plus private `AddressStore` / `ContractStore` / `KeyStore` / `CapsuleStore` / `SenderStore` |
| `execution`   | ACVM executor, oracle handlers, note selection (`pick_notes`), utility-execution oracle, field conversion |
| `kernel`      | `BbPrivateKernelProver`, `BbProverConfig`, `PrivateExecutionStep`, `PrivateKernelProver`, `PrivateKernelSimulateOutput`, `PrivateKernelOracle`, `PrivateKernelExecutionProver`, `SimulatedKernel`, `ChonkProofWithPublicInputs` |
| `sync`        | `BlockStateSynchronizer`, `BlockSyncConfig`, `ContractSyncService`, `EventService`, `LogService`, `NoteService`, `PrivateEventFilterValidator` |

## Stores

The PXE keeps all state behind a `KvStore` abstraction.
Two implementations ship:

- `InMemoryKvStore` — ephemeral, fastest, ideal for tests.
- `SledKvStore` — persistent, backed by [sled].

Higher-level stores (`NoteStore`, `PrivateEventStore`, `RecipientTaggingStore`, `SenderTaggingStore`, `AnchorBlockStore`) are typed facades over the KV.

## Execution

`execution/` runs private function bodies through the ACVM (`acvm_executor.rs`) with oracle-based access to PXE state (`oracle.rs`, `utility_oracle.rs`).
`pick_notes.rs` implements note selection for functions that consume notes.

## Kernel

`kernel/` folds the private-execution trace into kernel inputs and invokes the BB prover.
Simulation-only flows go through `SimulatedKernel`; real proving through `BbPrivateKernelProver` using `BbProverConfig`.

## Sync

`BlockStateSynchronizer` is the block follower.
It pulls new blocks, routes logs to `NoteService` / `EventService`, and keeps tagging stores fresh for every registered account.
`BlockSyncConfig` controls polling cadence and concurrency.

## Full API

Bundled rustdoc: [`api/aztec_pxe/`](../api/aztec_pxe/index.html).
Local regeneration:

```bash
cargo doc -p aztec-pxe --open
```

## See Also

- [`aztec-pxe-client`](./aztec-pxe-client.md) — the trait implemented here.
- [Architecture: PXE Runtime](../architecture/pxe-runtime.md)

[sled]: https://docs.rs/sled
