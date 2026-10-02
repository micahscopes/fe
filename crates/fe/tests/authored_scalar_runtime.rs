//! Execute the checked frozen consumer, without serializing or flattening its dependencies.
extern crate driver as fe_driver;

#[allow(dead_code)]
#[path = "../../driver/examples/authored_validator.rs"]
mod example;

use codegen::{OptLevel, SonatinaTestOptions, emit_test_module_sonatina};
use contract_harness::{ExecutionOptions, HarnessError, RuntimeInstance};

fn execute(provider: &str, consumer: &str, expected_panic: bool) {
    let consumer = format!("use core::abi::ByteInput\nuse std::evm::CallData\n{consumer}");
    let bound = example::build(provider, &consumer).expect("checked frozen consumer");
    for level in [OptLevel::O0, OptLevel::O2] {
        let output = emit_test_module_sonatina(
            bound.database(),
            bound.database().top_mod(bound.file()),
            level,
            SonatinaTestOptions::default(),
            None,
        )
        .expect("compile consumer and its selected dependency");
        assert_eq!(output.tests.len(), 1);
        let test = &output.tests[0];
        assert!(
            !test.bytecode.is_empty(),
            "compiled test has deployable code"
        );
        let mut runtime =
            RuntimeInstance::deploy(&hex::encode(&test.bytecode)).expect("deploy test bytecode");
        let result = runtime.call_raw(
            &[if expected_panic { 255 } else { 0 }; 32],
            ExecutionOptions::default(),
        );
        if expected_panic {
            let Err(HarnessError::Revert(data)) = result else {
                panic!("expected arithmetic panic at {level}, got {result:?}");
            };
            let mut expected = vec![0x4e, 0x48, 0x7b, 0x71];
            expected.extend([0; 31]);
            expected.push(0x11);
            assert_eq!(data.0, expected, "arithmetic overflow at {level}");
        } else {
            result.unwrap_or_else(|error| panic!("runtime failed at {level}: {error}"));
        }
    }
}

#[test]
fn reflected_authored_validator_executes_through_frozen_import() {
    for (provider, bound) in [
        (example::PROVIDER.to_owned(), 16),
        (
            example::PROVIDER.replace("second: bool", "second: bool, third: u256"),
            24,
        ),
    ] {
        execute(
            &provider,
            &format!(
                r#"
pub const fn validate(value: u256) -> bool {{}}
const BELOW: bool = validate(value: {below})
const EDGE: bool = validate(value: {bound})
const ABOVE: bool = validate(value: {above})
#[test]
fn check() {{
    assert!(BELOW)
    assert!(!EDGE)
    assert!(!ABOVE)
    let offset = CallData::new().word_at(0)
    assert!(validate(value: offset + {below}) == BELOW)
    assert!(validate(value: offset + {bound}) == EDGE)
    assert!(validate(value: offset + {above}) == ABOVE)
}}
"#,
                below = bound - 1,
                above = bound + 1
            ),
            false,
        );
    }
}

#[test]
fn unused_authored_add_still_panics_at_runtime() {
    let provider = r#"
use core::meta::{FunctionEmission, ScalarBody, ScalarOp}
const fn provide() -> FunctionEmission<ScalarBody<bool, 4>> {
    FunctionEmission { target: 7, value: ScalarBody {
        nodes: [ScalarOp::U256Parameter(0), ScalarOp::U256(1),
                ScalarOp::Add(0, 1), ScalarOp::Bool(true)],
        result: 3,
    } }
}
"#;
    // A valid control distinguishes the unused result from an always-failing artifact.
    execute(
        provider,
        r#"
pub const fn validate(value: u256) -> bool {}
#[test]
fn check() { assert!(validate(value: CallData::new().word_at(0) + 41)) }
"#,
        false,
    );
    execute(
        provider,
        r#"
pub const fn validate(value: u256) -> bool {}
#[test]
fn check() { let _ = validate(value: CallData::new().word_at(0)) }
"#,
        true,
    );
}

#[test]
fn authored_add_equality_and_negation_match_const_evaluation() {
    execute(
        r#"
use core::meta::{FunctionEmission, ScalarBody, ScalarOp}
const fn provide() -> FunctionEmission<ScalarBody<bool, 6>> {
    FunctionEmission { target: 7, value: ScalarBody {
        nodes: [ScalarOp::U256Parameter(0), ScalarOp::U256(1),
                ScalarOp::Add(0, 1), ScalarOp::U256(42),
                ScalarOp::Equal(2, 3), ScalarOp::Not(4)],
        result: 5,
    } }
}
"#,
        r#"
pub const fn validate(value: u256) -> bool {}
const MATCH: bool = validate(value: 41)
const DIFFERENT: bool = validate(value: 40)
#[test]
fn check() {
    assert!(!MATCH)
    assert!(DIFFERENT)
    let offset = CallData::new().word_at(0)
    assert!(validate(value: offset + 41) == MATCH)
    assert!(validate(value: offset + 40) == DIFFERENT)
}
"#,
        false,
    );
}
