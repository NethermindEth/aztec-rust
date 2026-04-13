# Testing

## Unit Tests

```bash
cargo test
```

Unit tests live inside each crate and do not need a running network.

## E2E Tests

End-to-end tests under `tests/` connect to a running Aztec node (defaults to `http://localhost:8080`).
They are marked `#[ignore]` so they are skipped by default; run them explicitly:

```bash
AZTEC_NODE_URL=http://localhost:8080 \
  cargo test --test contract e2e_token_transfer_private:: -- --ignored --nocapture
```

## Test Tiers

The E2E suite is organized into tiers; see `E2E_TEST_COVERAGE.md` at the repository root for the current matrix.

## Fixtures

Compiled Noir/Aztec contract artifacts live under `fixtures/` and are consumed by both tests and examples.
