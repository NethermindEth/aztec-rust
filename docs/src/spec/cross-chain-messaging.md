# Cross-Chain Messaging

Normative rules for producing, waiting on, and consuming L1↔L2 messages.

## Message Identity

**CROSS-1.** An `L1ToL2Message` MUST bind all of `{sender: L1Actor, recipient: L2Actor, content: Fr, secret_hash: Fr}`.
Altering any field produces a distinct message and MUST NOT succeed as a replacement for the original.

**CROSS-2.** `secret_hash` MUST be computed from the secret via the Aztec-canonical derivation (see `aztec_ethereum::messaging::generate_claim_secret`); arbitrary hashes MUST NOT be accepted on the consumption side.

**CROSS-3.** `generate_claim_secret()` MUST use a cryptographically secure random source.
Implementations that seed it deterministically across tx submissions are a vulnerability.

## L1 → L2 Production

**CROSS-4.** `send_l1_to_l2_message` MUST target the Inbox address obtained from the Aztec node's `NodeInfo` (`L1ContractAddresses::from_json`); hard-coded Inbox addresses MUST NOT be used across networks.

**CROSS-5.** An `L1ToL2MessageSentResult` MUST carry both the L1 tx hash and the derived L2 message hash; consumers MUST use the L2 message hash (not the L1 tx hash) for readiness checks.

## Readiness

**CROSS-6.** `is_l1_to_l2_message_ready` MUST return `false` until the node's archiver has committed the enclosing L1 block.
A `true` response is the precondition for consumption.

**CROSS-7.** Readiness is *advisory* relative to L1 finality.
Applications that require L1 finality MUST additionally wait for the L2 block consuming the message to reach `Proven`.

## L1 → L2 Consumption

**CROSS-8.** An L2 consumer MUST supply both the `secret` and the exact `content` originally produced.
Kernel verification MUST reject mismatches.

**CROSS-9.** A message MUST be consumed at most once.
The network enforces this via the L1-to-L2 nullifier tree; clients MUST NOT attempt to replay a successful consumption.

## L2 → L1

**CROSS-10.** An L2-emitted message MUST NOT be treated as consumable on L1 before the enclosing block is `Proven`.

**CROSS-11.** L1-side consumption MUST verify the inclusion proof against the Outbox root stored in the L1 rollup contract.

**CROSS-12.** An L2-to-L1 message MUST be consumed at most once on L1; the Outbox enforces this.

## Fee Juice Bridge

**CROSS-13.** `prepare_fee_juice_on_l1` MUST produce an `L2AmountClaim` whose `secret_hash` matches the one used in the L1 deposit; applications MUST NOT use a different secret/hash pair for the corresponding `FeeJuicePaymentMethodWithClaim`.

**CROSS-14.** The `L2AmountClaim` produced by a bridge helper MUST be usable exactly once; after consumption it MUST be discarded.
