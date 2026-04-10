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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

impl Default for GasSettings {
    /// Returns sensible defaults matching the TS `GasSettings.default()`.
    fn default() -> Self {
        use crate::constants::*;
        Self {
            gas_limits: Some(Gas {
                da_gas: DEFAULT_DA_GAS_LIMIT,
                l2_gas: DEFAULT_L2_GAS_LIMIT,
            }),
            teardown_gas_limits: Some(Gas {
                da_gas: DEFAULT_TEARDOWN_DA_GAS_LIMIT,
                l2_gas: DEFAULT_TEARDOWN_L2_GAS_LIMIT,
            }),
            max_fee_per_gas: Some(GasFees {
                fee_per_da_gas: 1,
                fee_per_l2_gas: 1,
            }),
            max_priority_fee_per_gas: Some(GasFees::default()),
        }
    }
}

impl Gas {
    pub fn new(da_gas: u64, l2_gas: u64) -> Self {
        Self { da_gas, l2_gas }
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn add(&self, other: &Gas) -> Gas {
        Gas {
            da_gas: self.da_gas + other.da_gas,
            l2_gas: self.l2_gas + other.l2_gas,
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn gas_settings_default_has_sensible_values() {
        let settings = GasSettings::default();
        let gl = settings.gas_limits.unwrap();
        assert_eq!(gl.da_gas, crate::constants::DEFAULT_DA_GAS_LIMIT);
        assert_eq!(gl.l2_gas, crate::constants::DEFAULT_L2_GAS_LIMIT);
        assert!(settings.teardown_gas_limits.is_some());
        assert!(settings.max_fee_per_gas.is_some());
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
