# Step 4: Authorization Witnesses (AuthWit) — Detailed Implementation Plan

**Parent:** [IMPLEMENTATION_PLAN.md](./IMPLEMENTATION_PLAN.md) — Step 4
**Target release:** 0.2.3
**Crates modified:** `aztec-core`, `aztec-wallet`, `aztec-account`, `aztec-contract`

---

## Overview

Authorization witnesses (authwit) enable multi-party interactions on Aztec — token approvals, DeFi allowances, and any action where one account authorizes another to act on its behalf. The Rust SDK already has the **plumbing** (the `AuthWitness` struct, `AuthorizationProvider` trait, wallet routing), but is missing the **cryptographic hash computation** and the **contract interaction layer** for public authwits and validity checking.

### What already exists

| Component | Location | Status |
|---|---|---|
| `AuthWitness` struct | `aztec-core/src/tx.rs` | Done |
| `ExecutionPayload.auth_witnesses` | `aztec-core/src/tx.rs` | Done |
| `MessageHashOrIntent` enum | `aztec-wallet/src/wallet.rs` | Done |
| `AuthorizationProvider` trait | `aztec-account/src/account.rs` | Done |
| `AccountProvider.create_auth_wit()` | `aztec-wallet/src/account_provider.rs` | Done |
| `Wallet.create_auth_wit()` | `aztec-wallet/src/wallet.rs` | Done |
| `BaseWallet.create_auth_wit()` routing | `aztec-wallet/src/base_wallet.rs` | Done |
| `SingleAccountProvider` | `aztec-account/src/single_account_provider.rs` | Done |
| Authorization module | `aztec-account/src/authorization.rs` | **Empty** |

### What needs to be built

1. Hash computation functions (`compute_inner_auth_wit_hash`, `compute_outer_auth_wit_hash`, etc.)
2. Domain separator constants (`AUTHWIT_INNER`, `AUTHWIT_OUTER`, `FUNCTION_ARGS`)
3. `compute_var_args_hash()` utility (prerequisite for inner hash)
4. `CallAuthorizationRequest` struct
5. `SetPublicAuthWitContractInteraction` in `aztec-contract`
6. `lookup_validity()` utility
7. `ProtocolContractAddress::AuthRegistry` constant
8. Tests for all of the above

### Reference implementation (TypeScript)

- `yarn-project/stdlib/src/auth_witness/auth_witness.ts` — `computeInnerAuthWitHash`, `computeOuterAuthWitHash`
- `yarn-project/aztec.js/src/utils/authwit.ts` — `computeAuthWitMessageHash`, `computeInnerAuthWitHashFromAction`, `lookupValidity`, `SetPublicAuthwitContractInteraction`
- `yarn-project/aztec.js/src/authorization/call_authorization_request.ts` — `CallAuthorizationRequest`
- `yarn-project/constants/src/constants.gen.ts` — domain separators

---

## Sub-step 4.1: Domain Separators & Protocol Constants

**Crate:** `aztec-core`
**File:** `crates/core/src/constants.rs`

### Deliverables

Add the following constants to the `protocol_contract_address` module and a new `domain_separator` module:

```rust
// Domain separators (from constants.gen.ts)
pub mod domain_separator {
    /// Domain separator for authwit inner hash.
    /// TS: DomainSeparator.AUTHWIT_INNER = 221354163
    pub const AUTHWIT_INNER: u32 = 221354163;

    /// Domain separator for authwit outer hash.
    /// TS: DomainSeparator.AUTHWIT_OUTER = 3283595782
    pub const AUTHWIT_OUTER: u32 = 3283595782;

    /// Domain separator for function args hashing.
    /// TS: DomainSeparator.FUNCTION_ARGS = 3576554347
    pub const FUNCTION_ARGS: u32 = 3576554347;
}

// Protocol contract address
pub fn auth_registry() -> AztecAddress {
    // TS: CANONICAL_AUTH_REGISTRY_ADDRESS = 1
    AztecAddress(Fr::from(1u64))
}
```

### Tests
- Assert constant values match the TS constants (regression guard).

