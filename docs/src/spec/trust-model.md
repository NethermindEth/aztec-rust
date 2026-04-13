# Trust Model

Consolidated normative trust boundaries across `aztec-rs`.
These rules generalize the per-layer statements in the other spec pages and the [Architecture: Security](../architecture/security.md) discussion.

## Trust Boundaries

**TRUST-1.** The PXE process is the trust root for the user's private state.
Secret keys, plaintext notes, decrypted logs, and capsule payloads MUST NOT leave the PXE process boundary.

**TRUST-2.** The Aztec node is *untrusted*.
All node responses are either (a) public state that will be re-verified inside the kernel, or (b) witnesses bound to a block hash whose integrity is verified by kernel circuits.
Clients MUST NOT act on node responses that cannot be verified in one of those two ways.

**TRUST-3.** L1 smart contracts are *trusted at pinned addresses*.
The addresses for Inbox, Outbox, rollup, and Fee Juice portal MUST be obtained from the Aztec node's `NodeInfo`; hard-coded alternatives MUST NOT be accepted across networks.

**TRUST-4.** `AccountProvider` is the trust root for signing material.
A wallet MUST NOT access signing keys through any channel other than its configured `AccountProvider`.

**TRUST-5.** Contract artifacts are *content-addressed*.
An implementation registering an artifact SHOULD verify the artifact's class hash matches the expected class id; artifacts whose class hash mismatches MUST NOT be registered without explicit operator override.

## Storage at Rest

**TRUST-6.** Persistent PXE stores (e.g. `SledKvStore`) hold notes and keys in plaintext.
Operators MUST treat the backing directory as sensitive; deployments that expose it to other local users are a vulnerability.

**TRUST-7.** An implementation MAY add an encryption layer over the KV store; such a layer MUST NOT alter the `Pxe` trait contract.

## Network

**TRUST-8.** RPC transports (`aztec-rpc`, `aztec-ethereum` L1 client) MUST support user-supplied TLS-terminated URLs.
Implementations MUST NOT disable certificate verification by default.

**TRUST-9.** JSON-RPC responses containing byte blobs MUST be parsed through the typed layers (`aztec-core`, `aztec-node-client`) before being treated as meaningful; raw `serde_json::Value` pass-throughs MUST NOT bypass that typing for security-relevant decisions.

## Proof Outputs

**TRUST-10.** A `TxProvingResult` SHALL be treated as public.
Sharing it with the network (via `AztecNode::send_tx`) MUST NOT leak anything private if the PXE implementation is correct (see TX-13, TX-14 in [Transaction Lifecycle](./tx-lifecycle.md)).

**TRUST-11.** An implementation that extends the `Pxe` trait with new methods returning richer data than the methods in `aztec-pxe-client` MUST document the associated trust implications, in particular whether the new output crosses the TRUST-1 boundary.

## Conformance

**TRUST-12.** Any implementation claiming conformance with this spec MUST satisfy TRUST-1 through TRUST-11.
Partial conformance MUST be documented (e.g. "conforms except TRUST-8: uses cleartext HTTP for a private sandbox only").
