//! Schnorr account contract — a real account implementation with Schnorr signing.
//!
//! This is the Rust equivalent of the TS `SchnorrAccountContract`. It uses
//! the Grumpkin-based Schnorr signature scheme for transaction authorization
//! and routes calls through the `DefaultAccountEntrypoint`.

use async_trait::async_trait;

use aztec_core::abi::{
    AbiParameter, AbiType, AbiValue, ContractArtifact, FunctionArtifact, FunctionType,
};
use aztec_core::fee::GasSettings;
use aztec_core::hash::{compute_auth_wit_message_hash, ChainInfo};
use aztec_core::tx::{AuthWitness, ExecutionPayload};
use aztec_core::types::{AztecAddress, CompleteAddress, Fr, Point};
use aztec_core::Error;

use aztec_crypto::keys::{derive_public_key_from_secret_key, derive_signing_key};
use aztec_crypto::schnorr::schnorr_sign;

use crate::account::{
    Account, AccountContract, AuthorizationProvider, EntrypointOptions, TxExecutionRequest,
};
use crate::entrypoint::{DefaultAccountEntrypoint, DefaultAccountEntrypointOptions};
use crate::wallet::{ChainInfo as WalletChainInfo, MessageHashOrIntent};

// ---------------------------------------------------------------------------
// SchnorrAuthorizationProvider
// ---------------------------------------------------------------------------

/// Authorization provider that signs messages with Schnorr on Grumpkin.
pub struct SchnorrAuthorizationProvider {
    signing_key: aztec_core::types::GrumpkinScalar,
}

#[async_trait]
impl AuthorizationProvider for SchnorrAuthorizationProvider {
    async fn create_auth_wit(
        &self,
        intent: MessageHashOrIntent,
        chain_info: &WalletChainInfo,
    ) -> Result<AuthWitness, Error> {
        let core_chain_info = ChainInfo {
            chain_id: chain_info.chain_id,
            version: chain_info.version,
        };
        let message_hash = compute_auth_wit_message_hash(&intent, &core_chain_info);
        let signature = schnorr_sign(&self.signing_key, &message_hash);

        Ok(AuthWitness {
            request_hash: message_hash,
            fields: signature.to_fields(),
        })
    }
}

// ---------------------------------------------------------------------------
// SchnorrAccount
// ---------------------------------------------------------------------------

/// An account that uses Schnorr signing and the default account entrypoint.
pub struct SchnorrAccount {
    address: CompleteAddress,
    signing_key: aztec_core::types::GrumpkinScalar,
    entrypoint: DefaultAccountEntrypoint,
}

#[async_trait]
impl AuthorizationProvider for SchnorrAccount {
    async fn create_auth_wit(
        &self,
        intent: MessageHashOrIntent,
        chain_info: &WalletChainInfo,
    ) -> Result<AuthWitness, Error> {
        let core_chain_info = ChainInfo {
            chain_id: chain_info.chain_id,
            version: chain_info.version,
        };
        let message_hash = compute_auth_wit_message_hash(&intent, &core_chain_info);
        let signature = schnorr_sign(&self.signing_key, &message_hash);

        Ok(AuthWitness {
            request_hash: message_hash,
            fields: signature.to_fields(),
        })
    }
}

#[async_trait]
impl Account for SchnorrAccount {
    fn complete_address(&self) -> &CompleteAddress {
        &self.address
    }

    fn address(&self) -> AztecAddress {
        self.address.address
    }

    async fn create_tx_execution_request(
        &self,
        exec: ExecutionPayload,
        gas_settings: GasSettings,
        chain_info: &WalletChainInfo,
        _options: EntrypointOptions,
    ) -> Result<TxExecutionRequest, Error> {
        let core_chain_info = ChainInfo {
            chain_id: chain_info.chain_id,
            version: chain_info.version,
        };
        self.entrypoint
            .create_tx_execution_request(
                exec,
                gas_settings,
                &core_chain_info,
                &DefaultAccountEntrypointOptions::default(),
            )
            .await
    }

    async fn wrap_execution_payload(
        &self,
        exec: ExecutionPayload,
        _options: EntrypointOptions,
    ) -> Result<ExecutionPayload, Error> {
        let chain_info = ChainInfo {
            chain_id: Fr::from(0u64),
            version: Fr::from(0u64),
        };
        self.entrypoint
            .wrap_execution_payload(
                exec,
                &chain_info,
                &DefaultAccountEntrypointOptions::default(),
            )
            .await
    }
}

