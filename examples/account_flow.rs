//! Example: Account lifecycle workflow.
//!
//! Demonstrates how to use `AccountManager` with a mock account contract
//! to prepare account creation and deployment.
//!
//! Run with:
//! ```bash
//! cargo run --example account_flow
//! ```

#![allow(clippy::print_stdout)]

use async_trait::async_trait;

use aztec_rs::abi::{
    AbiParameter, AbiType, AbiValue, ContractArtifact, FunctionArtifact, FunctionSelector,
    FunctionType,
};
use aztec_rs::account::{
    Account, AccountContract, AccountManager, AuthorizationProvider, DeployAccountOptions,
    EntrypointOptions, InitializationSpec, TxExecutionRequest,
};
use aztec_rs::fee::GasSettings;
use aztec_rs::tx::{AuthWitness, ExecutionPayload, HashedValues, TxContext};
use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
use aztec_rs::wallet::{ChainInfo, MessageHashOrIntent, MockWallet};

// ---------------------------------------------------------------------------
// A minimal Schnorr-like account contract for demonstration
// ---------------------------------------------------------------------------

struct DemoAccountContract {
    signing_key: Fr,
}

impl DemoAccountContract {
    const fn new(signing_key: Fr) -> Self {
        Self { signing_key }
    }
}

struct DemoAccount {
    addr: CompleteAddress,
}

#[async_trait]
impl AuthorizationProvider for DemoAccount {
    async fn create_auth_wit(
        &self,
        _intent: MessageHashOrIntent,
        _chain_info: &ChainInfo,
    ) -> Result<AuthWitness, aztec_rs::Error> {
        Ok(AuthWitness {
            fields: vec![self.addr.address.0],
            ..Default::default()
        })
    }
}

#[async_trait]
impl Account for DemoAccount {
    fn complete_address(&self) -> &CompleteAddress {
        &self.addr
    }

    fn address(&self) -> AztecAddress {
        self.addr.address
    }

    async fn create_tx_execution_request(
        &self,
        exec: ExecutionPayload,
        gas_settings: GasSettings,
        chain_info: &ChainInfo,
        _options: EntrypointOptions,
    ) -> Result<TxExecutionRequest, aztec_rs::Error> {
        Ok(TxExecutionRequest {
            origin: self.addr.address,
            function_selector: FunctionSelector::from_hex("0xaabbccdd")?,
            first_call_args_hash: Fr::from(1u64),
            tx_context: TxContext {
                chain_id: chain_info.chain_id,
                version: chain_info.version,
                gas_settings,
            },
            args_of_calls: vec![HashedValues::from_args(vec![Fr::from(1u64)])],
            auth_witnesses: exec.auth_witnesses,
            capsules: exec.capsules,
            salt: Fr::from(7u64),
        })
    }

    async fn wrap_execution_payload(
        &self,
        exec: ExecutionPayload,
        options: EntrypointOptions,
    ) -> Result<ExecutionPayload, aztec_rs::Error> {
        Ok(ExecutionPayload {
            fee_payer: options.fee_payer.or(exec.fee_payer),
            ..exec
        })
    }
}

#[async_trait]
impl AccountContract for DemoAccountContract {
    async fn contract_artifact(&self) -> Result<ContractArtifact, aztec_rs::Error> {
        Ok(ContractArtifact {
            name: "SchnorrAccount".to_owned(),
            functions: vec![FunctionArtifact {
                name: "constructor".to_owned(),
                function_type: FunctionType::Private,
                is_initializer: true,
                is_static: false,
                parameters: vec![AbiParameter {
                    name: "signing_pub_key".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                }],
                return_types: vec![],
                selector: Some(FunctionSelector::from_hex("0xaabb1122")?),
                bytecode: None,
                verification_key_hash: None,
                verification_key: None,
                custom_attributes: None,
                is_unconstrained: None,
                debug_symbols: None,
                error_types: None,
                is_only_self: None,
            }],
            outputs: None,
            file_map: None,
        })
    }

    async fn initialization_function_and_args(
        &self,
    ) -> Result<Option<InitializationSpec>, aztec_rs::Error> {
        Ok(Some(InitializationSpec {
            constructor_name: "constructor".to_owned(),
            constructor_args: vec![AbiValue::Field(self.signing_key)],
        }))
    }

    fn account(&self, address: CompleteAddress) -> Box<dyn Account> {
        Box::new(DemoAccount { addr: address })
    }

    fn auth_witness_provider(&self, address: CompleteAddress) -> Box<dyn AuthorizationProvider> {
        Box::new(DemoAccount { addr: address })
    }
}

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    println!("=== Aztec Account Lifecycle Demo ===\n");

    // 1. Create a mock wallet (stands in for a real wallet backend).
    let wallet = MockWallet::new(ChainInfo {
        chain_id: Fr::from(31337u64),
        version: Fr::from(1u64),
    });
    println!("Created mock wallet");

    // 2. Define the account contract and a secret key.
    let secret_key = Fr::from(12345u64);
    let account_contract = Box::new(DemoAccountContract::new(secret_key));
    println!("Account contract: SchnorrAccount");
    println!("Secret key:       {secret_key}");

    // 3. Create an AccountManager.
    let salt = Fr::from(99u64);
    let manager = AccountManager::create(wallet, secret_key, account_contract, Some(salt)).await?;
    println!("\nAccountManager created:");
    println!("  salt:           {}", manager.salt());
    println!("  secret_key:     {}", manager.secret_key());
    println!("  has_initializer: {}", manager.has_initializer());

    // 4. Access the contract instance with real derived address.
    let instance = manager.instance();
    println!("\nContract instance:");
    println!("  address: {}", instance.address);
    println!("  version: {}", instance.inner.version);
    println!("  salt:    {}", instance.inner.salt);

    // 5. Get the complete address (key derivation + address computation).
    match manager.complete_address().await {
        Ok(addr) => println!("\nComplete address: {}", addr.address),
        Err(e) => println!("\nComplete address error: {e}"),
    }

    // 6. Get a deploy method and build the deployment payload.
    match manager.deploy_method().await {
        Ok(deploy) => {
            println!(
                "\nDeploy method created for instance: {}",
                deploy.instance().address
            );
            let opts = DeployAccountOptions {
                skip_registration: true,
                ..Default::default()
            };
            match deploy.request(&opts).await {
                Ok(payload) => println!(
                    "Deployment payload built with {} call(s).",
                    payload.calls.len()
                ),
                Err(e) => println!("Deploy request error: {e}"),
            }
        }
        Err(e) => println!("\nDeploy method error: {e}"),
    }

    // 7. Demonstrate get_account_contract_address.
    let contract = DemoAccountContract::new(secret_key);
    let pre_addr =
        aztec_rs::account::get_account_contract_address(&contract, secret_key, salt).await?;
    println!("\nPre-deployment address: {pre_addr}");
    println!("Manager address:        {}", manager.address());
    assert_eq!(pre_addr, manager.address());
    println!("Addresses match!");

    println!("\nDone.");

    Ok(())
}
