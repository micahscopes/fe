use std::path::PathBuf;

use codegen::{OptLevel, SonatinaTestOptions, emit_test_module_sonatina};
use common::InputDb;
use contract_harness::{ExecutionOptions, RuntimeInstance};
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
            assert!(test.expected_revert.is_none());
            let mut instance = RuntimeInstance::deploy(&hex::encode(test.bytecode))
                .unwrap_or_else(|error| panic!("{level:?} {} deployment: {error}", test.hir_name));
            for seed in [0, 1, 6] {
                let mut calldata = [0; 32];
                calldata[31] = seed;
                instance
                    .call_raw(&calldata, ExecutionOptions::default())
                    .unwrap_or_else(|error| {
                        panic!("{level:?} {} seed {seed} execution: {error}", test.hir_name)
                    });
            }
        }
    }
}