// ---------------------------------------------------------------------------
// SchnorrAccountContract
// ---------------------------------------------------------------------------

/// A Schnorr-based account contract.
///
/// This is the primary account contract implementation, equivalent to the TS
/// `SchnorrAccountContract`. It uses a Grumpkin signing key for Schnorr
/// signatures and routes transactions through the `DefaultAccountEntrypoint`.
///
/// # Example
///
/// ```ignore
/// use aztec_rs::account::{AccountManager, SchnorrAccountContract};
/// use aztec_rs::types::Fr;
/// use aztec_rs::wallet::{ChainInfo, MockWallet};
///
/// # async fn example() -> Result<(), aztec_rs::Error> {
/// let wallet = MockWallet::new(ChainInfo {
///     chain_id: Fr::from(31337u64),
///     version: Fr::from(1u64),
/// });
/// let secret_key = Fr::from(12345u64);
/// let contract = SchnorrAccountContract::new(secret_key);
/// let manager = AccountManager::create(wallet, secret_key, Box::new(contract), None::<Fr>).await?;
/// # Ok(())
/// # }
/// ```
pub struct SchnorrAccountContract {
    secret_key: Fr,
    signing_key: aztec_core::types::GrumpkinScalar,
    signing_public_key: Point,
}

impl SchnorrAccountContract {
    /// Create a new Schnorr account contract from a secret key.
    ///
    /// Derives the signing key pair from the secret key using the standard
    /// Aztec key derivation.
    pub fn new(secret_key: Fr) -> Self {
        let signing_key = derive_signing_key(&secret_key);
        let signing_public_key = derive_public_key_from_secret_key(&signing_key);
        Self {
            secret_key,
            signing_key,
            signing_public_key,
        }
    }

    /// Returns the Schnorr signing public key.
    pub fn signing_public_key(&self) -> &Point {
        &self.signing_public_key
    }

    /// Returns the secret key used for key derivation.
    pub fn secret_key(&self) -> Fr {
        self.secret_key
    }

    fn constructor_artifact() -> FunctionArtifact {
        let public_key_struct = AbiType::Struct {
            name: "schnorr_account::auth::PublicKey".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "x".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                },
                AbiParameter {
                    name: "y".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                },
            ],
        };

        FunctionArtifact {
            name: "constructor".to_owned(),
            function_type: FunctionType::Private,
            is_initializer: true,
            is_static: false,
            parameters: vec![AbiParameter {
                name: "signing_pub_key".to_owned(),
                typ: public_key_struct,
                visibility: None,
            }],
            return_types: vec![],
            selector: None,
            bytecode: None,
            verification_key_hash: None,
            verification_key: None,
            custom_attributes: None,
            is_unconstrained: None,
            debug_symbols: None,
            error_types: None,
            is_only_self: None,
        }
    }
}

use crate::account::InitializationSpec;

#[async_trait]
impl AccountContract for SchnorrAccountContract {
    async fn contract_artifact(&self) -> Result<ContractArtifact, Error> {
        let entrypoint_abi = DefaultAccountEntrypoint::entrypoint_abi();

        Ok(ContractArtifact {
            name: "SchnorrAccount".to_owned(),
            functions: vec![Self::constructor_artifact(), entrypoint_abi],
            outputs: None,
            file_map: None,
        })
    }

    async fn initialization_function_and_args(
        &self,
    ) -> Result<Option<InitializationSpec>, Error> {
        let mut pk_fields = std::collections::BTreeMap::new();
        pk_fields.insert("x".to_owned(), AbiValue::Field(self.signing_public_key.x));
        pk_fields.insert("y".to_owned(), AbiValue::Field(self.signing_public_key.y));

        Ok(Some(InitializationSpec {
            constructor_name: "constructor".to_owned(),
            constructor_args: vec![AbiValue::Struct(pk_fields)],
        }))
    }

    fn account(&self, address: CompleteAddress) -> Box<dyn Account> {
        let auth = self.auth_witness_provider(address.clone());
        let entrypoint = DefaultAccountEntrypoint::new(address.address, auth);
        Box::new(SchnorrAccount {
            address,
            signing_key: self.signing_key,
            entrypoint,
        })
    }

