# `aztec-crypto`

Higher-level cryptographic primitives built on top of [`aztec-core`](./aztec-core.md): key derivation, Pedersen, Schnorr, address derivation, and SHA-512 → Grumpkin scalar reduction.

Source: `crates/crypto/src/`.

## Module Map

| Module     | Purpose                                                              |
| ---------- | -------------------------------------------------------------------- |
| `keys`     | Master + app-scoped key derivation, `DerivedKeys`, `KeyType`         |
| `schnorr`  | Schnorr sign / verify on Grumpkin, `SchnorrSignature`                |
| `pedersen` | `pedersen_hash` (legacy and domain-separated)                        |
| `address`  | `complete_address_from_secret_key_and_partial_address`               |
| `sha512`   | `sha512_to_grumpkin_scalar` (SHA-512 reduced mod Grumpkin order)     |

The crate also re-exports `aztec_core::hash::{compute_address, compute_secret_hash}` for ergonomics.

## Key Derivation

The Aztec key hierarchy splits a master secret into four master keys plus an application-scoped signing key:

```rust,ignore
use aztec_crypto::{derive_keys, DerivedKeys, KeyType};

let DerivedKeys {
    master_nullifier_hiding_key,
    master_incoming_viewing_secret_key,
    master_outgoing_viewing_secret_key,
    master_tagging_secret_key,
    public_keys,                 // PublicKeys bundle
} = derive_keys(&secret_key);
```

Individual derivation functions are exposed for fine-grained use:

- `derive_master_nullifier_hiding_key`
- `derive_master_incoming_viewing_secret_key`
- `derive_master_outgoing_viewing_secret_key`
- `derive_master_tagging_secret_key`
- `derive_signing_key` (Grumpkin scalar used by Schnorr accounts)
- `derive_public_key_from_secret_key` (Grumpkin scalar mult against generator)

Application-scoped derivations:

- `compute_app_secret_key(master, app_address, key_type)`
- `compute_app_nullifier_hiding_key(master, app_address)`
- `compute_ovsk_app(master, app_address)`

`KeyType` distinguishes the four master keys when computing app-scoped derivations.

## Schnorr Signatures

```rust,ignore
use aztec_crypto::{schnorr_sign, schnorr_verify, SchnorrSignature};

let sig: SchnorrSignature = schnorr_sign(&message, &signing_key);
assert!(schnorr_verify(&message, &public_key, &sig));
```

The sign/verify pair matches barretenberg's Grumpkin Schnorr scheme used by default Aztec accounts.

## Pedersen Hash

`pedersen_hash(inputs, generator_index)` — Pedersen commitment matching the TS SDK's `pedersenHash`.
Used for compatibility with historical protocol components that predate Poseidon2.

## Address Derivation

```rust,ignore
use aztec_crypto::complete_address_from_secret_key_and_partial_address;
let complete = complete_address_from_secret_key_and_partial_address(&sk, &partial);
```

Produces a `CompleteAddress` (address + public keys + partial address).

## Full API

Bundled rustdoc: [`api/aztec_crypto/`](../api/aztec_crypto/index.html).
Local regeneration:

```bash
cargo doc -p aztec-crypto --open
```

## See Also

- [`aztec-core`](./aztec-core.md) — underlying field / point types and Poseidon2.
- [`aztec-account`](./aztec-account.md) — consumers of `derive_keys` + `schnorr_sign`.
