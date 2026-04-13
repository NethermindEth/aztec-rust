# Data Flow

How a transaction moves through `aztec-rs` from application code to on-chain inclusion and back.

## Private-Call Transaction

```mermaid
sequenceDiagram
    autonumber
    participant App as Application
    participant Wal as BaseWallet
    participant Acct as AccountProvider
    participant Pxe as EmbeddedPxe
    participant Node as AztecNode

    App->>Wal: send_tx(ExecutionPayload, SendOptions)
    Wal->>Acct: create_tx_execution_request(exec, gas, chain_info, ...)
    Acct-->>Wal: TxExecutionRequest (entrypoint-wrapped)
    Wal->>Pxe: simulate_tx(request, SimulateTxOpts)
    Pxe->>Pxe: ACVM execute private functions (oracle-backed)
    Pxe->>Node: witness / storage reads (as needed)
    Node-->>Pxe: witnesses
    Pxe-->>Wal: TxSimulationResult
    Wal->>Pxe: prove_tx(request)
    Pxe->>Pxe: fold trace → kernel proof (BB prover)
    Pxe-->>Wal: TxProvingResult { Tx }
    Wal->>Node: send_tx(tx)
    Node-->>Wal: ack
    Wal-->>App: SendResult { TxHash, ... }
    loop polling
      App->>Node: get_tx_receipt(TxHash)
      Node-->>App: TxReceipt (Pending → Proposed → Checkpointed → ...)
    end
    Node-->>Pxe: new block (via sync loop)
    Pxe->>Pxe: discover notes, update nullifier / tagging stores
```

Key control points:

- The wallet is the only caller of `AccountProvider` — account knowledge is isolated.
- Simulation and proving both live inside the PXE.
- The node never sees private inputs; only the wire-format `Tx` with client proof.
- Sync runs continuously in the background; new notes become available without an explicit fetch.

## Public-Only Transaction

Same shape, but `simulate_tx` does no private ACVM work — the wallet just builds the public call list, sends it, and the sequencer runs the public execution.

## Utility Call (Off-Chain)

```mermaid
sequenceDiagram
    App->>Wal: execute_utility(call, ExecuteUtilityOptions)
    Wal->>Pxe: execute_utility(call, opts)
    Pxe->>Pxe: ACVM run + utility oracle (reads node state)
    Pxe-->>Wal: UtilityExecutionResult
    Wal-->>App: return values + logs
```

Utility calls never produce a transaction or a receipt.

## Cross-Chain (L1 → L2)

```mermaid
sequenceDiagram
    App->>Eth as EthClient: send_l1_to_l2_message(...)
    Eth-->>App: L1ToL2MessageSentResult
    loop readiness poll
      App->>Node: get_l1_to_l2_message_checkpoint(hash)
    end
    App->>Wal: send_tx(consume_call)
    Wal->>Pxe: simulate + prove (consumes the message)
    Wal->>Node: send_tx
```

See [Ethereum Layer](./ethereum-layer.md) for the opposite direction (L2 → L1).

## References

- [PXE Runtime](./pxe-runtime.md)
- [Wallet Layer](./wallet-layer.md)
- [Ethereum Layer](./ethereum-layer.md)
