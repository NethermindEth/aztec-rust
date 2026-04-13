# FAQ

## Why isn't `aztec-rs` on crates.io?

It depends on `noir` crates at `1.0.0-beta.18`, which are only available via git.
Once `noir` publishes stable versions, `aztec-rs` will follow.

## Do I run a separate PXE server?

No.
For Aztec v4.x, the PXE runs inside your process via `aztec-pxe`.
`create_embedded_wallet` wires it up for you.

## Can I use a subset of the stack?

Yes.
Depend only on the crates you need — e.g. `aztec-node-client` for read-only node access.
See the [Crate Index](../reference/crates.md).

## Does `aztec-rs` compile Noir?

No.
It consumes compiled artifacts (the JSON files under `fixtures/`) produced by the Aztec/Noir toolchain.

## Where are the examples?

`examples/` at the repository root.
Run them with `cargo run --example <name>`.
