# `aztec-account`

Account abstraction: traits, concrete account implementations, entrypoints, authorization, and deployment.

Source: `crates/account/src/`.

## Module Map

| Module                    | Highlights                                                                                              |
| ------------------------- | ------------------------------------------------------------------------------------------------------- |
| `account`                 | Core traits (`Account`, `AccountContract`, `AuthorizationProvider`), `AccountManager<W>`, `AccountWithSecretKey`, deployment types (`DeployAccountMethod`, `DeployAccountOptions`, `DeployResult`, `InitializationSpec`, `EntrypointOptions`, `TxExecutionRequest`), `get_account_contract_address` |
| `authorization`           | `CallAuthorizationRequest`                                                                              |
| `entrypoint`              | `DefaultAccountEntrypoint`, `DefaultAccountEntrypointOptions`, `DefaultMultiCallEntrypoint`, `AccountFeePaymentMethodOptions`, `EncodedAppEntrypointCalls`, encoding helpers |
| `schnorr`                 | `SchnorrAccountContract`, `SchnorrAccount`, `SchnorrAuthorizationProvider`                              |
| `signerless`              | `SignerlessAccount` (no-signature account for deployment bootstrap / tests)                             |
| `meta_payment`            | `AccountEntrypointMetaPaymentMethod` — payment method that reuses the account's own entrypoint          |
| `single_account_provider` | `SingleAccountProvider` — wrap one account to satisfy `AccountProvider` for `BaseWallet`                |

## Core Traits

```rust,ignore
pub trait AuthorizationProvider: Send + Sync {
    // Sign / assemble authorization witnesses for a given call set.
}

pub trait Account: Send + Sync + AuthorizationProvider {
    // The user-facing account: knows its address, entrypoint, and auth.
}

pub trait AccountContract: Send + Sync {
    // The deployable contract that implements the account logic on-chain.
}
```

`Account` composes `AuthorizationProvider` — authorization is the part that varies per signing scheme (for example Schnorr, signerless, or a custom downstream implementation).

## Account Flavors

| Flavor        | Type(s)                                                      | Signing     |
| ------------- | ------------------------------------------------------------ | ----------- |
| Schnorr       | `SchnorrAccount`, `SchnorrAccountContract`, `SchnorrAuthorizationProvider` | Grumpkin Schnorr (default Aztec account) |
| Signerless    | `SignerlessAccount`                                          | None — used during deployment bootstrap and for test accounts |

ECDSA account flavors live in the Aztec ecosystem but are not currently shipped from this crate; contributions welcome.

## `AccountManager`

`AccountManager<W>` is the high-level helper for the account lifecycle:

```rust,ignore
use aztec_account::{AccountManager, DeployAccountOptions, SchnorrAccountContract};
use aztec_wallet::SendOptions;
use aztec_core::types::Fr;

let secret_key = Fr::from(12345u64);
let contract = SchnorrAccountContract::new(secret_key);
let manager = AccountManager::create(
    wallet,
    secret_key,
    Box::new(contract),
    None::<Fr>,
)
    .await?;
let deploy = manager.deploy_method().await?;
let result = deploy
    .send(&DeployAccountOptions::default(), SendOptions::default())
    .await?;
println!("account address = {}", result.instance.address);
```

`DeployAccountMethod` is the builder behind `deploy_method`; you can drive it explicitly to customize fee payment, simulation, or send options.

## Entrypoints

- `DefaultAccountEntrypoint` — single-call entrypoint used by most account contracts.
- `DefaultMultiCallEntrypoint` — batches multiple calls into one tx.
- `EncodedAppEntrypointCalls` — the wire-format call array produced by either entrypoint.
- `AccountFeePaymentMethodOptions` — fee-payment opts carried through the entrypoint.

## Authorization

`CallAuthorizationRequest` encapsulates everything an `AuthorizationProvider` needs to produce `AuthWitness` values for a simulated tx: the call list, origin, nonce, and any required context.

## Providers

`SingleAccountProvider` adapts a single `Account` into the `AccountProvider` abstraction that [`BaseWallet`](./aztec-wallet.md) consumes.

## Full API

Bundled rustdoc: [`api/aztec_account/`](../api/aztec_account/index.html).
Local regeneration:

```bash
cargo doc -p aztec-account --open
```

## See Also

- [`aztec-wallet`](./aztec-wallet.md) — consumes `AccountProvider`.
- [`aztec-contract`](./aztec-contract.md) — used by `AccountManager` for deployment.
- [`aztec-crypto`](./aztec-crypto.md) — Schnorr signing primitives.
