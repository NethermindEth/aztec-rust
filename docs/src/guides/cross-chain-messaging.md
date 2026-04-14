# Cross-Chain Messaging

Send L1-to-L2 and L2-to-L1 messages using [`aztec-ethereum`](../reference/aztec-ethereum.md).

## Runnable Examples

- `examples/l1_to_l2_message.rs` — full L1 → L2 send + consume flow.
- `examples/l2_to_l1_message.rs` — L2 → L1 emit + consume flow.
- `examples/fee_juice_claim.rs` — the canonical claim-based bridge, reusing the messaging primitives.

## L1 → L2

```rust,ignore
use aztec_rs::l1_client::{self, EthClient, L1ContractAddresses};
use aztec_rs::messaging;
use aztec_rs::cross_chain::wait_for_l1_to_l2_message_ready;

// Resolve L1 portal addresses from the Aztec node.
let info = wallet.pxe().node().get_node_info().await?;
let l1   = L1ContractAddresses::from_json(&info.l1_contract_addresses)
    .ok_or_else(|| aztec_rs::Error::InvalidData("missing L1 addresses".into()))?;

let eth = EthClient::new(&ethereum_url);
let (secret, secret_hash) = messaging::generate_claim_secret();
let content = aztec_rs::types::Fr::random();

let sent = l1_client::send_l1_to_l2_message(
    &eth,
    &l1.inbox,
    &recipient_address,
    info.rollup_version,
    &content,
    &secret_hash,
).await?;

// Block until the message is consumable on L2.
wait_for_l1_to_l2_message_ready(
    wallet.pxe().node(),
    &sent.msg_hash,
    std::time::Duration::from_secs(30),
).await?;

// Now call the L2 contract function that consumes the message,
// passing `secret` + `content` as arguments.
```

## L2 → L1

L2-emitted messages are produced inside a contract function.
Consumption on L1 uses the Outbox:

1. Send an L2 tx whose body emits the message.
2. Wait for the block to be proven (`Wallet::wait_for_tx_proven`).
3. On L1, call the Outbox's consume function with the produced inclusion proof.

See `examples/l2_to_l1_message.rs` for the full flow; the L1-side call is handled by `EthClient::send_transaction` against the Outbox address from `L1ContractAddresses`.

## Message Identity

- `L1Actor { sender, chain_id }` — the L1 sender.
- `L2Actor { recipient, version }` — the L2 recipient.
- `L1ToL2Message { sender, recipient, content, secret_hash, index }` — bound by its hash and tree position.

Tampering with any field changes the hash and breaks consumption.

## Edge Cases

- **Not yet ready**: `is_l1_to_l2_message_ready` returns `false` until the archiver has seen the L1 tx; poll rather than retry `consume`.
- **Re-org on L1**: readiness is advisory until the block reaches the archiver's confirmation depth.
- **Double-consume**: the nullifier tree marks a consumed message as spent; retrying will revert at simulation.

## Full Runnable Example

Source: [`examples/l1_to_l2_message.rs`](https://github.com/NethermindEth/aztec-rs/blob/main/examples/l1_to_l2_message.rs).
For the reverse direction see `examples/l2_to_l1_message.rs`.

```rust,ignore
{{#include ../../../examples/l1_to_l2_message.rs}}
```

## References

- [Concepts: Cross-Chain Messaging](../concepts/cross-chain-messaging.md)
- [Architecture: Ethereum Layer](../architecture/ethereum-layer.md)
- [`aztec-ethereum` reference](../reference/aztec-ethereum.md)
