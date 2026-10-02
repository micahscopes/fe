use std::path::PathBuf;

use codegen::{OptLevel, SonatinaTestOptions, emit_test_module_sonatina};
use common::InputDb;
use contract_harness::{ExecutionOptions, RuntimeInstance};
use dir_test::{Fixture, dir_test};
use driver::{
    DriverDataBase,
    elaboration::{prepare, read_back::read_back},
};

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
    for level in [OptLevel::O0, OptLevel::O2] {
        let tests = emit_test_module_sonatina(
            &copied.output,
            copied.output.top_mod(file),
            level,
            SonatinaTestOptions::default(),
            None,
        )
        .unwrap_or_else(|error| panic!("{level:?} compilation failed: {error:?}"));
        assert_eq!(
            tests.tests.len(),
            2,
            "authored and copied execution oracles"
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