### Acceptance criteria
- `domain_separator::AUTHWIT_INNER`, `AUTHWIT_OUTER`, `FUNCTION_ARGS` exist and are public.
- `protocol_contract_address::auth_registry()` returns address `1`.
- All constants are documented with their TS source.

---

## Sub-step 4.2: Poseidon2 Hash with Domain Separator

**Crate:** `aztec-core`
**New file:** `crates/core/src/hash.rs`

### Deliverables

Introduce a `hash` module that wraps a Poseidon2 implementation:

```rust
/// Compute a Poseidon2 hash of `inputs` with a domain separator prepended.
///
/// Mirrors the TS `poseidon2HashWithSeparator(args, separator)`.
pub fn poseidon2_hash_with_separator(inputs: &[Fr], separator: u32) -> Fr;
```

**Implementation options (choose one):**

1. **FFI to `noir_stdlib` / `barretenberg`** — call the same native hash used by the circuits. This guarantees consistency with the TS SDK which uses the same native code. Investigate whether there is an existing Rust binding in `aztec-packages` (e.g., `barretenberg_wrapper` crate or similar).

2. **Pure Rust Poseidon2 crate** — use an existing Rust Poseidon2 library (e.g., the `poseidon2` crate or the one in `zkhash`) parameterized with the BN254 scalar field. Must produce identical results to the TS/C++ implementation.

3. **RPC-based** — call the PXE/node to compute the hash. Not ideal for offline use, but simplest for initial correctness.

**Recommended approach:** Start with option 2 (pure Rust) for testability and offline use. Add a cross-validation test that compares outputs against known TS test vectors to guarantee compatibility.

### Also add:

```rust
/// Hash function arguments using Poseidon2 with the FUNCTION_ARGS separator.
///
/// Returns Fr::zero() if args is empty.
/// Mirrors TS `computeVarArgsHash(args)`.
pub fn compute_var_args_hash(args: &[Fr]) -> Fr;
```

### Tests
- `compute_var_args_hash([])` returns `Fr::zero()`.
- `compute_var_args_hash` with known inputs matches TS output (cross-validation test vector).
- `poseidon2_hash_with_separator` with known inputs matches TS output.

### Acceptance criteria
- `hash::poseidon2_hash_with_separator` and `hash::compute_var_args_hash` are public and tested.
- Add `pub mod hash;` to `crates/core/src/lib.rs`.

---

## Sub-step 4.3: Auth Wit Hash Computation Functions

**Crate:** `aztec-core`
**File:** `crates/core/src/hash.rs` (extend)

### Deliverables

```rust
/// Compute the inner authwit hash — the "intent" before siloing with consumer.
///
/// `args` is typically `[caller, selector, args_hash]`.
/// Uses Poseidon2 with AUTHWIT_INNER domain separator.
///
/// Mirrors TS `computeInnerAuthWitHash(args)`.
pub fn compute_inner_auth_wit_hash(args: &[Fr]) -> Fr;

/// Compute the outer authwit hash — the value the approver signs.
///
/// Combines consumer address, chain ID, protocol version, and inner hash.
/// Uses Poseidon2 with AUTHWIT_OUTER domain separator.
///
/// Mirrors TS `computeOuterAuthWitHash(consumer, chainId, version, innerHash)`.
pub fn compute_outer_auth_wit_hash(
    consumer: &AztecAddress,
    chain_id: &Fr,
    version: &Fr,
    inner_hash: &Fr,
) -> Fr;

/// Compute the inner authwit hash from a caller address and a function call.
///
/// Computes `computeInnerAuthWitHash([caller, call.selector, varArgsHash(call.args)])`.
///
/// Mirrors TS `computeInnerAuthWitHashFromAction(caller, action)`.
pub fn compute_inner_auth_wit_hash_from_action(
    caller: &AztecAddress,
    call: &FunctionCall,
) -> Fr;
```

### Implementation details

`compute_inner_auth_wit_hash_from_action`:
1. Convert `call.args` (Vec<AbiValue>) to `Vec<Fr>` — need a helper to convert ABI values to field elements.
2. Compute `args_hash = compute_var_args_hash(&args_as_fields)`.
3. Return `compute_inner_auth_wit_hash(&[caller.to_field(), call.selector.to_field(), args_hash])`.

