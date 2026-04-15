# PXE Benchmark Runbook

This repo currently has two benchmark shapes. Use the PXE-local benchmark for
PXE implementation work; use the e2e benchmark only when you intentionally want
local-network transaction latency included.

## Benchmark Shapes

- `pxe_local_json`: PXE-local benchmark. It uses `EmbeddedPxe` with a mock node,
  so it avoids live-node tx submission, tx inclusion waits, and local-network
  state mutation. This is the benchmark to use when measuring local PXE
  bookkeeping and store performance.
- `pxe_e2e_local_network_json`: e2e transaction benchmark. This creates/imports a
  wallet, deploys a real `StateVars` contract, sends a public transaction, reads
  the tx effect, and executes a utility read. It measures PXE plus account,
  proving, fee, node, and inclusion behavior.
- `pxe_private_prove_and_note_decryption_json`: e2e private benchmark focused on
  `PXE.prove_tx` and note discovery. It deploys `StateVars`, proves a private
  `initialize_private` transaction that creates a note, submits it, then measures
  the first private utility read that syncs/decrypts/stores the note separately
  from a cached read.
- `pxe_private_real_proving_note_decryption_json`: Real-proving benchmark
  enabled with `PXE_BENCH_REAL_PROOFS=1`. It uses the same private tx and
  note-read flow, but the `prove_private_note_tx` row includes witness
  generation and ClientIVC proof generation via the persistent Node proof
  daemon. Both Rust and TypeScript can produce this report.

## What The PXE-Local Benchmark Measures

The Rust `pxe_local_json` benchmark covers the local PXE trait and store paths
available in `crates/pxe`:

- PXE lifecycle and metadata: `create_pxe`, `get_synced_block_header`, `stop`
- Account registry: `register_account`, `get_registered_accounts`
- Sender registry: `register_sender`, `get_senders`, `remove_sender`
- Contract registry: `register_contract_class`, `get_contract_artifact`,
  `register_contract`, `register_contract_with_artifact`,
  `get_contract_instance`, `get_contracts`, `update_contract`
- Private events: `get_private_events_empty`, `get_private_events_hit`,
  `private_event_store_add`, `private_event_store_get`
- Notes: `note_store_add`, `note_store_get_active`, `note_store_nullify`,
  `note_store_get_nullified`
- Capsules: `capsule_store_write_read_delete`, `capsule_store_copy`
- Sender tagging: `sender_tagging_next_index`,
  `sender_tagging_pending_lifecycle`

The local benchmark intentionally excludes these transaction execution paths
from the bookkeeping/store benchmark:

- `simulate_tx`
- `prove_tx`
- `profile_tx`
- `execute_utility`

Those should be measured as a separate ACVM/transaction-lifecycle benchmark,
because they need valid account tx requests and compiled ACIR fixtures and have
different performance characteristics from PXE store operations.

## Run Rust PXE-Local

From `aztec-rs`:

```bash
PXE_BENCH_ITERATIONS=25 \
PXE_BENCH_OUTPUT=/Users/alexmetelli/source/aztec-rs/crates/pxe/target/tmp/pxe-bench-rust-local-optimized-release.json \
cargo test --release -p aztec-pxe pxe_local_json_benchmark -- --ignored --nocapture
```

This does not require `AZTEC_NODE_URL`. Use `--release` for comparisons
against TypeScript; debug-mode Rust timings are not meaningful for performance
claims.

## Run TypeScript PXE-Local

From `aztec-packages/yarn-project`:

```bash
cd /Users/alexmetelli/source/aztec-packages/yarn-project

PXE_BENCH=1 \
PXE_BENCH_TARGET=local \
PXE_BENCH_ITERATIONS=25 \
PXE_BENCH_OUTPUT=/Users/alexmetelli/source/aztec-packages/yarn-project/pxe-bench-ts-local-focused.json \
JEST_MAX_WORKERS=1 \
yarn workspace @aztec/pxe test pxe_bench.test.ts
```

This does not require `AZTEC_NODE_URL`. Use `PXE_BENCH_TARGET=e2e` or omit
`PXE_BENCH_TARGET` for the existing local-network transaction benchmark.

## What The E2E Benchmark Measures

Both e2e implementations report these operations:

- `setup_wallet`
- `deploy_contract`
- `send_public_tx`
- `get_tx_effect`
- `execute_utility`

Both outputs use:

```json
{
  "benchmark": "pxe_e2e_local_network_json",
  "implementation": "rust | typescript",
  "node_url": "http://localhost:8080",
  "iterations": 25,
  "operations": []
}
```

This benchmark mutates the local node. Each iteration deploys a contract and
sends `initialize_public_immutable`.

## Prerequisites

Start a local Aztec network with initial test accounts enabled, then keep it
running:

