# Installation

## Prerequisites

- Rust toolchain (edition 2021 or later), installed via [rustup](https://www.rust-lang.org/tools/install).
- A running Aztec node for integration tests and examples (see [Aztec docs](https://docs.aztec.network)).

## Adding the Dependency

`aztec-rs` is not yet published to crates.io.
Add it as a git dependency:

```toml
[dependencies]
aztec-rs = { git = "https://github.com/NethermindEth/aztec-rs.git", tag = "v0.5.1" }
tokio   = { version = "1", features = ["full"] }
```

The workspace applies the required `[patch.crates-io]` entries for pre-release `noir` crates automatically.

## Subset Crates

If you only need part of the stack:

```toml
aztec-node-client = { git = "https://github.com/NethermindEth/aztec-rs.git", tag = "v0.5.1" }
aztec-wallet      = { git = "https://github.com/NethermindEth/aztec-rs.git", tag = "v0.5.1" }
aztec-contract    = { git = "https://github.com/NethermindEth/aztec-rs.git", tag = "v0.5.1" }
```

See the [Crate Index](../reference/crates.md) for the full list.

## Verifying the Install

```bash
cargo check
```

Continue with [Connecting to a Node](./connecting-to-a-node.md).
