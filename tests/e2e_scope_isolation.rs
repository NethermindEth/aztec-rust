//! Scope isolation tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_scope_isolation.test.ts`.
//!
//! All tests require ACVM integration (Phase 1) because they deploy a
//! ScopeTest contract and execute private/utility functions that read
//! notes and access keys within scope boundaries.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_scope_isolation -- --ignored
//! ```

#![allow(clippy::expect_used, clippy::print_stderr, dead_code)]

// NOTE: Upstream uses ScopeTestContract from @aztec/noir-test-contracts.js/ScopeTest.
// When compiled Noir artifacts for ScopeTest are available, the setup helpers
// and contract interactions will be wired here. These tests are #[ignore]d
// since they require ACVM.

const ALICE_NOTE_VALUE: u64 = 42;
const BOB_NOTE_VALUE: u64 = 100;

// ===========================================================================
// describe('e2e scope isolation')
// ===========================================================================

// Setup (mirrors upstream beforeAll):
// 1. setup(3) → 3 accounts: alice, bob, charlie
// 2. Deploy ScopeTestContract via alice
// 3. alice creates a note for herself with ALICE_NOTE_VALUE
// 4. bob creates a note for himself with BOB_NOTE_VALUE

// ---------------------------------------------------------------------------
// describe('external private')
// ---------------------------------------------------------------------------

/// TS: it('owner can read own notes')
///
/// Simulates contract.methods.read_note(alice) from alice's scope.
/// Expects result == ALICE_NOTE_VALUE.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_private_owner_can_read_own_notes() {
    // contract.methods.read_note(alice).simulate({ from: alice })
    // expect(value).toEqual(ALICE_NOTE_VALUE)
    todo!("blocked: requires ACVM (Phase 1) — ScopeTestContract deployment + private execution")
}

/// TS: it('cannot read notes belonging to a different account')
///
/// Simulates contract.methods.read_note(alice) from bob's scope.
/// Expects rejection: "Failed to get a note".
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_private_cannot_read_notes_belonging_to_a_different_account() {
    // contract.methods.read_note(alice).simulate({ from: bob })
    // expect → rejects with "Failed to get a note"
    todo!("blocked: requires ACVM (Phase 1) — scope-restricted note access")
}

/// TS: it('cannot access nullifier hiding key of a different account')
///
/// Simulates contract.methods.get_nhk(charlie) from bob's scope.
/// Expects rejection: "Key validation request denied".
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_private_cannot_access_nullifier_hiding_key_of_a_different_account() {
    // contract.methods.get_nhk(charlie).simulate({ from: bob })
    // expect → rejects with "Key validation request denied"
    todo!("blocked: requires ACVM (Phase 1) — scope-restricted key access")
}

/// TS: it('each account can access their isolated state on a shared wallet')
///
/// alice reads her note → ALICE_NOTE_VALUE
/// bob reads his note → BOB_NOTE_VALUE
/// Both use the same wallet but different scopes.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_private_each_account_can_access_their_isolated_state_on_a_shared_wallet() {
    // contract.methods.read_note(alice).simulate({ from: alice }) → ALICE_NOTE_VALUE
    // contract.methods.read_note(bob).simulate({ from: bob }) → BOB_NOTE_VALUE
    todo!("blocked: requires ACVM (Phase 1) — multi-scope private execution")
}

// ---------------------------------------------------------------------------
// describe('external utility')
// ---------------------------------------------------------------------------

/// TS: it('owner can read own notes')
///
/// Simulates contract.methods.read_note_utility(alice) from alice's scope.
/// Expects result == ALICE_NOTE_VALUE.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_utility_owner_can_read_own_notes() {
    // contract.methods.read_note_utility(alice).simulate({ from: alice })
    // expect(value).toEqual(ALICE_NOTE_VALUE)
    todo!("blocked: requires ACVM (Phase 1) — ScopeTestContract utility execution")
}

/// TS: it('cannot read notes belonging to a different account')
///
/// Simulates contract.methods.read_note_utility(alice) from bob's scope.
/// Expects rejection: "Failed to get a note".
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_utility_cannot_read_notes_belonging_to_a_different_account() {
    // contract.methods.read_note_utility(alice).simulate({ from: bob })
    // expect → rejects with "Failed to get a note"
    todo!("blocked: requires ACVM (Phase 1) — scope-restricted utility note access")
}

/// TS: it('cannot access nullifier hiding key of a different account')
///
/// Simulates contract.methods.get_nhk_utility(charlie) from bob's scope.
/// Expects rejection: "Key validation request denied".
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_utility_cannot_access_nullifier_hiding_key_of_a_different_account() {
    // contract.methods.get_nhk_utility(charlie).simulate({ from: bob })
    // expect → rejects with "Key validation request denied"
    todo!("blocked: requires ACVM (Phase 1) — scope-restricted utility key access")
}

/// TS: it('each account can access their isolated state on a shared wallet')
///
/// alice reads via utility → ALICE_NOTE_VALUE
/// bob reads via utility → BOB_NOTE_VALUE
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_utility_each_account_can_access_their_isolated_state_on_a_shared_wallet() {
    // contract.methods.read_note_utility(alice).simulate({ from: alice }) → ALICE_NOTE_VALUE
    // contract.methods.read_note_utility(bob).simulate({ from: bob }) → BOB_NOTE_VALUE
    todo!("blocked: requires ACVM (Phase 1) — multi-scope utility execution")
}
