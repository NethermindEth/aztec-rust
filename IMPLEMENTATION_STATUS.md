# Implementation Status — Ignored Tests

## Current State

**Working (13/18 Pxe methods):** All registration, retrieval, block sync, event queries, sender management.

**Blocked (5 methods):** `simulate_tx`, `prove_tx`, `profile_tx`, `execute_utility` — all return `Err("ACVM not yet available")`.

---

## Test Categories

### Category 1: Live-node infrastructure tests (7 tests)

`#[ignore = "requires live node via AZTEC_NODE_URL"]`

These **already work** — just need a running node. Nothing to implement.

### Category 2: ACVM-blocked tests (5 tests)

`#[ignore = "blocked: requires ACVM..."]`

| Test | Needs |
|------|-------|
| `transfers_funds_from_a_to_b` | deploy + prove_tx + execute_utility (balance check) |
| `public_function_call_across_pxes` | deploy + prove_tx |
| `private_state_is_zero_when_pxe_lacks_secret_key` | deploy + execute_utility |
| `permits_sending_funds_before_recipient_registers` | deploy + prove_tx + note discovery + deferred note reprocessing |
| `permits_sending_and_spending_before_recipient_registers` | deploy + prove_tx + note discovery + deferred notes |

---

## What Needs to Be Implemented (priority order)

### 1. ACVM Crate Integration (THE critical blocker)

`crates/pxe/src/execution/acvm_executor.rs` — currently returns `Err`.

**Need:**
- Add `acvm` + `brillig_vm` crate dependencies (from `noir-lang/noir`, pinned to the Noir version matching contract artifacts)
- Deserialize ACIR bytecode from `ContractArtifact` function bytecode
- Implement the ACVM solve loop with oracle callback routing
- Implement Brillig executor for unconstrained (utility) functions

This is ~200-300 lines of real code but the hard part is **version pinning** — the `acvm` crate version must exactly match the Noir compiler version used to compile the contract artifacts.

### 2. Oracle Completeness

`crates/pxe/src/execution/oracle.rs` has 10 handlers. The TS PXE has ~40. Key missing ones:

- `getKeyValidationRequest` — key verification for kernel
- `getNoteHashMembershipWitness` — proves note in state tree
- `getNullifierMembershipWitness` — proves nullifier non-existence
- `getPublicDataWitness` — merkle witness for public reads
- `getL1ToL2MessageMembershipWitness` — L1→L2 inclusion
- `computeNoteHashAndOptionallyANullifier` — note hash/nullifier computation
- `emitEncryptedLog`, `emitPrivateLog` — log emission
- `getAuthWitness` — auth witness retrieval
- `getRandomField` — randomness
- `fetchLogs` / `getNoteTaggingSecret` — for sync_state

### 3. simulate_tx Wiring

Wire ACVM executor + oracle + SimulatedKernel into `EmbeddedPxe::simulate_tx()`:

1. Sync block state (done)
2. Contract sync for scopes (structure exists, needs ACVM for sync_state)
3. Execute private function via ACVM with oracle
4. Process through SimulatedKernel (exists)
5. Optionally simulate public calls via node
6. Assemble `TxSimulationResult`

### 4. execute_utility Wiring

Wire ACVM Brillig executor into `EmbeddedPxe::execute_utility()`:

1. Look up function in artifact (done)
2. Execute Brillig program with UtilityExecutionOracle
3. Return decoded result

Unblocks: `balance_of_private` checks, `private_state_is_zero_when_pxe_lacks_secret_key`

### 5. prove_tx Pipeline

Wire ACVM + kernel prover into `EmbeddedPxe::prove_tx()`:

1. Execute private function (same as simulate)
2. Run kernel circuit sequence via `BbPrivateKernelProver` (structure exists)
   - init → inner → reset → tail → hiding
3. Shell out to `bb` binary for each circuit
4. Assemble `TxProvingResult`
5. `node.send_tx()` to submit

Unblocks: deployments, transfers, all transaction tests

### 6. Note Discovery (ContractSyncService)

`crates/pxe/src/sync/contract_sync.rs` exists but needs:

- `LogService`: fetch tagged logs from node via `get_private_logs_by_tags`
- Decrypt logs using viewing keys from KeyStore
- Store discovered notes in NoteStore
- `NoteService`: sync nullifiers against on-chain nullifier tree
- Run `sync_state` utility function per contract (needs execute_utility working)

Unblocks: `permits_sending_funds_before_recipient_registers` (deferred note reprocessing)

### 7. Contract Deployment Helper

Tests need to deploy contracts. This requires:

- Build a deployment `TxExecutionRequest` from artifact
- Prove + send via the wallet
- Return deployed instance

---

## Dependency Chain

```
ACVM crate integration
  ├── execute_utility (Brillig)
  │     └── balance checks, sync_state
  ├── simulate_tx (ACIR + oracle + SimulatedKernel)
  │     └── dry-run transactions
  └── prove_tx (ACIR + oracle + BB prover)
        ├── contract deployment
        ├── token transfers
        └── note discovery (sync_state via execute_utility)
              └── deferred note reprocessing tests
```

**Bottom line:** Everything is architecturally in place. The single blocker is wiring the `acvm` crate into `AcvmExecutor` with the correct Noir version pin. Once that's done, the rest is connecting existing pieces.
