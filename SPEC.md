# aztec-rust SDK Specification

## Scope

`aztec-rust` is a Rust SDK for the Aztec Network. It should take structural inspiration from `starknet-rust`, but its public API should map to the current `aztec.js` surface from `aztec-packages`, not to an older or imagined PXE API.

This document replaces the previous draft and corrects the main errors:

- do not start with a large multi-crate workspace; the repository is empty and the first milestone should be minimal
- model the SDK around the actual `aztec.js` modules: `abi`, `account`, `wallet`, `contracts`, `deployment`, `node`, `tx`, `events`, `fee`, `authorization`, `messaging`
- make `wallet`, not raw `PXE`, the main private execution interface for end users
- use the current transaction lifecycle: `dropped`, `pending`, `proposed`, `checkpointed`, `proven`, `finalized`
- use current contract instance fields: `version`, `salt`, `deployer`, `current_contract_class_id`, `original_contract_class_id`, `initialization_hash`, `public_keys`, `address`
- keep contract calls and deployment builder-based, but avoid specifying speculative features before the foundation exists

## Upstream Basis

This spec is based on the current local checkouts:

- `starknet-rust` at `/Users/alexmetelli/source/starknet-rust`
- `aztec-packages` at `/Users/alexmetelli/source/aztec-packages`

Relevant upstream entrypoints observed in `aztec.js`:

- `@aztec/aztec.js/abi`
- `@aztec/aztec.js/account`
- `@aztec/aztec.js/wallet`
- `@aztec/aztec.js/contracts`
- `@aztec/aztec.js/deployment`
- `@aztec/aztec.js/node`
- `@aztec/aztec.js/tx`
- `@aztec/aztec.js/events`
- `@aztec/aztec.js/fee`
- `@aztec/aztec.js/authorization`
- `@aztec/aztec.js/messaging`

## Design Principles

- Start monolithic, split later. Unlike `starknet-rust`, this repository currently has no crates, so the MVP should begin as one crate with modules. Split into subcrates only after the API stabilizes.
- Mirror Aztec concepts, not Starknet concepts. `wallet`, account lifecycle, authwit, contract registration, and private/public event access are first-class.
- Match upstream naming where practical, but use idiomatic Rust type names and traits.
- Keep the first version transport-light: JSON-RPC over HTTP only.
- Make async part of the public API from the start.
- Preserve room for a later workspace split similar to `starknet-rust`.

## Initial Repository Layout

Phase 1 should use a single crate:

```text
aztec-rust/
  Cargo.toml
  src/
    lib.rs
    abi.rs
    types.rs
    node.rs
    wallet.rs
    account.rs
    contract.rs
    deployment.rs
    tx.rs
    events.rs
    fee.rs
    authorization.rs
    messaging.rs
    error.rs
  examples/
```

Planned later split, only after the monolithic crate is proven:

```text
aztec-rust/
  aztec-rs-core
  aztec-rs-node
  aztec-rs-wallet
  aztec-rs-account
  aztec-rs-contract
  aztec-rs-crypto
  aztec-rs
```

The umbrella crate `aztec-rs` can then mirror the `starknet-rust` re-export pattern.

## Public Module Map

The Rust crate should expose modules analogous to the `aztec.js` subpath exports:

| aztec.js | aztec-rust | Notes |
|---|---|---|
| `abi` | `aztec_rs::abi` | Artifacts, selectors, ABI encoding/decoding |
| `account` | `aztec_rs::account` | Account traits, account contracts, account manager |
| `wallet` | `aztec_rs::wallet` | Main private execution client trait |
| `contracts` | `aztec_rs::contract` | `Contract`, interactions, batch calls |
| `deployment` | `aztec_rs::deployment` | Publish class/instance, deployers |
| `node` | `aztec_rs::node` | Node client, readiness, receipt polling |
| `tx` | `aztec_rs::tx` | `TxHash`, `TxReceipt`, payloads, simulation/profile output |
| `events` | `aztec_rs::events` | Public event queries and decoding |
| `fee` | `aztec_rs::fee` | Fee payment methods and gas options |
| `authorization` | `aztec_rs::authorization` | Authwit types and helpers |
| `messaging` | `aztec_rs::messaging` | L1<->L2 messaging helpers |

