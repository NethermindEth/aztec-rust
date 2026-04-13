//! AbiTypes contract tests -- 1:1 mirror of upstream
//! `end-to-end/src/e2e_abi_types.test.ts`.
//!
//! Verifies that different Noir types (bool, Field, u64, i64, struct
//! containing all four) can be passed from TS (here, Rust) to contract
//! functions and received back unchanged via `simulate()`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_abi_types -- --ignored --nocapture
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
    clippy::many_single_char_names,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    dead_code,
    unused_imports
)]

mod common;
use common::*;

// BN254 scalar field modulus - 1.  Upstream uses `@aztec/constants`'s
// `MAX_FIELD_VALUE` which equals `Fr::MODULUS - 1n`.
const MAX_FIELD_VALUE_HEX: &str =
    "0x30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000000";

const U64_MAX: u64 = u64::MAX;
const I64_MAX: i64 = i64::MAX;
const I64_MIN: i64 = i64::MIN;

// ---------------------------------------------------------------------------
// Shared state (deploy the AbiTypes contract once for all tests)
// ---------------------------------------------------------------------------

struct AbiTypesState {
    wallet: TestWallet,
    account: AztecAddress,
    contract_address: AztecAddress,
    artifact: ContractArtifact,
}

static SHARED_STATE: OnceCell<Option<AbiTypesState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static AbiTypesState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<AbiTypesState> {
    let artifact = load_abi_types_artifact()?;
    let (wallet, account) = setup_wallet(TEST_ACCOUNT_0).await?;
    let (contract_address, artifact, _instance) =
        deploy_contract(&wallet, artifact, vec![], account).await;
    Some(AbiTypesState {
        wallet,
        account,
        contract_address,
        artifact,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build the `CustomStruct { w: Field, x: bool, y: u64, z: i64 }` ABI value.
fn custom_struct(w: Fr, x: bool, y: u64, z: i64) -> AbiValue {
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("w".to_owned(), AbiValue::Field(w));
    fields.insert("x".to_owned(), AbiValue::Boolean(x));
    fields.insert("y".to_owned(), AbiValue::Integer(i128::from(y)));
    fields.insert("z".to_owned(), AbiValue::Integer(i128::from(z)));
    AbiValue::Struct(fields)
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

/// Invoke a method via `simulate_tx` and return the flat list of return field
/// values.
///
/// Upstream TS calls `contract.methods.foo(...).simulate({ from })` which
/// routes to the AVM simulator for public fns and to the private-call
/// simulator for private fns.  In this SDK `simulate_tx` does both and
/// exposes per-call return arrays under `return_values` as:
///
///     Array<{ values: [Field], nested: [...] }>
///
/// for each top-level call.  This helper extracts the first call's `values`.
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

/// Call a utility function via `execute_utility` and return the flat list of
/// return field values.
///
/// Upstream TS's `.simulate()` on an `abi_utility`-annotated method internally
/// dispatches to `pxe.executeUtility`.  The Rust SDK exposes the same via
/// `Wallet::execute_utility`, which is the correct entry point — `simulate_tx`
/// would try to execute the utility bytecode through the private-tx path and
/// fail because utility oracles (`utilityGetUtilityContext`, etc.) aren't
/// wired into that path.
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

/// Shared extractor for `simulate_tx` and `execute_utility` return shapes.
fn extract_return_fields(v: &serde_json::Value, method_name: &str) -> Vec<Fr> {
    // `simulate_tx` shape: [ { values: [...], nested: [...] }, ... ] — one entry per top-level call.
    if let Some(outer) = v.as_array() {
        if let Some(first) = outer.first() {
            if let Some(values) = first.get("values").and_then(|x| x.as_array()) {
                return values.iter().filter_map(json_to_fr).collect();
            }
            // Flat Array case: use the whole array
            return outer.iter().filter_map(json_to_fr).collect();
        }
    }
    // `execute_utility` / object shapes: { values | returnValues: [...] } or bare array of fields.
    if let Some(arr) = v
        .get("values")
        .or_else(|| v.get("returnValues"))
        .and_then(|x| x.as_array())
    {
        return arr.iter().filter_map(json_to_fr).collect();
    }
    panic!("unexpected return_values shape for {method_name}: {v:?}")
}

/// Convert a signed i64 to its field-level u64 representation (two's complement
/// mod 2^64), mirroring how Noir serialises `i64`.
const fn i64_as_u64(value: i64) -> u64 {
    value as u64
}

/// Assert the 8-field return vector equals `(a, b, c, d, struct{w, x, y, z})`.
fn assert_return_tuple(
    returned: &[Fr],
    a: bool,
    b: Fr,
    c: u64,
    d: i64,
    w: Fr,
    x: bool,
    y: u64,
    z: i64,
) {
    assert!(
        returned.len() >= 8,
        "expected at least 8 return fields, got {}: {returned:?}",
        returned.len()
    );
    assert_eq!(returned[0], Fr::from(u64::from(a)), "a (bool) mismatch");
    assert_eq!(returned[1], b, "b (Field) mismatch");
    assert_eq!(returned[2], Fr::from(c), "c (u64) mismatch");
    assert_eq!(returned[3], Fr::from(i64_as_u64(d)), "d (i64) mismatch");
    assert_eq!(returned[4], w, "e.w (Field) mismatch");
    assert_eq!(returned[5], Fr::from(u64::from(x)), "e.x (bool) mismatch");
    assert_eq!(returned[6], Fr::from(y), "e.y (u64) mismatch");
    assert_eq!(returned[7], Fr::from(i64_as_u64(z)), "e.z (i64) mismatch");
}

/// Which SDK entry point to invoke the method through.
#[derive(Clone, Copy)]
enum Dispatch {
    /// `abi_public` / `abi_private` — go through the tx simulator.
    SimulateTx,
    /// `abi_utility` — execute locally via the utility ACVM.
    ExecuteUtility,
}

async fn exercise_parameters(method_name: &str, dispatch: Dispatch) {
    let Some(s) = get_shared_state().await else {
        return;
    };

    // --- min values ---
    let min_args = vec![
        AbiValue::Boolean(false),
        AbiValue::Field(Fr::zero()),
        AbiValue::Integer(0),
        AbiValue::Integer(i128::from(I64_MIN)),
        custom_struct(Fr::zero(), false, 0, I64_MIN),
    ];
    let min_fields = match dispatch {
        Dispatch::SimulateTx => {
            simulate_return_fields(
                &s.wallet,
                &s.artifact,
                s.contract_address,
                method_name,
                min_args,
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
                min_args,
                s.account,
            )
            .await
        }
    };
    assert_return_tuple(
        &min_fields,
        false,
        Fr::zero(),
        0,
        I64_MIN,
        Fr::zero(),
        false,
        0,
        I64_MIN,
    );

    // --- max values ---
    let max_field = Fr::from_hex(MAX_FIELD_VALUE_HEX).expect("parse MAX_FIELD_VALUE");
    let max_args = vec![
        AbiValue::Boolean(true),
        AbiValue::Field(max_field),
        AbiValue::Integer(i128::from(U64_MAX)),
        AbiValue::Integer(i128::from(I64_MAX)),
        custom_struct(max_field, true, U64_MAX, I64_MAX),
    ];
    let max_fields = match dispatch {
        Dispatch::SimulateTx => {
            simulate_return_fields(
                &s.wallet,
                &s.artifact,
                s.contract_address,
                method_name,
                max_args,
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
                max_args,
                s.account,
            )
            .await
        }
    };
    assert_return_tuple(
        &max_fields,
        true,
        max_field,
        U64_MAX,
        I64_MAX,
        max_field,
        true,
        U64_MAX,
        I64_MAX,
    );
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: `it('passes public parameters', ...)` — routes through the AVM
/// simulator (`simulate_tx` → node public-call preflight).
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn passes_public_parameters() {
    let _guard = serial_guard();
    exercise_parameters("return_public_parameters", Dispatch::SimulateTx).await;
}

/// TS: `it('passes private parameters', ...)` — routes through the private-tx
/// simulator.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn passes_private_parameters() {
    let _guard = serial_guard();
    exercise_parameters("return_private_parameters", Dispatch::SimulateTx).await;
}

/// TS: `it('passes utility parameters', ...)` — upstream `.simulate()` on an
/// `abi_utility` method routes to `pxe.executeUtility` under the hood.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn passes_utility_parameters() {
    let _guard = serial_guard();
    exercise_parameters("return_utility_parameters", Dispatch::ExecuteUtility).await;
}
