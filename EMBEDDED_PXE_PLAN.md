# Embedded PXE Implementation Plan

**Goal:** Replace the dead `HttpPxeClient` with an `EmbeddedPxe` that runs PXE
logic in-process, matching the Aztec v4.x architecture where PXE is client-side.

---

## Problem

In Aztec v4.x, PXE is embedded in the client — there is no `pxe_*` RPC endpoint.
The `--local-network` sandbox starts PXE internally (logs show
`embedded-wallet:pxe:service Started PXE connected to chain ...`) but only
exposes `node_*` methods on the RPC server. The TS SDK uses `EmbeddedWallet`
which creates a PXE instance in-process and talks to the node over `node_*` RPC.

Our `HttpPxeClient` calls `pxe_simulateTx`, `pxe_proveTx`, etc. over HTTP.
These endpoints do not exist. Every PXE-dependent operation fails.

---

## Architecture

```
Aztec v4.x (TS)                       Rust SDK (target)
┌─────────────────────┐               ┌─────────────────────────┐
│  EmbeddedWallet      │               │  BaseWallet              │
│  ┌────────────────┐  │               │  ┌───────────────────┐  │
│  │ PXE (in-process)│  │               │  │ EmbeddedPxe        │  │
│  │  - WASM Sim     │  │               │  │  - ACVM (native)   │  │
│  │  - BB prover    │  │               │  │  - bb binary/FFI   │  │
│  │  - local stores │  │               │  │  - local stores    │  │
│  └───────┬────────┘  │               │  └──────────┬────────┘  │
│          │            │               │             │            │
│  ┌───────▼────────┐  │               │  ┌──────────▼─────────┐ │
│  │ AztecNode       │──── node_* ────►│  │ HttpNodeClient      │ │
│  └────────────────┘  │               │  └────────────────────┘ │
└─────────────────────┘               └─────────────────────────┘
```

---

## What PXE Does (per the TS implementation)

### Local stores
| Store | Purpose | Phase |
|-------|---------|-------|
| ContractStore | Artifacts, instances, classes, function membership witnesses | 1 |
| KeyStore | Master secret keys, app-siloed key derivation | 1 |
| AddressStore | CompleteAddress records | 1 |
| AnchorBlockStore | Current synced block header | 1 |
| NoteStore | Discovered notes (active + nullified) | 2 |
| CapsuleStore | Ephemeral capsule data for execution | 1 |
| SenderTaggingStore | Outgoing tag index tracking | 3 |
| RecipientTaggingStore | Incoming tag index tracking | 3 |
| SenderAddressBookStore | Registered sender addresses | 3 |
| PrivateEventStore | Discovered private events | 3 |

### External dependencies
| Dependency | TS Implementation | Rust Strategy |
|------------|-------------------|---------------|
| Noir ACIR/Brillig executor | `WASMSimulator` (WASM build of ACVM) | Native Rust `acvm` crate from noir-lang/noir |
| Kernel circuit prover | `BBBundlePrivateKernelProver` (bb binary via msgpack) | `bb` binary CLI or FFI to `bb.js/nodejs_module.node` |
| KV Store | LMDB (server) or IndexedDB (browser) | `sled`, `redb`, or LMDB via `heed` |
| AztecNode | JSON-RPC (`node_*` methods) | Existing `HttpNodeClient` (extended) |

### Key method flows

**`simulateTx` (skipKernels=true):**
1. Sync block header from node
2. Sync contract state (run `sync_state` utility to discover notes)
3. Execute private function via ACVM (Noir bytecode) with oracle callbacks
4. Assemble `PrivateKernelTailCircuitPublicInputs` in software (no proving)
5. Optionally call `node.simulatePublicCalls()` for public part
6. Return simulation result

**`proveTx` (full send path):**
1. Same private execution as simulate
2. Run kernel circuit sequence: init → inner → reset → tail → hiding → ChonkProof
3. All kernel circuits go through `bb` prover
4. Return proven tx ready for `node.sendTx()`

**`registerContract`:** Purely local — store artifact + instance in ContractStore.

**`registerAccount`:** Purely local — derive keys, store in KeyStore + AddressStore.