## Core Types

These types should exist early because the rest of the SDK depends on them.

### Fields and Curves

Use BN254 and Grumpkin-compatible representations.

```rust
pub struct Fr(...);
pub struct Fq(...);
pub struct GrumpkinScalar(...);

pub struct Point {
    pub x: Fr,
    pub y: Fr,
    pub is_infinite: bool,
}
```

Notes:

- `Fr` and `Fq` must round-trip cleanly to Aztec JSON formats
- `Point` must match the semantics used for Aztec public keys
- implementation may initially wrap existing field/curve crates; do not commit to a custom field implementation in the MVP spec

### Addresses and Keys

```rust
pub struct AztecAddress(pub Fr);
pub struct EthAddress(pub [u8; 20]);

pub struct PublicKeys {
    pub master_nullifier_public_key: Point,
    pub master_incoming_viewing_public_key: Point,
    pub master_outgoing_viewing_public_key: Point,
    pub master_tagging_public_key: Point,
}

pub struct CompleteAddress {
    pub address: AztecAddress,
    pub public_keys: PublicKeys,
    pub partial_address: Fr,
}
```

### ABI and Contract Metadata

```rust
pub struct FunctionSelector(pub [u8; 4]);
pub struct EventSelector(pub Fr);

pub enum FunctionType {
    Private,
    Public,
    Utility,
}

pub struct FunctionCall {
    pub to: AztecAddress,
    pub selector: FunctionSelector,
    pub args: Vec<AbiValue>,
    pub function_type: FunctionType,
    pub is_static: bool,
}

pub struct ContractArtifact {
    pub name: String,
    pub functions: Vec<FunctionArtifact>,
}

pub struct FunctionArtifact {
    pub name: String,
    pub function_type: FunctionType,
    pub is_initializer: bool,
}
```

The Rust SDK should not try to fully invent a Rust-native ABI schema in v0.1. The priority is:

1. deserialize Aztec artifacts
2. compute selectors
3. encode arguments
4. decode return values and events

### Contract Instance

This must match the current upstream shape, not the simplified older draft:

```rust
pub struct ContractInstance {
    pub version: u8,
    pub salt: Fr,
    pub deployer: AztecAddress,
    pub current_contract_class_id: Fr,
    pub original_contract_class_id: Fr,
    pub initialization_hash: Fr,
    pub public_keys: PublicKeys,
}

pub struct ContractInstanceWithAddress {
    pub address: AztecAddress,
    pub inner: ContractInstance,
}
```

Rust may flatten `ContractInstanceWithAddress` ergonomically, but serialized form must match upstream.

### Transactions

```rust
pub struct TxHash(pub [u8; 32]);

pub enum TxStatus {
    Dropped,
    Pending,
    Proposed,
    Checkpointed,
    Proven,
    Finalized,
}

pub enum TxExecutionResult {
    Success,
    AppLogicReverted,
    TeardownReverted,
    BothReverted,
}

pub struct TxReceipt {
    pub tx_hash: TxHash,
    pub status: TxStatus,
    pub execution_result: Option<TxExecutionResult>,
    pub error: Option<String>,
    pub transaction_fee: Option<u128>,
    pub block_hash: Option<[u8; 32]>,
    pub block_number: Option<u64>,
    pub epoch_number: Option<u64>,
}

pub struct ExecutionPayload {
    pub calls: Vec<FunctionCall>,
    pub auth_witnesses: Vec<AuthWitness>,
    pub capsules: Vec<Capsule>,
    pub extra_hashed_args: Vec<HashedValues>,
    pub fee_payer: Option<AztecAddress>,
}
```

The SDK should also expose typed simulation/profile outputs, but their exact shape can remain thin wrappers around upstream JSON until stabilized.

## Client Model

### Node Client

This corresponds to `@aztec/aztec.js/node`.

