# Security Model

Trust assumptions and threat boundaries inside `aztec-rs`.

> The **normative** version of the rules summarized on this page lives in [Specification → Trust Model](../spec/trust-model.md).
> This page is the readable prose narrative; the spec is the authoritative reference.

## Trust Boundaries

| Boundary        | Trusted                               | Untrusted                          |
| --------------- | ------------------------------------- | ---------------------------------- |
| PXE ↔ Node      | PXE-owned state and keys              | Node RPC responses                 |
| PXE ↔ Contracts | Artifact class hash (if verified)     | Artifact JSON content pre-verify   |
| Wallet ↔ Account| Account provider (signer)             | Call arguments from the app layer  |
| L1 ↔ L2         | Portal contracts at pinned addresses  | Arbitrary cross-chain payloads     |

## Highlights

- Private keys never leave the PXE process — see [TRUST-1](../spec/trust-model.md#trust-boundaries).
- The node is untrusted; responses are either public or kernel-verified — see [TRUST-2](../spec/trust-model.md#trust-boundaries).
- L1 portal addresses come from `NodeInfo`, not from code — see [TRUST-3](../spec/trust-model.md#trust-boundaries).
- The `AccountProvider` is the signing-material trust root — see [TRUST-4](../spec/trust-model.md#trust-boundaries).
- Cross-chain consumption verifies inclusion against a kernel-bound root — see [CROSS-8](../spec/cross-chain-messaging.md#l1--l2-consumption).
- Fee payment payloads bind to one tx and one chain head — see [FEE-5](../spec/fee-payment.md#binding-to-a-transaction).

## References

- [Specification: Trust Model](../spec/trust-model.md)
- [Specification: Transaction Lifecycle](../spec/tx-lifecycle.md)
- [Concepts: PXE](../concepts/pxe.md)
- [Concepts: Accounts & Wallets](../concepts/accounts-and-wallets.md)
