//! Entrypoint call encoding for account and multi-call entrypoints.
//!
//! Ports the `EncodedAppEntrypointCalls` logic from the upstream TS
//! `yarn-project/entrypoints/src/encoding.ts`.

use aztec_core::abi::FunctionType;
use aztec_core::constants::domain_separator;
use aztec_core::hash::{abi_values_to_fields, poseidon2_hash_with_separator};
use aztec_core::tx::{FunctionCall, HashedValues};
use aztec_core::types::Fr;
use aztec_core::Error;

/// Maximum number of function calls in a single entrypoint payload.
pub const APP_MAX_CALLS: usize = 5;

/// A single encoded function call within an entrypoint payload.
#[derive(Clone, Debug)]
struct EncodedCall {
    args_hash: Fr,
    function_selector: Fr,
    target_address: Fr,
    is_public: bool,
    hide_msg_sender: bool,
    is_static: bool,
}

/// Borrowed view of an encoded function call.
#[derive(Clone, Copy, Debug)]
pub struct EncodedCallView {
    /// Arguments hash for the call.
    pub args_hash: Fr,
    /// Function selector as a field.
    pub function_selector: Fr,
    /// Target address as a field.
    pub target_address: Fr,
    /// Whether the call is public.
    pub is_public: bool,
    /// Whether the msg sender is hidden.
    pub hide_msg_sender: bool,
    /// Whether the call is static.
    pub is_static: bool,
}

impl EncodedCall {
    /// Serialize this encoded call to field elements.
    fn to_fields(&self) -> Vec<Fr> {
        vec![
            self.args_hash,
            self.function_selector,
            self.target_address,
            Fr::from(self.is_public),
            Fr::from(self.hide_msg_sender),
            Fr::from(self.is_static),
        ]
    }
}

/// Encoded entrypoint calls ready for hashing and field serialization.
pub struct EncodedAppEntrypointCalls {
    encoded_calls: Vec<EncodedCall>,
    tx_nonce: Fr,
    hashed_args_list: Vec<HashedValues>,
}

impl EncodedAppEntrypointCalls {
    /// Encode function calls for passing to an account entrypoint.
    ///
    /// Pads to `APP_MAX_CALLS` with empty calls.
    pub fn create(calls: &[FunctionCall], tx_nonce: Option<Fr>) -> Result<Self, Error> {
        if calls.len() > APP_MAX_CALLS {
            return Err(Error::InvalidData(format!(
                "Too many calls: {} > {}",
                calls.len(),
                APP_MAX_CALLS
            )));
        }

        let tx_nonce = tx_nonce.unwrap_or_else(Fr::random);
        let mut encoded_calls = Vec::with_capacity(APP_MAX_CALLS);
        let mut hashed_args_list = Vec::with_capacity(APP_MAX_CALLS);
        let padded_calls = calls
            .iter()
            .cloned()
            .chain(std::iter::repeat_with(FunctionCall::empty).take(APP_MAX_CALLS - calls.len()));

        for call in padded_calls {
            let is_public = call.function_type == FunctionType::Public;
            let arg_fields = abi_values_to_fields(&call.args);

            let (args_hash, hashed_values) = if is_public {
                let mut calldata = vec![call.selector.to_field()];
                calldata.extend_from_slice(&arg_fields);
                let hv = HashedValues::from_calldata(calldata);
                let h = hv.hash();
                (h, hv)
            } else {
                let hv = HashedValues::from_args(arg_fields);
                let h = hv.hash();
                (h, hv)
            };

            hashed_args_list.push(hashed_values);

            encoded_calls.push(EncodedCall {
                args_hash,
                function_selector: call.selector.to_field(),
                target_address: call.to.0,
                is_public,
                hide_msg_sender: call.hide_msg_sender,
                is_static: call.is_static,
            });
        }

        Ok(Self {
            encoded_calls,
            tx_nonce,
            hashed_args_list,
        })
    }

    /// Serialize the full payload to field elements for ABI encoding.
    ///
    /// Layout: [call_0_fields..., call_1_fields..., ..., call_N_fields..., tx_nonce]
    pub fn to_fields(&self) -> Vec<Fr> {
        let mut fields = Vec::new();
        for call in &self.encoded_calls {
            fields.extend(call.to_fields());
        }
        fields.push(self.tx_nonce);
        fields
    }

    /// Hash the payload using Poseidon2 with `SIGNATURE_PAYLOAD` separator.
    pub fn hash(&self) -> Fr {
        let fields = self.to_fields();
        poseidon2_hash_with_separator(&fields, domain_separator::SIGNATURE_PAYLOAD)
    }

    /// Return the hashed arguments for oracle access.
    pub fn hashed_args(&self) -> &[HashedValues] {
        &self.hashed_args_list
    }

