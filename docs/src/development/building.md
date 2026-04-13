# Building

## Full Workspace

```bash
cargo build
```

## Individual Crate

```bash
cargo build -p aztec-pxe
```

## Release Build

```bash
cargo build --release
```

## Dependency Notes

The workspace patches a handful of `noir` crates to pre-release git revisions.
Those patches are declared in the root `Cargo.toml` and apply automatically.
Make sure your Rust toolchain matches the edition (2021+).
