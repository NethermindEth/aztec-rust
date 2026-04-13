# Examples

This directory contains runnable examples for the local Aztec network. The examples are organized around the real Aztec v4.x happy path in this repo:

- embedded PXE
- local Aztec node
- minimal boilerplate
- one clear workflow per file

Run them with `cargo run --example <name>`.

## Prerequisites

Start a local network first:

```bash
aztec start --local-network
```

By default the examples use:

- `AZTEC_NODE_URL=http://localhost:8080`
- `ETHEREUM_HOST=http://localhost:8545`

Most examples use pre-funded local-network accounts imported through `examples/common/mod.rs`.

## Recommended Order

If you are new to the repo, start here:

1. `node_info`
2. `wallet_minimal`
3. `deploy_contract`
4. `private_token_transfer`
5. `simulate_profile_send`
6. `event_logs`
7. `account_deploy`

Then move to the PXE-focused and advanced flows.

## Core Examples

| Example | What it shows |
| --- | --- |
| `node_info.rs` | Connect to the node and print basic chain and block metadata |
| `wallet_minimal.rs` | Create an embedded wallet and inspect PXE/account state |
| `deploy_contract.rs` | Deploy a contract and verify class and instance registration |
| `private_token_transfer.rs` | Private mint and private transfer flow with PXE note discovery |
| `simulate_profile_send.rs` | Difference between simulate, profile, and send for the same transaction |
| `event_logs.rs` | Emitting and reading public and private events |
| `account_deploy.rs` | Deploying a fresh Schnorr account through `AccountManager` |

## PXE Examples

| Example | What it shows |
| --- | --- |
| `two_pxes.rs` | Two PXEs registering the same contract and observing cross-wallet behavior |
| `scope_isolation.rs` | Scope-based note isolation; one account cannot read another account's private notes |
| `note_getter.rs` | Utility note lookups and note-query behavior |
| `multiple_accounts_one_encryption_key.rs` | Multiple accounts sharing one encryption key with different signing keys |

## Contract And Wallet Examples

| Example | What it shows |
| --- | --- |
| `authwit.rs` | Creating, validating, and consuming an auth witness |
| `deploy_options.rs` | Deployment variants such as class publication and registration behavior |
| `public_storage.rs` | Public storage reads and writes with explicit slot derivation |
| `contract_update.rs` | Publishing an updated class and upgrading a deployed contract |
| `block_building.rs` | Sending multiple txs and inspecting resulting block behavior |

## Fee Examples

| Example | What it shows |
| --- | --- |
| `fee_native.rs` | Native fee payment flow |
| `fee_sponsored.rs` | Sponsored fee payment using the vendored `SponsoredFPC` artifact |
| `fee_juice_claim.rs` | Bridging FeeJuice from L1 and spending it through a claim-based fee method |

## Cross-Chain Examples

| Example | What it shows |
| --- | --- |
| `l1_to_l2_message.rs` | Sending an L1 to L2 message and waiting until it becomes consumable |
| `l2_to_l1_message.rs` | Creating L2 to L1 messages and inspecting resulting hashes and outbox state |

## Runtime Notes

- All examples are intended to run against `aztec start --local-network`.
- The fee and cross-chain examples assume the local L1 side of the network is available.
- `fee_sponsored.rs` depends on the vendored [sponsored_fpc_contract_compiled.json](../fixtures/sponsored_fpc_contract_compiled.json).
- `fee_juice_claim.rs` and `l1_to_l2_message.rs` actively advance L2 blocks while waiting for L1 to L2 messages to become ready.
- `scope_isolation.rs` intentionally prints a blocked read for the cross-scope case; that is the expected security behavior.

## Shared Helpers

Most of the boilerplate lives in [common/mod.rs](./common/mod.rs). That helper module provides:

- local-network account import
- artifact loading from `fixtures/`
- contract deployment helpers
- common call/send utilities
- PXE registration helpers
- public storage and cross-chain wait helpers

Examples should stay focused on the workflow they demonstrate rather than re-implementing setup code inline.
