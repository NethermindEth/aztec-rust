//! Field element conversion between aztec-core Fr and ACVM FieldElement.
//!
//! Both wrap `ark_bn254::Fr` internally, so conversion is essentially
//! copying the inner bytes.

use acir::native_types::{Witness, WitnessMap};
use acir::{AcirField, FieldElement};
use aztec_core::types::Fr;
use std::collections::BTreeSet;

/// Convert an ACVM FieldElement to our Fr type.
pub fn fe_to_fr(fe: &FieldElement) -> Fr {
    let hex = fe.to_hex();
    Fr::from_hex(&format!("0x{hex}")).unwrap_or_else(|_| Fr::zero())
}

/// Convert our Fr type to an ACVM FieldElement.
pub fn fr_to_fe(fr: &Fr) -> FieldElement {
    let bytes = fr.to_be_bytes();
    FieldElement::from_be_bytes_reduce(&bytes)
}

/// Extract ordered field values from a witness map for the given return witnesses.
pub fn witness_map_to_frs(
    witness: &WitnessMap<FieldElement>,
    return_witnesses: &BTreeSet<Witness>,
) -> Vec<Fr> {
    return_witnesses
        .iter()
        .filter_map(|w| witness.get(w).map(fe_to_fr))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_conversion() {
        let original = Fr::from(42u64);
        let fe = fr_to_fe(&original);
        let back = fe_to_fr(&fe);
        assert_eq!(original, back);
    }

    #[test]
    fn zero_roundtrip() {
        let zero = Fr::zero();
        let fe = fr_to_fe(&zero);
        let back = fe_to_fr(&fe);
        assert_eq!(zero, back);
    }

    #[test]
    fn large_value_roundtrip() {
        let large = Fr::from(u64::MAX);
        let fe = fr_to_fe(&large);
        let back = fe_to_fr(&fe);
        assert_eq!(large, back);
    }
}
