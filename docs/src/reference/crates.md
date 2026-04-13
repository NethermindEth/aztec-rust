# Crate Index

The workspace members and their roles.
Each crate has its own reference page with module layout, public types, and API notes.

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
