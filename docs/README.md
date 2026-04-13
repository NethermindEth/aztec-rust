# `aztec-rs` Documentation

mdBook-based documentation for the `aztec-rs` Rust workspace.

## Build Locally

```bash
cargo install mdbook
cargo install mdbook-mermaid

# one-time: wire mermaid assets into book.toml
mdbook-mermaid install docs

# serve the prose with live reload (no rustdoc API):
mdbook serve docs --open

# one-shot prose-only build:
mdbook build docs

# full build (mdBook + workspace rustdoc, matches CI):
./docs/build.sh
```

`mdbook build docs` produces the prose-only site at `docs/book/`.
`./docs/build.sh` additionally runs `cargo doc --workspace --no-deps` and drops the output at `docs/book/api/`, so every per-crate reference page's `Full API` link resolves.

Output goes to `docs/book/` (git-ignored).

> `mdbook-linkcheck` 0.7.x and `mdbook-admonish` 1.20.x do not currently build against `mdbook` 0.5.x.
> Their config blocks are left commented out in `book.toml` and will be re-enabled once upstream updates.

## Layout

```
docs/
├─ book.toml                # mdBook configuration
├─ src/
│  ├─ SUMMARY.md            # navigation (mirrors the src/ tree)
│  ├─ introduction.md
│  ├─ quickstart.md
│  ├─ concepts/             # theoretical background
│  ├─ guides/               # task-oriented tutorials
│  ├─ architecture/         # system design
│  ├─ reference/            # per-crate API surface + errors + config
│  ├─ development/          # contributor docs
│  └─ appendix/             # glossary, faq, resources
├─ assets/                  # images referenced from pages
├─ diagrams/                # Mermaid / Excalidraw / PlantUML sources
└─ theme/                   # optional mdBook theme overrides
```

## Writing Style

See [`src/development/writing-docs.md`](src/development/writing-docs.md) for the authoring conventions used in this book.

## CI

`.github/workflows/docs.yml` builds the book on every push and pull request.
If `mdbook-linkcheck` is installed, broken links break the build.
