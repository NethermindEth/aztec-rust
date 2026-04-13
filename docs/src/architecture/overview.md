# System Overview

Architectural map of `aztec-rs` and how the crates fit together.

## High-Level Diagram

```mermaid
graph TD
  App[Application] --> Wallet[aztec-wallet]
  Wallet --> PxeClient[aztec-pxe-client]
  PxeClient --> Pxe[aztec-pxe]
  Wallet --> NodeClient[aztec-node-client]
  Wallet --> Account[aztec-account]
  Account --> Contract[aztec-contract]
  Pxe --> NodeClient
  Pxe --> Crypto[aztec-crypto]
  Contract --> Core[aztec-core]
  NodeClient --> Rpc[aztec-rpc]
  Eth[aztec-ethereum] --> Core
```

## Responsibility Boundaries

| Boundary          | Crate(s)                                                     |
| ----------------- | ------------------------------------------------------------ |
| Transport         | `aztec-rpc`, `aztec-node-client`, `aztec-ethereum`           |
| Runtime           | `aztec-pxe`, `aztec-pxe-client`                              |
| User-facing APIs  | `aztec-wallet`, `aztec-account`, `aztec-contract`, `aztec-fee` |
| Primitives        | `aztec-core`, `aztec-crypto`                                 |
| Umbrella          | `aztec-rs`                                                   |

## References

- [Workspace Layout](./workspace-layout.md)
- [Data Flow](./data-flow.md)
