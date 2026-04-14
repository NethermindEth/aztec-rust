# Crate Index

Use this page to choose the API surface for the task you are trying to finish.
Each crate still has a detailed reference page, but start with the job-oriented map below before dropping into module layout or rustdoc.

## Choose By Task

| I want to... | Start with | Example |
| ------------ | ---------- | ------- |
| Add one dependency and build a normal app | [`aztec-rs`](./aztec-rs.md) | `cargo run --example wallet_minimal` |
| Connect to a node and inspect chain state | [`aztec-node-client`](./aztec-node-client.md) | `cargo run --example node_info` |
| Create an embedded wallet | [`aztec-wallet`](./aztec-wallet.md) + [`aztec-pxe`](./aztec-pxe.md) | `cargo run --example wallet_minimal` |
| Deploy a contract | [`aztec-contract`](./aztec-contract.md) | `cargo run --example deploy_contract` |
| Call, simulate, profile, and send contract functions | [`aztec-contract`](./aztec-contract.md) + [`aztec-wallet`](./aztec-wallet.md) | `cargo run --example simulate_profile_send` |
| Create or deploy accounts | [`aztec-account`](./aztec-account.md) | `cargo run --example account_deploy` |
| Pay fees | [`aztec-fee`](./aztec-fee.md) | `cargo run --example fee_native` |
| Read public or private events | [`aztec-contract`](./aztec-contract.md) + [`aztec-wallet`](./aztec-wallet.md) | `cargo run --example event_logs` |
| Bridge or consume L1 ↔ L2 messages | [`aztec-ethereum`](./aztec-ethereum.md) | `cargo run --example l1_to_l2_message` |
| Derive keys, sign, or hash protocol values | [`aztec-crypto`](./aztec-crypto.md) + [`aztec-core`](./aztec-core.md) | See the key and hashing snippets below |
| Accept any PXE implementation in a library | [`aztec-pxe-client`](./aztec-pxe-client.md) | Depend on the `Pxe` trait, not `EmbeddedPxe` |
| Make a raw JSON-RPC call | [`aztec-rpc`](./aztec-rpc.md) | Prefer typed clients unless the method is not wrapped yet |

## Common Workflows

### Connect to a node

```rust,no_run
use aztec_rs::node::{create_aztec_node_client, wait_for_node, AztecNode};

# async fn example() -> Result<(), aztec_rs::Error> {
let node = create_aztec_node_client("http://localhost:8080");
let info = wait_for_node(&node).await?;
println!("node v{} at block {}", info.node_version, node.get_block_number().await?);
# Ok(())
# }
```

Run the complete version with:

```bash
cargo run --example node_info
```

### Build an embedded wallet

Most applications should start from the umbrella crate and the embedded wallet path.
It gives you a node client, local PXE, account provider, and wallet methods behind one object.

```rust,ignore
use aztec_rs::wallet::create_embedded_wallet;

let wallet = create_embedded_wallet("http://localhost:8080", account_provider).await?;
let chain = wallet.get_chain_info().await?;
```

For setup details, see [Embedded Wallet Setup](../guides/embedded-wallet-setup.md).

### Deploy and call a contract

```rust,ignore
use aztec_rs::contract::Contract;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::wallet::SendOptions;

let deploy = Contract::deploy(&wallet, artifact.clone(), constructor_args, None)?;
let deployed = deploy
    .send(&DeployOptions::default(), SendOptions { from: owner, ..Default::default() })
    .await?;

let contract = Contract::at(deployed.instance.address, artifact, wallet.clone());
let tx_hash = contract
    .method("increment_public_value", call_args)?
    .send(SendOptions { from: owner, ..Default::default() })
    .await?
    .tx_hash;
```

Run the full deployment example with:

```bash
cargo run --example deploy_contract
```

### Simulate, profile, then send

Use simulation before sending when you need return values or gas estimates.
Use profiling when you need gate or execution-step detail.

```rust,ignore
use aztec_rs::wallet::{ProfileMode, ProfileOptions, SendOptions, SimulateOptions, Wallet};

let sim = wallet
    .simulate_tx(payload.clone(), SimulateOptions { from: owner, estimate_gas: true, ..Default::default() })
    .await?;
let profile = wallet
    .profile_tx(payload.clone(), ProfileOptions { from: owner, profile_mode: Some(ProfileMode::Full), ..Default::default() })
    .await?;
let sent = wallet
    .send_tx(payload, SendOptions { from: owner, ..Default::default() })
    .await?;
```

Run:

```bash
cargo run --example simulate_profile_send
```

### Pay fees explicitly

