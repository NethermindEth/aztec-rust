//! L1↔L2 messaging types and utilities.
//!
//! Mirrors upstream:
//! - `stdlib/src/messaging/l1_actor.ts`
//! - `stdlib/src/messaging/l2_actor.ts`
//! - `stdlib/src/messaging/l1_to_l2_message.ts`

use aztec_core::types::{AztecAddress, EthAddress, Fr};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// L1Actor — sender on Ethereum
// ---------------------------------------------------------------------------

/// An actor on L1 (Ethereum), identified by an Eth address and chain ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct L1Actor {
    /// Ethereum address of the sender.
    pub sender: EthAddress,
    /// L1 chain ID.
    pub chain_id: u64,
}

impl L1Actor {
    pub fn new(sender: EthAddress, chain_id: u64) -> Self {
        Self { sender, chain_id }
    }

    pub fn empty() -> Self {
        Self {
            sender: EthAddress::default(),
            chain_id: 0,
        }
    }

    /// Serialize to field elements: `[sender_as_field, chain_id_as_field]`.
    pub fn to_fields(&self) -> [Fr; 2] {
        let sender_fr = {
            let mut bytes = [0u8; 32];
            bytes[12..32].copy_from_slice(&self.sender.0);
            Fr::from(bytes)
        };
        [sender_fr, Fr::from(self.chain_id)]
    }
}

// ---------------------------------------------------------------------------
// L2Actor — recipient on Aztec
// ---------------------------------------------------------------------------

/// An actor on L2 (Aztec), identified by an Aztec address and protocol version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2Actor {
    /// Aztec address of the recipient.
    pub recipient: AztecAddress,
    /// Protocol version.
    pub version: u64,
}

impl L2Actor {
    pub fn new(recipient: AztecAddress, version: u64) -> Self {
        Self { recipient, version }
    }

    pub fn empty() -> Self {
        Self {
            recipient: AztecAddress::zero(),
            version: 0,
        }
    }

    /// Serialize to field elements: `[recipient_as_field, version_as_field]`.
    pub fn to_fields(&self) -> [Fr; 2] {
        [Fr::from(self.recipient), Fr::from(self.version)]
    }
}

// ---------------------------------------------------------------------------
// L1ToL2Message
// ---------------------------------------------------------------------------

/// A message sent from L1 to L2 via the Inbox contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct L1ToL2Message {
    /// The sender on L1.
    pub sender: L1Actor,
    /// The recipient on L2.
    pub recipient: L2Actor,
    /// Message content (application-specific payload).
    pub content: Fr,
    /// Hash of the secret needed to consume this message.
    pub secret_hash: Fr,
    /// Global index in the L1-to-L2 message tree.
    pub index: Fr,
}

impl L1ToL2Message {
    pub fn new(
        sender: L1Actor,
        recipient: L2Actor,
        content: Fr,
        secret_hash: Fr,
        index: Fr,
    ) -> Self {
        Self {
            sender,
            recipient,
            content,
            secret_hash,
            index,
        }
    }

    pub fn empty() -> Self {
        Self {
            sender: L1Actor::empty(),
            recipient: L2Actor::empty(),
            content: Fr::zero(),
            secret_hash: Fr::zero(),
            index: Fr::zero(),
        }
    }

    /// Serialize to field elements (6 total).
    pub fn to_fields(&self) -> Vec<Fr> {
        let s = self.sender.to_fields();
        let r = self.recipient.to_fields();
        vec![s[0], s[1], r[0], r[1], self.content, self.secret_hash]
    }

    /// Compute the message hash: `sha256_to_field(to_fields())`.
    pub fn hash(&self) -> Fr {
        let fields = self.to_fields();
        let mut data = Vec::with_capacity(fields.len() * 32);
        for f in &fields {
            data.extend_from_slice(&f.to_be_bytes());
        }
        aztec_core::hash::sha256_to_field_pub(&data)
    }
}

// ---------------------------------------------------------------------------
// Claim types
// ---------------------------------------------------------------------------

/// Information needed to claim tokens bridged from L1 to L2.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2Claim {
    /// Random secret for claiming.
    pub claim_secret: Fr,
    /// `poseidon2([secret], SECRET_HASH)`.
    pub claim_secret_hash: Fr,
    /// Keccak256 hash of the L1 message (from Inbox event).
    pub message_hash: Fr,
    /// Index in the L1-to-L2 message tree.
    pub message_leaf_index: u64,
}

/// Claim information including the bridged amount.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2AmountClaim {
    /// Base claim data.
    #[serde(flatten)]
    pub claim: L2Claim,
    /// Amount of tokens bridged.
    pub claim_amount: u128,
}

/// Claim information including amount and recipient.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2AmountClaimWithRecipient {
    /// Amount claim data.
    #[serde(flatten)]
    pub amount_claim: L2AmountClaim,
    /// L2 recipient address.
    pub recipient: AztecAddress,
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Generate a random claim secret and its hash.
///
/// Returns `(secret, secret_hash)` where
/// `secret_hash = poseidon2([secret], SECRET_HASH)`.
///
/// Mirrors TS `generateClaimSecret()`.
pub fn generate_claim_secret() -> (Fr, Fr) {
    let secret = Fr::random();
    let hash = aztec_core::hash::compute_secret_hash(&secret);
    (secret, hash)
}