```bash
export AZTEC_NODE_URL=http://localhost:8080
```

The TypeScript benchmark loads the compiled StateVars artifact JSON directly:

```bash
/workspaces/aztec-rs/fixtures/state_vars_contract_compiled.json
```

If your checkout layout differs, point the benchmark at the artifact explicitly:

```bash
PXE_BENCH_STATE_VARS_ARTIFACT=/path/to/state_vars_contract_compiled.json
```

If Jest fails before loading tests with `@swc/core` native binding errors,
reinstall/build optional native dependencies for the platform you are running on.

The TypeScript benchmark intentionally avoids a full `@aztec/protocol-contracts`
TypeScript build because that build can walk into generated L1 artifact outputs
that are irrelevant for PXE benchmarking. Generate only the protocol contract
source data and Aztec.js protocol wrappers:

```bash
cd /workspaces/aztec-packages/yarn-project

yarn workspace @aztec/protocol-contracts generate
yarn workspace @aztec/aztec.js generate
```

If the protocol-contract generation cannot find pinned Noir artifacts, extract
them first:

```bash
cd /workspaces/aztec-packages/noir-projects/noir-contracts
mkdir -p target
tar xzf pinned-protocol-contracts.tar.gz -C target
```

Do not run `yarn workspace @aztec/protocol-contracts build:ts` for this
benchmark path unless you also have the generated L1 artifacts available.

## Run Rust E2E

Use `--release` for performance numbers. From `aztec-rs` in devbox:

```bash
cd /workspaces/aztec-rs

AZTEC_NODE_URL=http://localhost:8080 \
PXE_BENCH_ITERATIONS=25 \
PXE_BENCH_OUTPUT=/workspaces/aztec-rs/crates/pxe/target/tmp/pxe-bench-rust-e2e-local.json \
cargo test --release --test pxe pxe_e2e_local_network_json_benchmark -- --ignored --nocapture
```

On macOS paths:

```bash
cd /Users/alexmetelli/source/aztec-rs

AZTEC_NODE_URL=http://localhost:8080 \
PXE_BENCH_ITERATIONS=25 \
PXE_BENCH_OUTPUT=/Users/alexmetelli/source/aztec-rs/crates/pxe/target/tmp/pxe-bench-rust-e2e-local.json \
cargo test --release --test pxe pxe_e2e_local_network_json_benchmark -- --ignored --nocapture
```

## Run Rust Private Proving + Note Decryption (simulated proofs)

Use this when investigating PXE-local bookkeeping without paying real proving
cost. `prove_private_note_tx` uses a simulated/dummy proof:

```bash
cd /Users/alexmetelli/source/aztec-rs

AZTEC_NODE_URL=http://localhost:8080 \
PXE_BENCH_ITERATIONS=25 \
PXE_BENCH_OUTPUT=/Users/alexmetelli/source/aztec-rs/crates/pxe/target/tmp/pxe-bench-rust-private-prove-note-decryption.json \
cargo test --release --test pxe pxe_private_prove_and_note_decryption_json_benchmark -- --ignored --nocapture
```

Reported operations:

- `setup_wallet`: wallet/PXE setup against the live node.
- `deploy_contract`: deploy the `StateVars` contract used by the private note tx.
- `build_private_tx_request`: account entrypoint request construction.
- `prove_private_note_tx`: local `PXE.prove_tx` for `initialize_private`.
- `submit_private_note_tx`: node submission plus checkpoint wait.
- `get_tx_effect`: node tx-effect lookup after inclusion.
- `note_decryption_first_read`: first `get_private_mutable` utility read, which
  forces PXE contract sync, tagged-log fetch, note decryption, validation, and
  note storage.
- `note_cached_read`: repeat `get_private_mutable` after the note has already
  been decrypted/stored.

## Run Rust Private Real Proving + Note Decryption

Use this when measuring Rust `prove_private_note_tx` with real witness
generation and ClientIVC proof creation via the persistent Node proof daemon.
The same test function emits `pxe_private_real_proving_note_decryption_json`
when `PXE_BENCH_REAL_PROOFS=1`:

```bash
cd /Users/alexmetelli/source/aztec-rs

AZTEC_NODE_URL=http://localhost:8080 \
PXE_BENCH_REAL_PROOFS=1 \
PXE_BENCH_ITERATIONS=3 \
PXE_BENCH_TIMEOUT_MS=3600000 \
PXE_BENCH_OUTPUT=/Users/alexmetelli/source/aztec-rs/crates/pxe/target/tmp/pxe-bench-rust-private-real-proving-note-decryption-3iter.json \
cargo test --release --test pxe pxe_private_prove_and_note_decryption_json_benchmark -- --ignored --nocapture
```

Use 3 iterations — each proof takes roughly 4–5 seconds. Keep
`PXE_BENCH_TIMEOUT_MS=3600000` to avoid the default 2-minute Jest-style
timeout.

