#!/usr/bin/env bash
# Build the full aztec-rs docs site:
#   1. Build the mdBook (prose + guides + reference pages) into docs/book.
#   2. Build workspace rustdoc (no deps) and copy it to docs/book/api/.
#
# The mdBook links to <doc-root>/api/<crate_name>/ for every per-crate
# reference page, so the rustdoc output is expected at exactly that path.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"

echo ">>> mdbook build"
( cd "$ROOT" && mdbook build docs )

echo ">>> cargo doc --workspace --no-deps --all-features"
( cd "$ROOT" && cargo doc --workspace --no-deps --all-features )

echo ">>> copy target/doc -> docs/book/api"
rm -rf "$HERE/book/api"
mkdir -p "$HERE/book/api"
# Use rsync if available for a readable diff; fall back to cp -R.
if command -v rsync > /dev/null; then
  rsync -a --delete "$ROOT/target/doc/" "$HERE/book/api/"
else
  cp -R "$ROOT/target/doc/." "$HERE/book/api/"
fi

echo ">>> done: $HERE/book/index.html (book)  |  $HERE/book/api/aztec_rs/index.html (rustdoc)"
