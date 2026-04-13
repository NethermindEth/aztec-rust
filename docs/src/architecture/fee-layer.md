# Fee Layer

`aztec-fee` produces the `ExecutionPayload` that pays a transaction's fee.

## Context

Aztec transactions pay fees in Fee Juice.
The payment payload has to be built and merged with the user's call payload *before* the wallet passes the combined request to the account provider.

Several scenarios require different strategies:

- Established account holding Fee Juice.
- Sponsored onboarding (e.g. sandbox, faucet).
- First-time payer whose Fee Juice was deposited on L1.

## Design

A single trait unifies the strategies:

```rust,ignore
#[async_trait]
pub trait FeePaymentMethod: Send + Sync {
    async fn get_asset(&self) -> Result<AztecAddress, Error>;
    async fn get_fee_payer(&self) -> Result<AztecAddress, Error>;
    async fn get_fee_execution_payload(&self) -> Result<ExecutionPayload, Error>;
}
```

Shipped implementations:

| Type                              | When to use                                                                  |
| --------------------------------- | ---------------------------------------------------------------------------- |
| `NativeFeePaymentMethod`          | Sender already holds Fee Juice                                               |
| `SponsoredFeePaymentMethod`       | Public sponsor contract pays unconditionally (sandboxes, faucets)            |
| `FeeJuicePaymentMethodWithClaim`  | Sender is claiming Fee Juice from an L1 deposit and paying in the same tx   |

A private-FPC strategy is on the roadmap but is not currently shipped; consumers needing it must implement `FeePaymentMethod` themselves.

## Implementation

Each strategy's `get_fee_execution_payload` returns an `ExecutionPayload` containing:

- The protocol-contract calls that move Fee Juice to the sequencer.
- Any auth witnesses needed to satisfy those calls.

`aztec-wallet::SendOptions::fee` selects the strategy; the wallet merges the payload with the user's payload and hands the combined payload to the account provider, which wraps everything in the entrypoint.

`AccountEntrypointMetaPaymentMethod` in [`aztec-account`](../reference/aztec-account.md) is a specialised method that piggybacks the payment through the account's own entrypoint, avoiding a separate public call.

## Edge Cases

- **Estimate drift**: fee estimates can diverge between simulation and inclusion; `GasSettings::max_fee_per_gas` SHOULD include a safety margin.
- **L1 not yet ready**: for `FeeJuicePaymentMethodWithClaim`, the L1 deposit MUST be observable on L2 (via `is_l1_to_l2_message_ready`) before proving; use `wait_for_l1_to_l2_message_ready` when building onboarding flows.
- **Sponsor refusal**: `SponsoredFeePaymentMethod` may be refused by the sponsor contract at inclusion; applications SHOULD handle the resulting revert.

## Security Considerations

- The fee payer can always be inspected via `get_fee_payer` before sending — use this to display the intended payer in UIs.
- Claim messages MUST NOT be replayed; consumption is bound to the specific tx by the kernel.
- Sponsored flows create trust relationships with the sponsor contract — review its policy before depending on it in production.

## References

- [`aztec-fee` reference](../reference/aztec-fee.md)
- [`aztec-ethereum` reference](../reference/aztec-ethereum.md) — `prepare_fee_juice_on_l1` for claim-based flows.
- [Concepts: Fees](../concepts/fees.md)
