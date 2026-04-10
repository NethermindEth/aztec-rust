//! Event log tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_event_logs.test.ts`.
//!
//! All tests require ACVM integration (Phase 1) because they deploy a
//! `TestLog` contract and execute functions that emit encrypted/unencrypted events.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_event_logs -- --ignored
//! ```

#![allow(
    clippy::expect_used,
    clippy::print_stderr,
    clippy::todo,
    dead_code,
    unused_imports
)]

// NOTE: Upstream uses TestLogContract from @aztec/noir-test-contracts.js/TestLog.
// When compiled Noir artifacts for TestLog are available, the setup helpers
// and contract interactions will be wired here. These tests are #[ignore]d
// since they require ACVM.

// ===========================================================================
// describe('Logs')
// ===========================================================================

// describe('functionality around emitting an encrypted log')

/// TS: it('emits multiple events as private logs and decodes them')
///
/// Deploys `TestLogContract`, calls `emit_encrypted_events` 5 times with random
/// preimages, then retrieves private events and verifies:
/// - 10 `ExampleEvent0s` (2 per tx * 5 txs)
/// - 5 `ExampleEvent1s` (1 per tx * 5 txs)
/// - Event fields match the preimages
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn emits_multiple_events_as_private_logs_and_decodes_them() {
    // Mirrors upstream: deploy TestLogContract, emit 5 txs with encrypted events,
    // retrieve via wallet.getPrivateEvents and verify counts + field values.
    //
    // Blocked on: ACVM integration (contract deployment + execution)
    todo!("blocked: requires ACVM (Phase 1) — TestLogContract deployment + emit_encrypted_events execution")
}

/// TS: it('emits multiple unencrypted events as public logs and decodes them')
///
/// Calls `emit_unencrypted_events` 5 times, retrieves public events via
/// getPublicEvents, and verifies 5 `ExampleEvent0s` + 5 `ExampleEvent1s`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn emits_multiple_unencrypted_events_as_public_logs_and_decodes_them() {
    // Mirrors upstream: emit unencrypted events, retrieve via getPublicEvents,
    // verify counts and field values match preimages.
    //
    // Blocked on: ACVM integration (contract execution)
    todo!("blocked: requires ACVM (Phase 1) — TestLogContract execution")
}

/// TS: it('decodes public events with nested structs')
///
/// Calls `emit_nested_event` with random fields (a, b, c, extra), retrieves
/// the public event, and verifies nested struct fields.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn decodes_public_events_with_nested_structs() {
    // Mirrors upstream: emit nested event, retrieve, verify nested.a/b/c and extra_value.
    //
    // Blocked on: ACVM integration (contract execution)
    todo!("blocked: requires ACVM (Phase 1) — TestLogContract execution")
}

/// TS: it('produces unique tags for encrypted logs across nested calls and different transactions')
///
/// Verifies that tags remain unique:
/// 1. Across nested calls within the same contract (proper propagation of
///    `ExecutionTaggingIndexCache` between calls)
/// 2. Across separate transactions that interact with the same function
///    (proper persistence of cache in `TaggingDataProvider` after proving)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn produces_unique_tags_for_encrypted_logs_across_nested_calls_and_different_transactions() {
    // Mirrors upstream:
    // - Call emit_encrypted_events_nested(account2, 4) → 5 calls * 2 logs = 10 logs
    // - Verify all 10 tags are unique within tx1
    // - Call emit_encrypted_events_nested(account2, 2) → 3 calls * 2 logs = 6 logs
    // - Verify all 6 tags are unique within tx2
    // - Verify all 16 tags across both txs are unique
    //
    // Blocked on: ACVM integration (contract deployment + nested execution)
    todo!("blocked: requires ACVM (Phase 1) — nested contract execution + tag verification")
}
