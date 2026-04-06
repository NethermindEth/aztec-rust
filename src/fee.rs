use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gas {
    pub da_gas: u64,
    pub l2_gas: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GasFees {
    pub fee_per_da_gas: u128,
    pub fee_per_l2_gas: u128,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GasSettings {
    pub gas_limits: Option<Gas>,
    pub teardown_gas_limits: Option<Gas>,
    pub max_fee_per_gas: Option<GasFees>,
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
