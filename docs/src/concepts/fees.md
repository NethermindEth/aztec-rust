# Fees

Aztec transactions pay fees in **Fee Juice**.
`aztec-rs` exposes several fee payment strategies via the [`aztec-fee`](../reference/aztec-fee.md) crate.

## Context

Not every user has Fee Juice on the account they are acting from.
Aztec supports sponsored and delegated payment flows to handle onboarding and UX smoothly.

## Design

Shipped strategies (implementors of `FeePaymentMethod`):

- **`NativeFeePaymentMethod`** — the sending account pays directly in Fee Juice.
- **`SponsoredFeePaymentMethod`** — a public sponsor contract pays unconditionally.
- **`FeeJuicePaymentMethodWithClaim`** — the account first claims Fee Juice from an L1 deposit, then pays.

Private FPC support is planned but not yet shipped in [`aztec-fee`](../reference/aztec-fee.md); consumers needing that flow must implement `FeePaymentMethod` themselves.

## Implementation

See [`aztec-fee`](../reference/aztec-fee.md) for payment-method builders and the
[`aztec-ethereum`](../reference/aztec-ethereum.md) `FeeJuicePortal` helpers for L1 deposits.

## Edge Cases

- A transaction that deploys the paying account itself MUST use a strategy that doesn't presume prior deployment.
- Sponsored flows MUST budget for simulation variance between estimate and actual gas.

## Security Considerations

- FPC contracts may refuse service for any reason; applications must handle rejection.
- Claim messages MUST be cleared from the L1 portal to prevent double-claim.

## References

- [Guide: Fee Payments](../guides/fee-payments.md)
- [Architecture: Fee Layer](../architecture/fee-layer.md)
