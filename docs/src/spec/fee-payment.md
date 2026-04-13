# Fee Payment

Normative rules for implementations of `FeePaymentMethod` and for wallets assembling fee-paid transactions.

## `FeePaymentMethod` Contract

**FEE-1.** `FeePaymentMethod::get_asset` MUST return the address of the Aztec asset that will be debited to pay the fee.
For Fee-Juice-based methods, this MUST equal `protocol_contract_address::fee_juice()`.

**FEE-2.** `FeePaymentMethod::get_fee_payer` MUST return the account address that will be debited, which MAY differ from the transaction's `from`.

**FEE-3.** `FeePaymentMethod::get_fee_execution_payload` MUST return an `ExecutionPayload` that, when merged with the user's payload, is sufficient to satisfy the network's fee-payment constraints for the resulting transaction — no additional calls SHOULD be required.

**FEE-4.** A `FeePaymentMethod` instance MUST produce deterministic output for a given input state; repeat calls across the same chain head MUST yield equivalent payloads.

## Binding to a Transaction

**FEE-5.** A fee-execution payload MUST NOT be reused across unrelated transactions.
A wallet MUST recompute the payload for each distinct `TxExecutionRequest`.

**FEE-6.** `GasSettings` submitted alongside a fee payment MUST include a `max_fee_per_gas` no lower than the simulation estimate at the time of submission.
Wallets SHOULD budget a margin; callers MAY choose the margin.

## Native Payment

**FEE-7.** `NativeFeePaymentMethod::new(payer)` produces a payment from `payer`'s Fee Juice balance.
Simulation MUST verify the payer holds sufficient balance at the chain head; inclusion-time failure is a correctness concern and SHOULD surface via a simulation error.

## Sponsored Payment

**FEE-8.** `SponsoredFeePaymentMethod` MAY be refused by the sponsor contract at inclusion.
Applications consuming sponsored payment MUST surface inclusion-time revert to the user with the sponsor's reason.

## Claim-Based Payment

**FEE-9.** `FeeJuicePaymentMethodWithClaim` MUST verify the referenced L1 deposit is consumable on L2 before proving the transaction; consumption of an unready claim is a correctness bug.

**FEE-10.** A claim message MUST be consumed at most once.
The network enforces this via the nullifier tree; implementations MUST NOT attempt to replay a successful claim tx.

## Meta Payment

**FEE-11.** `AccountEntrypointMetaPaymentMethod` wraps the fee call through the account's own entrypoint; it MUST be used only when the account entrypoint supports fee-carrying calls, and MUST produce the same asset/payer guarantees as the underlying strategy it wraps.

## Custom `FeePaymentMethod` Implementations

**FEE-12.** A custom implementation MUST satisfy FEE-1 through FEE-5.
Implementations serving private FPC flows or other patterns MUST document any additional guarantees they provide.
