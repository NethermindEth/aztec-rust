# `prove_private_note_tx` Optimization Plan

This document captures the current performance state and the optimizations that
should improve Rust `prove_private_note_tx` beyond the first daemon-based win.

The goal is not just to make the benchmark look better. The goal is to make the
Rust PXE private proving path materially faster while preserving protocol
correctness and continuing to rely on battle-tested crypto/proving libraries.

## Current Baseline

Latest trusted comparison:

- Benchmark: `pxe_private_real_proving_note_decryption_json`
- Iterations: `3`
- Rust mode: `cargo test --release`
- TS mode: Jest benchmark with `PXE_BENCH_REAL_PROOFS=1`
- Node: `http://localhost:8080`
- Rust output:
  `crates/pxe/target/tmp/pxe-bench-rust-private-real-proving-note-decryption-3iter-optimized.json`
- TS output:
  `crates/pxe/target/tmp/pxe-bench-ts-private-real-proving-note-decryption-3iter.json`
- Comparison:
  `crates/pxe/target/tmp/pxe-benchmark-private-real-proving-note-decryption-3iter-optimized-comparison.md`

| operation | Rust mean ms | TS mean ms | TS/Rust | status |
| --- | ---: | ---: | ---: | --- |
| `build_private_tx_request` | 3.352 | 36.027 | 10.748x | strong Rust win |
| `prove_private_note_tx` | 4462.964 | 4771.783 | 1.069x | Rust win, but small |
| `note_decryption_first_read` | 28.362 | 70.684 | 2.492x | strong Rust win |
| `note_cached_read` | 3.543 | 5.008 | 1.413x | Rust win, below 1.5x |

The trusted PXE-local total is:

| implementation | trusted mean total |
| --- | ---: |
| Rust | 4498.221ms |
| TypeScript | 4883.501ms |

Rust is currently `1.086x` faster on trusted total and `2.522x` faster by
geometric mean. The main remaining limiter is `prove_private_note_tx`.

## Optimization Already Applied

The first optimization removed per-proof process startup overhead.

Before:

- Rust spawned `node tools/private_kernel_prove_from_rust.mjs` per proof.
- That loaded the whole TS module graph per proof.
- That also created a fresh `WASMSimulator` and `BBBundlePrivateKernelProver`
  per proof.

After:

- Rust keeps a persistent Node proof daemon alive.
- The daemon owns the upstream TS `WASMSimulator` and
  `BBBundlePrivateKernelProver`.
- Rust sends proof jobs over stdin/stdout using request/response JSON files.

Measured effect:

| row | before | after | improvement |
| --- | ---: | ---: | ---: |
| Rust `prove_private_note_tx` mean | 5274.562ms | 4462.964ms | 811.598ms |
| Rust before/after | | | 1.182x |
| Rust vs TS before | | | Rust was about 10.5% slower |
| Rust vs TS after | | | Rust is about 6.9% faster |

This was a benchmark-valid and product-relevant fix: a real PXE should not pay
module/prover startup per transaction.

## Current Bottleneck Breakdown

The latest daemon response shows the remaining time is inside the upstream
proving path:

| component | observed |
| --- | ---: |
| daemon request total | about 4.31s |
| `proveWithKernels` | about 4.30s |
| `toTx` conversion | about 3ms |
| execution steps | 9 |

Logs from the optimized run:

| component | observed range |
| --- | ---: |
| private-kernel witness generation | about 0.4s to 0.66s |
| ClientIVC proof generation | about 3.87s to 3.94s |

This means further large wins must target one of these:

- reduce ClientIVC proof time,
- reduce the number or size of ClientIVC execution steps,
- reduce kernel witness-generation time,
- avoid unnecessary witness/proof work in the specific benchmark path,
- remove remaining Rust-to-TS serialization and reconstruction overhead.

Changing `HARDWARE_CONCURRENCY=10` did not materially improve the 1-iteration
probe. That suggests the current BB path is not simply under-threaded on this
machine.

## Optimization Principles

