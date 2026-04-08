use serde::{Deserialize, Serialize};

use crate::types::Fr;

/// Describes the storage slot layout for a single contract storage field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldLayout {
    /// Slot number in the contract's storage tree.
    pub slot: Fr,
}

/// Maps storage field names to their slot layout descriptors.
///
/// Used for off-chain reads and storage proofs.
pub type ContractStorageLayout = std::collections::BTreeMap<String, FieldLayout>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn field_layout_roundtrip() {
        let layout = FieldLayout {
            slot: Fr::from(42u64),
        };
        let json = serde_json::to_string(&layout).unwrap();
        let decoded: FieldLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, layout);
    }

    #[test]
    fn contract_storage_layout_insert_and_lookup() {
        let mut storage = ContractStorageLayout::new();
        storage.insert(
            "balances".to_owned(),
            FieldLayout {
                slot: Fr::from(1u64),
            },
        );
        storage.insert(
            "total_supply".to_owned(),
            FieldLayout {
                slot: Fr::from(2u64),
            },
        );

        assert_eq!(storage.len(), 2);
        assert_eq!(storage["balances"].slot, Fr::from(1u64));
        assert_eq!(storage["total_supply"].slot, Fr::from(2u64));
    }
}
