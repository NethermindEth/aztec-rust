# Linting & Formatting

## Clippy

The workspace ships a strict Clippy configuration via a `cargo lint` alias:

```bash
cargo lint
```

New warnings are errors in CI.

## Rustfmt

```bash
cargo fmt
```

Run before every commit.
CI enforces a clean `cargo fmt --check`.
