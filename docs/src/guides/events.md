# Events

Filter and decode contract events.
Public events come from the node; private events come from the PXE (they require decryption).

## Runnable Example

- `examples/event_logs.rs` — reads both public and private events from a sample contract.

## Public Events

```rust,ignore
use aztec_rs::events::{get_public_events, PublicEventFilter};

let filter = PublicEventFilter::new(contract_address, from_block, to_block)
    .with_event::<MyEvent>();

let result = get_public_events::<MyEvent>(&node, filter).await?;
for ev in result.events {
    println!("{:?}", ev.data);
}
```

The node's `get_public_logs` endpoint backs this; `aztec-contract` decodes the field vector into your typed struct.

## Private Events

Private events are encrypted and delivered via notes; the PXE decrypts them for any account whose key is registered.

```rust,ignore
use aztec_rs::wallet::{EventMetadataDefinition, PrivateEventFilter};

let events = wallet.get_private_events(&event_metadata, filter).await?;
for ev in events {
    /* decoded via PrivateEventMetadata */
}
```

If no registered account can decrypt a given log, the decoder simply returns no result — that is *not* an error.

## Edge Cases

- **Chain re-orgs**: previously observed events may be replaced; UIs should reconcile after each block sync rather than cache forever.
- **Missing recipient keys**: private events are skipped rather than failing; register the relevant account first (`Pxe::register_account`).
- **Block range bounds**: wide ranges can be slow on the node; paginate by block when possible.

## Full Runnable Example

Source: [`examples/event_logs.rs`](https://github.com/NethermindEth/aztec-rs/blob/main/examples/event_logs.rs) — reads both public and private events from a sample contract.

```rust,ignore
{{#include ../../../examples/event_logs.rs}}
```

## References

- [`aztec-contract` reference](../reference/aztec-contract.md)
- [`aztec-wallet` reference](../reference/aztec-wallet.md) — `PrivateEventFilter`, `PrivateEventMetadata`.
