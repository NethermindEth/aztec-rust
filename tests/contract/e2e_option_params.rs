//! OptionParam contract tests -- 1:1 mirror of upstream
//! `end-to-end/src/e2e_option_params.test.ts`.
//!
//! Verifies that `Option<SomeStruct>` can be passed as a function argument and
//! received back as a return value via simulate() for public, private, and
//! utility functions, covering the `None` and `Some` branches.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_option_params -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    dead_code,
    unused_imports
)]

use crate::common::*;

// Same MAX_FIELD_VALUE as upstream e2e_abi_types.
const MAX_FIELD_VALUE_HEX: &str =
    "0x30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000000";

const U64_MAX: u64 = u64::MAX;
const I64_MIN: i64 = i64::MIN;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct OptionParamState {
    wallet: TestWallet,
    account: AztecAddress,
    contract_address: AztecAddress,
    artifact: ContractArtifact,
}

static SHARED_STATE: OnceCell<Option<OptionParamState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static OptionParamState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<OptionParamState> {
    let artifact = load_option_param_artifact()?;
    let (wallet, account) = setup_wallet(TEST_ACCOUNT_0).await?;
    let (contract_address, artifact, _instance) =
        deploy_contract(&wallet, artifact, vec![], account).await;
    Some(OptionParamState {
        wallet,
        account,
        contract_address,
        artifact,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build the Noir `Option<SomeStruct>` argument with `_is_some = false`.  When
/// `_is_some` is false the inner struct fields are ignored, so we pass zeros.
fn option_none() -> AbiValue {
    let mut inner = std::collections::BTreeMap::new();
    inner.insert("w".to_owned(), AbiValue::Field(Fr::zero()));
    inner.insert("x".to_owned(), AbiValue::Boolean(false));
    inner.insert("y".to_owned(), AbiValue::Integer(0));
    inner.insert("z".to_owned(), AbiValue::Integer(0));
    let mut outer = std::collections::BTreeMap::new();
    outer.insert("_is_some".to_owned(), AbiValue::Boolean(false));
    outer.insert("_value".to_owned(), AbiValue::Struct(inner));
    AbiValue::Struct(outer)
}

/// Build `Option::some(SomeStruct { w, x, y, z })`.
fn option_some(w: Fr, x: bool, y: u64, z: i64) -> AbiValue {
    let mut inner = std::collections::BTreeMap::new();
    inner.insert("w".to_owned(), AbiValue::Field(w));
    inner.insert("x".to_owned(), AbiValue::Boolean(x));
    inner.insert("y".to_owned(), AbiValue::Integer(i128::from(y)));
    inner.insert("z".to_owned(), AbiValue::Integer(i128::from(z)));
    let mut outer = std::collections::BTreeMap::new();
    outer.insert("_is_some".to_owned(), AbiValue::Boolean(true));
    outer.insert("_value".to_owned(), AbiValue::Struct(inner));
    AbiValue::Struct(outer)
}

/// Which SDK entry point to invoke the method through.  Mirrors the routing
/// upstream TS's `.simulate()` performs implicitly per `abi_*` attribute.
#[derive(Clone, Copy)]
enum Dispatch {
    /// `abi_public` / `abi_private` — go through the tx simulator.
    SimulateTx,
    /// `abi_utility` — execute locally via the utility ACVM.
    ExecuteUtility,
}

/// Parse a JSON value that may be a hex string, decimal string, number, or
/// boolean into an `Fr` field element.
fn json_to_fr(item: &serde_json::Value) -> Option<Fr> {
    if let Some(s) = item.as_str() {
        return Fr::from_hex(s)
            .ok()
            .or_else(|| s.parse::<u128>().ok().map(Fr::from));
    }
    if let Some(n) = item.as_u64() {
        return Some(Fr::from(n));
    }
    if let Some(b) = item.as_bool() {
        return Some(Fr::from(u64::from(b)));
    }
    if let Some(arr) = item.as_array() {
        return arr.first().and_then(json_to_fr);
    }
    None
}

/// Shared extractor for `simulate_tx` and `execute_utility` return shapes.
///
/// `simulate_tx` returns `Array<{ values: [...], nested: [...] }>` (one entry
/// per top-level call); `execute_utility` returns either a flat array of
/// fields or `{ values | returnValues: [...] }`.
fn extract_return_fields(v: &serde_json::Value, method_name: &str) -> Vec<Fr> {
    if let Some(outer) = v.as_array() {
        if let Some(first) = outer.first() {
            if let Some(values) = first.get("values").and_then(|x| x.as_array()) {
                return values.iter().filter_map(json_to_fr).collect();
            }
            return outer.iter().filter_map(json_to_fr).collect();
        }
    }
    if let Some(arr) = v
        .get("values")
        .or_else(|| v.get("returnValues"))
        .and_then(|x| x.as_array())
    {
        return arr.iter().filter_map(json_to_fr).collect();
    }
    panic!("unexpected return_values shape for {method_name}: {v:?}")
}

/// Simulate a public/private fn via `simulate_tx` and return the flat fields.
async fn simulate_return_fields(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    contract: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    from: AztecAddress,
) -> Vec<Fr> {
    let call = build_call(artifact, contract, method_name, args);
    let sim = wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from,
                ..Default::default()
            },
        )
        .await
        .unwrap_or_else(|e| panic!("simulate {method_name}: {e}"));

    extract_return_fields(&sim.return_values, method_name)
}

/// Execute a utility fn via `execute_utility` and return the flat fields.
///
/// Upstream TS's `.simulate()` on an `abi_utility` method dispatches to
/// `pxe.executeUtility`; the Rust SDK exposes the same via `Wallet::
/// execute_utility`, which is the correct entry point.  `simulate_tx` would
/// try to run the utility bytecode through the private-tx path and fail
/// because utility oracles (`utilityGetUtilityContext`, etc.) aren't wired
/// into that path.
async fn utility_return_fields(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    contract: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    from: AztecAddress,
) -> Vec<Fr> {
    let func = artifact
        .find_function(method_name)
        .expect("method not found");
    let call = FunctionCall {
        to: contract,
        selector: func.selector.expect("selector"),
        args,
        function_type: FunctionType::Utility,
        is_static: false,
        hide_msg_sender: false,
    };
    let result = wallet
        .execute_utility(
            call,
            ExecuteUtilityOptions {
                scope: from,
                auth_witnesses: vec![],
            },
        )
        .await
        .unwrap_or_else(|e| panic!("execute_utility {method_name}: {e}"));

    extract_return_fields(&result.result, method_name)
}

/// Exercise `method_name` with undefined/null/some values, dispatched via the
/// SDK entry point appropriate for the function's `abi_*` attribute.
#[allow(clippy::cognitive_complexity)]
async fn exercise_optional(method_name: &str, dispatch: Dispatch) {
    let Some(s) = get_shared_state().await else {
        return;
    };

    let invoke = |args: Vec<AbiValue>| async {
        match dispatch {
            Dispatch::SimulateTx => {
                simulate_return_fields(
                    &s.wallet,
                    &s.artifact,
                    s.contract_address,
                    method_name,
                    args,
                    s.account,
                )
                .await
            }
            Dispatch::ExecuteUtility => {
                utility_return_fields(
                    &s.wallet,
                    &s.artifact,
                    s.contract_address,
                    method_name,
                    args,
                    s.account,
                )
                .await
            }
        }
    };

    // --- undefined (`None`) ---
    let none_fields = invoke(vec![option_none()]).await;
    assert!(
        !none_fields.is_empty(),
        "Option return should have at least the `_is_some` tag"
    );
    assert_eq!(
        none_fields[0],
        Fr::zero(),
        "_is_some should be 0 when None is passed in"
    );

    // --- null is indistinguishable from undefined at this layer (both encode
    // as `_is_some = false`), so we do not repeat the simulation here; upstream
    // covers the TS-level `null` vs `undefined` ergonomic that doesn't apply.

    // --- some value (max-ish values to stress all four inner types) ---
    let max_field = Fr::from_hex(MAX_FIELD_VALUE_HEX).expect("parse MAX_FIELD_VALUE");
    let some_fields = invoke(vec![option_some(max_field, true, U64_MAX, I64_MIN)]).await;
    assert!(
        some_fields.len() >= 5,
        "Some variant should return _is_some + 4 inner fields, got {}",
        some_fields.len()
    );
    assert_eq!(
        some_fields[0],
        Fr::from(1u64),
        "_is_some should be 1 when Some(...) is passed in"
    );
    assert_eq!(some_fields[1], max_field, "SomeStruct.w mismatch");
    assert_eq!(some_fields[2], Fr::from(1u64), "SomeStruct.x mismatch");
    assert_eq!(some_fields[3], Fr::from(U64_MAX), "SomeStruct.y mismatch");
    assert_eq!(
        some_fields[4],
        Fr::from(I64_MIN as u64),
        "SomeStruct.z (i64) mismatch"
    );
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: `it('accepts ergonomic Option params for public functions', ...)`
/// — `abi_public` routes through the AVM via `simulate_tx`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn accepts_option_params_for_public_functions() {
    let _guard = serial_guard();
    exercise_optional("return_public_optional_struct", Dispatch::SimulateTx).await;
}

/// TS: `it('accepts ergonomic Option params for utility functions', ...)`
/// — `abi_utility` routes through `pxe.executeUtility` upstream.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn accepts_option_params_for_utility_functions() {
    let _guard = serial_guard();
    exercise_optional("return_utility_optional_struct", Dispatch::ExecuteUtility).await;
}

/// TS: `it('accepts ergonomic Option params for private functions', ...)`
/// — `abi_private` routes through the private-tx simulator.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn accepts_option_params_for_private_functions() {
    let _guard = serial_guard();
    exercise_optional("return_private_optional_struct", Dispatch::SimulateTx).await;
}
