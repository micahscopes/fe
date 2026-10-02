use std::path::PathBuf;

use codegen::{ExpectedRevert, OptLevel, SonatinaTestOptions, emit_test_module_sonatina};
use common::InputDb;
use contract_harness::{ExecutionOptions, HarnessError, RuntimeInstance};
use dir_test::{Fixture, dir_test};
use driver::{
    DriverDataBase,
    elaboration::{prepare, read_back::read_back},
};
use url::Url;

#[path = "../../driver/tests/support/read_back.rs"]
mod read_back_support;

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/read_back",
    glob: "*.fe",
)]
fn copied_bodies_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_copied_bodies(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/read_back_loops",
    glob: "*.fe",
)]
fn copied_loop_bodies_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_copied_bodies(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/read_back_bound_constants",
    glob: "*.fe",
)]
fn copied_const_binders_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_copied_bodies(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/read_back_symbolic",
    glob: "*.fe",
)]
fn copied_symbolic_bodies_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_copied_bodies(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/read_back_matches",
    glob: "*.fe",
)]
fn copied_match_bodies_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_copied_bodies(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/read_back_enums",
    glob: "*.fe",
)]
fn copied_enum_bodies_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_copied_bodies(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/read_back_struct_patterns",
    glob: "*.fe",
)]
fn copied_struct_pattern_bodies_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_copied_bodies(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test_runner",
    glob: "quote_match_body.fe",
)]
fn quoted_match_matches_its_twin_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test_runner",
    glob: "quote_enum_checked_roots.fe",
)]
fn quoted_enum_owners_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test_runner",
    glob: "quote_struct_checked_roots.fe",
)]
fn quoted_struct_patterns_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test_runner",
    glob: "quote_enum_target.fe",
)]
fn quoted_enum_targets_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test_runner",
    glob: "quote_enum_reflection.fe",
)]
fn quoted_enum_reflection_executes_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test_runner",
    glob: "quote_reflect_shared_helper.fe",
)]
fn shared_field_helpers_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test_runner",
    glob: "quote_enum_binders.fe",
)]
fn enum_group_binders_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test_runner",
    glob: "quote_enum_generic_binders.fe",
)]
fn generic_enum_binders_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test_runner",
    glob: "quote_enum_eq.fe",
)]
fn enum_arm_fold_executes_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture_with_count(fixture, 3);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test_runner",
    glob: "quote_arm_composition.fe",
)]
fn long_arm_composition_executes_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test_runner",
    glob: "quote_enum_generic.fe",
)]
fn enum_payload_where_bounds_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture_with_count(fixture, 3);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test_runner",
    glob: "quote_enum_binder_hygiene.fe",
)]
fn enum_destination_constants_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test_runner",
    glob: "pending/quote_string_*.fe",
)]
#[dir_test_attr(#[ignore = "pre-RD86 string splice transport is pending"])]
fn string_splices_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

fn execute_quoted_fixture(fixture: Fixture<&str>) {
    execute_quoted_fixture_with_count(fixture, 1);
}

fn execute_quoted_fixture_with_count(fixture: Fixture<&str>, expected_tests: usize) {
    let url = Url::from_file_path(fixture.path()).expect("fixture URL");
    let mut authored = DriverDataBase::default();
    authored
        .workspace()
        .update(&mut authored, url.clone(), fixture.content().to_string());
    let output = prepare(&authored)
        .expect("quotation succeeds")
        .expect("quoted output");
    let file = output.workspace().get(&output, &url).unwrap();
    execute_checked_module(&output, output.top_mod(file), expected_tests);
}

fn execute_copied_bodies(fixture: Fixture<&str>) {
    let adapter = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../driver/tests/fixtures/read_back_runtime")
        .join(
            std::path::Path::new(fixture.path())
                .file_name()
                .expect("fixture file name"),
        );
    let copied = read_back_support::transplant_fixture(fixture.path(), fixture.content(), &adapter);
    assert!(!copied.bodies.is_empty());
    let file = copied
        .output
        .workspace()
        .get(&copied.output, &copied.url)
        .expect("fixture file");
    execute_checked_module(&copied.output, copied.output.top_mod(file), 2);
}