```rust
#[async_trait]
pub trait AztecNode: Send + Sync {
    async fn get_node_info(&self) -> Result<NodeInfo, Error>;
    async fn get_block_number(&self) -> Result<u64, Error>;
    async fn get_tx_receipt(&self, tx_hash: &TxHash) -> Result<TxReceipt, Error>;
    async fn get_public_logs(&self, filter: PublicLogFilter) -> Result<PublicLogsResponse, Error>;
}

pub fn create_aztec_node_client(url: impl Into<String>) -> HttpNodeClient;
pub async fn wait_for_node(node: &impl AztecNode) -> Result<(), Error>;
pub async fn wait_for_tx(node: &impl AztecNode, tx_hash: &TxHash, opts: WaitOpts) -> Result<TxReceipt, Error>;
```

The Rust naming should stay close to upstream:

- `create_aztec_node_client`
- `wait_for_node`
- `wait_for_tx`

Do not collapse node and wallet into one trait. Public node access and private wallet access are distinct.

### Wallet

This is the main private execution interface and should be the central trait in the first SDK design.

```rust
#[async_trait]
pub trait Wallet: Send + Sync {
    async fn get_chain_info(&self) -> Result<ChainInfo, Error>;
    async fn get_accounts(&self) -> Result<Vec<Aliased<AztecAddress>>, Error>;
    async fn get_address_book(&self) -> Result<Vec<Aliased<AztecAddress>>, Error>;
    async fn register_sender(&self, address: AztecAddress, alias: Option<String>) -> Result<AztecAddress, Error>;

    async fn get_contract_metadata(&self, address: AztecAddress) -> Result<ContractMetadata, Error>;
    async fn get_contract_class_metadata(&self, class_id: Fr) -> Result<ContractClassMetadata, Error>;
    async fn register_contract(
        &self,
        instance: ContractInstanceWithAddress,
        artifact: Option<ContractArtifact>,
        secret_key: Option<Fr>,
    ) -> Result<ContractInstanceWithAddress, Error>;

    async fn get_private_events<T: Send + 'static>(
        &self,
        event_metadata: EventMetadataDefinition,
        filter: PrivateEventFilter,
    ) -> Result<Vec<PrivateEvent<T>>, Error>;

    async fn simulate_tx(&self, exec: ExecutionPayload, opts: SimulateOptions) -> Result<TxSimulationResult, Error>;
    async fn execute_utility(
        &self,
        call: FunctionCall,
        opts: ExecuteUtilityOptions,
    ) -> Result<UtilityExecutionResult, Error>;
    async fn profile_tx(&self, exec: ExecutionPayload, opts: ProfileOptions) -> Result<TxProfileResult, Error>;
    async fn send_tx(&self, exec: ExecutionPayload, opts: SendOptions) -> Result<SendResult, Error>;

    async fn create_auth_wit(
        &self,
        from: AztecAddress,
        message_hash_or_intent: MessageHashOrIntent,
    ) -> Result<AuthWitness, Error>;
}
```

Important correction:

- the previous spec made `PXE` the primary user-facing trait
- the current SDK should make `Wallet` primary and keep any lower-level PXE transport internal or secondary

## Account Model

### Account

Follow `aztec.js/account`: an account is an entrypoint plus authwit provider plus complete address.

```rust
#[async_trait]
pub trait AuthorizationProvider: Send + Sync {
    async fn create_auth_wit(
        &self,
        intent: MessageHashOrIntent,
        chain_info: &ChainInfo,
    ) -> Result<AuthWitness, Error>;
}

#[async_trait]
pub trait Account: Send + Sync + AuthorizationProvider {
    fn complete_address(&self) -> &CompleteAddress;
    fn address(&self) -> AztecAddress;

    async fn create_tx_execution_request(
        &self,
        exec: ExecutionPayload,
        gas_settings: GasSettings,
        chain_info: &ChainInfo,
        options: EntrypointOptions,
    ) -> Result<TxExecutionRequest, Error>;

    async fn wrap_execution_payload(
        &self,
        exec: ExecutionPayload,
        options: EntrypointOptions,
    ) -> Result<ExecutionPayload, Error>;
}
```