**Note:** This requires `AbiValue -> Fr` conversion. Currently `AbiValue` has variants like `Field(Fr)`, `Integer(u64)`, `Boolean(bool)`, `Array(Vec<AbiValue>)`, `Struct(...)`, `String(String)`. For authwit purposes, the TS SDK hashes the already-encoded field elements. We need a `fn abi_values_to_fields(args: &[AbiValue]) -> Vec<Fr>` utility that flattens ABI values into their field representation.

### Also add:

A high-level convenience function that mirrors TS `computeAuthWitMessageHash`:

```rust
/// Compute the full authwit message hash from an intent and chain info.
///
/// For `MessageHashOrIntent::Hash` — returns the hash directly.
/// For `MessageHashOrIntent::Intent { caller, call }`:
///   1. inner_hash = compute_inner_auth_wit_hash_from_action(caller, call)
///   2. consumer = call.to (the contract being called)
///   3. outer_hash = compute_outer_auth_wit_hash(consumer, chain_id, version, inner_hash)
///
/// Mirrors TS `computeAuthWitMessageHash(intent, metadata)`.
pub fn compute_auth_wit_message_hash(
    intent: &MessageHashOrIntent,
    chain_info: &ChainInfo,
) -> Fr;
```

**Note:** `MessageHashOrIntent` is currently defined in `aztec-wallet`. It should either be moved to `aztec-core` (preferred, since it's used in hash computation) or `aztec-core::hash` should depend on the wallet types. **Recommendation:** move `MessageHashOrIntent` and `ChainInfo` to `aztec-core` so they can be used in hash computation without a circular dependency.

### Type relocation plan

| Type | Current location | New location |
|---|---|---|
| `MessageHashOrIntent` | `aztec-wallet/src/wallet.rs` | `aztec-core/src/tx.rs` |
| `ChainInfo` | `aztec-wallet/src/wallet.rs` | `aztec-core/src/types.rs` |

Both `aztec-wallet` and `aztec-account` would re-export these from `aztec-core`. This is a non-breaking change since the types are identical.

### Tests
- `compute_inner_auth_wit_hash` with known inputs matches TS vector.
- `compute_outer_auth_wit_hash` with known inputs matches TS vector.
- `compute_inner_auth_wit_hash_from_action` with a sample `FunctionCall` matches TS vector.
- `compute_auth_wit_message_hash` with `MessageHashOrIntent::Hash` returns hash unchanged.
- `compute_auth_wit_message_hash` with `MessageHashOrIntent::Intent` produces correct outer hash.

### Acceptance criteria
- All four hash functions are public, documented, and tested.
- `MessageHashOrIntent` and `ChainInfo` are in `aztec-core`.
- Cross-validation test vectors from the TS SDK pass.

---

## Sub-step 4.4: `CallAuthorizationRequest` Struct

**Crate:** `aztec-account`
**File:** `crates/account/src/authorization.rs` (currently empty)

### Deliverables

```rust
/// An authorization request for a function call, including the full preimage
/// of the data to be signed.
///
/// Mirrors TS `CallAuthorizationRequest`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallAuthorizationRequest {
    /// The inner hash of the authwit (poseidon2([msg_sender, selector, args_hash])).
    pub inner_hash: Fr,
    /// The address performing the call (msg_sender).
    pub msg_sender: AztecAddress,
    /// The selector of the function being authorized.
    pub function_selector: FunctionSelector,
    /// The hash of the function arguments.
    pub args_hash: Fr,
    /// The raw function arguments.
    pub args: Vec<Fr>,
}

impl CallAuthorizationRequest {
    /// Construct from a caller, function call, and pre-computed inner hash.
    pub fn new(
        inner_hash: Fr,
        msg_sender: AztecAddress,
        function_selector: FunctionSelector,
        args_hash: Fr,
        args: Vec<Fr>,
    ) -> Self;

    /// Construct from field elements (deserialization from on-chain data).
    ///
    /// Validates that the first field matches the expected authorization selector.
    pub fn from_fields(fields: &[Fr]) -> Result<Self, Error>;
}
```

### Tests
- Round-trip: construct a `CallAuthorizationRequest`, verify all fields.
- `from_fields` with valid fields succeeds and reconstructs correctly.
- `from_fields` with invalid selector returns error.

### Acceptance criteria
- `CallAuthorizationRequest` is public and re-exported from `aztec-account`.
- `authorization.rs` is no longer empty.

---

## Sub-step 4.5: `SetPublicAuthWitContractInteraction`

**Crate:** `aztec-contract`
**New file:** `crates/contract/src/authwit.rs`

### Deliverables

A convenience type for setting public authorization witnesses in the AuthRegistry protocol contract:

```rust
/// Convenience interaction for setting a public authwit in the AuthRegistry.
///
/// Wraps a call to `AuthRegistry.set_authorized(message_hash, authorize)`.
/// Automatically enforces that only the authorizer (`from`) is the sender.
///
/// Mirrors TS `SetPublicAuthwitContractInteraction`.
pub struct SetPublicAuthWitInteraction<'a, W> {
    wallet: &'a W,
    from: AztecAddress,
    call: FunctionCall,
}

impl<'a, W: Wallet> SetPublicAuthWitInteraction<'a, W> {
    /// Create a new interaction for setting a public authwit.
    ///
    /// Computes the message hash from the intent and chain info,
    /// then constructs a call to `AuthRegistry.set_authorized(hash, authorized)`.
    pub async fn create(
        wallet: &'a W,
        from: AztecAddress,
        message_hash_or_intent: MessageHashOrIntent,
        authorized: bool,
    ) -> Result<Self, Error>;

    /// Build the execution payload.
    pub fn request(&self) -> Result<ExecutionPayload, Error>;

    /// Simulate the interaction (sender is always `from`).
    pub async fn simulate(
        &self,
        opts: SimulateOptions,
    ) -> Result<TxSimulationResult, Error>;

    /// Send the interaction (sender is always `from`).
    pub async fn send(
        &self,
        opts: SendOptions,
    ) -> Result<SendResult, Error>;
}
```

### Implementation details

The `create` method:
1. Get chain info from wallet: `wallet.get_chain_info()`.
2. Compute message hash: `compute_auth_wit_message_hash(&intent, &chain_info)`.
3. Build a `FunctionCall` targeting `ProtocolContractAddress::auth_registry()` with:
   - selector: `FunctionSelector` for `"set_authorized"`
   - args: `[AbiValue::Field(message_hash), AbiValue::Boolean(authorized)]`
   - function_type: `FunctionType::Public`
   - is_static: `false`

The `simulate` and `send` methods always inject `from` as the sender address (via `SimulateOptions.from` / `SendOptions.from`).

**Note on FunctionSelector:** The TS SDK hardcodes the ABI for `set_authorized`. We should do the same — construct the `FunctionCall` with a hardcoded selector for `set_authorized(Field, bool)`. If `FunctionSelector::from_name()` is not yet implemented (it's noted as a stub in the implementation plan), we can hardcode the selector value or compute it at build time.

### Tests
- `SetPublicAuthWitInteraction::create` produces correct function call targeting AuthRegistry address.
- `request()` returns a payload with the expected call.
- Verify the `from` address is enforced in simulate/send.

### Acceptance criteria
- `SetPublicAuthWitInteraction` is public and re-exported from `aztec-contract`.
- Add `pub mod authwit;` to `crates/contract/src/lib.rs`.

---

## Sub-step 4.6: `lookup_validity()` Utility

**Crate:** `aztec-contract`
**File:** `crates/contract/src/authwit.rs` (extend)

### Deliverables

```rust
/// Result of an authwit validity check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthWitValidity {
    /// Whether the authwit is valid in private context (signature check).
    pub is_valid_in_private: bool,
    /// Whether the authwit is valid in public context (AuthRegistry check).
    pub is_valid_in_public: bool,
}

/// Check whether an authorization witness is valid in both private and public contexts.
///
/// - **Private:** Simulates a `lookup_validity(consumer, inner_hash)` utility call
///   on the `on_behalf_of` account contract, passing the witness. If simulation
///   succeeds and returns `true`, the authwit is valid privately.
///
/// - **Public:** Simulates a `utility_is_consumable(address, message_hash)` utility call
///   on the AuthRegistry protocol contract. If it returns `true`, the authwit is
///   valid publicly.
///
/// Mirrors TS `lookupValidity(wallet, onBehalfOf, intent, witness)`.
pub async fn lookup_validity<W: Wallet>(
    wallet: &W,
    on_behalf_of: &AztecAddress,
    intent: &MessageHashOrIntent,
    witness: &AuthWitness,
) -> Result<AuthWitValidity, Error>;
```

### Implementation details

**Private validity check:**
1. Extract `inner_hash` and `consumer` from the intent:
   - `Intent { caller, call }` → `inner_hash = compute_inner_auth_wit_hash_from_action(caller, call)`, `consumer = call.to`
   - `Hash { hash }` → caller must also provide consumer separately (or we extend `MessageHashOrIntent` — see note below)
2. Build a `FunctionCall` targeting `on_behalf_of` with:
   - function name: `lookup_validity`
   - function_type: `FunctionType::Utility`
   - args: `[consumer, inner_hash]`
   - Hardcode the ABI (same approach as TS)
3. Call `wallet.execute_utility(call, opts)` with `auth_witnesses: [witness]`
4. Parse the boolean result. If simulation throws, treat as `false`.

**Public validity check:**
1. Compute `message_hash = compute_auth_wit_message_hash(&intent, &chain_info)`.
2. Build a `FunctionCall` targeting `ProtocolContractAddress::auth_registry()` with:
   - function name: `utility_is_consumable`
   - function_type: `FunctionType::Utility`
   - args: `[on_behalf_of, message_hash]`
   - Hardcode the ABI
3. Call `wallet.execute_utility(call, opts)` with `from: on_behalf_of`.
4. Parse the boolean result.

**Note on `MessageHashOrIntent::Hash`:** The TS SDK's `IntentInnerHash` variant includes both `consumer` and `innerHash`. Our current `MessageHashOrIntent::Hash` only has `hash: Fr`. For `lookup_validity` to work with pre-computed hashes, we need to either:
- (a) Add a `consumer` field to `MessageHashOrIntent::Hash`, or
- (b) Add a new variant `InnerHash { consumer: AztecAddress, inner_hash: Fr }`, or
- (c) Accept that `lookup_validity` only works with `Intent` variants (simplest for now).

**Recommendation:** Add a third variant to `MessageHashOrIntent`:

```rust
pub enum MessageHashOrIntent {
    Hash { hash: Fr },
    Intent { caller: AztecAddress, call: FunctionCall },
    InnerHash { consumer: AztecAddress, inner_hash: Fr },
}
```

This mirrors the TS distinction between `Fr`, `CallIntent`, and `IntentInnerHash`.

### Tests
- `lookup_validity` with a valid private authwit returns `is_valid_in_private: true` (mock wallet).
- `lookup_validity` with a valid public authwit returns `is_valid_in_public: true` (mock wallet).
- `lookup_validity` with invalid witness returns both `false`.
- Test all three `MessageHashOrIntent` variants.

### Acceptance criteria
- `lookup_validity()` and `AuthWitValidity` are public.
- The function handles both `Intent` and `InnerHash` variants.
- Private check failure (simulation error) is gracefully handled as `false`.

---

## Sub-step 4.7: Wire `BaseWallet.create_auth_wit()` to Hash Computation

**Crate:** `aztec-wallet`
**File:** `crates/wallet/src/base_wallet.rs`

### Deliverables

The current `BaseWallet.create_auth_wit()` already routes through `AccountProvider`. However, the `AccountProvider` receives a `MessageHashOrIntent` and delegates to `AuthorizationProvider.create_auth_wit()`. The key change is ensuring the **hash computation** happens correctly in this chain:

1. In `SingleAccountProvider.create_auth_wit()`:
   - If intent is `Intent { caller, call }`: compute `message_hash = compute_auth_wit_message_hash(intent, chain_info)`, then pass to the account's `AuthorizationProvider.create_auth_wit(MessageHashOrIntent::Hash { hash: message_hash }, chain_info)`.
   - If intent is already `Hash { hash }`: pass through directly.
   - If intent is `InnerHash { consumer, inner_hash }`: compute `outer_hash = compute_outer_auth_wit_hash(consumer, chain_info.chain_id, chain_info.version, inner_hash)`, then pass as `Hash`.

2. The `AuthorizationProvider` then receives a resolved hash and signs it (the signing is account-type-specific — Schnorr, ECDSA, etc.).

### Current state check

Verify `SingleAccountProvider` currently passes `MessageHashOrIntent` through to the account's `AuthorizationProvider`. If it does, the hash computation needs to happen either:
- In `SingleAccountProvider` (resolving intent → hash before passing to auth provider), OR
- In the `AuthorizationProvider` implementation itself.

**Recommendation:** Resolve in `SingleAccountProvider` so that `AuthorizationProvider` always receives a resolved hash. This keeps auth providers simple (they just sign a hash).

### Tests
- `BaseWallet.create_auth_wit()` with `Intent` variant produces an `AuthWitness` with the correct `requestHash` (add `request_hash` field to `AuthWitness` — see sub-step 4.8).
- `BaseWallet.create_auth_wit()` with `Hash` variant passes through.
- End-to-end: create an authwit, verify the message hash matches manual computation.

### Acceptance criteria
- Intent → hash resolution is done in `SingleAccountProvider`.
- `AuthorizationProvider` implementations receive a resolved `Fr` hash.
- Existing tests still pass.

---

## Sub-step 4.8: Enhance `AuthWitness` with `request_hash`

**Crate:** `aztec-core`
**File:** `crates/core/src/tx.rs`

### Deliverables

The TS `AuthWitness` includes a `requestHash` field that identifies which message this witness authorizes. The current Rust `AuthWitness` only has `fields: Vec<Fr>`. Add the request hash:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuthWitness {
    /// The message hash this witness authorizes.
    #[serde(default)]
    pub request_hash: Fr,
    /// The witness data (e.g., signature bytes as field elements).
    #[serde(default)]
    pub fields: Vec<Fr>,
}
```

### Migration
- `AuthWitness::default()` already returns zero-valued fields, so adding `request_hash: Fr::default()` is backward compatible.
- Update all constructors and test helpers.
- Ensure JSON serialization includes `request_hash` (needed for RPC).

### Tests
- Serialization/deserialization round-trip includes `request_hash`.
- Default authwit has zero `request_hash`.

### Acceptance criteria
- `AuthWitness.request_hash` exists and is serialized.
- No breaking changes to existing code.

---

## Sub-step 4.9: Re-exports & Public API

**Crates:** `aztec-core`, `aztec-account`, `aztec-contract`, umbrella `aztec-rs`

### Deliverables

Ensure all new types are properly exported:

**`aztec-core/src/lib.rs`:**
```rust
pub mod hash;  // NEW
```

**`aztec-account/src/lib.rs`:**
```rust
pub use authorization::CallAuthorizationRequest;  // NEW
```

**`aztec-contract/src/lib.rs`:**
```rust
pub mod authwit;  // NEW
pub use authwit::{SetPublicAuthWitInteraction, lookup_validity, AuthWitValidity};
```

**Umbrella crate `aztec-rs`:**
- Re-export hash functions from `aztec-core::hash`.
- Re-export `CallAuthorizationRequest` from `aztec-account`.
- Re-export `SetPublicAuthWitInteraction`, `lookup_validity`, `AuthWitValidity` from `aztec-contract`.

### Acceptance criteria
- All new public types are accessible through the umbrella crate.
- `cargo doc` generates documentation for all new items.

---

## Sub-step 4.10: Integration Tests

**Location:** `crates/contract/tests/` or a dedicated `tests/` directory

### Deliverables

1. **Hash consistency test:** Compute authwit hashes in Rust and compare against hardcoded values generated by the TS SDK. This is the critical correctness guarantee.

2. **End-to-end authwit flow (mock):**
   ```
   Given: Alice wallet, Bob address, Token contract
   When: Bob wants to transfer from Alice
   1. Build intent: { caller: Bob, call: token.transfer(Alice, Charlie, 100) }
   2. Compute message hash
   3. Alice creates auth witness: alice_wallet.create_auth_wit(alice, intent)
   4. Verify the witness has the correct request_hash
   5. Lookup validity (mock): should be valid in private
   6. Set public authwit: SetPublicAuthWitInteraction::create(...)
   7. Lookup validity: should be valid in public
   ```

3. **CallAuthorizationRequest round-trip test.**

4. **Edge cases:**
   - Empty args in function call → var args hash is zero.
   - `MessageHashOrIntent::Hash` passes through without computation.
   - `MessageHashOrIntent::InnerHash` correctly computes outer hash.

### Acceptance criteria
- All tests pass.
- At least one cross-validation test vector from the TS SDK.

---

## Implementation Order & Dependencies

```
4.1  Domain Separators & Protocol Constants
  │
  └─► 4.2  Poseidon2 Hash with Separator
        │
        └─► 4.3  AuthWit Hash Functions  ◄── 4.8  Enhance AuthWitness
              │
              ├─► 4.4  CallAuthorizationRequest
              │
              ├─► 4.5  SetPublicAuthWitInteraction
              │
              ├─► 4.6  lookup_validity()
              │
              └─► 4.7  Wire BaseWallet hash computation
                    │
                    └─► 4.9  Re-exports
                          │
                          └─► 4.10  Integration Tests
```

**Parallelizable:** 4.4, 4.5, 4.6, 4.7, and 4.8 can all be worked on in parallel once 4.3 is done.

---

## Risk & Open Questions

### R1: Poseidon2 Implementation
The biggest risk is ensuring Poseidon2 hash output matches the TS/C++ implementation exactly. The BN254 parameters, round constants, and S-box configuration must be identical. **Mitigation:** Generate test vectors from the TS SDK and cross-validate.

### R2: ABI Value → Field Encoding
`compute_inner_auth_wit_hash_from_action` needs to convert `Vec<AbiValue>` to `Vec<Fr>` for hashing. The TS SDK has `encodeArguments()` for this. We need at least a minimal `abi_values_to_fields()` helper. This may partially overlap with Step 6 (ABI encoding). **Mitigation:** Implement a minimal version that handles `Field`, `Integer`, `Boolean`, and `Address` — the common types used in authwit scenarios. Defer full ABI encoding to Step 6.

### R3: `FunctionSelector` for Hardcoded ABIs
`SetPublicAuthWitInteraction` and `lookup_validity` hardcode ABIs with known selectors. If `FunctionSelector::from_name()` is still a stub, we need to either hardcode the selector values or implement the selector computation. **Mitigation:** Hardcode the selector values matching the TS SDK, add a TODO for Step 6.

### R4: `MessageHashOrIntent` Relocation
Moving `MessageHashOrIntent` and `ChainInfo` from `aztec-wallet` to `aztec-core` is a refactor that touches multiple crates. **Mitigation:** Re-export from `aztec-wallet` to maintain backward compatibility.

---

## Estimated Scope

| Sub-step | New lines (est.) | Modified lines (est.) |
|---|---|---|
| 4.1 Domain separators | ~30 | ~5 |
| 4.2 Poseidon2 hash | ~80-200 | ~5 |
| 4.3 Hash functions | ~120 | ~40 (type relocation) |
| 4.4 CallAuthorizationRequest | ~80 | ~5 |
| 4.5 SetPublicAuthWitInteraction | ~120 | ~10 |
| 4.6 lookup_validity | ~130 | ~15 |
| 4.7 Wire BaseWallet | ~30 | ~20 |
| 4.8 Enhance AuthWitness | ~10 | ~15 |
| 4.9 Re-exports | ~15 | ~10 |
| 4.10 Integration tests | ~200 | ~0 |
| **Total** | **~815-935** | **~125** |