1. Keep cryptographic correctness first.
   Do not hand-roll crypto, proof serialization, hash functions, or curve
   arithmetic where upstream Aztec/BB libraries already exist.

2. Separate proof-core speedups from benchmark plumbing.
   Process reuse is useful, but the remaining 3.8s to 4.0s ClientIVC time is
   the actual core cost.

3. Match TS semantics before claiming wins.
   If an optimization skips a reset, hiding kernel, fee path, or account
   entrypoint step, it must be backed by upstream behavior or circuit rules.

4. Measure inside the row.
   Every future change should emit internal timings for:
   `rust_execution_ms`, `request_serialization_ms`,
   `daemon_reconstruction_ms`, `kernel_witness_ms`, `client_ivc_ms`,
   `to_tx_ms`, and `response_decode_ms`.

## Ranked Optimization Candidates

### 1. Direct BB/ClientIVC Integration From Rust

Status: highest product value, likely medium-to-large impact.

Current path:

```text
Rust PXE
  -> JSON request file
  -> persistent Node daemon
  -> TS object reconstruction
  -> upstream BB prover
  -> JSON response file
  -> Rust decode
```

Target path:

```text
Rust PXE
  -> typed execution steps
  -> BB client/prover binding or stable BB subprocess protocol
  -> Rust TxProvingResult
```

Expected impact:

- Removes JS object reconstruction and some buffer/base64/JSON overhead.
- More importantly, it creates the foundation to optimize witness packaging and
  proof input layout in Rust.
- Likely immediate win is modest, maybe 100ms to 400ms.
- Long-term win can be larger if Rust can feed BB directly without TS-side
  conversions.

Implementation sketch:

1. Identify the exact BB input shape used by
   `BBBundlePrivateKernelProver.createClientIvcProof`.
2. Add a Rust `ClientIvcProofBackend` abstraction.
3. Implement a first backend that still shells out to `bb`, but with binary
   inputs rather than JSON/base64 where BB supports it.
4. Preserve the TS daemon backend behind an env flag:
   `PXE_REAL_PROOF_BACKEND=ts-daemon|bb-subprocess`.
5. Require byte-for-byte equivalent proof/public-input behavior in smoke tests.

Risks:

- BB command/protocol stability.
- More Rust code must track upstream Aztec proof input changes.
- If BB's public interface still requires the same serialized structures, the
  short-term speedup may be small.

Validation:

- Compare tx hash and public inputs against the TS daemon backend.
- Run the real-proof smoke.
- Run 3 and 10 iteration benchmarks.

### 2. Avoid JSON/Base64 for Large Proof Inputs

Status: high-confidence incremental win.

Current daemon request serializes these through JSON:

- ACIR bytecode as base64,
- VK as base64,
- partial witness entries as JSON arrays,
- public inputs as field strings,
- nested execution results recursively,
- metadata as field strings.

Target:

- Store large binary blobs in sidecar files or shared binary envelope.
- Keep small control metadata as JSON.
- Use hex or binary field arrays consistently, not nested object-heavy JSON.

Expected impact:

- Reduces CPU and allocation overhead on both sides.
- Reduces request file size and parse time.
- Likely impact: 50ms to 250ms per proof for this benchmark.
- Bigger impact for larger private executions.

Implementation sketch:

1. Add internal timings around:
   `serde_json::to_vec(request)`, file write, daemon JSON parse,
   `callFromJson`, witness map creation.
2. Replace `acirBase64` and `vkBase64` with file paths or binary blobs.
3. Encode witnesses as a compact binary list:
   `(u32 witness_index, [u8; 32] field)`.
4. Keep a compatibility path:
   `PXE_REAL_PROOF_REQUEST_FORMAT=json|binary-v1`.

Risks:

- More custom protocol surface between Rust and the daemon.
- Must avoid introducing endian/order mismatches.

Validation:

- Compare execution step count, tx hash, public inputs, and proof verification.
- Run with `PXE_REAL_PROOF_REQUEST_FORMAT=json` and `binary-v1` back to back.