fn execute_checked_module<'db>(
    db: &'db DriverDataBase,
    module: hir::hir_def::TopLevelMod<'db>,
    expected_tests: usize,
) {
    for level in [OptLevel::O0, OptLevel::O2] {
        let tests =
            emit_test_module_sonatina(db, module, level, SonatinaTestOptions::default(), None)
                .unwrap_or_else(|error| panic!("{level:?} compilation failed: {error:?}"));
        assert_eq!(
            tests.tests.len(),
            expected_tests,
            "all authored execution oracles are present"
        );
        for test in tests.tests {
            let mut instance = RuntimeInstance::deploy(&hex::encode(test.bytecode))
                .unwrap_or_else(|error| panic!("{level:?} {} deployment: {error}", test.hir_name));
            for seed in [0, 1, 6] {
                let mut calldata = [0; 32];
                calldata[31] = seed;
                let result = instance.call_raw(&calldata, ExecutionOptions::default());
                match (&test.expected_revert, result) {
                    (None, Ok(_)) => {}
                    (Some(expected), Err(HarnessError::Revert(data))) => match expected {
                        ExpectedRevert::Any => {}
                        ExpectedRevert::Selector(selector) => assert!(
                            data.0.starts_with(selector),
                            "{level:?} {} wrong revert: {data}",
                            test.hir_name
                        ),
                        ExpectedRevert::PanicCode(bytes) => {
                            assert_eq!(&data.0, bytes, "{level:?} {} wrong panic", test.hir_name)
                        }
                    },
                    (expected, result) => panic!(
                        "{level:?} {} seed {seed}: expected {expected:?}, got {result:?}",
                        test.hir_name
                    ),
                }
            }
        }
    }
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/quote_field_authority/allowed",
    glob: "*.fe",
)]
fn checked_field_authority_executes_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/quote_open_scope/allowed",
    glob: "*.fe",
)]
// Each fixture includes an ordinary twin and an independent expected value.
fn quoted_open_captures_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/quote_fold_limits/allowed",
    glob: "*.fe",
)]
fn realistic_quote_derives_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/quote_alias_arguments/allowed", glob: "*.fe")]
fn quoted_alias_arguments_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture_with_count(fixture, 4);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/quote_private_nominals", glob: "*.fe")]
fn quoted_private_nominals_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/quote_private_nominals/readback", glob: "source.fe")]
fn copied_generic_constructors_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    let adapter = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../driver/tests/fixtures/quote_private_nominals/readback/adapter.fe");
    let copied = read_back_support::transplant_fixture(fixture.path(), fixture.content(), &adapter);
    let file = copied
        .output
        .workspace()
        .get(&copied.output, &copied.url)
        .unwrap();
    execute_checked_module(&copied.output, copied.output.top_mod(file), 1);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/quote_self_constructors", glob: "*.fe")]
fn quoted_self_constructors_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/quote_struct_alias/allowed", glob: "*.fe")]
fn quoted_struct_aliases_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    let count = if fixture.path().ends_with("width.fe") {
        3
    } else {
        1
    };
    execute_quoted_fixture_with_count(fixture, count);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/quote_signature_alias/allowed", glob: "*.fe")]
fn quoted_signature_aliases_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    let count = if fixture.path().ends_with("fixed_width.fe") {
        2
    } else {
        1
    };
    execute_quoted_fixture_with_count(fixture, count);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/quote_fragment_locals",
    glob: "*.fe",
)]
fn ordinary_fragment_locals_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/../driver/tests/fixtures/quote_member_owners", glob: "*.fe")]
fn mixed_quote_members_execute_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test_runner",
    glob: "quote_impl_method_generic.fe",
)]
fn method_generic_shadowing_owner_executes_at_o0_and_o2(fixture: Fixture<&str>) {
    execute_quoted_fixture_with_count(fixture, 1);
}