---

## Node RPC Methods Needed

Our `AztecNode` trait currently has 8 methods. The PXE oracle needs ~20 more:

### Currently implemented
- `get_node_info`, `get_block_number`, `get_proven_block_number`
- `get_tx_receipt`, `get_public_logs`, `send_tx`
- `get_contract`, `get_contract_class`

### Required for Phase 1 (simulate)
- `get_block_header` — current block header for oracle
- `get_block` — full block data
- `get_note_hash_membership_witness` — verify note existence in state tree
- `get_nullifier_membership_witness` — verify nullifier non-existence
- `get_low_nullifier_membership_witness` — low-leaf witness for non-membership
- `get_public_storage_at` — read public state during simulation
- `get_public_data_witness` — merkle witness for public data reads
- `get_l1_to_l2_message_membership_witness` — L1→L2 message inclusion proof
- `simulate_public_calls` — delegate public simulation to node
- `is_valid_tx` — validate a simulated tx
- `get_private_logs_by_tags` — discover tagged private logs
- `get_public_logs_by_tags_from_contract` — discover tagged public logs
- `register_contract_function_signatures` — register function names for debugging

### Required for Phase 2 (send)
- `get_block_hash_membership_witness` — block archive proofs for kernel reset
- `find_leaves_indexes` — locate leaves in merkle trees

---

## Phased Implementation

### Phase 1: Local Stores + Contract Registration + Simulate (skipKernels)

**Crate:** `crates/pxe` (new, replaces `crates/pxe-client` as the primary Pxe impl)

**Goal:** `EmbeddedPxe` implements the existing `Pxe` trait. Users can simulate
transactions and execute utility functions.

#### 1.1 Local KV store abstraction

```rust
/// Simple key-value store trait (async for future flexibility).
pub trait KvStore: Send + Sync {
    async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Error>;
    async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), Error>;
    async fn delete(&self, key: &[u8]) -> Result<(), Error>;
    async fn list_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Error>;
}
```

Implementations: `InMemoryKvStore` (testing), `SledKvStore` or `RedbKvStore` (persistent).

#### 1.2 Domain stores (built on KvStore)

- `ContractStore` — CRUD for `ContractArtifact`, `ContractInstance`, contract
  classes. Computes function membership witnesses from artifact merkle trees.
- `KeyStore` — `add_account(secret_key, partial_address)`, derives all master
  keys via `aztec_crypto::derive_keys()`. Returns app-siloed keys on demand.
- `AddressStore` — `CompleteAddress` records.
- `CapsuleStore` — ephemeral capsule storage, consumed during execution.
- `NoteStore` — discovered notes indexed by (contract, storage_slot, owner).
  Initially can be minimal (manual insertion, no auto-discovery).

#### 1.3 Extend `AztecNode` trait

Add the ~13 additional RPC methods listed above to the `AztecNode` trait and
`HttpNodeClient`. Each is a straightforward `transport.call("node_<method>", ...)`
with appropriate request/response types.

#### 1.4 Private execution oracle

The oracle bridges ACVM foreign calls to local stores + node RPC:

```rust
pub struct PrivateExecutionOracle<'a, N: AztecNode> {
    node: &'a N,
    contract_store: &'a ContractStore,
    key_store: &'a KeyStore,
    note_store: &'a NoteStore,
    capsule_store: &'a CapsuleStore,
    // execution-scoped caches
    note_cache: ExecutionNoteCache,
    block_header: BlockHeader,
}
```

Implements the oracle callback interface expected by ACVM:
- `getSecretKey`, `getPublicKey` → KeyStore
- `getNotes`, `checkNoteHashExists` → NoteStore + node witness RPCs
- `getPublicStorageAt` → node RPC
- `getContractInstance` → ContractStore
- `getCapsule` → CapsuleStore
- etc.

#### 1.5 ACVM integration

**Strategy:** Use the Noir `acvm` crate (Rust-native, from `noir-lang/noir`).

The Noir project is written in Rust. The ACVM (Abstract Circuit Virtual Machine)
executes ACIR bytecode and Brillig (unconstrained code). We add it as a
dependency and use it to execute private/utility functions:

```rust
use acvm::acir::circuit::Program;
use acvm::pwg::ACVM;

pub async fn execute_private_function(
    program: &Program,
    initial_witness: WitnessMap,
    oracle: &mut PrivateExecutionOracle<'_, impl AztecNode>,
) -> Result<PrivateExecutionResult, Error> {
    let mut acvm = ACVM::new(program, initial_witness);
    loop {
        match acvm.solve() {
            ACVMStatus::Solved => break,
            ACVMStatus::RequiresForeignCall(request) => {
                let response = oracle.handle_foreign_call(request).await?;
                acvm.resolve_pending_foreign_call(response);
            }
            ACVMStatus::Failure(err) => return Err(err.into()),
        }
    }
    Ok(extract_execution_result(&acvm))
}
```

**Version pinning:** The `acvm` version MUST match the Noir version used to
compile the contract artifacts. Check
`~/.aztec/versions/4.2.0-aztecnr-rc.2/node_modules/@aztec/bb.js/package.json`
for the exact Barretenberg/Noir version.

#### 1.6 Simulated proving result

For `simulateTx` with `skipKernels: true` (the default), the TS PXE assembles
`PrivateKernelTailCircuitPublicInputs` in pure TypeScript without running kernel
circuits. Port this logic to Rust:

- Silo note hashes and nullifiers
- Squash transient note hash / nullifier pairs
- Verify read requests against merkle witnesses
- Compute gas used
- Assemble the public inputs struct

This is ~500 lines of TS logic with no external dependencies.

#### 1.7 `EmbeddedPxe` struct

```rust
pub struct EmbeddedPxe<N: AztecNode> {
    node: N,
    contract_store: ContractStore,
    key_store: KeyStore,
    address_store: AddressStore,
    note_store: NoteStore,
    capsule_store: CapsuleStore,
    block_header: RwLock<Option<BlockHeader>>,
}

impl<N: AztecNode> EmbeddedPxe<N> {
    pub async fn create(node: N) -> Result<Self, Error>;
}

#[async_trait]
impl<N: AztecNode> Pxe for EmbeddedPxe<N> {
    // All Pxe trait methods implemented
}
```

#### 1.8 Update `BaseWallet` and `create_wallet_from_urls`

Add `create_embedded_wallet` that constructs `BaseWallet<EmbeddedPxe<HttpNodeClient>, HttpNodeClient, A>`:

```rust
pub async fn create_embedded_wallet<A: AccountProvider>(
    node_url: impl Into<String>,
    accounts: A,
) -> Result<BaseWallet<EmbeddedPxe<HttpNodeClient>, HttpNodeClient, A>, Error> {
    let node = create_aztec_node_client(node_url);
    let pxe = EmbeddedPxe::create(node.clone()).await?;
    Ok(BaseWallet::new(pxe, node, accounts))
}
```

**Deliverable:** `cargo run --example contract_call` successfully simulates
transactions against `aztec start --local-network`.

---

### Phase 2: Real Proving + Send

**Goal:** Full `proveTx` so transactions can actually be submitted to the network.

#### 2.1 Kernel circuit prover (bb integration)

The `bb` binary at `~/.aztec/versions/<ver>/node_modules/@aztec/bb.js/build/<arch>/bb`
supports `prove`, `verify`, `gates`, and `msgpack` subcommands.

**Strategy:** Shell out to `bb prove` for each kernel circuit step, or use the
`msgpack` IPC interface (shared memory) that the TS SDK uses for better
performance.

Kernel circuit sequence:
1. **Init** — first private call → kernel public inputs
2. **Inner** — each subsequent nested call chains with previous output
3. **Reset** — between calls: squash transient side effects, verify read requests
4. **Tail** — finalize private kernel public inputs
5. **Hiding** — wrap into privacy-preserving proof (to-rollup or to-public)
6. **ChonkProof** — aggregate all execution steps into one proof

#### 2.2 Note discovery (ContractSyncService)

To read private state, PXE must discover notes by running each contract's
`sync_state` utility function. This function scans tagged logs, decrypts them,
and populates the NoteStore.

