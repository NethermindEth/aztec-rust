# Contributing

`aztec-rs` is an open-source Rust workspace.
Contributions are welcome via pull requests on GitHub.

## Before You Start

- Discuss non-trivial changes in a GitHub issue first.
- Read the [Architecture overview](../architecture/overview.md) to locate the right crate.
- Skim recent entries in the [`CHANGELOG.md`](https://github.com/NethermindEth/aztec-rs/blob/main/CHANGELOG.md).

## Workflow

1. Fork + branch.
2. Build locally (`cargo build`).
3. Run tests (`cargo test`), including any new ones.
4. Run `cargo lint` (the strict Clippy alias) and `cargo fmt`.
5. Update documentation — see [Writing Documentation](./writing-docs.md).
6. Open a PR with a clear description and changelog entry when appropriate.

## Commit Style

- One logical change per commit.
- Use the imperative mood ("add x", "fix y").

## Code of Conduct

Be respectful and constructive.
