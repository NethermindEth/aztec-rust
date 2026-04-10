//! Private event filter validation and sanitization.
//!
//! Ports the TS `PrivateEventFilterValidator` which ensures filter
//! parameters are valid relative to the current anchor block.

use aztec_core::error::Error;
use aztec_core::types::AztecAddress;

use crate::stores::private_event_store::PrivateEventQueryFilter;

use aztec_pxe_client::PrivateEventFilter;

/// Validates and sanitizes private event filters.
///
/// Ensures block ranges are valid and within the anchor block boundary.
pub struct PrivateEventFilterValidator {
    /// The current anchor block number.
    anchor_block_number: u64,
}

impl PrivateEventFilterValidator {
    pub fn new(anchor_block_number: u64) -> Self {
        Self {
            anchor_block_number,
        }
    }

    /// Validate and convert a `PrivateEventFilter` to a `PrivateEventQueryFilter`.
    ///
    /// Validation rules (matching upstream TS implementation):
    /// - At least one scope is required
    /// - `from_block` defaults to 1 (genesis), clamped to at least 1
    /// - `to_block` defaults to anchor_block_number + 1 (exclusive upper bound)
    /// - `to_block` must not exceed anchor_block_number + 1 (rejects, not clamps)
    /// - `from_block` must be less than `to_block`
    pub fn validate(&self, filter: &PrivateEventFilter) -> Result<PrivateEventQueryFilter, Error> {
        // Upstream rejects empty scopes: "At least one scope is required to get private events"
        if filter.scopes.is_empty() {
            return Err(Error::InvalidData(
                "at least one scope is required to get private events".into(),
            ));
        }

        let from_block = filter.from_block.unwrap_or(1).max(1);

        // to_block is an exclusive upper bound; default to anchor + 1
        let to_block = filter.to_block.unwrap_or(self.anchor_block_number + 1);

        // Validate rather than clamp: if caller requests beyond anchor, reject
        if to_block > self.anchor_block_number + 1 {
            return Err(Error::InvalidData(format!(
                "to_block ({to_block}) exceeds anchor block + 1 ({})",
                self.anchor_block_number + 1
            )));
        }

        if from_block >= to_block {
            return Err(Error::InvalidData(format!(
                "invalid block range: from_block={from_block} >= to_block={to_block}"
            )));
        }

        // Convert exclusive to_block to inclusive for our store
        let inclusive_to_block = to_block - 1;

        Ok(PrivateEventQueryFilter {
            contract_address: filter.contract_address,
            from_block: Some(from_block),
            to_block: Some(inclusive_to_block),
            scopes: filter.scopes.clone(),
            tx_hash: filter.tx_hash,
        })
    }
}

/// Convert a `PrivateEventFilter` to `PrivateEventQueryFilter` without anchor
/// block validation (for testing or when anchor is unknown).
pub fn to_query_filter_unchecked(filter: &PrivateEventFilter) -> PrivateEventQueryFilter {
    PrivateEventQueryFilter {
        contract_address: filter.contract_address,
        from_block: filter.from_block,
        to_block: filter.to_block,
        scopes: filter.scopes.clone(),
        tx_hash: filter.tx_hash,
    }
}

/// Create a default "all events" query filter for a contract.
pub fn all_events_filter(contract_address: AztecAddress) -> PrivateEventQueryFilter {
    PrivateEventQueryFilter {
        contract_address,
        from_block: None,
        to_block: None,
        scopes: vec![],
        tx_hash: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_filter(from: Option<u64>, to: Option<u64>) -> PrivateEventFilter {
        PrivateEventFilter {
            contract_address: AztecAddress::from(1u64),
            from_block: from,
            to_block: to,
            tx_hash: None,
            after_log: None,
            scopes: vec![AztecAddress::from(99u64)], // at least one scope required
        }
    }

    #[test]
    fn defaults_block_range_to_full() {
        let validator = PrivateEventFilterValidator::new(10);
        let result = validator.validate(&make_filter(None, None)).unwrap();
        assert_eq!(result.from_block, Some(1));
        assert_eq!(result.to_block, Some(10)); // inclusive = exclusive - 1
    }

    #[test]
    fn rejects_to_block_beyond_anchor() {
        let validator = PrivateEventFilterValidator::new(5);
        let result = validator.validate(&make_filter(Some(1), Some(100)));
        assert!(result.is_err(), "to_block beyond anchor should be rejected");
    }

    #[test]
    fn clamps_from_block_to_at_least_1() {
        let validator = PrivateEventFilterValidator::new(10);
        let result = validator.validate(&make_filter(Some(0), None)).unwrap();
        assert_eq!(result.from_block, Some(1));
    }

    #[test]
    fn rejects_invalid_range() {
        let validator = PrivateEventFilterValidator::new(5);
        // from=10, to defaults to 6 (anchor+1), so 10 >= 6 -> error
        let result = validator.validate(&make_filter(Some(10), None));
        assert!(result.is_err());
    }

    #[test]
    fn rejects_empty_scopes() {
        let validator = PrivateEventFilterValidator::new(10);
        let filter = PrivateEventFilter {
            contract_address: AztecAddress::from(1u64),
            from_block: None,
            to_block: None,
            tx_hash: None,
            after_log: None,
            scopes: vec![],
        };
        let result = validator.validate(&filter);
        assert!(result.is_err(), "empty scopes should be rejected");
    }

    #[test]
    fn preserves_scopes() {
        let validator = PrivateEventFilterValidator::new(10);
        let mut filter = make_filter(None, None);
        filter.scopes = vec![AztecAddress::from(42u64)];
        let result = validator.validate(&filter).unwrap();
        assert_eq!(result.scopes.len(), 1);
    }

    #[test]
    fn to_query_filter_unchecked_passes_through() {
        let filter = PrivateEventFilter {
            contract_address: AztecAddress::from(1u64),
            from_block: Some(3),
            to_block: Some(7),
            tx_hash: None,
            after_log: None,
            scopes: vec![AztecAddress::from(2u64)],
        };
        let query = to_query_filter_unchecked(&filter);
        assert_eq!(query.from_block, Some(3));
        assert_eq!(query.to_block, Some(7));
        assert_eq!(query.scopes.len(), 1);
    }
}