    fn auth_witness_provider(
        &self,
        _address: CompleteAddress,
    ) -> Box<dyn AuthorizationProvider> {
        Box::new(SchnorrAuthorizationProvider {
            signing_key: self.signing_key,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::account::AccountManager;
    use crate::wallet::MockWallet;

    fn sample_chain_info() -> WalletChainInfo {
        WalletChainInfo {
            chain_id: Fr::from(31337u64),
            version: Fr::from(1u64),
        }
    }

    #[test]
    fn new_derives_keys() {
        let contract = SchnorrAccountContract::new(Fr::from(12345u64));
        let pk = contract.signing_public_key();
        assert!(!pk.is_zero());
        assert!(!pk.is_infinite);
    }

    #[tokio::test]
    async fn contract_artifact_has_constructor_and_entrypoint() {
        let contract = SchnorrAccountContract::new(Fr::from(12345u64));
        let artifact = contract.contract_artifact().await.expect("artifact");
        assert_eq!(artifact.name, "SchnorrAccount");
        assert_eq!(artifact.functions.len(), 2);
        assert_eq!(artifact.functions[0].name, "constructor");
        assert!(artifact.functions[0].is_initializer);
        assert_eq!(artifact.functions[1].name, "entrypoint");
    }

    #[tokio::test]
    async fn initialization_spec_contains_public_key() {
        let contract = SchnorrAccountContract::new(Fr::from(12345u64));
        let spec = contract
            .initialization_function_and_args()
            .await
            .expect("init spec")
            .expect("should have spec");
        assert_eq!(spec.constructor_name, "constructor");
        assert_eq!(spec.constructor_args.len(), 1);
    }

    #[tokio::test]
    async fn auth_provider_creates_real_signature() {
        let contract = SchnorrAccountContract::new(Fr::from(12345u64));
        let addr = CompleteAddress::default();
        let provider = contract.auth_witness_provider(addr);
        let chain_info = sample_chain_info();

        let wit = provider
            .create_auth_wit(
                MessageHashOrIntent::Hash {
                    hash: Fr::from(42u64),
                },
                &chain_info,
            )
            .await
            .expect("create auth wit");

        // Real Schnorr signature: 64 fields (one per byte)
        assert_eq!(wit.fields.len(), 64);
        // request_hash should match input hash (Hash variant is passthrough)
        assert_eq!(wit.request_hash, Fr::from(42u64));
    }

    #[tokio::test]
    async fn signature_is_verifiable() {
        let secret = Fr::from(8923u64);
        let contract = SchnorrAccountContract::new(secret);
        let addr = CompleteAddress::default();
        let provider = contract.auth_witness_provider(addr);
        let chain_info = sample_chain_info();

        let message = Fr::from(999u64);
        let wit = provider
            .create_auth_wit(
                MessageHashOrIntent::Hash { hash: message },
                &chain_info,
            )
            .await
            .expect("create auth wit");

        // Reconstruct the signature bytes from fields
        let sig_bytes: Vec<u8> = wit.fields.iter().map(|f| f.to_usize() as u8).collect();
        let mut sig_arr = [0u8; 64];
        sig_arr.copy_from_slice(&sig_bytes);
        let sig = aztec_crypto::schnorr::SchnorrSignature::from_bytes(&sig_arr);

        // Verify against the public key
        let pk = contract.signing_public_key();
        assert!(aztec_crypto::schnorr::schnorr_verify(pk, &message, &sig));
    }

    #[tokio::test]
    async fn account_manager_integration() {
        let wallet = MockWallet::new(sample_chain_info());
        let secret = Fr::from(12345u64);
        let contract = SchnorrAccountContract::new(secret);

        let manager = AccountManager::create(wallet, secret, Box::new(contract), None::<Fr>)
            .await
            .expect("create manager");

        assert_ne!(manager.address(), AztecAddress::zero());
        assert!(manager.has_initializer());

        let account = manager.account().await.expect("get account");
        assert_eq!(account.address(), manager.address());
    }

    #[tokio::test]
    async fn deploy_method_builds_payload() {
        let wallet = MockWallet::new(sample_chain_info());
        let secret = Fr::from(12345u64);
        let contract = SchnorrAccountContract::new(secret);

        let manager = AccountManager::create(wallet, secret, Box::new(contract), None::<Fr>)
            .await
            .expect("create manager");

        let deploy = manager.deploy_method().await.expect("deploy method");
        let opts = crate::account::DeployAccountOptions {
            skip_registration: true,
            ..Default::default()
        };
        let payload = deploy.request(&opts).await.expect("deploy payload");
        assert!(!payload.calls.is_empty());
    }
}