- Uses the tagging protocol: `get_private_logs_by_tags` node RPC
- Decrypts logs using viewing keys from KeyStore
- Stores discovered notes in NoteStore

#### 2.3 Tagging stores

- `SenderTaggingStore` — track outgoing tag indexes (prevent reuse)
- `RecipientTaggingStore` — track incoming tag indexes

**Deliverable:** `contract.method("transfer", args).send(opts).await` succeeds
end-to-end and the transaction is included in a block.

---

### Phase 3: Full PXE Feature Parity

- `getPrivateEvents` — event discovery via tagged logs
- Block reorg handling — rollback note store on chain reorgs
- `SenderAddressBookStore` — multi-sender tagged log discovery
- Profile modes (gate counting via `bb gates`)
- Persistent KV store (survive process restarts)
- Browser-compatible store (IndexedDB via wasm)

---

## Dependency Analysis

### Noir ACVM version

The ACVM version must exactly match the Noir compiler used for contract artifacts.
Aztec v4.2.0-aztecnr-rc.2 uses:
- `@aztec/noir-acvm_js` v4.2.0-aztecnr-rc.2
- `@aztec/bb.js` v4.2.0-aztecnr-rc.2

These correspond to a specific Noir/Barretenberg commit in the Aztec monorepo.
Pin the `acvm` Rust crate to the matching `noir-lang/noir` tag/commit.

### Barretenberg (bb) binary

Already available at:
```
~/.aztec/versions/4.2.0-aztecnr-rc.2/node_modules/@aztec/bb.js/build/arm64-macos/bb
```

For distribution: either bundle the binary, download on first use, or build from
source via the `barretenberg` Rust bindings.

### New crate dependencies (Phase 1)

| Crate | Purpose | Source |
|-------|---------|--------|
| `acvm` | ACIR/Brillig execution | `noir-lang/noir` (pin to matching version) |
| `brillig_vm` | Brillig (unconstrained) execution | `noir-lang/noir` |
| `sled` or `redb` | Persistent KV store | crates.io |

---

## Migration Path

1. **Keep `HttpPxeClient`** as a fallback for future versions that might
   re-expose PXE over RPC, or for custom setups with a standalone PXE server.
2. **Add `EmbeddedPxe`** as the default/recommended implementation.
3. **Update `create_wallet_from_urls`** to use `EmbeddedPxe` by default,
   accepting a single `node_url` instead of separate `pxe_url` + `node_url`.
4. **Deprecate** the two-URL pattern in examples and docs.

---

## Estimated Scope

| Phase | Components | Effort |
|-------|-----------|--------|
| 1 | KV stores, ACVM integration, oracle, simulated proving, node trait extension | Large |
| 2 | bb prover integration, note discovery, tagging | Large |
| 3 | Events, reorg handling, persistence, browser compat | Medium |

Phase 1 is the critical path — it unblocks simulation, which is the most
common developer workflow (simulate → inspect results → iterate).

---

## File Layout

```
crates/
  pxe/                          # NEW — EmbeddedPxe implementation
    src/
      lib.rs
      embedded_pxe.rs           # EmbeddedPxe struct + Pxe trait impl
      stores/
        mod.rs
        kv.rs                   # KvStore trait + InMemoryKvStore
        contract_store.rs
        key_store.rs
        address_store.rs
        note_store.rs
        capsule_store.rs
      execution/
        mod.rs
        acvm_executor.rs        # ACVM integration
        oracle.rs               # PrivateExecutionOracle
        utility_oracle.rs       # UtilityExecutionOracle
      kernel/
        mod.rs
        simulated.rs            # Simulated proving (Phase 1)
        prover.rs               # BB prover integration (Phase 2)
      sync/
        mod.rs
        block_sync.rs           # Block header synchronization
        contract_sync.rs        # Note discovery (Phase 2)
  pxe-client/                   # EXISTING — keep as HttpPxeClient fallback
    src/
      pxe.rs                    # Unchanged, still useful for custom setups
  node-client/
    src/
      node.rs                   # Extended with ~13 new RPC methods
```
