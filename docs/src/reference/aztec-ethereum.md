# `aztec-ethereum`

L1 (Ethereum) client and L1↔L2 messaging helpers.

Source: `crates/ethereum/src/`.

## Module Map

| Module         | Highlights                                                                                  |
| -------------- | ------------------------------------------------------------------------------------------- |
| `messaging`    | `L1Actor`, `L2Actor`, `L1ToL2Message`, `L2Claim`, `L2AmountClaim`, `L2AmountClaimWithRecipient`, `generate_claim_secret` |
| `cross_chain`  | `is_l1_to_l2_message_ready`, `wait_for_l1_to_l2_message_ready`                              |
| `l1_client`    | `EthClient`, `L1ContractAddresses`, `L1ToL2MessageSentResult`, `send_l1_to_l2_message`, `prepare_fee_juice_on_l1`, `FeeJuiceBridgeResult` |

## Messaging Types

```rust,ignore
pub struct L1Actor {
    pub address: EthAddress,
    pub chain_id: u64,
}

pub struct L2Actor {
    pub address: AztecAddress,
    pub version: u64,
}

pub struct L1ToL2Message {
    pub sender: L1Actor,
    pub recipient: L2Actor,
    pub content: Fr,
    pub secret_hash: Fr,
}
```

`generate_claim_secret() -> (Fr, Fr)` produces a random `(secret, secret_hash)` pair suitable for funding claim-based flows (e.g. Fee Juice deposits).

`L2Claim` / `L2AmountClaim` / `L2AmountClaimWithRecipient` carry the L2-side data needed to consume a bridged deposit.

## L1 Client

```rust,ignore
use aztec_ethereum::l1_client::{EthClient, L1ContractAddresses, send_l1_to_l2_message};

let eth = EthClient::new(&EthClient::default_url());
let account = eth.get_account().await?;
let tx_hash = eth.send_transaction(/* ... */).await?;
let receipt = eth.wait_for_receipt(&tx_hash).await?;
```

`EthClient` is a minimal JSON-RPC client: `rpc_call`, `get_account`, `send_transaction`, `wait_for_receipt`.
`L1ContractAddresses` can be produced from the Aztec node's `NodeInfo` via `L1ContractAddresses::from_json(...)`.

### Sending L1 → L2

```rust,ignore
let result: L1ToL2MessageSentResult = send_l1_to_l2_message(
    &eth,
    &l1_addresses,
    &message,
).await?;
```

Returns the L1 tx hash and the on-L2 message hash / leaf index needed later for consumption.

### Fee Juice Bridge

```rust,ignore
let FeeJuiceBridgeResult { claim, .. } = prepare_fee_juice_on_l1(
    &eth,
    &l1_addresses,
    recipient,
    amount,
).await?;
// `claim` can be handed to `FeeJuicePaymentMethodWithClaim` in `aztec-fee`.
```

## Cross-Chain Readiness

```rust,ignore
use aztec_ethereum::cross_chain::{is_l1_to_l2_message_ready, wait_for_l1_to_l2_message_ready};

if is_l1_to_l2_message_ready(&node, &message_hash).await? {
    // safe to consume on L2
}

// Or block until ready:
wait_for_l1_to_l2_message_ready(&node, &message_hash, timeout).await?;
```

The readiness check queries the node's archiver for L1-to-L2 message checkpoints.

## Full API

Bundled rustdoc: [`api/aztec_ethereum/`](../api/aztec_ethereum/index.html).
Local regeneration:

```bash
cargo doc -p aztec-ethereum --open
```

## See Also

- [`aztec-fee`](./aztec-fee.md) — consumes `L2AmountClaim` for claim-based fee payment.
- [Architecture: Ethereum Layer](../architecture/ethereum-layer.md)
- [Concepts: Cross-Chain Messaging](../concepts/cross-chain-messaging.md)
