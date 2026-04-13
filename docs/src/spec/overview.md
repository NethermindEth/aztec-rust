# Specification Overview

This section collects the **normative client-side rules** that an `aztec-rs`-based
implementation MUST follow to be considered correct.
It is intentionally separate from [Architecture](../architecture/overview.md) — architecture
pages describe how the code is organized; these pages describe what the code must guarantee.

## Scope

Normative for:

- Any implementation of the [`Pxe` trait](../reference/aztec-pxe-client.md).
- Any implementation of [`Wallet`](../reference/aztec-wallet.md) or [`AccountProvider`](../reference/aztec-wallet.md).
- Any implementation of [`FeePaymentMethod`](../reference/aztec-fee.md).
- Client code that sends transactions, consumes cross-chain messages, or produces authwits.

Out of scope:

- The Aztec network protocol itself (block validity, consensus, public kernel rules).
  Those live in the [Aztec protocol docs](https://docs.aztec.network) and the [aztec-packages](https://github.com/AztecProtocol/aztec-packages) monorepo.
- Noir / ACVM semantics.
- L1 smart-contract behavior beyond what portals expose to clients.

## Conformance Language

Normative keywords in this section follow [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) / [RFC 8174](https://www.rfc-editor.org/rfc/rfc8174) when rendered in all caps:

| Keyword              | Meaning                                                                 |
| -------------------- | ----------------------------------------------------------------------- |
| **MUST**             | Absolute requirement. Non-compliance is a bug.                          |
| **MUST NOT**         | Absolute prohibition.                                                   |
| **SHOULD**           | Recommended; deviation requires justification.                          |
| **SHOULD NOT**       | Recommended against.                                                    |
| **MAY**              | Optional.                                                               |

Clauses are numbered per page (e.g. `TX-1`, `AUTH-3`) for cross-reference.

## Sections

- [Transaction Lifecycle](./tx-lifecycle.md) — rules on `TxStatus`, waiting, revert handling.
- [Authorization](./authorization.md) — authwit construction and consumption.
- [Fee Payment](./fee-payment.md) — rules on `FeePaymentMethod` implementations.
- [Cross-Chain Messaging](./cross-chain-messaging.md) — L1↔L2 message production and consumption.
- [Trust Model](./trust-model.md) — consolidated trust boundaries across all layers.

## Relationship to Architecture

Each architecture page remains the readable "how it works" description.
When an architecture page says *"messages MUST bind sender + recipient + content"*, the authoritative statement of that rule lives in the matching spec page; the architecture page is the prose version.

Corrections to the spec are normative changes and SHOULD bump the workspace minor version.
