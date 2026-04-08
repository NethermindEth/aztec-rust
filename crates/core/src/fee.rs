use serde::{Deserialize, Serialize};

/// Gas consumption broken down by DA and L2 components.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gas {
    /// Data availability gas consumed.
    pub da_gas: u64,
    /// L2 execution gas consumed.
    pub l2_gas: u64,
}

/// Per-unit gas fee prices.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GasFees {
    /// Fee per unit of DA gas.
    pub fee_per_da_gas: u128,
    /// Fee per unit of L2 gas.
    pub fee_per_l2_gas: u128,
}

/// Gas limits and fee caps for a transaction.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GasSettings {
    /// Maximum gas allowed for the main execution phase.
    pub gas_limits: Option<Gas>,
    /// Maximum gas allowed for the teardown phase.
    pub teardown_gas_limits: Option<Gas>,
    /// Maximum fee per gas unit the sender is willing to pay.
    pub max_fee_per_gas: Option<GasFees>,
    /// Maximum priority fee per gas unit (tip).
    pub max_priority_fee_per_gas: Option<GasFees>,
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn gas_settings_default_is_empty() {
        let settings = GasSettings::default();
        assert_eq!(settings.gas_limits, None);
        assert_eq!(settings.teardown_gas_limits, None);
        assert_eq!(settings.max_fee_per_gas, None);
        assert_eq!(settings.max_priority_fee_per_gas, None);
    }

    #[test]
    fn gas_settings_roundtrip() {
        let settings = GasSettings {
            gas_limits: Some(Gas {
                da_gas: 1,
                l2_gas: 2,
            }),
            teardown_gas_limits: Some(Gas {
                da_gas: 3,
                l2_gas: 4,
            }),
            max_fee_per_gas: Some(GasFees {
                fee_per_da_gas: 5,
                fee_per_l2_gas: 6,
            }),
            max_priority_fee_per_gas: None,
        };

        let json = match serde_json::to_string(&settings) {
            Ok(json) => json,
            Err(err) => panic!("serializing GasSettings should succeed: {err}"),
        };
        let decoded: GasSettings = match serde_json::from_str(&json) {
            Ok(decoded) => decoded,
            Err(err) => panic!("deserializing GasSettings should succeed: {err}"),
        };
        assert_eq!(decoded, settings);
    }
}
