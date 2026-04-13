# E2E Test Coverage: aztec-rust vs aztec-packages

## Scope

This project covers **aztec.js + PXE functionality**: wallet lifecycle, contract
interaction, private execution, authwit, fees, events, deployment, and
cryptography. Tests that exercise sequencer, block building, consensus, provers,
P2P, L1 publishing, or multi-validator infrastructure are **out of scope**.

---

## Test Groups (per-crate)

Tests are grouped by the primary `crates/<name>` they exercise. Each group is a
single test binary under `tests/<group>.rs` with its test modules under
`tests/<group>/`. This lets you run only the tests relevant to a crate change:

| Group | Crate(s) touched | Tests | Run |
|-------|------------------|------:|-----|
| `account` | `crates/account` | 2 | `cargo test --test account -- --ignored` |
| `contract` | `crates/contract` | 28 | `cargo test --test contract -- --ignored` |
| `core` | `crates/core` | 3 | `cargo test --test core -- --ignored` |
| `crypto` | `crates/crypto` | 1 | `cargo test --test crypto -- --ignored` |
| `ethereum` | `crates/ethereum` | 5 | `cargo test --test ethereum -- --ignored` |
| `fee` | `crates/fee` | 8 | `cargo test --test fee -- --ignored` |
| `node_client` | `crates/node-client` | 1 | `cargo test --test node_client -- --ignored` |
| `pxe` | `crates/pxe`, `crates/pxe-client` | 8 | `cargo test --test pxe -- --ignored` |
| `wallet` | `crates/wallet` | 1 | `cargo test --test wallet -- --ignored` |

Layer crates (`core`, `wallet`, `pxe`, `node-client`) are transitively exercised
by most tests; the grouping reflects each test's *primary* focus, not every
crate it touches. Changes in a layer crate still warrant running the full suite
(`cargo test -- --ignored`).

The shared test utilities live in `tests/common/` and are mounted into each
group binary as `crate::common`.

---

## Implemented Tests (57 files)

| File | Tests | What it covers |
|------|-------|----------------|
| `e2e_2_pxes.rs` | 5 | Multi-PXE interactions, cross-PXE transfers, note delivery |
| `e2e_abi_types.rs` | 3 | ABI encoding/decoding across public/private/utility (bool, Field, u64, i64, struct) |
| `e2e_account_contracts.rs` | — | ECDSA, Schnorr, SingleKey account flavors |
| `e2e_authwit.rs` | — | Authwit creation, validation, proxy patterns |
| `e2e_block_building.rs` | — | Block construction with multiple txs |
| `e2e_contract_updates.rs` | — | Contract class upgrades |
| `e2e_cross_chain_l1_to_l2.rs` | — | L1 to L2 message consumption |
| `e2e_cross_chain_l2_to_l1.rs` | — | L2 to L1 message creation |
| `e2e_cross_chain_token_bridge_failure_cases.rs` | 3 | Bridge authwit-required public burn rejection; wrong content / wrong secret rejected on claim |
| `e2e_cross_chain_token_bridge_private.rs` | — | Private side of token bridge |
| `e2e_cross_chain_token_bridge_public.rs` | 2 | Public L1→L2 deposit and L2→L1 withdraw; third-party consumption with correct recipient |
| `e2e_deploy_contract_class_registration.rs` | 14 | On-chain class registration, instance deployment |
| `e2e_deploy_legacy.rs` | 5 | Legacy deploy codepath, duplicate-salt reject, bad-public-part revert |
| `e2e_escrow_contract.rs` | 3 | Escrow custom keypair, withdraw, batched multi-key tx |
| `e2e_event_only.rs` | 1 | Private event for contract with no notes |
| `e2e_expiration_timestamp.rs` | 6 | Tx validity windows (future/next-slot/past, with/without public enqueue) |
| `e2e_deploy_method.rs` | 10 | Constructor/deploy patterns, batch deploy |
| `e2e_deploy_private_initialization.rs` | 8 | Private init, batch init, wrong args, deployer check |
| `e2e_double_spend.rs` | — | Duplicate nullifier detection |
| `e2e_event_logs.rs` | 4 | Private/public event emission and decoding |
| `e2e_fee_account_init.rs` | 5 | Paying fees during account deployment (native, self-claim, FPC) |
| `e2e_fee_failures.rs` | 4 | Fee-related error paths, reverts that still pay fees |
| `e2e_fee_gas_estimation.rs` | 3 | Gas estimation with Fee Juice and public payment |
| `e2e_fee_juice_payments.rs` | 5 | Fee Juice balance, claim-and-pay, public/private tx fees |
| `e2e_fee_private_payments.rs` | 6 | FPC private fee payment, balance checks, insufficient funds |
| `e2e_fee_public_payments.rs` | 1 | FPC public fee payment for public transfers |
| `e2e_fee_settings.rs` | 2 | Gas settings, max fees per gas, priority fees |
| `e2e_fee_sponsored_payments.rs` | 1 | SponsoredFeePaymentMethod, sponsor pays unconditionally |
| `e2e_kernelless_simulation.rs` | 4 | AMM add_liquidity, matching gas estimates, note squashing, settled read requests |
| `e2e_keys.rs` | — | Key derivation, nullifier hiding keys, outgoing viewing keys |
| `e2e_multiple_accounts_1_enc_key.rs` | — | Shared encryption key, different signing keys |
| `e2e_nft.rs` | 6 | NFT set_minter, mint, transfer_to/in private/public |
| `e2e_nested_contract_manual_private_call.rs` | — | Nested private function execution |
| `e2e_nested_contract_manual_private_enqueue.rs` | — | Enqueueing public calls from private |
| `e2e_nested_contract_manual_public.rs` | 3 | Nested public calls, fresh-read-after-write, public call ordering |
| `e2e_nested_contract_importer.rs` | 3 | Autogenerated interface calls (no-args, public from private, public from public) |
| `e2e_note_getter.rs` | — | Note retrieval with comparators/filtering |
| `e2e_offchain_effects.rs` | — | Offchain effects emission |
| `e2e_option_params.rs` | 3 | Ergonomic `Option<_>` params for public/private/utility |
| `e2e_ordering.rs` | — | Proper sequencing of public/private calls |
| `e2e_partial_notes.rs` | — | Partial note discovery/sync |
| `e2e_pending_note_hashes.rs` | — | Note creation, nullification, squashing within a tx |
| `e2e_phase_check.rs` | 2 | Tx phase validation; `#[allow_phase_change]` opt-out |
| `e2e_pruned_blocks.rs` | 1 | Note discovery across pruned blocks |
| `e2e_scope_isolation.rs` | 8 | Scope isolation, multi-account access control |
| `e2e_state_vars.rs` | — | PublicImmutable, PublicMutable, Private storage |
| `e2e_static_calls.rs` | — | View-only private function calls |
| `e2e_token_access_control.rs` | — | Token access control (minter/admin roles) |
| `e2e_token_burn.rs` | — | Token burning |
| `e2e_token_contract_reading_constants.rs` | 6 | Private/public name, symbol, decimals getters |
| `e2e_token_contract_transfer.rs` | 4 | Unified transfer, self, non-deployed, overspend |
| `e2e_token_minting.rs` | — | Token minting operations |
| `e2e_token_transfer_private.rs` | — | Private-to-private token transfers |
| `e2e_token_transfer_public.rs` | — | Public-to-public transfers |
| `e2e_token_transfer_recursion.rs` | — | Recursive nested private transfers |
| `e2e_token_transfer_to_private.rs` | — | Public to private (shield) |
| `e2e_token_transfer_to_public.rs` | — | Private to public (unshield) |

