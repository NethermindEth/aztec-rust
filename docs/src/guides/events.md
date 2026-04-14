# Events

Filter and decode contract events.
Public events come from the node; private events come from the PXE (they require decryption).

## Runnable Example

- `examples/event_logs.rs` — reads both public and private events from a sample contract.

## Public Events

```rust,ignore
use aztec_rs::events::{get_public_events, PublicEventFilter};
use aztec_rs::wallet::EventMetadataDefinition;

let metadata = EventMetadataDefinition {
    event_selector,
    abi_type,
    field_names,
};
let filter = PublicEventFilter {
    contract_address: Some(contract_address),
    from_block: Some(from_block),
    to_block: Some(to_block),
    ..Default::default()
};

let result = get_public_events(&node, &metadata, filter).await?;
for ev in result.events {
    println!("{:?}", ev.event);
}
```

The node's `get_public_logs` endpoint backs this; `aztec-contract` decodes the field vector into named fields from the event metadata.

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
