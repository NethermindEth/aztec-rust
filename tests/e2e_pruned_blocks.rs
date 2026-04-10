//! Pruned blocks tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_pruned_blocks.test.ts`.
//!
//! Tests PXE interacting with a node that has pruned relevant blocks,
//! preventing usage of the archive API (which PXE should not rely on).
//!
//! All tests require ACVM integration (Phase 1) because they deploy a
//! Token contract and execute mint/transfer transactions across pruned
//! block boundaries.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_pruned_blocks -- --ignored
//! ```

#![allow(clippy::expect_used, clippy::print_stderr, clippy::todo, dead_code)]

const MINT_AMOUNT: u64 = 1000;

// Don't make this value too high since we need to mine this number of empty
// blocks, which is relatively slow.
const _WORLD_STATE_CHECKPOINT_HISTORY: u64 = 2;
const _WORLD_STATE_CHECK_INTERVAL_MS: u64 = 300;
const _ARCHIVER_POLLING_INTERVAL_MS: u64 = 300;

// ===========================================================================
// describe('e2e_pruned_blocks')
// ===========================================================================

// Setup (mirrors upstream beforeAll):
// 1. setup(3, { worldStateCheckpointHistory: 2, worldStateBlockCheckIntervalMS: 300,
//    archiverPollingIntervalMS: 300, aztecProofSubmissionEpochs: 1024 })
//    → 3 accounts: admin, sender, recipient
// 2. Deploy TokenContract via admin
// 3. Log token address

/// TS: it('can discover and use notes created in both pruned and available blocks')
///
/// This is the only test in this suite. The flow:
/// 1. Mint half of `MINT_AMOUNT` to sender (creates first note in block N)
/// 2. Verify first note's leaf index is findable via historical query
/// 3. Mine enough blocks (`WORLD_STATE_CHECKPOINT_HISTORY` + 3) to push
///    block N out of the node's available history
/// 4. Verify historical query for block N now fails ("Unable to find leaf")
/// 5. Mint second half of `MINT_AMOUNT` to sender (creates second note)
/// 6. Transfer full `MINT_AMOUNT` from sender to recipient
///    (requires discovering and proving BOTH the old and new notes)
/// 7. Verify recipient balance == `MINT_AMOUNT`
/// 8. Verify sender balance == 0
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn can_discover_and_use_notes_created_in_both_pruned_and_available_blocks() {
    // Mirrors upstream step-by-step:
    //
    // 1. token.methods.mint_to_private(sender, MINT_AMOUNT / 2).send({ from: admin })
    //    → firstMintReceipt
    //
    // 2. aztecNode.getTxEffect(firstMintReceipt.txHash)
    //    → verify noteHashes.length == 1
    //    → aztecNode.findLeavesIndexes(firstMintReceipt.blockNumber, NOTE_HASH_TREE, [mintedNote])
    //    → expect data > 0
    //
    // 3. aztecNodeAdmin.setConfig({ minTxsPerBlock: 0 })
    //    → mine WORLD_STATE_CHECKPOINT_HISTORY + 3 empty blocks via token.methods.private_get_name().send()
    //
    // 4. retryUntil: aztecNode.findLeavesIndexes(firstMintReceipt.blockNumber, ...)
    //    → expect error containing "Unable to find leaf"
    //
    // 5. token.methods.mint_to_private(sender, MINT_AMOUNT / 2).send({ from: admin })
    //
    // 6. token.methods.transfer(recipient, MINT_AMOUNT).send({ from: sender })
    //
    // 7. token.methods.balance_of_private(recipient).simulate({ from: recipient })
    //    → expect result == MINT_AMOUNT
    //
    // 8. token.methods.balance_of_private(sender).simulate({ from: sender })
    //    → expect result == 0
    //
    // Blocked on: ACVM integration (contract deployment + execution + note discovery)
    todo!("blocked: requires ACVM (Phase 1) — Token contract deployment, minting, transfer across pruned blocks")
}