    /// Return encoded calls for ABI construction and inspection.
    pub fn encoded_calls(&self) -> Vec<EncodedCallView> {
        self.encoded_calls
            .iter()
            .map(|call| EncodedCallView {
                args_hash: call.args_hash,
                function_selector: call.function_selector,
                target_address: call.target_address,
                is_public: call.is_public,
                hide_msg_sender: call.hide_msg_sender,
                is_static: call.is_static,
            })
            .collect()
    }

    /// Return the tx nonce used for this encoding.
    pub fn tx_nonce(&self) -> Fr {
        self.tx_nonce
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use aztec_core::abi::{AbiValue, FunctionSelector, FunctionType};
    use aztec_core::types::AztecAddress;

    fn make_private_call(addr: u64, selector_hex: &str) -> FunctionCall {
        FunctionCall {
            to: AztecAddress::from(addr),
            selector: FunctionSelector::from_hex(selector_hex).expect("valid"),
            args: vec![AbiValue::Field(Fr::from(42u64))],
            function_type: FunctionType::Private,
            is_static: false,
            hide_msg_sender: false,
        }
    }

    fn make_public_call(addr: u64, selector_hex: &str) -> FunctionCall {
        FunctionCall {
            to: AztecAddress::from(addr),
            selector: FunctionSelector::from_hex(selector_hex).expect("valid"),
            args: vec![AbiValue::Field(Fr::from(99u64))],
            function_type: FunctionType::Public,
            is_static: false,
            hide_msg_sender: false,
        }
    }

    #[test]
    fn encode_single_private_call() {
        let call = make_private_call(1, "0x11223344");
        let encoded =
            EncodedAppEntrypointCalls::create(&[call], Some(Fr::from(1u64))).expect("encode");

        let fields = encoded.to_fields();
        // 5 calls * 6 fields each + 1 nonce = 31 fields
        assert_eq!(fields.len(), 31);
        // First call should have non-zero args_hash
        assert_ne!(fields[0], Fr::zero());
        // Nonce should be last
        assert_eq!(*fields.last().unwrap(), Fr::from(1u64));
    }

    #[test]
    fn encode_pads_to_max_calls() {
        let call = make_private_call(1, "0x11223344");
        let encoded =
            EncodedAppEntrypointCalls::create(&[call], Some(Fr::from(1u64))).expect("encode");

        assert_eq!(encoded.encoded_calls.len(), APP_MAX_CALLS);
        // Calls 2-5 should be empty (zero target address)
        for i in 1..APP_MAX_CALLS {
            assert_eq!(encoded.encoded_calls[i].target_address, Fr::zero());
        }
    }

    #[test]
    fn encode_multiple_calls() {
        let calls = vec![
            make_private_call(1, "0x11111111"),
            make_public_call(2, "0x22222222"),
        ];
        let encoded =
            EncodedAppEntrypointCalls::create(&calls, Some(Fr::from(1u64))).expect("encode");

        assert_eq!(encoded.hashed_args().len(), APP_MAX_CALLS);
        // Second call should be public
        assert!(encoded.encoded_calls[1].is_public);
    }

    #[test]
    fn encode_rejects_too_many_calls() {
        let calls: Vec<_> = (0..6)
            .map(|i| make_private_call(i + 1, "0x11223344"))
            .collect();
        let result = EncodedAppEntrypointCalls::create(&calls, None);
        assert!(result.is_err());
    }

    #[test]
    fn hash_is_deterministic() {
        let call = make_private_call(1, "0x11223344");
        let nonce = Fr::from(42u64);

        let h1 = EncodedAppEntrypointCalls::create(&[call.clone()], Some(nonce))
            .expect("encode")
            .hash();
        let h2 = EncodedAppEntrypointCalls::create(&[call], Some(nonce))
            .expect("encode")
            .hash();

        assert_eq!(h1, h2);
    }

    #[test]
    fn different_nonce_different_hash() {
        let call = make_private_call(1, "0x11223344");

        let h1 = EncodedAppEntrypointCalls::create(&[call.clone()], Some(Fr::from(1u64)))
            .expect("encode")
            .hash();
        let h2 = EncodedAppEntrypointCalls::create(&[call], Some(Fr::from(2u64)))
            .expect("encode")
            .hash();

        assert_ne!(h1, h2);
    }

    #[test]
    fn encode_mix_of_public_and_private() {
        let calls = vec![
            make_private_call(1, "0x11111111"),
            make_public_call(2, "0x22222222"),
            make_private_call(3, "0x33333333"),
        ];
        let encoded =
            EncodedAppEntrypointCalls::create(&calls, Some(Fr::from(1u64))).expect("encode");

        assert!(!encoded.encoded_calls[0].is_public);
        assert!(encoded.encoded_calls[1].is_public);
        assert!(!encoded.encoded_calls[2].is_public);
        assert_eq!(encoded.hashed_args().len(), APP_MAX_CALLS);
    }
}
