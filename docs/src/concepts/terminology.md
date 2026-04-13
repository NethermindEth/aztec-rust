# Terminology

Shared vocabulary used across the book and the codebase.
See the [Glossary](../appendix/glossary.md) for the alphabetical index.

## Runtime

- **PXE** — Private Execution Environment.
  The client-side runtime that executes private functions, decrypts notes, and produces client proofs.
- **Embedded PXE** — the in-process PXE runtime shipped as `aztec-pxe`.
- **Node** — an Aztec network node that serves public state, block data, and accepts transactions via JSON-RPC.

## Transactions

- **Private function** — a function executed inside the PXE; inputs and outputs remain encrypted on-chain.
- **Public function** — a function executed by the sequencer against public state.
- **Utility function** — an off-chain helper exposed by a contract artifact; does not mutate state.
- **Entrypoint** — the account contract function used to authenticate and dispatch a user's transaction.
- **Authwit** — an authorization witness granting one account permission to act on behalf of another.

## State

- **Note** — a unit of encrypted private state owned by an account.
- **Nullifier** — a value that marks a note as spent.
- **Contract instance** — a deployed contract identified by an address and class.
- **Contract class** — an artifact registered on-chain by its class identifier.

## Cross-Chain

- **L1 message** — an Ethereum-originated message delivered to L2.
- **L2-to-L1 message** — an L2-originated message for consumption on Ethereum.
- **Portal** — the L1 contract that anchors a cross-chain pair.

## Fees

- **Fee Juice** — the asset used to pay fees on Aztec.
- **FPC** — Fee Payment Contract; a contract that sponsors fees for a user.
