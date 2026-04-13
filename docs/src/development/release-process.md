# Release Process

`aztec-rs` is not yet on crates.io (blocked on pre-release `noir` dependencies).
Releases are cut as git tags on GitHub.

## Cutting a Release

1. Update the workspace version in the root `Cargo.toml`.
2. Update `CHANGELOG.md` with the new section, tagging each entry with the affected crate in parens (e.g. `(aztec-ethereum)`).
   The change will surface verbatim in the rendered [Changelog appendix](../appendix/changelog.md) on the next docs build.
3. Run `cargo build` and the full test matrix.
4. Tag the commit: `git tag vX.Y.Z && git push origin vX.Y.Z`.
5. Publish a GitHub release with notes mirroring the changelog.
6. The docs workflow (`.github/workflows/docs.yml`) rebuilds the book + rustdoc on push to `main`, picking up the new changelog automatically.

## Post-crates.io Plan

Once `noir` stabilizes on crates.io, the workspace will publish there and the `[patch.crates-io]` entries will be removed.