```rust,ignore
use aztec_rs::fee::{FeePaymentMethod, NativeFeePaymentMethod};
use aztec_rs::wallet::SendOptions;

let fee_payload = NativeFeePaymentMethod::new(alice)
    .get_fee_execution_payload()
    .await?;
let sent = wallet
    .send_tx(payload, SendOptions {
        from: alice,
        fee_execution_payload: Some(fee_payload),
        ..Default::default()
    })
    .await?;
```

Run:

```bash
cargo run --example fee_native
cargo run --example fee_sponsored
cargo run --example fee_juice_claim
```

### Read events

```rust,ignore
let private_events = wallet
    .get_private_events(&event_metadata, private_filter)
    .await?;

let public_events = aztec_rs::events::get_public_events(
    wallet.node(),
    &event_metadata,
    public_filter,
)
.await?;
```

Run:

```bash
cargo run --example event_logs
```

### Use L1 ↔ L2 messaging

```rust,ignore
let sent = aztec_rs::l1_client::send_l1_to_l2_message(
    &eth_client,
    &l1_addresses.inbox,
    &l2_recipient,
    rollup_version,
    &content,
    &secret_hash,
)
.await?;

aztec_rs::cross_chain::wait_for_l1_to_l2_message_ready(
    &node,
    &sent.msg_hash,
    timeout,
)
.await?;
```

Run:

```bash
cargo run --example l1_to_l2_message
cargo run --example l2_to_l1_message
```

## Crate Reference

| Crate | Purpose | Reference |
| ----- | ------- | --------- |
| `aztec-rs` | Umbrella crate; re-exports the full stack | [→](./aztec-rs.md) |
| `aztec-core` | Primitives: ABI, hashes, fees, errors, tx types | [→](./aztec-core.md) |
| `aztec-rpc` | JSON-RPC transport layer | [→](./aztec-rpc.md) |
| `aztec-crypto` | BN254/Grumpkin, Poseidon2, Pedersen, Schnorr, key derivation | [→](./aztec-crypto.md) |
| `aztec-node-client` | Aztec node HTTP client + polling | [→](./aztec-node-client.md) |
| `aztec-pxe-client` | PXE trait + shared request/response types | [→](./aztec-pxe-client.md) |
| `aztec-pxe` | Embedded PXE runtime (stores, execution, kernel, sync) | [→](./aztec-pxe.md) |
| `aztec-wallet` | `BaseWallet` + account-provider integration | [→](./aztec-wallet.md) |
| `aztec-contract` | Contract handles, deployment, authwits, events | [→](./aztec-contract.md) |
| `aztec-account` | Account flavors, entrypoints, deployment helpers | [→](./aztec-account.md) |
| `aztec-fee` | Fee payment strategies | [→](./aztec-fee.md) |
| `aztec-ethereum` | L1 client + L1↔L2 messaging | [→](./aztec-ethereum.md) |

## API Documentation

The full rustdoc for every workspace crate is bundled with this book under [`api/`](../api/aztec_rs/index.html).

| Crate                | Rustdoc index                                                      |
| -------------------- | ------------------------------------------------------------------ |
| `aztec-rs`           | [`api/aztec_rs/`](../api/aztec_rs/index.html)                      |
| `aztec-core`         | [`api/aztec_core/`](../api/aztec_core/index.html)                  |
| `aztec-rpc`          | [`api/aztec_rpc/`](../api/aztec_rpc/index.html)                    |
| `aztec-crypto`       | [`api/aztec_crypto/`](../api/aztec_crypto/index.html)              |
| `aztec-node-client`  | [`api/aztec_node_client/`](../api/aztec_node_client/index.html)    |
| `aztec-pxe-client`   | [`api/aztec_pxe_client/`](../api/aztec_pxe_client/index.html)      |
| `aztec-pxe`          | [`api/aztec_pxe/`](../api/aztec_pxe/index.html)                    |
| `aztec-wallet`       | [`api/aztec_wallet/`](../api/aztec_wallet/index.html)              |
| `aztec-contract`     | [`api/aztec_contract/`](../api/aztec_contract/index.html)          |
| `aztec-account`      | [`api/aztec_account/`](../api/aztec_account/index.html)            |
| `aztec-fee`          | [`api/aztec_fee/`](../api/aztec_fee/index.html)                    |
| `aztec-ethereum`     | [`api/aztec_ethereum/`](../api/aztec_ethereum/index.html)          |

Local regeneration:

```bash
# Whole workspace
cargo doc --workspace --no-deps --open

# Umbrella crate only (public-facing surface)
cargo doc --open

# Bundled build matching what CI produces
./docs/build.sh
```

The `docs/build.sh` script builds the mdBook and the workspace rustdoc together, placing the rustdoc at `docs/book/api/` so the per-crate links above resolve.

## Release Notes

Per-crate changes are tagged inline in the project [Changelog](../appendix/changelog.md) — search for the crate name (e.g. `(aztec-ethereum)`) to filter.
