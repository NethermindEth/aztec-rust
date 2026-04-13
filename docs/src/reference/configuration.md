# Configuration

`aztec-rs` deliberately has no global configuration: every knob is passed explicitly at construction.
This page lists the main types you'll encounter and the environment variables consumed by the shipped examples.

## Environment Variables

| Variable              | Consumer               | Default                  | Purpose                             |
| --------------------- | ---------------------- | ------------------------ | ----------------------------------- |
| `AZTEC_NODE_URL`      | Examples (`examples/`) | `http://localhost:8080`  | Aztec node endpoint                 |
| `AZTEC_ETHEREUM_URL`  | Examples (`examples/`) | `http://localhost:8545`  | L1 JSON-RPC endpoint                |
| `RUST_LOG`            | Workspace-wide         | (unset)                  | `tracing` / `env_logger` filter     |

## Node Client

- `create_aztec_node_client(url: impl Into<String>) -> HttpNodeClient` — URL is passed at construction time; no global state.
- Retry / timeout on transaction polling is expressed as `WaitOpts` passed to `wait_for_tx`.

### `WaitOpts`

```rust,ignore
pub struct WaitOpts {
    pub timeout:                       Duration,  // default 300 s
    pub interval:                      Duration,  // default 1 s
    pub wait_for_status:               TxStatus,  // default Checkpointed
    pub dont_throw_on_revert:          bool,      // default false
    pub ignore_dropped_receipts_for:   Duration,  // default 5 s
}
```

## PXE

### `EmbeddedPxeConfig`

```rust,ignore
pub struct EmbeddedPxeConfig {
    pub prover_config:     BbProverConfig,
    pub block_sync_config: BlockSyncConfig,
}
```

Construction paths:

- `EmbeddedPxe::create_ephemeral(node)` — in-memory KV + defaults.
- `EmbeddedPxe::create(node, kv)` — bring your own KV backend.
- `EmbeddedPxe::create_with_config(node, kv, config)` — full control.
- `EmbeddedPxe::create_with_prover_config(node, kv, prover_config)` — override just the prover.

Artifact registration is explicit via `register_contract_class` / `register_contract`; there is no implicit lookup.

## Wallet

`SendOptions`, `SimulateOptions`, `ProfileOptions`, and `ExecuteUtilityOptions` are the per-call knobs.
The most commonly set fields on `SendOptions`:

```rust,ignore
SendOptions {
    from:                   alice,
    fee_execution_payload:  Some(fee_payload),    // from a FeePaymentMethod
    gas_settings:           Some(GasSettings::default()),
    ..Default::default()
}
```

## Gas Defaults

`GasSettings::default()` returns the protocol defaults from `aztec_core::constants` (`DEFAULT_DA_GAS_LIMIT`, `DEFAULT_L2_GAS_LIMIT`, `DEFAULT_TEARDOWN_*`).
Override any field for tighter or looser limits.

## See Also

- [`aztec-node-client` reference](./aztec-node-client.md)
- [`aztec-pxe` reference](./aztec-pxe.md)
- [`aztec-wallet` reference](./aztec-wallet.md)
- [`aztec-fee` reference](./aztec-fee.md)
