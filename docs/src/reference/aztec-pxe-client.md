# `aztec-pxe-client`

The PXE trait and shared request / response types.
Depend on this crate when you want to accept any PXE backend (embedded, remote, mock).

Source: `crates/pxe-client/src/`.

## The `Pxe` Trait

```rust,ignore
#[async_trait]
pub trait Pxe: Send + Sync {
    // --- Sync / state ---
    async fn get_synced_block_header(&self) -> Result<BlockHeader, Error>;
    async fn get_contracts(&self) -> Result<Vec<AztecAddress>, Error>;
    async fn get_contract_instance(&self, address: &AztecAddress) -> Result<Option<ContractInstanceWithAddress>, Error>;
    async fn get_contract_artifact(&self, id: &Fr) -> Result<Option<ContractArtifact>, Error>;

    // --- Accounts & senders ---
    async fn register_account(&self, secret_key: &Fr, partial: &PartialAddress) -> Result<CompleteAddress, Error>;
    async fn get_registered_accounts(&self) -> Result<Vec<CompleteAddress>, Error>;
    async fn register_sender(&self, sender: &AztecAddress) -> Result<AztecAddress, Error>;
    async fn get_senders(&self) -> Result<Vec<AztecAddress>, Error>;
    async fn remove_sender(&self, sender: &AztecAddress) -> Result<(), Error>;

    // --- Contracts ---
    async fn register_contract_class(&self, artifact: &ContractArtifact) -> Result<(), Error>;
    async fn register_contract(&self, request: RegisterContractRequest) -> Result<(), Error>;
    async fn update_contract(&self, /* ... */) -> Result<(), Error>;

    // --- Transactions ---
    async fn simulate_tx(&self, request: &TxExecutionRequest, opts: SimulateTxOpts) -> Result<TxSimulationResult, Error>;
    async fn prove_tx(&self, request: &TxExecutionRequest) -> Result<TxProvingResult, Error>;
    async fn profile_tx(&self, request: &TxExecutionRequest, opts: ProfileTxOpts) -> Result<TxProfileResult, Error>;

    // --- Utility execution ---
    async fn execute_utility(&self, call: &FunctionCall, opts: ExecuteUtilityOpts) -> Result<UtilityExecutionResult, Error>;

    // --- Events ---
    async fn get_private_events(&self, filter: PrivateEventFilter) -> Result<Vec<PackedPrivateEvent>, Error>;

    // --- Lifecycle ---
    async fn stop(&self) -> Result<(), Error>;
}
```

Object-safe (see `pxe_is_object_safe` test).
Implementations MUST be `Send + Sync`.

## Key Types

| Type                         | Purpose                                                              |
| ---------------------------- | -------------------------------------------------------------------- |
| `BlockHeader`                | Header returned from `get_synced_block_header`                       |
| `BlockHash`                  | 32-byte block hash                                                   |
| `TxExecutionRequest`         | Everything needed to simulate / prove a tx (origin, calls, authwits, capsules) |
| `TxSimulationResult`         | Output of `simulate_tx`                                              |
| `TxProvingResult`            | Output of `prove_tx` — carries the wire-format `Tx`                  |
| `TxProfileResult`            | Output of `profile_tx` (gate counts, execution steps)                |
| `UtilityExecutionResult`     | Output of `execute_utility` (return values, logs)                    |
| `SimulateTxOpts`             | Skip-verification / fee-enforcement / public-simulation toggles      |
| `ProfileTxOpts`              | `ProfileMode` + capture toggles                                      |
| `ProfileMode`                | Gate-count vs execution-step profiling mode                          |
| `ExecuteUtilityOpts`         | Scopes + arguments for utility calls                                 |
| `PrivateEventFilter`         | Filter for `get_private_events`                                      |
| `PackedPrivateEvent`         | Decoded private event payload                                        |
| `RegisterContractRequest`    | Instance + optional artifact payload for `register_contract`         |
| `LogId`                      | Locator for a specific log entry                                     |

## Full API

Bundled rustdoc: [`api/aztec_pxe_client/`](../api/aztec_pxe_client/index.html).
Local regeneration:

```bash
cargo doc -p aztec-pxe-client --open
```

## See Also

- [`aztec-pxe`](./aztec-pxe.md) — the embedded in-process implementation.
- [`aztec-wallet`](./aztec-wallet.md) — primary consumer of the trait.