---

## Tests to Add

### Tier 9 — Complex application contracts

| Test | Source file | What it tests |
|------|-----------|---------------|
| Private voting | `e2e_private_voting_contract.test.ts` | Privacy-preserving voting |
| Crowdfunding + claim | `e2e_crowdfunding_and_claim.test.ts` | Multi-phase crowdfunding and claim pattern |
| Lending | `e2e_lending_contract.test.ts` | DeFi lending with private state |
| Card game | `e2e_card_game.test.ts` | Complex game state management |
| AMM | `e2e_amm.test.ts` | Automated market maker swap and liquidity |
| Orderbook | `e2e_orderbook.test.ts` | Order matching patterns |
| Blacklist token (7 tests) | `e2e_blacklist_token_contract/*.test.ts` | Access-controlled token with blacklisting |

### Tier 10 — Account patterns and smoke

| Test | Source file | What it tests |
|------|-----------|---------------|
| Custom account contract | `guides/writing_an_account_contract.test.ts` | Writing custom account contract implementations |
| Multi EOA | `e2e_multi_eoa.test.ts` | Multiple externally-owned account management |
| Simple e2e | `e2e_simple.test.ts` | Basic smoke test |

---

## Out of Scope

The following upstream test directories/files are **not relevant** to the
aztec-rust SDK and should not be mirrored:

- `bench/` — Performance benchmarks
- `composed/` — Multi-service integration, HA, tutorials
- `devnet/` — Devnet deployment
- `e2e_epochs/` — Consensus, epoch management, reorgs
- `e2e_l1_publisher/` — L1 rollup publishing
- `e2e_multi_validator/` — Validator network
- `e2e_p2p/` — P2P gossip, slashing, attestations
- `e2e_prover/` — Prover infrastructure
- `e2e_public_testnet/` — Testnet deployment
- `e2e_sequencer/` — Sequencer config, governance
- `e2e_storage_proof/` — L1 storage proof verification
- `spartan/` — Stress/performance harness
- `e2e_avm_simulator.test.ts` — AVM internals
- `e2e_bot.test.ts` — Automation infrastructure
- `e2e_cheat_codes.test.ts` — Test-only cheat codes
- `e2e_circuit_recorder.test.ts` — Circuit recording
- `e2e_debug_trace.test.ts` — Debug tracing
- `e2e_fee_asset_price_oracle.test.ts` — Sequencer oracle
- `e2e_l1_with_wall_time.test.ts` — L1 timing
- `e2e_mempool_limit.test.ts` — Node mempool
- `e2e_multiple_blobs.test.ts` — L1 blob handling
- `e2e_sequencer_config.test.ts` — Sequencer configuration
- `e2e_snapshot_sync.test.ts` — Node snapshot sync
- `e2e_synching.test.ts` — Node-level sync

---

All source files reference: `aztec-packages/yarn-project/end-to-end/src/`