### Account Contract

This should match the current account contract abstraction, not a signer-generic trait invented from scratch.

```rust
#[async_trait]
pub trait AccountContract: Send + Sync {
    async fn contract_artifact(&self) -> Result<ContractArtifact, Error>;

    async fn initialization_function_and_args(
        &self,
    ) -> Result<Option<InitializationSpec>, Error>;

    fn account(&self, address: CompleteAddress) -> Box<dyn Account>;
    fn auth_witness_provider(&self, address: CompleteAddress) -> Box<dyn AuthorizationProvider>;
}
```

### Account Manager

`AccountManager` should be wallet-backed and deployment-oriented.

```rust
pub struct AccountManager<W> {
    wallet: W,
    secret_key: Fr,
    account_contract: Box<dyn AccountContract>,
    instance: ContractInstanceWithAddress,
    salt: Fr,
}

impl<W: Wallet> AccountManager<W> {
    pub async fn create(
        wallet: W,
        secret_key: Fr,
        account_contract: Box<dyn AccountContract>,
        salt: Option<Fr>,
    ) -> Result<Self, Error>;

    pub async fn complete_address(&self) -> Result<CompleteAddress, Error>;
    pub fn address(&self) -> AztecAddress;
    pub fn instance(&self) -> &ContractInstanceWithAddress;
    pub async fn account(&self) -> Result<AccountWithSecretKey, Error>;
    pub async fn deploy_method(&self) -> Result<DeployAccountMethod<W>, Error>;
}
```

This is materially closer to `aztec.js` than the previous draft.

## Contract Interaction

### Contract

```rust
pub struct Contract<W> {
    pub address: AztecAddress,
    pub artifact: ContractArtifact,
    wallet: W,
}

impl<W: Wallet> Contract<W> {
    pub fn at(address: AztecAddress, artifact: ContractArtifact, wallet: W) -> Self;

    pub fn method(&self, name: &str, args: Vec<AbiValue>) -> Result<ContractFunctionInteraction<'_, W>, Error>;
}
```

Typed code generation is not an MVP requirement. Dynamic ABI-driven calling is enough initially.

### ContractFunctionInteraction

```rust
pub struct ContractFunctionInteraction<'a, W> {
    wallet: &'a W,
    call: FunctionCall,
}

impl<'a, W: Wallet> ContractFunctionInteraction<'a, W> {
    pub fn request(&self) -> Result<ExecutionPayload, Error>;
    pub async fn simulate(&self, opts: SimulateOptions) -> Result<TxSimulationResult, Error>;
    pub async fn send(&self, opts: SendOptions) -> Result<SendResult, Error>;
}
```

### BatchCall

`BatchCall` is explicitly part of the current `aztec.js` contract API and should exist in the Rust MVP after single-call interactions work.

### Deployment

Expose both low-level deployment helpers and the user-facing deployer:

```rust
pub async fn publish_contract_class<W: Wallet>(
    wallet: &W,
    artifact: &ContractArtifact,
) -> Result<ContractFunctionInteraction<'_, W>, Error>;

pub fn publish_instance<W: Wallet>(
    wallet: &W,
    instance: &ContractInstanceWithAddress,
) -> Result<ContractFunctionInteraction<'_, W>, Error>;

pub struct ContractDeployer<W> {
    artifact: ContractArtifact,
    wallet: W,
}

impl<W: Wallet> ContractDeployer<W> {
    pub fn deploy(&self, args: Vec<AbiValue>) -> DeployMethod<W>;
}
```

## Events

Public and private events are intentionally split, matching upstream.

```rust
pub struct Event<T, M> {
    pub event: T,
    pub metadata: M,
}

pub type PrivateEvent<T> = Event<T, InTx>;
pub type PublicEvent<T> = Event<T, PublicEventMetadata>;

pub async fn get_public_events<T>(
    node: &impl AztecNode,
    event_metadata: &EventMetadataDefinition,
    filter: PublicEventFilter,
) -> Result<GetPublicEventsResult<T>, Error>;
```