### 3. Cache Static Contract Metadata and VK Membership Data

Status: low risk, likely small-to-medium impact. Contract class preimage
caching is **already done** (commit `b504d85`). The remaining work is
caching contract address preimages, function membership witnesses, and VK
membership witnesses per-request.

For each proof, Rust currently rebuilds metadata for each contract/function
call:

- contract address preimage,
- contract class preimage,
- function membership witness,
- VK membership witness,
- protocol class/function metadata.

Some of this is static for the account, SponsoredFPC, MultiCallEntrypoint, and
benchmark contract during an iteration.

**Already done:** `ContractStore::add_class_preimage` /
`get_class_preimage` is implemented and used in `PrivateKernelOracle::get_contract_class_id_preimage`.
The oracle first checks the store before recomputing or hitting the node.

**Remaining:** The per-request metadata build in `prove_with_real_kernel_helper`
still calls `get_contract_address_preimage` and `get_function_membership_witness`
for every call in every proof iteration.

Expected impact:

- Small for the current 3-call benchmark.
- More meaningful as private tx complexity grows.
- Likely impact: 10ms to 100ms per proof.

Implementation sketch:

1. Add an in-memory cache on `EmbeddedPxe` keyed by:
   `(contract_address, function_selector, block_hash_or_class_id)`.
2. Cache:
   `contractAddressPreimage`, `functionMembershipWitness`.
   (`contractClassIdPreimage` is already cached in `ContractStore`.)
3. Cache VK membership witnesses by VK hash or VK index.
4. Invalidate contract preimage cache on contract registration/update.

Risks:

- Must handle contract updates and class updates correctly.
- Must not cache block-specific membership witnesses across incompatible
  anchors.

Validation:

- Add tests for update-contract invalidation.
- Add benchmark rows for metadata build time before/after.

### 4. Reduce Execution Steps in the Proved Path

Status: highest potential impact, requires careful protocol validation.

The optimized run reports `executionSteps: 9`.

Those steps likely include:

- app/account private circuit step(s),
- user private app circuit step,
- SponsoredFPC private call,
- private kernel init,
- inner kernels,
- reset,
- tail,
- hiding kernel.

ClientIVC time is roughly proportional to the number and complexity of steps.
Reducing one or more steps could produce a real improvement.

Expected impact:

- Potentially large.
- Removing one kernel step could save hundreds of milliseconds.
- Removing a private fee/sponsor step or avoiding unnecessary reset/hiding work
  could save more, if protocol-valid.

Candidate sub-optimizations:

1. Avoid unnecessary intermediate reset.
   Check whether the benchmark path requires reset before tail or whether the
   final reset dimensions can be smaller.

2. Use the minimal reset artifact.
   Confirm Rust/TS select the same reset dimensions. If Rust is forcing a larger
   reset variant, fix that.

3. Avoid extra synthetic wrapper steps.
   Ensure Rust's ACVM entrypoint result tree is not adding a wrapper that TS
   does not prove.

4. SponsoredFPC step alternatives.
   The current real-proof flow uses SponsoredFPC to match TS and avoid account
   fee balance issues. If the account can be funded once and use preexisting
   fee juice, the private tx may avoid a sponsor call. This must be benchmarked
   as a separate scenario because it changes the fee path.

Risks:

- Very high if done by assumption.
- Kernel reset/tail/hiding choices are protocol-sensitive.
- A faster tx that is not accepted by the node is useless.

Validation:

- Instrument and print the exact execution step names in Rust and TS.
- Compare Rust and TS step lists for the same tx.
- For any removed step, submit the tx to the node and wait for checkpoint.
- Verify tx hash, note creation, and note decryption after inclusion.

### 5. Native Rust Private-Kernel Witness Generation

Status: important medium-term product work.

Currently the daemon uses upstream TS `PrivateKernelExecutionProver` and
`BBBundlePrivateKernelProver` for witness generation. Rust has partial private
kernel types, but real witness generation is still delegated.

