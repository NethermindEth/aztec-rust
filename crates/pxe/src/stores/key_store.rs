//! Key storage with master key derivation.

use std::sync::Arc;

use aztec_core::error::Error;
use aztec_core::types::{AztecAddress, Fr, GrumpkinScalar, PublicKeys};
use aztec_crypto::keys::{
    compute_app_nullifier_hiding_key, compute_app_secret_key, compute_ovsk_app, derive_keys,
    DerivedKeys, KeyType,
};

use super::kv::KvStore;

/// Stores master secret keys and derives app-scoped keys on demand.
pub struct KeyStore {
    kv: Arc<dyn KvStore>,
}

impl KeyStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    /// Add an account by storing its secret key, indexed by the public keys hash.
    pub async fn add_account(&self, secret_key: &Fr) -> Result<DerivedKeys, Error> {
        let derived = derive_keys(secret_key);
        let pk_hash = derived.public_keys.hash();
        let key = account_key(&pk_hash);
        let value = secret_key.to_be_bytes();
        self.kv.put(&key, &value).await?;
        Ok(derived)
    }

    /// Get the secret key for an account identified by its public keys hash.
    pub async fn get_secret_key(&self, pk_hash: &Fr) -> Result<Option<Fr>, Error> {
        let key = account_key(pk_hash);
        match self.kv.get(&key).await? {
            Some(bytes) => {
                let mut arr = [0u8; 32];
                if bytes.len() == 32 {
                    arr.copy_from_slice(&bytes);
                    Ok(Some(Fr::from(arr)))
                } else {
                    Err(Error::InvalidData("invalid secret key length".into()))
                }
            }
            None => Ok(None),
        }
    }

    /// Get the master nullifier hiding key for an account.
    pub async fn get_master_nullifier_hiding_key(
        &self,
        pk_hash: &Fr,
    ) -> Result<Option<GrumpkinScalar>, Error> {
        match self.get_secret_key(pk_hash).await? {
            Some(sk) => Ok(Some(derive_keys(&sk).master_nullifier_hiding_key)),
            None => Ok(None),
        }
    }

    /// Get the master incoming viewing secret key for an account.
    pub async fn get_master_incoming_viewing_secret_key(
        &self,
        pk_hash: &Fr,
    ) -> Result<Option<GrumpkinScalar>, Error> {
        match self.get_secret_key(pk_hash).await? {
            Some(sk) => Ok(Some(derive_keys(&sk).master_incoming_viewing_secret_key)),
            None => Ok(None),
        }
    }

    /// Get the master outgoing viewing secret key for an account.
    pub async fn get_master_outgoing_viewing_secret_key(
        &self,
        pk_hash: &Fr,
    ) -> Result<Option<GrumpkinScalar>, Error> {
        match self.get_secret_key(pk_hash).await? {
            Some(sk) => Ok(Some(derive_keys(&sk).master_outgoing_viewing_secret_key)),
            None => Ok(None),
        }
    }

    /// Get the master tagging secret key for an account.
    pub async fn get_master_tagging_secret_key(
        &self,
        pk_hash: &Fr,
    ) -> Result<Option<GrumpkinScalar>, Error> {
        match self.get_secret_key(pk_hash).await? {
            Some(sk) => Ok(Some(derive_keys(&sk).master_tagging_secret_key)),
            None => Ok(None),
        }
    }

    /// Get the public keys for an account.
    pub async fn get_public_keys(&self, pk_hash: &Fr) -> Result<Option<PublicKeys>, Error> {
        match self.get_secret_key(pk_hash).await? {
            Some(sk) => Ok(Some(derive_keys(&sk).public_keys)),
            None => Ok(None),
        }
    }

    /// Compute the app-scoped nullifier hiding key.
    pub async fn get_app_nullifier_hiding_key(
        &self,
        pk_hash: &Fr,
        app: &AztecAddress,
    ) -> Result<Option<Fr>, Error> {
        match self.get_master_nullifier_hiding_key(pk_hash).await? {
            Some(nhk_m) => Ok(Some(compute_app_nullifier_hiding_key(&nhk_m, app))),
            None => Ok(None),
        }
    }

    /// Compute an app-scoped secret key for a given key type.
    pub async fn get_app_secret_key(
        &self,
        pk_hash: &Fr,
        app: &AztecAddress,
        key_type: KeyType,
    ) -> Result<Option<Fr>, Error> {
        let sk = match self.get_secret_key(pk_hash).await? {
            Some(sk) => sk,
            None => return Ok(None),
        };
        let derived = derive_keys(&sk);
        let master_key = match key_type {
            KeyType::Nullifier => &derived.master_nullifier_hiding_key,
            KeyType::IncomingViewing => &derived.master_incoming_viewing_secret_key,
            KeyType::OutgoingViewing => &derived.master_outgoing_viewing_secret_key,
            KeyType::Tagging => &derived.master_tagging_secret_key,
        };
        Ok(Some(compute_app_secret_key(master_key, app, key_type)))
    }

    /// Compute the app-scoped outgoing viewing secret key (returns GrumpkinScalar).
    pub async fn get_app_ovsk(
        &self,
        pk_hash: &Fr,
        app: &AztecAddress,
    ) -> Result<Option<GrumpkinScalar>, Error> {
        match self.get_master_outgoing_viewing_secret_key(pk_hash).await? {
            Some(ovsk_m) => Ok(Some(compute_ovsk_app(&ovsk_m, app))),
            None => Ok(None),
        }
    }

    /// List all stored public keys hashes.
    pub async fn get_accounts(&self) -> Result<Vec<Fr>, Error> {
        let entries = self.kv.list_prefix(b"key:account:").await?;
        entries
            .into_iter()
            .map(|(k, _)| {
                let key_str = String::from_utf8_lossy(&k);
                let hex_part = key_str
                    .strip_prefix("key:account:")
                    .ok_or_else(|| Error::InvalidData("invalid key prefix".into()))?;
                Fr::from_hex(hex_part)
            })
            .collect()
    }
}

fn account_key(pk_hash: &Fr) -> Vec<u8> {
    format!("key:account:{pk_hash}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::InMemoryKvStore;

    #[tokio::test]
    async fn add_and_retrieve_account() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = KeyStore::new(kv);
        let sk = Fr::from(8923u64);

        let derived = store.add_account(&sk).await.unwrap();
        let pk_hash = derived.public_keys.hash();

        let retrieved_sk = store.get_secret_key(&pk_hash).await.unwrap().unwrap();
        assert_eq!(retrieved_sk, sk);

        let public_keys = store.get_public_keys(&pk_hash).await.unwrap().unwrap();
        assert_eq!(public_keys.hash(), pk_hash);
    }
}
