# E2E Test Coverage: aztec-rust vs aztec-packages

## Current Tests (4 files, 17 tests)

| File | Tests | Status |
|------|-------|--------|
| `e2e_2_pxes.rs` | 5 | Multi-PXE interactions, cross-PXE transfers, note delivery |
| `e2e_event_logs.rs` | 4 | Private/public event emission and decoding |
| `e2e_pruned_blocks.rs` | 1 | Note discovery across pruned blocks |
| `e2e_scope_isolation.rs` | 8 | Scope isolation, multi-account access control |

---

## Tests to Add

### Tier 1 — Core PXE functionality (add first)

| Test | Source file | What it tests |
|------|-----------|---------------|
| Token transfers (private) | `e2e_token_contract/transfer_in_private.test.ts` | Private-to-private token transfers |
| Token transfers (public) | `e2e_token_contract/transfer_in_public.test.ts` | Public-to-public transfers |
| Shielding | `e2e_token_contract/transfer_to_private.test.ts` | Public to private (shield) |
| Unshielding | `e2e_token_contract/transfer_to_public.test.ts` | Private to public (unshield) |
| Minting | `e2e_token_contract/minting.test.ts` | Token minting operations |
| Note getter | `e2e_note_getter.test.ts` | Note retrieval with comparators/filtering |
| Pending note hashes | `e2e_pending_note_hashes_contract.test.ts` | Note creation, nullification, squashing within a tx |
| Auth witnesses | `e2e_authwit.test.ts` | Authwit creation, validation, proxy patterns |
| Double spend | `e2e_double_spend.test.ts` | Duplicate nullifier detection |
| Keys | `e2e_keys.test.ts` | Key derivation, nullifier hiding keys, outgoing viewing keys |

### Tier 2 — Important features

| Test | Source file | What it tests |
|------|-----------|---------------|
| Account contracts | `e2e_account_contracts.test.ts` | ECDSA, Schnorr, SingleKey account flavors |
| Nested private calls | `e2e_nested_contract/manual_private_call.test.ts` | Nested private function execution |
| Private to public enqueue | `e2e_nested_contract/manual_private_enqueue.test.ts` | Enqueueing public calls from private |
| Static calls | `e2e_static_calls.test.ts` | View-only private function calls |
| State vars | `e2e_state_vars.test.ts` | PublicImmutable, PublicMutable, Private storage |
| Ordering | `e2e_ordering.test.ts` | Proper sequencing of public/private calls |
| Private transfer recursion | `e2e_token_contract/private_transfer_recursion.test.ts` | Recursive nested private transfers |
| Multiple accounts 1 enc key | `e2e_multiple_accounts_1_enc_key.test.ts` | Shared encryption key, different signing keys |
| Partial notes | `e2e_partial_notes.test.ts` | Partial note discovery/sync |
| Token burn | `e2e_token_contract/burn.test.ts` | Token burning |

### Tier 3 — Deployment and fees

| Test | Source file | What it tests |
|------|-----------|---------------|
| Contract class registration | `e2e_deploy_contract/contract_class_registration.test.ts` | On-chain class registration |
| Deploy method | `e2e_deploy_contract/deploy_method.test.ts` | Constructor/deploy patterns |
| Private initialization | `e2e_deploy_contract/private_initialization.test.ts` | Private execution during init |
| Fee juice payments | `e2e_fees/fee_juice_payments.test.ts` | Gas payments |
| Private fee payments | `e2e_fees/private_payments.test.ts` | Fee from private state |
| Gas estimation | `e2e_fees/gas_estimation.test.ts` | Gas estimation |

### Tier 4 — Cross-chain and advanced

| Test | Source file | What it tests |
|------|-----------|---------------|
| L1 to L2 messaging | `e2e_cross_chain_messaging/l1_to_l2.test.ts` | L1 to L2 message consumption |
| L2 to L1 messaging | `e2e_cross_chain_messaging/l2_to_l1.test.ts` | L2 to L1 message creation |
| Token bridge | `e2e_cross_chain_messaging/token_bridge_private.test.ts` | Private side of bridge |
| Access control | `e2e_token_contract/access_control.test.ts` | Token access control |
| Contract updates | `e2e_contract_updates.test.ts` | Contract class upgrades |
| Block building | `e2e_block_building.test.ts` | Block construction with multiple txs |
| Offchain effects | `e2e_offchain_effect.test.ts` | Offchain effects emission |

---

All source files reference: `aztec-packages/yarn-project/end-to-end/src/`
