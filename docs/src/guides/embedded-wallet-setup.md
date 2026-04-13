# Embedded Wallet Setup

For Aztec v4.x the typical entrypoint is `aztec_rs::wallet::create_embedded_wallet`.
It composes an in-process PXE ([`aztec-pxe`](../reference/aztec-pxe.md)), a node client ([`aztec-node-client`](../reference/aztec-node-client.md)), and an account provider ([`aztec-account`](../reference/aztec-account.md)) into a production-ready `BaseWallet`.

## Signature

```rust,ignore
// Requires the `embedded-pxe` feature on `aztec-wallet` (enabled by default via `aztec-rs`).
pub async fn create_embedded_wallet<A: AccountProvider>(
    node_url: impl Into<String>,
    accounts: A,
) -> Result<
    BaseWallet<aztec_pxe::EmbeddedPxe<HttpNodeClient>, HttpNodeClient, A>,
    aztec_rs::Error,
>;
```

Internally it:

1. Calls `create_aztec_node_client(node_url)` to build an `HttpNodeClient`.
2. Calls `EmbeddedPxe::create_ephemeral(node.clone())` — an in-memory KV-backed PXE.
3. Wraps the three into `BaseWallet`.

## Minimal Example

```rust,ignore
use aztec_rs::account::{AccountContract, SingleAccountProvider, SchnorrAccountContract};
use aztec_rs::crypto::{complete_address_from_secret_key_and_partial_address, derive_keys};
use aztec_rs::deployment::{
    get_contract_instance_from_instantiation_params, ContractInstantiationParams,
};
use aztec_rs::hash::{compute_partial_address, compute_salted_initialization_hash};
use aztec_rs::types::{AztecAddress, Fr};
use aztec_rs::wallet::create_embedded_wallet;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    // 1. Load the secret and deployment salt for an existing account.
    let secret_key: Fr = /* Fr::from_hex(...) or Fr::random() */;
    let salt: Fr = /* the salt this account was deployed with */;

    // 2. Reconstruct the account contract and complete address.
    let account_contract = SchnorrAccountContract::new(secret_key);
    let artifact = account_contract.contract_artifact().await?;
    let init_spec = account_contract.initialization_function_and_args().await?;
    let public_keys = derive_keys(&secret_key).public_keys;

    let instance = get_contract_instance_from_instantiation_params(
        &artifact,
        ContractInstantiationParams {
            constructor_name: init_spec.as_ref().map(|spec| spec.constructor_name.as_str()),
            constructor_args: init_spec
                .as_ref()
                .map(|spec| spec.constructor_args.clone())
                .unwrap_or_default(),
            salt,
            public_keys: public_keys.clone(),
            deployer: AztecAddress::zero(),
        },
    )?;

    let salted_init_hash = compute_salted_initialization_hash(
        instance.inner.salt,
        instance.inner.initialization_hash,
        instance.inner.deployer,
    );
    let partial_address = compute_partial_address(
        instance.inner.original_contract_class_id,
        salted_init_hash,
    );
    let complete_address =
        complete_address_from_secret_key_and_partial_address(&secret_key, &partial_address)?;
    let provider = SingleAccountProvider::new(
        complete_address,
        Box::new(account_contract),
        "main",
    );

    // 3. Build the wallet.
    let wallet = create_embedded_wallet("http://localhost:8080", provider).await?;

    // 4. Use it.
    let chain = wallet.get_chain_info().await?;
    println!("chain id = {}", chain.chain_id);
    Ok(())
}
```

See `examples/wallet_minimal.rs` in the repository for the end-to-end version.

## Configuration Points

| Knob                   | Where to set                                                 |
| ---------------------- | ------------------------------------------------------------ |
| Node URL               | Argument to `create_embedded_wallet`                         |
| Account flavor         | The `AccountProvider` you hand in                            |
| Persistent PXE storage | Bypass `create_embedded_wallet` and build `BaseWallet` + `EmbeddedPxe::create(node, kv)` with a `SledKvStore` |
| Prover settings        | `EmbeddedPxeConfig { prover_config, .. }` via `create_with_config` |

## Persistent Storage

If you need persistence, construct the PXE explicitly:

```rust,ignore
use std::sync::Arc;
use aztec_pxe::{EmbeddedPxe, SledKvStore};
use aztec_wallet::create_wallet;

let node = aztec_node_client::create_aztec_node_client("http://localhost:8080");
let kv   = Arc::new(SledKvStore::open("./pxe.sled")?);
let pxe  = EmbeddedPxe::create(node.clone(), kv).await?;
let wallet = create_wallet(pxe, node, provider);
```

## Full Runnable Example

Source: [`examples/wallet_minimal.rs`](https://github.com/NethermindEth/aztec-rs/blob/main/examples/wallet_minimal.rs).
Depends on `examples/common/` for shared test-account setup.

```rust,ignore
{{#include ../../../examples/wallet_minimal.rs}}
```

## Next

- [Account Lifecycle](./account-lifecycle.md) — creating + deploying a fresh account on top of the wallet.
- [Calling Contracts](./calling-contracts.md)
