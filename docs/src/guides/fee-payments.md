# Fee Payments

Choose and apply a fee payment strategy via [`aztec-fee`](../reference/aztec-fee.md).

## Runnable Examples

- `examples/fee_native.rs` — sender holds Fee Juice.
- `examples/fee_sponsored.rs` — public sponsor pays unconditionally.
- `examples/fee_juice_claim.rs` — claim Fee Juice bridged from L1 and pay in the same tx.

## Strategies

| Type                              | When to use                                                  |
| --------------------------------- | ------------------------------------------------------------ |
| `NativeFeePaymentMethod`          | Account already holds Fee Juice                              |
| `SponsoredFeePaymentMethod`       | A public sponsor contract will pay unconditionally           |
| `FeeJuicePaymentMethodWithClaim`  | First-time user claiming Fee Juice from an L1 deposit        |

Private Fee Payment Contract (FPC) support is planned but is not currently shipped; consumers needing that flow must implement `FeePaymentMethod` themselves.

## Typical Flow

```rust,ignore
use aztec_rs::fee::{FeePaymentMethod, NativeFeePaymentMethod};
use aztec_rs::wallet::SendOptions;

let fee_payload = NativeFeePaymentMethod::new(alice)
    .get_fee_execution_payload()
    .await?;

wallet.send_tx(
    execution_payload,
    SendOptions {
        from: alice,
        fee_execution_payload: Some(fee_payload),
        gas_settings: Some(aztec_rs::fee::GasSettings::default()),
        ..Default::default()
    },
).await?;
```

The wallet merges the fee payload with `execution_payload` before handing the combined request to the account provider.

## Claim-Based Flow (Bridge from L1)

```rust,ignore
use aztec_rs::l1_client::{EthClient, prepare_fee_juice_on_l1};
use aztec_rs::fee::{FeeJuicePaymentMethodWithClaim, FeePaymentMethod};

let bridge = prepare_fee_juice_on_l1(&eth_client, &l1_addresses, &alice).await?;
// Wait for L2-side readiness first (see cross-chain guide).
let fee   = FeeJuicePaymentMethodWithClaim::new(alice, bridge.claim);
let payload = fee.get_fee_execution_payload().await?;
```

## Edge Cases

- **Estimate drift**: adjust `GasSettings::max_fee_per_gas` to include a margin above the simulated value.
- **Sponsor refusal**: `SponsoredFeePaymentMethod` can still be rejected by the sponsor contract at inclusion time; handle the revert.
- **Claim not ready**: verify via `is_l1_to_l2_message_ready` before invoking the claim-based strategy.

## Full Runnable Example

Source: [`examples/fee_native.rs`](https://github.com/NethermindEth/aztec-rs/blob/main/examples/fee_native.rs).

```rust,ignore
{{#include ../../../examples/fee_native.rs}}
```

For sponsored and claim-based variants, see `examples/fee_sponsored.rs` and `examples/fee_juice_claim.rs` in the repository.

## References

- [Concepts: Fees](../concepts/fees.md)
- [`aztec-fee` reference](../reference/aztec-fee.md)
- [`aztec-ethereum` reference](../reference/aztec-ethereum.md)
