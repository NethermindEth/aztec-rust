# aztec-rust Implementation Plan

This plan is derived from [SPEC.md](/Users/alexmetelli/source/aztec-rust/SPEC.md) and corrects the main issues in the previous version:

- keep the repository single-crate at first
- avoid locking into speculative artifact schemas too early
- avoid assuming an unsupported standalone `wallet_*` JSON-RPC surface
- use `wait_for_node` via `get_node_info`, matching current `aztec.js`
- keep `AztecNode` small in the MVP
- defer account deployment, class publication, and batching until the base client and contract flow work

## Constraints

- repository currently has no Rust code
- MVP is one crate with modules, not a workspace
- HTTP JSON-RPC only in early phases
- first useful milestone is: connect to node, load artifact, build contract handle, simulate/send through a wallet abstraction

## Phase 1 — Crate Skeleton **COMPLETED**

### Step 1.1: Initialize crate

Create a single crate named `aztec-rs` with:

- `Cargo.toml`
- `src/lib.rs`
- modules: `abi`, `types`, `node`, `wallet`, `account`, `contract`, `deployment`, `tx`, `events`, `fee`, `authorization`, `messaging`, `error`
- `examples/`

Initial dependencies:

- `serde`, `serde_json`
- `tokio`
- `reqwest`
- `async-trait`
- `thiserror`
- `url`
- `hex`
- one BN254-compatible field/curve library

Verification:

```bash
cargo check
cargo test
```

### Step 1.2: Error type

Implement a crate-level `Error` enum in `src/error.rs` and re-export it from `lib.rs`.

Requirements:

- transport error mapping
- JSON parsing error mapping
- ABI/data validation failures
- JSON-RPC error representation
- timeout/revert helpers

Do not over-design per-module errors yet.

Verification:

```bash
cargo test error
```

## Phase 2 — Core Types **COMPLETED**

### Step 2.1: Fields, points, addresses

Implement in `src/types.rs`:

- `Fr`
- `Fq`
- `GrumpkinScalar`
- `Point`
- `AztecAddress`
- `EthAddress`
- `PublicKeys`
- `CompleteAddress`

Requirements:

- serde round-trip with Aztec-compatible hex/string representations
- no commitment yet to full upstream helper parity like hashing/derivation methods
- enough helpers for tests and later client code

Verification:

- field round-trip tests
- address round-trip tests
- `PublicKeys` and `CompleteAddress` serde tests

### Step 2.2: Transaction and execution types

Implement in `src/tx.rs`:

- `TxHash`
- `TxStatus`
- `TxExecutionResult`
- `TxReceipt`
- `ExecutionPayload`
- placeholder-but-typed `AuthWitness`
- placeholder-but-typed `Capsule`
- placeholder-but-typed `HashedValues`

Implement the receipt helpers:

- `is_mined`
- `is_pending`
- `is_dropped`
- `has_execution_succeeded`
- `has_execution_reverted`

Important:

- status set must be exactly: `Dropped`, `Pending`, `Proposed`, `Checkpointed`, `Proven`, `Finalized`
- do not use older statuses like `Mined`

Verification:

- receipt helper unit tests
- status serde round-trip tests

### Step 2.3: ABI foundation

Implement the minimum viable ABI layer in `src/abi.rs`:

- `FunctionSelector`
- `EventSelector`
- `FunctionType`
- `AbiType`
- `AbiValue`
- `FunctionArtifact`
- `ContractArtifact`

Keep this intentionally thin.

Requirements:

- load contract artifact JSON
- identify functions by name and type
- compute/select selectors
- represent arguments and return values for simple types first

Do not try to fully mirror the whole upstream artifact schema in the first pass.

Verification:

- parse one or two minimal fixture artifacts
- selector tests against known values once confirmed from upstream

### Step 2.4: Contract instance types

Implement:

- `ContractInstance`
- `ContractInstanceWithAddress`

Match the corrected spec fields:

- `version`
- `salt`
- `deployer`
- `current_contract_class_id`
- `original_contract_class_id`
- `initialization_hash`
- `public_keys`
- `address`

Important:

- do not rename this back to the older simplified `class_id` shape
- serialized form must match upstream naming/shape, even if Rust ergonomics add helper methods

Verification:

- serde fixture tests

## Phase 3 — Node Client **COMPLETED**

### Step 3.1: Shared JSON-RPC transport

Create a small internal JSON-RPC helper used first by the node client and later reused by any wallet transport.

Requirements:

- request envelope creation
- response parsing
- JSON-RPC error handling
- transport timeout propagation

Keep this private. Do not expose a generic RPC layer publicly yet.

Verification:

- request/response unit tests with mocked payloads

### Step 3.2: `AztecNode` trait and HTTP client

Implement in `src/node.rs`:

- `AztecNode` trait
- `HttpNodeClient`
- `create_aztec_node_client`

MVP node trait:

- `get_node_info`
- `get_block_number`
- `get_tx_receipt`
- `get_public_logs`

Do not add speculative node methods to the trait unless they are needed immediately.

Important corrections from the old plan:

- `create_aztec_node_client` should align with the spec and return a usable client, not introduce extra behavior not present in the spec
- `wait_for_node` should use `get_node_info`, not a separate `is_ready` RPC assumption
- avoid prematurely adding methods like `get_proven_block_number`, `get_version`, or `get_chain_id` to the core trait unless their exact source and need are already validated

Verification:

- `NodeInfo` fixture deserialization tests
- mock-based RPC tests for each implemented node method

### Step 3.3: Readiness and receipt polling

Implement:

- `wait_for_node`
- `wait_for_tx`
- `WaitOpts`

Behavior:

- `wait_for_node` retries `get_node_info`
- `wait_for_tx` retries `get_tx_receipt`
- terminal success threshold defaults to `Checkpointed` or higher
- pending/dropped/revert behavior must follow the corrected spec, not an invented simplified flow

Verification:

- mock client tests for success, delayed success, timeout
- receipt progression tests

### Step 3.4: Example

Add `examples/node_info.rs`:

- create node client
- wait for node
- print node info
- print block number

Verification:

```bash
cargo run --example node_info
```

## Phase 4 — Wallet Abstraction **COMPLETED**

### Step 4.1: Define the `Wallet` trait

Implement the `Wallet` trait in `src/wallet.rs` following the corrected spec.

Include supporting types only as needed:

- `ChainInfo`
- `Aliased<T>`
- `ContractMetadata`
- `ContractClassMetadata`
- `SimulateOptions`
- `SendOptions`
- `ProfileOptions`
- `ExecuteUtilityOptions`
- `TxSimulationResult`
- `TxProfileResult`
- `UtilityExecutionResult`
- `SendResult`
- `EventMetadataDefinition`
- `PrivateEventFilter`
- `PrivateEvent<T>`
- `MessageHashOrIntent`

Important:

- keep these types small and explicit
- do not import the entire `aztec.js` wallet capability surface into the MVP

Verification:

```bash
cargo check
```

### Step 4.2: First concrete wallet backend

Do not assume, without verification, that there is a stable standalone `wallet_*` JSON-RPC server matching `aztec.js`.

Instead:

1. define the `Wallet` trait cleanly
2. implement a mock/in-memory test wallet first for contract-flow testing
3. only add an HTTP-backed wallet client once the actual transport target is validated in the local Aztec environment

This avoids baking a wrong transport model into the SDK.

Deliverables:

- `MockWallet` or equivalent test implementation
- enough behavior to support contract interaction tests

Optional follow-up, only after transport validation:

- `HttpWalletClient`

Verification:

- compile-time trait conformance
- unit tests for trait-backed simulation/send flows

## Phase 5 — Contract Interaction **COMPLETED**

### Step 5.1: `Contract`

Implement in `src/contract.rs`:

- `Contract<W>`
- `Contract::at`
- dynamic method lookup by ABI

Behavior:

- locate function by name
- build `FunctionCall`
- preserve function type and staticness from artifact metadata

Verification:

- artifact-based method lookup tests
- function-not-found error tests

### Step 5.2: `ContractFunctionInteraction`

Implement:

- `request`
- `simulate`
- `send`

Requirements:

- `request` wraps exactly one call into `ExecutionPayload`
- `simulate` delegates to `Wallet::simulate_tx`
- `send` delegates to `Wallet::send_tx`

Verification:

- mock wallet delegation tests

### Step 5.3: Example

Add `examples/contract_call.rs`:

- load artifact from JSON
- build contract handle
- simulate one method call

This example may use a mock wallet first, and later a real backend when available.

## Phase 6 — Events

### Step 6.1: Public events helper

Implement `src/events.rs` with:

- `PublicEvent<T>`
- `PublicEventFilter`
- `GetPublicEventsResult<T>`
- `get_public_events`

Requirements:

- use `AztecNode::get_public_logs`
- decode using ABI/event metadata
- preserve pagination-related metadata where needed

Verification:

- mocked public log decoding tests

### Step 6.2: Private event types

Add the data types and wallet-facing interfaces for private events, but keep the first implementation minimal.

Do not block the MVP on full private-event support.

## Phase 7 — Account Model

### Step 7.1: Account traits

Implement in `src/account.rs`:

- `AuthorizationProvider`
- `Account`
- `AccountContract`
- minimal supporting request/options types

Important:

- keep this aligned to the corrected spec’s account-entrypoint model
- do not redesign accounts around a generic signer abstraction first

Verification:

- compile-time trait tests with mocks

### Step 7.2: `AccountManager`

Implement:

- `AccountManager`
- address/instance accessors
- `account()`
- `deploy_method()`

Important:

- deployment logic should depend on a validated wallet/deployment path
- if exact account deployment transport details remain uncertain, keep the first implementation thin and explicit rather than speculative

Verification:

- deterministic constructor/unit tests for account manager state

## Phase 8 — Deployment

### Step 8.1: Deployment helpers

Implement in `src/deployment.rs`:

- `publish_contract_class`
- `publish_instance`
- `ContractDeployer`
- `DeployMethod`

Requirements:

- keep the API builder-based
- support simulation and send
- avoid embedding unverified deployment publication shortcuts in the first implementation

Verification:

- payload construction unit tests
- delegation tests through a mock wallet

### Step 8.2: `BatchCall`

Implement `BatchCall` after single-call simulate/send is stable.

Requirements:

- aggregate `FunctionCall`s into one `ExecutionPayload`
- expose `request`, `simulate`, `send`

Verification:

- payload-size and delegation tests

## Phase 9 — Hardening

Deliverables:

- artifact fixtures copied from real Aztec outputs
- ignored integration tests against a local Aztec environment
- examples for node, contract, and later deployment/account flows
- documentation comments on public types

## Exit Criteria For First Useful Release

The implementation is ready for the first useful release when it can:

1. create an Aztec node client over HTTP JSON-RPC
2. wait for node readiness via `get_node_info`
3. fetch node info and transaction receipts
4. deserialize a real Aztec contract artifact
5. construct `Contract::at(address, artifact, wallet)`
6. simulate a contract call through the `Wallet` trait
7. send a transaction through the `Wallet` trait
8. poll a receipt until `Checkpointed` or higher

Everything else is important, but should be layered on after those eight work reliably.
