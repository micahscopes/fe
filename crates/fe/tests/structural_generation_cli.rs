use std::{fs, path::Path, process::Command};

const SOURCE: &str = include_str!("fixtures/structural_generation/accessors.fe");
const ABI_SOURCE: &str = include_str!("fixtures/structural_generation/abi.fe");
const FOLD_SHAPE_SOURCE: &str = include_str!("fixtures/structural_generation/fold_shape.fe");
const FOLD_HYGIENE_SOURCE: &str = include_str!("fixtures/structural_generation/fold_hygiene.fe");
const FOLD_MATCHES_ERROR_SOURCE: &str =
    include_str!("fixtures/structural_generation/fold_matches_error.fe");

fn succeeds(root: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_fe"))
        .current_dir(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fe {args:?}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn structural_accessors_use_normal_check_build_and_test() {
    exercise_project(
        SOURCE,
        "AccessorContract",
        &["all_accessors_execute", "selected_accessor_executes"],
    );
}

#[test]
fn structural_abi_uses_normal_check_build_and_exact_payload_execution() {
    exercise_project(
        ABI_SOURCE,
        "AbiContract",
        &[
            "empty_payload",
            "single_payload",
            "heterogeneous_payload",
            "reordered_payload",
        ],
    );
}

#[test]
fn std_fold_follows_the_record_shape_in_projects() {
    exercise_project(
        FOLD_SHAPE_SOURCE,
        "ShapeContract",
        &[
            "empty_record_is_the_unit",
            "fields_fold_in_declaration_order",
            "a_chosen_enum_payload_folds_like_a_struct",
            "abi_steps_sum_heads_and_combine_dynamic_parts",
            "a_step_can_use_an_open_name",
            "folds_follow_generic_field_types",
        ],
    );
}

#[test]
fn std_fold_is_hygienic_in_projects() {
    exercise_project(
        FOLD_HYGIENE_SOURCE,
        "GuardContract",
        &["destination_names_do_not_capture_generated_code"],
    );
}

#[test]
fn std_fold_agrees_with_the_error_desugaring() {
    exercise_project(
        FOLD_MATCHES_ERROR_SOURCE,
        "MatchContract",
        &[
            "fold_and_error_desugaring_agree_on_head_size",
            "fold_and_error_desugaring_agree_on_dynamic_parts",
            "the_compared_sizes_are_not_all_equal",
        ],
    );
}

#[test]
fn nested_abi_preserves_aliases_and_distinct_nominal_codecs() {
    let authors = ABI_SOURCE.split("\nmod empty {").next().unwrap();
    let source = format!(
        "{authors}\n{}",
        include_str!("fixtures/structural_generation/nested_abi.fe")
    );
    exercise_project(
        &source,
        "NestedContract",
        &["nested_alias_payload", "same_spelled_nominal_payload"],
    );
}

fn exercise_project(source: &str, contract: &str, tests: &[&str]) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("fe.toml"),
        "name = \"structural_fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(root.path().join("src/lib.fe"), source).unwrap();
    fs::write(root.path().join("standalone.fe"), source).unwrap();
    for target in [".", "standalone.fe"] {
        let standalone = target.ends_with(".fe");
        let mut args = vec!["check", target];
        if standalone {
            args.push("--standalone");
        }
        succeeds(root.path(), &args);
        let mut args = vec!["build", target, "--out-dir", "out"];
        if standalone {
            args.push("--standalone");
        }
        succeeds(root.path(), &args);
        for level in ["-O0", "-O2"] {
            let output = succeeds(root.path(), &["test", target, level, "--jobs", "1"]);
            assert!(
                output.contains(&format!("{} passed; 0 failed", tests.len())),
                "{output}"
            );
            for test in tests {
                assert!(output.contains(test), "{output}");
            }
        }
    }
    assert_eq!(
        fs::read_to_string(root.path().join("src/lib.fe")).unwrap(),
        source
    );
    assert_eq!(
        fs::read_to_string(root.path().join("standalone.fe")).unwrap(),
        source
    );
    assert!(root.path().join(format!("out/{contract}.bin")).exists());
}

#[test]
fn normal_check_rejects_invalid_generated_output_without_rewriting_source() {
    let root = tempfile::tempdir().unwrap();
    let source = SOURCE.replace("-> ${ty} { value.${f} }", "-> bool { value.${f} }");
    assert_ne!(source, SOURCE, "invalid fixture did not change the quote");
    let file = root.path().join("invalid.fe");
    fs::write(&file, &source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_fe"))
        .current_dir(root.path())
        .args(["check", "invalid.fe"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let diagnostic = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "{diagnostic}");
    assert!(diagnostic.contains("type mismatch"), "{diagnostic}");
    assert!(!diagnostic.contains("panicked"), "{diagnostic}");
    assert_eq!(fs::read_to_string(file).unwrap(), source);
}
