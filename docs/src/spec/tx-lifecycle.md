# Transaction Lifecycle

Normative rules for transaction construction, submission, and status observation.

## Status Transitions

**TX-1.** A `TxStatus` MUST progress monotonically through the ordered set
`Pending → Proposed → Checkpointed → Proven → Finalized`, with the exception of `Dropped`,
which MAY be observed from any pre-`Proposed` state.

**TX-2.** An implementation observing a status transition that skips levels (e.g. `Pending → Proven`)
MUST treat each intermediate level as having been reached for any callbacks or wait conditions targeting them.

**TX-3.** Once a transaction is observed at `Proposed` or later, it MUST NOT be reported as `Dropped`.

## Waiting

**TX-4.** `wait_for_tx` MUST return once the observed `TxStatus` is `>=` the `WaitOpts::wait_for_status` threshold
(using the ordering from TX-1).

**TX-5.** `wait_for_tx` MUST treat `Dropped` receipts received within `WaitOpts::ignore_dropped_receipts_for`
of the submission timestamp as non-terminal, to absorb the mempool/inclusion race.

**TX-6.** `wait_for_tx` MUST fail with `Error::Timeout` when `WaitOpts::timeout` elapses without reaching the target status.

**TX-7.** An implementation of `WaitOpts::dont_throw_on_revert`:

- If `true`, `wait_for_tx` MUST return the reverted `TxReceipt` instead of raising.
- If `false` (the default), `wait_for_tx` MUST raise `Error::Reverted` with the revert reason.

## Submission

**TX-8.** Before calling `AztecNode::send_tx`, the wallet MUST have obtained a `TxProvingResult` from the PXE that corresponds to the exact `TxExecutionRequest` being submitted.

**TX-9.** A wallet MUST NOT submit a transaction whose `fee_execution_payload` was prepared for a different `TxExecutionRequest`.

**TX-10.** If the node rejects a submission with an `Error::Rpc` containing a validation failure,
the wallet MUST NOT retry the same wire-format `Tx` without first re-simulating (chain state may have advanced).

## Receipts

**TX-11.** `get_tx_receipt` MUST return the latest known status from the perspective of the queried node.
Status MAY be stale relative to a different node; callers MUST NOT assume global finality from a single node's response.

**TX-12.** Implementations rendering UI status SHOULD display `Proposed` as "included" rather than "confirmed", since a re-org before `Checkpointed` MAY still invalidate inclusion.

## Private Kernel Outputs

**TX-13.** A `TxProvingResult` MUST carry:

- a wire-format `Tx` with the chonk proof,
- the public inputs consistent with the proven private kernel,
- no private inputs, decrypted notes, or secret keys.

**TX-14.** Anything matching TX-13's exclusions observed in a `TxProvingResult` is a correctness bug and MUST be treated as such.
