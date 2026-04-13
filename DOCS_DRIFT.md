• Findings

  - [x] High: docs/src/guides/embedded-wallet-setup.md:27 contained a non-compiling setup snippet. It used SchnorrAccountContract::new(&keys) and
    SingleAccountProvider::new(account_contract), but the real signatures are SchnorrAccountContract::new(secret_key: Fr) in crates/account/src/schnorr.rs:187 and
    SingleAccountProvider::new(complete_address, Box<dyn AccountContract>, alias) in crates/account/src/single_account_provider.rs:28.
  - [x] High: docs/src/guides/account-lifecycle.md:20 and docs/src/reference/aztec-account.md:50 documented a nonexistent AccountManager::new(...).deploy(...) flow. The current API is
    async AccountManager::create(...) in crates/account/src/account.rs:455, then deploy_method().await? in crates/account/src/account.rs:591, then .send(&DeployAccountOptions,
    SendOptions) in crates/account/src/account.rs:384.
  - [x] High: docs/src/reference/aztec-contract.md:49 and docs/src/guides/deploying-contracts.md:14 described the deployment API with the wrong result fields and helper signatures.
    DeployResult is { send_result, instance }, not { contract, receipt }, per crates/contract/src/deployment.rs:306. The documented helper calls were also stale:
    publish_contract_class is (wallet, &artifact) at crates/contract/src/deployment.rs:103, publish_instance is (wallet, &instance) at crates/contract/src/deployment.rs:178, and
    get_contract_instance_from_instantiation_params is (artifact, params) at crates/contract/src/deployment.rs:259.
  - [x] Medium: README.md:24 and docs/src/introduction.md:25 overstate shipped features. They say the repo includes ECDSA / SingleKey account flavors and private FPC fee payment, but
    the public account exports are only schnorr, signerless, and single_account_provider in crates/account/src/lib.rs:27, and the fee crate exports only NativeFeePaymentMethod,
    SponsoredFeePaymentMethod, and FeeJuicePaymentMethodWithClaim in crates/fee/src/lib.rs:12.
  - [x] Medium: README.md:160, docs/src/development/testing.md:16, and tests/contract/e2e_token_transfer_private.rs:4 tell users to run cargo test --test e2e_token_transfer_private,
    but that target does not exist. The actual integration target is contract, documented in tests/contract.rs:5. I verified this directly: cargo test --test
    e2e_token_transfer_private -- --list fails, while cargo test --test contract -- --list exposes e2e_token_transfer_private::*.

  This was a first pass over the high-traffic docs: README, top-level book pages, guides, and key reference pages. The drift looks systemic around account/deploy APIs, so a second
  pass will likely find more stale snippets in the mdBook.
