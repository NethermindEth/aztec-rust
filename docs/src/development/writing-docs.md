# Writing Documentation

Documentation lives under `docs/` and is built with [mdBook](https://rust-lang.github.io/mdBook/).

## Prerequisites

```bash
cargo install mdbook
cargo install mdbook-mermaid

# After install, wire the generated assets into book.toml:
mdbook-mermaid install docs
```

The `mdbook-linkcheck` and `mdbook-admonish` preprocessors are listed (commented out) in `book.toml`.
Their current releases are incompatible with `mdbook` 0.5.x; re-enable them once upstream ships a compatible version.

## Local Build

Prose-only, with live reload:

```bash
mdbook serve docs --open
```

Full build (mdBook + workspace rustdoc bundled at `docs/book/api/`):

```bash
./docs/build.sh
```

The build script is what CI runs; use it when you want to verify the `Full API` link on each per-crate reference page.

## CI Build

`.github/workflows/docs.yml` runs `mdbook build` on every push and PR touching `docs/**`.
It installs `mdbook` and `mdbook-mermaid`.
Link validation via `mdbook-linkcheck` is gated until the preprocessor is compatible with `mdbook` 0.5.x.

## Style Rules

- Write for the user's task first; describe modules and types only after the reader knows which problem they solve.
- For each user-facing functionality on a page, include at least one of: a runnable `cargo run --example ...` command, a minimal Rust snippet, or a link to the guide that performs the flow end-to-end.
- Start reference pages with **Start From User Tasks** when they expose app-facing APIs.
  Keep module maps, trait dumps, and rustdoc links below that task map.
- Avoid flat API catalog prose such as "this module contains X" unless it is paired with "use this when you need Y".
- Prefer examples that show the full action shape: create the client / wallet / handle, call the method, and show what the user does with the result.
- Link to the closest shipped example under `examples/` whenever one exists.
- Follow the page template: **User Task → Example → Details → Edge Cases → Security → References** for user-facing pages, and **Context → Design → Implementation → Edge Cases → Security → References** for internal architecture/spec pages.
- Use **sentence-per-line** formatting — one sentence per line for clean diffs.
- Avoid heading levels deeper than `###`.
- Use **relative links** between pages: `[...](../architecture/overview.md)`.
- Use normative language (MUST / SHOULD / MAY) only in specification and security sections.
- Keep the `SUMMARY.md` navigation mirror of `src/` layout.

## Adding a Page

1. Create the markdown file in the right section directory.
2. Add an entry to `docs/src/SUMMARY.md`.
3. Cross-link from sibling pages as appropriate.
4. Run `mdbook build docs` to confirm it renders.

## Diagrams

Store diagram sources under `docs/diagrams/`.
Prefer Mermaid for architectural diagrams; use Excalidraw or PlantUML for more complex ones.