The node client owns public log access. The wallet owns private event access.

## Fees and Gas

Keep the initial fee API small and aligned to current upstream usage:

- gas settings on `simulate`, `profile`, and `send`
- `FeeJuice`-style default flow first
- optional payment-method abstractions after basic tx execution works

Suggested first pass:

```rust
pub struct Gas {
    pub da_gas: u64,
    pub l2_gas: u64,
}

pub struct GasFees {
    pub fee_per_da_gas: u128,
    pub fee_per_l2_gas: u128,
}

pub struct GasSettings {
    pub gas_limits: Option<Gas>,
    pub teardown_gas_limits: Option<Gas>,
    pub max_fee_per_gas: Option<GasFees>,
    pub max_priority_fee_per_gas: Option<GasFees>,
}
```

Do not make a separate `aztec-rs-fee` crate in the MVP.

## Authorization and Messaging

These should exist as helper modules, not as day-one large subsystems.

### Authorization

- `AuthWitness`
- call-intent hashing helpers
- public authwit registration helpers later

### Messaging

- `L1ToL2Message`
- readiness polling helpers
- bridge convenience methods later

## Error Model

Use typed crate-local errors, but do not overfit the hierarchy in v0.1.

```rust
pub enum Error {
    Transport(String),
    Json(String),
    Abi(String),
    InvalidData(String),
    Rpc { code: i64, message: String },
    Reverted(String),
    Timeout(String),
}
```

Later, this can become per-module error types if the crate is split.

## Minimal MVP

The first implementation should cover only the smallest useful vertical slice:

1. `node`
   - `create_aztec_node_client`
   - `get_node_info`
   - `get_block_number`
   - `get_tx_receipt`
   - `wait_for_node`
   - `wait_for_tx`
2. `types` and `tx`
   - addresses, selectors, receipts, statuses, payload structs
3. `abi`
   - contract artifact loading
   - selector derivation
   - argument encoding for basic field/address types
4. `wallet`
   - a transport-backed `Wallet` trait plus one HTTP JSON-RPC implementation
   - `get_chain_info`
   - `get_accounts`
   - `register_sender`
   - `simulate_tx`
   - `send_tx`
5. `contract`
   - `Contract::at`
   - dynamic method lookup by ABI
   - `simulate`
   - `send`

This is the first milestone that is both minimal and actually useful.

## Incremental Phases

### Phase 1

- single crate `aztec-rs`
- node client
- base types
- receipt polling
- artifact loading
- dynamic contract interaction

### Phase 2

- wallet HTTP implementation
- private tx simulation and send
- wait/send option types
- public event decoding helper

### Phase 3

- account abstractions
- account contract trait
- account manager
- deploy account flow

### Phase 4

- contract deployment helpers
- `BatchCall`
- class publication and instance registration
- private event support

### Phase 5

- fee payment method abstractions
- authwit helper coverage
- optional typed binding generation
- workspace split into subcrates if justified

## Guidance From starknet-rust

The following ideas should be copied from `starknet-rust`:

- clean module boundaries
- strong typed model layer
- builder-based contract interaction
- umbrella re-export crate later
- examples for each major workflow
- traits over concrete backends where it helps testing

The following should not be copied blindly:

- large crate explosion on day one
- Starknet-specific provider/account assumptions
- prioritizing sequencer/provider coverage before the user-facing wallet flow exists

## Non-Goals For v0.1

- full typed contract binding generation
- every upstream wallet capability schema
- browser/WASM support
- hardware signers
- all account contract flavors
- internal PXE reimplementation
- protocol-complete cryptography surface

## Acceptance Criteria

The spec is satisfied for the first milestone when the repository can do all of the following:

1. connect to an Aztec node over HTTP JSON-RPC
2. wait for node readiness
3. fetch node info and transaction receipts
4. load an Aztec contract artifact from JSON
5. construct a contract handle from address plus artifact
6. simulate a contract method through a wallet implementation
7. send a transaction and poll until `checkpointed` or higher

If those seven items work, the SDK has a valid minimal base. Everything else should build on top of that.
