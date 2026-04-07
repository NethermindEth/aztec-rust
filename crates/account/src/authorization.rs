//! Authorization request types for authwit scenarios.
//!
//! Provides [`CallAuthorizationRequest`] which captures the full preimage
//! of an authorization witness.

use crate::abi::{AuthorizationSelector, FunctionSelector};
use crate::error::Error;
use crate::types::{AztecAddress, Fr};

/// An authorization request for a function call, including the full preimage
/// of the data to be signed.
///
/// Mirrors TS `CallAuthorizationRequest`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallAuthorizationRequest {
    /// The selector identifying the authwit request type.
    pub selector: AuthorizationSelector,
    /// The inner hash of the authwit (poseidon2([msg_sender, selector, args_hash])).
    pub inner_hash: Fr,
    /// The address performing the call (msg_sender).
    pub msg_sender: AztecAddress,
    /// The selector of the function being authorized.
    pub function_selector: FunctionSelector,
    /// The hash of the function arguments.
    pub args_hash: Fr,
    /// The raw function arguments as field elements.
    pub args: Vec<Fr>,
}

impl CallAuthorizationRequest {
    /// Construct a new `CallAuthorizationRequest`.
    pub fn new(
        selector: AuthorizationSelector,
        inner_hash: Fr,
        msg_sender: AztecAddress,
        function_selector: FunctionSelector,
        args_hash: Fr,
        args: Vec<Fr>,
    ) -> Self {
        Self {
            selector,
            inner_hash,
            msg_sender,
            function_selector,
            args_hash,
            args,
        }
    }

    /// The selector used by upstream Aztec for `CallAuthorizationRequest`.
    pub fn selector() -> AuthorizationSelector {
        AuthorizationSelector::from_signature("CallAuthorization((Field),(u32),Field)")
    }

    /// Construct from field elements (deserialization from on-chain data).
    ///
    /// Expected layout:
    /// `[selector, inner_hash, msg_sender, function_selector, args_hash, ...args]`
    pub fn from_fields(fields: &[Fr]) -> Result<Self, Error> {
        if fields.len() < 5 {
            return Err(Error::InvalidData(
                "CallAuthorizationRequest requires at least 5 fields".to_owned(),
            ));
        }

        let selector = AuthorizationSelector::from_field(fields[0]);
        let expected_selector = Self::selector();
        if selector != expected_selector {
            return Err(Error::InvalidData(format!(
                "invalid authorization selector for CallAuthorizationRequest: expected {expected_selector}, got {selector}",
            )));
        }

        let inner_hash = fields[1];
        let msg_sender = AztecAddress(fields[2]);
        let function_selector = FunctionSelector::from_field(fields[3]);
        let args_hash = fields[4];
        let args = fields[5..].to_vec();

        Ok(Self {
            selector,
            inner_hash,
            msg_sender,
            function_selector,
            args_hash,
            args,
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn new_and_fields_accessible() {
        let req = CallAuthorizationRequest::new(
            CallAuthorizationRequest::selector(),
            Fr::from(1u64),
            AztecAddress(Fr::from(2u64)),
            FunctionSelector::from_hex("0xaabbccdd").expect("valid"),
            Fr::from(3u64),
            vec![Fr::from(4u64), Fr::from(5u64)],
        );

        assert_eq!(req.selector, CallAuthorizationRequest::selector());
        assert_eq!(req.inner_hash, Fr::from(1u64));
        assert_eq!(req.msg_sender, AztecAddress(Fr::from(2u64)));
        assert_eq!(
            req.function_selector,
            FunctionSelector::from_hex("0xaabbccdd").expect("valid")
        );
        assert_eq!(req.args_hash, Fr::from(3u64));
        assert_eq!(req.args.len(), 2);
    }

    #[test]
    fn from_fields_roundtrip() {
        let selector_val = CallAuthorizationRequest::selector().to_field();
        let fields = vec![
            selector_val,            // auth request selector
            Fr::from(1u64),          // inner_hash
            Fr::from(2u64),          // msg_sender
            Fr::from(0xAABBCCDDu64), // function selector
            Fr::from(3u64),          // args_hash
            Fr::from(4u64),          // arg0
            Fr::from(5u64),          // arg1
        ];

        let req = CallAuthorizationRequest::from_fields(&fields).expect("valid fields");
        assert_eq!(req.selector, CallAuthorizationRequest::selector());
        assert_eq!(req.inner_hash, Fr::from(1u64));
        assert_eq!(req.msg_sender, AztecAddress(Fr::from(2u64)));
        assert_eq!(
            req.function_selector,
            FunctionSelector::from_hex("0xaabbccdd").expect("valid")
        );
        assert_eq!(req.args_hash, Fr::from(3u64));
        assert_eq!(req.args, vec![Fr::from(4u64), Fr::from(5u64)]);
    }

    #[test]
    fn from_fields_too_short() {
        let fields = vec![Fr::from(1u64), Fr::from(2u64)];
        assert!(CallAuthorizationRequest::from_fields(&fields).is_err());
    }

    #[test]
    fn from_fields_minimum_length() {
        let fields = vec![
            CallAuthorizationRequest::selector().to_field(),
            Fr::from(1u64),
            Fr::from(2u64),
            Fr::from(0u64),
            Fr::from(3u64),
        ];
        let req = CallAuthorizationRequest::from_fields(&fields).expect("valid fields");
        assert!(req.args.is_empty());
    }

    #[test]
    fn from_fields_rejects_wrong_selector() {
        let fields = vec![
            Fr::from(0u64),
            Fr::from(1u64),
            Fr::from(2u64),
            Fr::from(0xAABBCCDDu64),
            Fr::from(3u64),
        ];
        assert!(CallAuthorizationRequest::from_fields(&fields).is_err());
    }
}
