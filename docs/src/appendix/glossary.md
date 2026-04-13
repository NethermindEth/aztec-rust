# Glossary

Alphabetical index of the vocabulary used in this book.
See [Terminology](../concepts/terminology.md) for grouped definitions; each entry here links to the page that introduces the term.

- **Account provider** — a signer abstraction that wraps an account and feeds transaction requests / authwits into the wallet.
  See [`aztec-wallet`](../reference/aztec-wallet.md) (the `AccountProvider` trait) and [Accounts & Wallets](../concepts/accounts-and-wallets.md).
- **Authwit** — authorization witness; delegated permission to act on behalf of another account inside a call.
  See [`aztec-contract`](../reference/aztec-contract.md) (authwit helpers) and [Contracts](../concepts/contracts.md).
- **BN254** — pairing-friendly elliptic curve used by Aztec; its scalar field is the `Fr` type.
  See [`aztec-core`](../reference/aztec-core.md).
- **Class (contract class)** — the on-chain registration of a compiled contract artifact, identified by a class id.
  See [Contracts](../concepts/contracts.md).
- **Embedded PXE** — the in-process PXE implementation shipped as `aztec-pxe`.
  See [`aztec-pxe`](../reference/aztec-pxe.md) and [Architecture: PXE Runtime](../architecture/pxe-runtime.md).
- **Entrypoint** — the account contract function that authenticates a transaction and dispatches its calls.
  See [Accounts & Wallets](../concepts/accounts-and-wallets.md) and [`aztec-account`](../reference/aztec-account.md).
- **Fee Juice** — the fee asset on Aztec.
  See [Fees](../concepts/fees.md).
- **FPC** — Fee Payment Contract; a contract that sponsors (or rebates) fees on behalf of a user.
  See [Fees](../concepts/fees.md) and [`aztec-fee`](../reference/aztec-fee.md).
- **Grumpkin** — curve paired with BN254; its scalar field is `Fq`, used by Aztec account Schnorr signatures.
  See [`aztec-crypto`](../reference/aztec-crypto.md).
- **Inbox / Outbox** — L1 portal contracts for cross-chain messaging.
  See [Cross-Chain Messaging](../concepts/cross-chain-messaging.md) and [`aztec-ethereum`](../reference/aztec-ethereum.md).
- **Instance (contract instance)** — a deployed contract at a specific address, belonging to a registered class.
  See [Contracts](../concepts/contracts.md).
- **Kernel** — the private kernel circuits that validate private execution traces.
  See [Architecture: PXE Runtime](../architecture/pxe-runtime.md).
- **Node** — an Aztec network node reachable over JSON-RPC.
  See [`aztec-node-client`](../reference/aztec-node-client.md).
- **Note** — an encrypted unit of private state owned by an account.
  See [PXE](../concepts/pxe.md).
- **Nullifier** — value marking a note as spent (prevents double-spend of private state).
  See [PXE](../concepts/pxe.md).
- **Poseidon2** — hash function used across Aztec commitments and selectors.
  See [`aztec-core`](../reference/aztec-core.md) (`hash` module).
- **PXE** — Private Execution Environment; the client-side runtime that executes private functions.
  See [PXE](../concepts/pxe.md).
- **Sequencer** — the node that orders and executes public state transitions before block proposal.
  See [Concepts Overview](../concepts/overview.md).
- **Utility function** — off-chain helper exposed by a contract artifact; runs inside the PXE without producing a transaction.
  See [Contracts](../concepts/contracts.md).