## Run TypeScript Private Real Proving + Note Decryption

From `aztec-packages/yarn-project` on macOS:

```bash
cd /Users/alexmetelli/source/aztec-packages/yarn-project

AZTEC_NODE_URL=http://localhost:8080 \
PXE_BENCH=1 \
PXE_BENCH_TARGET=private \
PXE_BENCH_REAL_PROOFS=1 \
PXE_BENCH_ITERATIONS=3 \
PXE_BENCH_TIMEOUT_MS=3600000 \
PXE_BENCH_STATE_VARS_ARTIFACT=/Users/alexmetelli/source/aztec-rs/fixtures/state_vars_contract_compiled.json \
PXE_BENCH_OUTPUT=/Users/alexmetelli/source/aztec-packages/yarn-project/pxe-bench-ts-private-real-proving-note-decryption-3iter.json \
JEST_MAX_WORKERS=1 \
yarn workspace @aztec/pxe test pxe_bench.test.ts
```

Measured local 3-iteration baseline on this machine:

| operation | TypeScript mean ms | note |
| --- | ---: | --- |
| `build_private_tx_request` | 20.606 | local request construction |
| `prove_private_note_tx` | 4937.761 | real witness generation + ClientIVC proof generation |
| `note_decryption_first_read` | 39.221 | first sync/decrypt/store read |
| `note_cached_read` | 3.121 | cached utility read |

Do not compare this report to the Rust simulated-proving report. It is a
TypeScript real-proving baseline until Rust real private-kernel proving is wired.

## Run TypeScript E2E

From `aztec-packages/yarn-project` in devbox:

```bash
cd /workspaces/aztec-packages/yarn-project

AZTEC_NODE_URL=http://localhost:8080 \
PXE_BENCH=1 \
PXE_BENCH_ITERATIONS=25 \
PXE_BENCH_STATE_VARS_ARTIFACT=/workspaces/aztec-rs/fixtures/state_vars_contract_compiled.json \
PXE_BENCH_OUTPUT=/workspaces/aztec-packages/yarn-project/pxe-bench-ts-e2e-local.json \
JEST_MAX_WORKERS=1 \
yarn workspace @aztec/pxe test pxe_bench.test.ts
```

On macOS paths:

```bash
cd /Users/alexmetelli/source/aztec-packages/yarn-project

AZTEC_NODE_URL=http://localhost:8080 \
PXE_BENCH=1 \
PXE_BENCH_ITERATIONS=25 \
PXE_BENCH_STATE_VARS_ARTIFACT=/Users/alexmetelli/source/aztec-rs/fixtures/state_vars_contract_compiled.json \
PXE_BENCH_OUTPUT=/Users/alexmetelli/source/aztec-packages/yarn-project/pxe-bench-ts-e2e-local.json \
JEST_MAX_WORKERS=1 \
yarn workspace @aztec/pxe test pxe_bench.test.ts
```

## Compare

From `aztec-rs` in devbox:

```bash
cd /workspaces/aztec-rs

node tools/compare_pxe_benchmarks.mjs \
  crates/pxe/target/tmp/pxe-bench-rust-e2e-local.json \
  /workspaces/aztec-packages/yarn-project/pxe-bench-ts-e2e-local.json \
  crates/pxe/target/tmp/pxe-benchmark-e2e-local-comparison.md

cat crates/pxe/target/tmp/pxe-benchmark-e2e-local-comparison.md
```

For the PXE-local comparison on macOS:

```bash
cd /Users/alexmetelli/source/aztec-rs

node tools/compare_pxe_benchmarks.mjs \
  crates/pxe/target/tmp/pxe-bench-rust-local-optimized-release.json \
  /Users/alexmetelli/source/aztec-packages/yarn-project/pxe-bench-ts-local-focused.json \
  crates/pxe/target/tmp/pxe-benchmark-local-optimized-release-comparison.md

cat crates/pxe/target/tmp/pxe-benchmark-local-optimized-release-comparison.md
```

On macOS paths:

```bash
cd /Users/alexmetelli/source/aztec-rs

node tools/compare_pxe_benchmarks.mjs \
  crates/pxe/target/tmp/pxe-bench-rust-e2e-local.json \
  /Users/alexmetelli/source/aztec-packages/yarn-project/pxe-bench-ts-e2e-local.json \
  crates/pxe/target/tmp/pxe-benchmark-e2e-local-comparison.md

cat crates/pxe/target/tmp/pxe-benchmark-e2e-local-comparison.md
```

Interpretation:

- `ts/rust mean > 1`: TypeScript is slower for that operation.
- `ts/rust mean < 1`: TypeScript is faster for that operation.
- Compare only reports with the same `benchmark` value and operation names.
