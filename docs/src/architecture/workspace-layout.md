# Workspace Layout

`aztec-rs` is a Cargo workspace. The root `Cargo.toml` declares members under `crates/` and re-exports them via the umbrella `aztec-rs` crate.

## Directory Tree

```
aztec-rs/
├─ Cargo.toml              # workspace + umbrella crate
├─ src/                    # umbrella re-exports
├─ crates/
│  ├─ core/                # primitives, ABI, errors, tx, validation
│  ├─ rpc/                 # JSON-RPC transport
│  ├─ crypto/              # BN254, Grumpkin, Poseidon2, Schnorr, keys
│  ├─ node-client/         # Aztec node HTTP client + polling
│  ├─ pxe-client/          # PXE trait + shared types
│  ├─ pxe/                 # embedded PXE runtime
│  ├─ wallet/              # BaseWallet + account provider glue
│  ├─ contract/            # contract handles, deployer, events, authwits
│  ├─ account/             # account flavors, entrypoints, deployment
│  ├─ fee/                 # fee payment strategies
│  └─ ethereum/            # L1 client + cross-chain messaging
├─ examples/               # runnable end-to-end samples
├─ fixtures/               # compiled contract artifacts for tests/examples
└─ tests/                  # E2E integration tests
```

## Dependency Layering

Higher layers depend on lower ones; there are no cycles.

```mermaid
graph BT
  subgraph Primitives
    core[aztec-core]
    crypto[aztec-crypto]
  end
  subgraph Transport
    rpc[aztec-rpc]
    node[aztec-node-client]
    eth[aztec-ethereum]
  end
  subgraph Runtime
    pxeClient[aztec-pxe-client]
    pxe[aztec-pxe]
  end
  subgraph UserFacing["User-facing"]
    contract[aztec-contract]
    account[aztec-account]
    fee[aztec-fee]
  end
  wallet[aztec-wallet]
  umbrella[aztec-rs]

  Transport --> Primitives
  pxeClient --> Transport
  pxe --> pxeClient
  UserFacing --> Runtime
  UserFacing --> Transport
  wallet --> UserFacing
  wallet --> Runtime
  umbrella --> wallet
```

## References

- [System Overview](./overview.md)
- [Crate Index](../reference/crates.md)
