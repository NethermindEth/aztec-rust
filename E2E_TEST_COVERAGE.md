# E2E Test Coverage: aztec-rust vs aztec-packages

## Scope

This project covers **aztec.js + PXE functionality**: wallet lifecycle, contract
interaction, private execution, authwit, fees, events, deployment, and
cryptography. Tests that exercise sequencer, block building, consensus, provers,
P2P, L1 publishing, or multi-validator infrastructure are **out of scope**.

---

## Implemented Tests (30 files)

| File | Tests | What it covers |
|------|-------|----------------|
| `e2e_2_pxes.rs` | 5 | Multi-PXE interactions, cross-PXE transfers, note delivery |
| `e2e_account_contracts.rs` | — | ECDSA, Schnorr, SingleKey account flavors |
| `e2e_authwit.rs` | — | Authwit creation, validation, proxy patterns |
| `e2e_deploy_contract_class_registration.rs` | 14 | On-chain class registration, instance deployment |
| `e2e_deploy_method.rs` | 10 | Constructor/deploy patterns, batch deploy |
| `e2e_deploy_private_initialization.rs` | 8 | Private init, batch init, wrong args, deployer check |
| `e2e_double_spend.rs` | — | Duplicate nullifier detection |
| `e2e_event_logs.rs` | 4 | Private/public event emission and decoding |
| `e2e_fee_gas_estimation.rs` | 3 | Gas estimation with Fee Juice and public payment |
| `e2e_fee_juice_payments.rs` | 5 | Fee Juice balance, claim-and-pay, public/private tx fees |
| `e2e_fee_private_payments.rs` | 6 | FPC private fee payment, balance checks, insufficient funds |
| `e2e_keys.rs` | — | Key derivation, nullifier hiding keys, outgoing viewing keys |
| `e2e_multiple_accounts_1_enc_key.rs` | — | Shared encryption key, different signing keys |
| `e2e_nested_contract_manual_private_call.rs` | — | Nested private function execution |
| `e2e_nested_contract_manual_private_enqueue.rs` | — | Enqueueing public calls from private |
| `e2e_note_getter.rs` | — | Note retrieval with comparators/filtering |
| `e2e_ordering.rs` | — | Proper sequencing of public/private calls |
| `e2e_partial_notes.rs` | — | Partial note discovery/sync |
| `e2e_pending_note_hashes.rs` | — | Note creation, nullification, squashing within a tx |
| `e2e_pruned_blocks.rs` | 1 | Note discovery across pruned blocks |
| `e2e_scope_isolation.rs` | 8 | Scope isolation, multi-account access control |
| `e2e_state_vars.rs` | — | PublicImmutable, PublicMutable, Private storage |
| `e2e_static_calls.rs` | — | View-only private function calls |
| `e2e_token_burn.rs` | — | Token burning |
| `e2e_token_minting.rs` | — | Token minting operations |
| `e2e_token_transfer_private.rs` | — | Private-to-private token transfers |
| `e2e_token_transfer_public.rs` | — | Public-to-public transfers |
| `e2e_token_transfer_recursion.rs` | — | Recursive nested private transfers |
| `e2e_token_transfer_to_private.rs` | — | Public to private (shield) |
| `e2e_token_transfer_to_public.rs` | — | Private to public (unshield) |

---

## Tests to Add

### Tier 4 — Cross-chain and advanced

| Test | Source file | What it tests |
|------|-----------|---------------|
| L1 to L2 messaging | `e2e_cross_chain_messaging/l1_to_l2.test.ts` | L1 to L2 message consumption |
| L2 to L1 messaging | `e2e_cross_chain_messaging/l2_to_l1.test.ts` | L2 to L1 message creation |
| Token bridge (private) | `e2e_cross_chain_messaging/token_bridge_private.test.ts` | Private side of bridge |
| Access control | `e2e_token_contract/access_control.test.ts` | Token access control (minter/admin roles) |
| Contract updates | `e2e_contract_updates.test.ts` | Contract class upgrades |
| Block building | `e2e_block_building.test.ts` | Block construction with multiple txs |
| Offchain effects | `e2e_offchain_effect.test.ts` | Offchain effects emission |

### Tier 5 — Remaining fee patterns

| Test | Source file | What it tests |
|------|-----------|---------------|
| Public fee payments | `e2e_fees/public_payments.test.ts` | FPC public fee payment |
| Sponsored payments | `e2e_fees/sponsored_payments.test.ts` | SponsoredFeePaymentMethod |
| Account init fees | `e2e_fees/account_init.test.ts` | Paying fees during account deployment |
| Fee failures | `e2e_fees/failures.test.ts` | Fee-related error paths and reverts |
| Fee settings | `e2e_fees/fee_settings.test.ts` | Gas settings, max fees, priority fees |

### Tier 6 — Token completeness, events, and common contracts

| Test | Source file | What it tests |
|------|-----------|---------------|
| Token transfer (unified) | `e2e_token_contract/transfer.test.ts` | Combined transfer patterns |
| Reading constants | `e2e_token_contract/reading_constants.test.ts` | Reading contract constants/metadata |
| NFT contract | `e2e_nft.test.ts` | NFT mint, transfer, ownership patterns |
| Escrow contract | `e2e_escrow_contract.test.ts` | Escrow with authwit approval flows |
| Event-only tx | `e2e_event_only.test.ts` | Transactions that only emit events |

### Tier 7 — ABI, encoding, and edge cases

| Test | Source file | What it tests |
|------|-----------|---------------|
| ABI types | `e2e_abi_types.test.ts` | ABI encoding/decoding of all Noir types |
| Optional params | `e2e_option_params.test.ts` | Optional/nullable parameter handling |
| Expiration timestamp | `e2e_expiration_timestamp.test.ts` | Tx validity windows |
| Public nested calls | `e2e_nested_contract/manual_public.test.ts` | Nested public function execution |
| Import patterns | `e2e_nested_contract/importer.test.ts` | Contract import/library patterns |
| Legacy deploy | `e2e_deploy_contract/legacy.test.ts` | Legacy deployment codepaths |
| Kernelless simulation | `e2e_kernelless_simulation.test.ts` | PXE simulation without kernel proofs |
| Phase check | `e2e_phase_check.test.ts` | Tx phase validation (setup/app/teardown) |

### Tier 8 — Cross-chain extended

| Test | Source file | What it tests |
|------|-----------|---------------|
| Bridge failure cases | `e2e_cross_chain_messaging/token_bridge_failure_cases.test.ts` | Bridge error handling and edge cases |
| Bridge public side | `e2e_cross_chain_messaging/token_bridge_public.test.ts` | Public bridge deposit/withdraw |

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
