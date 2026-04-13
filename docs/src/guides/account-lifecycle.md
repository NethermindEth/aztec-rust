# Account Lifecycle

End-to-end: generate keys, register, deploy the account contract, send the first transaction.

## Runnable Example

`examples/account_deploy.rs` walks through the full lifecycle with the embedded wallet.
`examples/multiple_accounts_one_encryption_key.rs` shows sharing encryption keys across accounts.

## Stages

1. **Key generation** — derive signing + master keys via [`aztec-crypto::derive_keys`](../reference/aztec-crypto.md).
2. **Account flavor** — pick `SchnorrAccount` (default) or `SignerlessAccount` (tests / bootstrap).
3. **Registration** — install the account into the PXE so it can decrypt notes (`Pxe::register_account`).
4. **Deployment** — call `AccountManager::create(...)`, then `deploy_method().await?`, then `DeployAccountMethod::send(...)`, funded by a fee strategy (see [Fee Payments](./fee-payments.md)).
5. **First transaction** — send a call via the newly deployed account's entrypoint through the wallet.

## Sketch

```rust,ignore
use aztec_rs::account::{AccountManager, DeployAccountOptions, SchnorrAccountContract};
use aztec_rs::wallet::SendOptions;

let secret_key = /* Fr::random() or loaded */;
let contract = SchnorrAccountContract::new(secret_key);

let manager = AccountManager::create(
    wallet.clone(),
    secret_key,
    Box::new(contract),
    None::<aztec_rs::types::Fr>,
)
.await?;
let deploy = manager.deploy_method().await?;
let result = deploy
    .send(
        &DeployAccountOptions::default(),
        SendOptions {
            from: /* sponsor or fee-paying account */,
            ..Default::default()
        },
    )
    .await?;
println!("account deployed at {}", result.instance.address);
```

## Edge Cases

- **Self-paid deployment**: the account can't authorize its own deployment before it exists on-chain; use `SignerlessAccount` + a claim- or sponsor-based fee strategy for the very first tx.
- **Re-registration** is idempotent.
- **Chain re-orgs** may rewind the deployment tx; applications SHOULD wait for `wait_for_contract` before treating the account as usable.

## Full Runnable Example

Source: [`examples/account_deploy.rs`](https://github.com/NethermindEth/aztec-rs/blob/main/examples/account_deploy.rs).

```rust,ignore
{{#include ../../../examples/account_deploy.rs}}
```

## References

- [Concepts: Accounts & Wallets](../concepts/accounts-and-wallets.md)
- [`aztec-account` reference](../reference/aztec-account.md)
- [`aztec-crypto` reference](../reference/aztec-crypto.md)
