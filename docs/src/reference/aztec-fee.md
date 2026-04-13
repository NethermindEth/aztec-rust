# `aztec-fee`

Fee payment strategies for Aztec transactions.
Each strategy produces an `ExecutionPayload` that is merged into the user's tx before submission.

Source: `crates/fee/src/`.

## The `FeePaymentMethod` Trait

```rust,ignore
#[async_trait]
pub trait FeePaymentMethod: Send + Sync {
    /// The asset used for payment (for fee-juice methods, this is the Fee Juice protocol contract).
    async fn get_asset(&self) -> Result<AztecAddress, Error>;

    /// The account paying the fee.
    async fn get_fee_payer(&self) -> Result<AztecAddress, Error>;

    /// The function calls + authwits that actually perform the payment.
    async fn get_fee_execution_payload(&self) -> Result<ExecutionPayload, Error>;
}
```

## Shipped Strategies

| Type                              | Use when                                                                                         |
| --------------------------------- | ------------------------------------------------------------------------------------------------ |
| `NativeFeePaymentMethod`          | The sending account already holds Fee Juice.                                                     |
| `SponsoredFeePaymentMethod`       | A public sponsor contract will pay unconditionally (typical for onboarding flows / test sandboxes). |
| `FeeJuicePaymentMethodWithClaim`  | First-time payer whose Fee Juice has been deposited on L1 and needs to be claimed on L2 before paying. |

## Supporting Types

- `L2AmountClaim` — the claim tuple produced from an L1 deposit and consumed by `FeeJuicePaymentMethodWithClaim`.

## Choosing a Strategy

| Situation                                                          | Strategy                              |
| ------------------------------------------------------------------ | ------------------------------------- |
| Established account with Fee Juice                                 | `NativeFeePaymentMethod`              |
| Sandbox / sponsored onboarding                                     | `SponsoredFeePaymentMethod`           |
| Deploying a fresh account that was funded via `FeeJuicePortal` on L1 | `FeeJuicePaymentMethodWithClaim`      |

Private Fee Payment Contract (FPC) support is on the roadmap but is not currently shipped in this crate; consumers requiring private FPC flows need to implement `FeePaymentMethod` themselves.

## Typical Use

```rust,ignore
use aztec_fee::{FeePaymentMethod, NativeFeePaymentMethod};

let payment = NativeFeePaymentMethod::new(account_address);
let payload = payment.get_fee_execution_payload().await?;
// The wallet merges `payload` with the user's call payload.
```

## Full API

Bundled rustdoc: [`api/aztec_fee/`](../api/aztec_fee/index.html).
Local regeneration:

```bash
cargo doc -p aztec-fee --open
```

## See Also

- [`aztec-core`](./aztec-core.md) — `GasSettings`, `Gas`, `GasFees`.
- [`aztec-ethereum`](./aztec-ethereum.md) — `FeeJuicePortal` helpers for producing `L2AmountClaim`.
- [Concepts: Fees](../concepts/fees.md)
