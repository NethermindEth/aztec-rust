# `aztec-wallet`

Wallet trait, `BaseWallet` composition root, and the `AccountProvider` abstraction consumed by [account flavors](./aztec-account.md).

Source: `crates/wallet/src/`.

## Module Map

| Module             | Highlights                                                                                                                |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------- |
| `wallet`           | `Wallet` trait, `MockWallet`, option structs (`SimulateOptions`, `SendOptions`, `ProfileOptions`, `ExecuteUtilityOptions`), result structs (`TxSimulationResult`, `TxProfileResult`, `UtilityExecutionResult`, `SendResult`), `ProfileMode`, `Aliased<T>`, `ContractMetadata`, `ContractClassMetadata`, `EventMetadataDefinition`, `PrivateEventFilter`, `PrivateEventMetadata`, `PrivateEvent` |
| `base_wallet`      | `BaseWallet<P, N, A>`, `create_wallet`, `create_embedded_wallet` (feature-gated)                                          |
| `account_provider` | `AccountProvider` trait                                                                                                    |

## The `Wallet` Trait

```rust,ignore
#[async_trait]
pub trait Wallet: Send + Sync {
    // Identity
    async fn get_chain_info(&self) -> Result<ChainInfo, Error>;
    async fn get_accounts(&self) -> Result<Vec<Aliased<AztecAddress>>, Error>;
    async fn get_address_book(&self) -> Result<Vec<Aliased<AztecAddress>>, Error>;
    async fn register_sender(&self, address: AztecAddress, alias: Option<String>) -> Result<AztecAddress, Error>;

    // Contracts
    async fn register_contract(
        &self,
        instance: ContractInstanceWithAddress,
        artifact: Option<ContractArtifact>,
        secret_key: Option<Fr>,
    ) -> Result<ContractInstanceWithAddress, Error>;
    async fn get_contract_metadata(&self, address: AztecAddress) -> Result<ContractMetadata, Error>;
    async fn get_contract_class_metadata(&self, class_id: Fr) -> Result<ContractClassMetadata, Error>;
    async fn wait_for_contract(&self, address: AztecAddress) -> Result<(), Error>;

    // Transactions
    async fn simulate_tx(&self, exec: ExecutionPayload, opts: SimulateOptions) -> Result<TxSimulationResult, Error>;
    async fn profile_tx(&self, exec: ExecutionPayload, opts: ProfileOptions) -> Result<TxProfileResult, Error>;
    async fn send_tx(&self, exec: ExecutionPayload, opts: SendOptions) -> Result<SendResult, Error>;
    async fn execute_utility(&self, call: FunctionCall, opts: ExecuteUtilityOptions) -> Result<UtilityExecutionResult, Error>;
    async fn wait_for_tx_proven(&self, tx_hash: TxHash) -> Result<(), Error>;

    // Authorization
    async fn create_auth_wit(&self, from: AztecAddress, intent: MessageHashOrIntent) -> Result<AuthWitness, Error>;

    // State access
    async fn get_private_events(&self, meta: &EventMetadataDefinition, filter: PrivateEventFilter) -> Result<Vec<PrivateEvent>, Error>;
    async fn get_public_storage_at(&self, contract: &AztecAddress, slot: &Fr) -> Result<Fr, Error>;
}
```

A blanket `impl<W: Wallet> Wallet for Arc<W>` is provided so `Arc<BaseWallet<_, _, _>>` is itself a `Wallet` — convenient for sharing across tasks.

## `BaseWallet`

```rust,ignore
pub struct BaseWallet<P, N, A> { /* pxe, node, accounts */ }

impl<P: Pxe, N: AztecNode, A: AccountProvider> BaseWallet<P, N, A> { /* Wallet impl */ }
```

- `P` — a PXE backend implementing [`aztec_pxe_client::Pxe`](./aztec-pxe-client.md).
- `N` — a node implementing [`aztec_node_client::AztecNode`](./aztec-node-client.md).
- `A` — an account provider implementing [`AccountProvider`](#accountprovider).

## Construction

### With explicit backends

```rust,ignore
use aztec_wallet::create_wallet;

let wallet = create_wallet(pxe, node, account_provider);
```

### Embedded (recommended for v4.x apps)

```rust,ignore
use aztec_wallet::create_embedded_wallet;

let wallet = create_embedded_wallet("http://localhost:8080", account_provider).await?;
```

Requires the `embedded-pxe` crate feature.
Internally creates an `HttpNodeClient` + ephemeral `EmbeddedPxe` + the provided `AccountProvider`.

## `AccountProvider`

```rust,ignore
#[async_trait]
pub trait AccountProvider: Send + Sync {
    async fn create_tx_execution_request(
        &self,
        from: &AztecAddress,
        exec: ExecutionPayload,
        gas_settings: GasSettings,
        chain_info: &ChainInfo,
        fee_payer: Option<AztecAddress>,
        fee_payment_method: Option<u8>,
    ) -> Result<TxExecutionRequest, Error>;

    async fn create_auth_wit(&self, from: &AztecAddress, intent: MessageHashOrIntent, chain_info: &ChainInfo) -> Result<AuthWitness, Error>;
    async fn get_complete_address(&self, address: &AztecAddress) -> Result<Option<CompleteAddress>, Error>;
    async fn get_accounts(&self) -> Result<Vec<Aliased<AztecAddress>>, Error>;
}
```

Different backends (embedded accounts, CLI signer, browser extension) implement this trait.
[`aztec_account::SingleAccountProvider`](./aztec-account.md) adapts a single concrete account.

## Options & Results

| Type                      | Purpose                                              |
| ------------------------- | ---------------------------------------------------- |
| `SimulateOptions`         | Skip-verification / skip-fee-enforcement toggles     |
| `ProfileOptions`          | Chooses `ProfileMode` (gates vs execution steps)     |
| `SendOptions`             | Fee payment method, gas settings, wait behavior      |
| `ExecuteUtilityOptions`   | Scopes + arguments for utility calls                 |
| `TxSimulationResult`      | Wallet-facing simulation result                      |
| `TxProfileResult`         | Profile output                                       |
| `UtilityExecutionResult`  | Utility return values + logs                         |
| `SendResult`              | `TxHash` + follow-up polling hooks                   |

## Testing

`MockWallet` implements `Wallet` for use in tests — inject scripted responses per method.

## Full API

Bundled rustdoc: [`api/aztec_wallet/`](../api/aztec_wallet/index.html).
Local regeneration:

```bash
cargo doc -p aztec-wallet --open
```

## See Also

- [`aztec-pxe-client`](./aztec-pxe-client.md) — PXE trait consumed by `BaseWallet`.
- [`aztec-account`](./aztec-account.md) — providers (`SingleAccountProvider`) and concrete accounts.
- [`aztec-contract`](./aztec-contract.md) — user-facing APIs generic over `Wallet`.