Expected impact:

- Witness generation currently costs about 0.4s to 0.66s.
- A native Rust implementation may reduce that materially.
- Maximum direct win for this benchmark is probably under 700ms unless it also
  reduces ClientIVC input size or step count.

Implementation sketch:

1. Port the kernel input builders to typed Rust structures.
2. Use upstream Noir protocol circuit artifacts.
3. Use battle-tested ACVM/witness generation crates where possible.
4. Keep TS witness generation as an oracle backend during development.
5. Add property tests comparing Rust-generated public inputs/witness outputs to
   TS for the same request.

Risks:

- Large correctness surface.
- Upstream Aztec kernel circuits change frequently.
- Easy to pass a simple benchmark but fail other private tx shapes.

Validation:

- Golden fixtures for init, inner, reset, tail, hiding.
- Cross-check field-by-field against TS outputs.
- Run account entrypoint, app private call, sponsored fee call, and nested call
  cases.

### 6. Reuse Prover Internals More Aggressively

Status: incremental, needs upstream API inspection.

The daemon keeps `BBBundlePrivateKernelProver` alive, but it is not yet clear
whether the underlying BB backend reuses all expensive internal state across
proofs.

Expected impact:

- Potentially small if BB already caches internally.
- Potentially medium if each ClientIVC proof still reinitializes expensive
  backend state.

Implementation sketch:

1. Instrument BB backend creation and proof creation separately.
2. Inspect whether `BBBundlePrivateKernelProver` creates a new
   `AztecClientBackend` or `Barretenberg` per proof.
3. If it does, keep the backend instance alive at a lower layer.
4. Prefer upstream APIs over monkey-patching private fields.

Risks:

- Upstream TS internals may not expose a stable lifecycle API.
- Leaking BB resources across proofs can cause memory growth.

Validation:

- Track process RSS across 25 proofs.
- Run repeated proofs and ensure no stale proof state crosses tx boundaries.

### 7. Pre-Warm Kernel Artifacts and WASM/BB State Before Timed Region

Status: benchmark-valid if product PXE would also do this at startup.

The daemon is warmed, but the first proof may still pay lazy artifact
decompression, WASM initialization, or BB backend warmup inside
`prove_private_note_tx`.

Expected impact:

- Mostly first-proof improvement.
- In 3-iteration runs this can move the mean if iteration 1 is slower.

Implementation sketch:

1. Add a daemon `warmup` command.
2. Warm:
   - protocol artifact provider,
   - reset/tail/init/hiding artifacts,
   - WASM simulator,
   - BB backend where the API permits.
3. Call warmup from `set_prover_enabled(true)`.
4. Do not generate a dummy proof unless it is cheap and product-realistic.

Risks:

- If warmup proves a dummy circuit, it can hide real user latency while adding
  startup cost elsewhere.
- Warmup must not mutate node or PXE state.

Validation:

- Compare iteration 1 against iterations 2 and 3.
- Report cold and warm proof latencies separately.

### 8. Funded-Account Benchmark Variant Without SponsoredFPC

Status: separate benchmark, not a replacement for SponsoredFPC comparison.

The current real-proof path uses SponsoredFPC so the benchmark account can
submit a valid tx without owning Fee Juice. That matches a common test flow but
adds fee-related execution work.

A separate benchmark can answer:

> How fast is Rust proving for a normal funded account paying directly?

Expected impact:

- Could reduce execution steps if it removes a private sponsor call.
- Could materially improve `prove_private_note_tx`.
- It changes the benchmark scenario, so results must not replace the current
  SponsoredFPC apples-to-apples comparison.

Implementation sketch:

1. Add benchmark target:
   `pxe_private_real_proving_note_decryption_funded_account_json`.
2. Deploy/import a fresh real Schnorr account.
3. Bridge or mint Fee Juice to that account using the same upstream TS flow.
4. Build tx with preexisting fee juice.
5. Compare Rust vs TS using the same funded-account path.

Risks:

- Fee setup may dominate setup time.
- If only Rust uses this path, the comparison is invalid.

Validation:

- Add matching TS benchmark target.
- Keep SponsoredFPC benchmark as the main continuity baseline.

## Recommended Execution Order

### Phase 1: Better Instrumentation

Add timings before making deeper changes:

- Rust private execution and ACVM entrypoint time.
- Metadata build time.
- Request serialization and write time.
- Daemon parse/reconstruction time.
- Kernel witness generation time.
- ClientIVC proof generation time.
- Response decode time.

Deliverable:

- `prove_private_note_tx` timing breakdown in the benchmark JSON under
  `stats.provingTimings`.

Why first:

- We need to avoid optimizing the wrong layer.
- We need enough evidence to defend future claims.

### Phase 2: Compact Request Format

Reduce JSON/base64 overhead without touching proof semantics.

Deliverable:

- `PXE_REAL_PROOF_REQUEST_FORMAT=binary-v1`.
- Backward-compatible JSON mode.
- Bench comparison JSON vs binary request format.

Expected outcome:

- Incremental proof-row improvement.
- Better scaling for larger private txs.

### Phase 3: Step List Audit and Reset Dimension Audit

Compare Rust and TS exact execution step lists for the same tx.

Deliverable:

- Step name, bytecode size, witness size, VK size per step.
- Rust vs TS step parity report.
- Explicit answer to whether Rust proves any extra wrapper/reset step.

Expected outcome:

- If there is an extra or oversized Rust step, this is the best chance for a
  larger win.

### Phase 4: Direct BB Backend

Move from TS daemon to a Rust-managed BB backend/protocol.

Deliverable:

- `PXE_REAL_PROOF_BACKEND=bb-subprocess`.
- Equivalent proof/public inputs/tx hash.
- 3 and 10 iteration benchmark results.

Expected outcome:

- Cleaner product architecture.
- Potential moderate speedup.
- Enables native Rust witness generation later.

### Phase 5: Native Rust Kernel Witness Generation

Port witness generation with TS cross-checks.

Deliverable:

- Rust witness generation for init, inner, reset, tail, hiding.
- Golden fixture parity tests against TS.
- End-to-end node-accepted tx.

Expected outcome:

- Removes TS witness generation overhead.
- Improves control over step construction.
- Sets up further proof input optimizations.

## What Not To Do

Do not:

- skip ClientIVC proof creation and still call the result "real proving",
- use dummy proofs in real-proof benchmark rows,
- bypass account entrypoint validation,
- bypass fee enforcement,
- remove SponsoredFPC from the current benchmark without adding the same TS
  variant,
- hand-roll cryptographic primitives,
- claim node-dominated deploy/submit rows as PXE speedups.

## Success Criteria

Minimum near-term success:

- Rust `prove_private_note_tx` is consistently faster than TS across 10
  iterations.
- p95 is faster, not just mean.
- `note_decryption_first_read` remains at least 2x faster.
- The node accepts every proven tx.

Target success:

- Rust `prove_private_note_tx` is at least 1.5x faster than TS.
- Trusted PXE-local total is at least 1.5x faster.
- Trust verdict becomes `PASS` for the real-proof benchmark.

Stretch success:

- Rust real-proof trusted total is at least 2x faster.
- `prove_private_note_tx` drops below 3.2s on this machine for the benchmark tx.

## Next Concrete Task

Implement Phase 1 instrumentation.

The immediate patch should add structured timings to the Rust benchmark output
without changing behavior:

```json
{
  "name": "prove_private_note_tx",
  "internal_timings": {
    "rust_execution_ms": 0,
    "metadata_build_ms": 0,
    "request_serialize_ms": 0,
    "daemon_parse_ms": 0,
    "kernel_witness_ms": 0,
    "client_ivc_ms": 0,
    "to_tx_ms": 0,
    "response_decode_ms": 0
  }
}
```

Once those numbers are available, choose between compact request format and
execution-step reduction based on measured cost.
