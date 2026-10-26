//! `fe test --locate-failures` explains a failed test with the Fe source of the
//! instruction that failed and the call stack that reached it.

use std::{path::PathBuf, process::Command};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/locate_failures")
}

/// Runs `fe test` on the fixture from its own directory, so printed paths are
/// relative and stable.
fn run_fe_test(extra: &[&str]) -> (String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fe"))
        .arg("test")
        .arg("failing.fe")
        .args(extra)
        .current_dir(fixture_dir())
        .env("NO_COLOR", "1")
        .output()
        .expect("run fe test");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

/// `file:line:column` of the first occurrence of `needle` in the fixture.
fn position_of(needle: &str) -> String {
    let source = std::fs::read_to_string(fixture_dir().join("failing.fe")).expect("fixture");
    let offset = source.find(needle).expect("needle in fixture");
    let line = source[..offset].matches('\n').count() + 1;
    let column = offset - source[..offset].rfind('\n').map_or(0, |i| i + 1) + 1;
    format!("failing.fe:{line}:{column}")
}

/// The lines printed under one failed test, up to the next test or the summary.
fn section<'a>(output: &'a str, test: &str) -> Vec<&'a str> {
    let mut lines = output.lines().skip_while(|line| !line.contains(test));
    lines.next().expect("test is reported");
    lines.take_while(|line| line.starts_with("    ")).collect()
}

#[test]
fn failed_tests_stay_unexplained_without_the_flag() {
    let (output, code) = run_fe_test(&["-O", "0"]);
    assert_eq!(code, 1, "{output}");
    let overflow = section(&output, "failing_overflow");
    assert_eq!(overflow.len(), 1, "{output}");
    assert!(
        overflow[0].contains("Test reverted: 0x4e487b71"),
        "{output}"
    );
}

#[test]
fn overflow_failure_points_at_the_arithmetic_expression() {
    let (output, _) = run_fe_test(&["-O", "0", "--locate-failures"]);
    let overflow = section(&output, "failing_overflow").join("\n");
    let expected = format!("REVERT at {}", position_of("big + one"));
    assert!(overflow.contains(&expected), "{output}");
    assert!(overflow.contains("`big + one`"), "{output}");
}

#[test]
fn assertion_failure_lists_the_call_chain() {
    let (output, _) = run_fe_test(&["-O", "0", "--locate-failures"]);
    let failure = section(&output, "failing_assert_two_frames_deep");
    let text = failure.join("\n");
    let assert_site = position_of("assert(total == 10)");
    let call_site = position_of("check_total(3, 4)");
    assert!(
        text.contains(&format!("reached from {assert_site}")),
        "{output}"
    );
    let stack: Vec<_> = failure
        .iter()
        .skip_while(|line| !line.contains("call stack"))
        .skip(1)
        .map(|line| line.trim())
        .collect();
    assert_eq!(stack.len(), 3, "{output}");
    assert!(stack[0].starts_with("assert "), "{output}");
    assert!(
        stack[1].starts_with(&format!("check_total  {assert_site}")),
        "{output}"
    );
    assert!(
        stack[2].starts_with(&format!("failing_assert_two_frames_deep  {call_site}")),
        "{output}"
    );
}

#[test]
fn folded_test_says_so_instead_of_guessing() {
    let (output, _) = run_fe_test(&["-O", "1", "--locate-failures", "--filter", "assert_two"]);
    let failure = section(&output, "failing_assert_two_frames_deep").join("\n");
    assert!(failure.contains("no Fe source was reached"), "{output}");
    assert!(!failure.contains("failing.fe:"), "{output}");
}

#[test]
fn passing_tests_print_nothing_extra() {
    let (output, _) = run_fe_test(&["-O", "0", "--locate-failures", "--filter", "passing_case"]);
    assert!(section(&output, "passing_case").is_empty(), "{output}");
}

#[test]
fn ingot_failure_spans_modules() {
    let output = Command::new(env!("CARGO_BIN_EXE_fe"))
        .args(["test", ".", "-O", "0", "--locate-failures"])
        .current_dir(fixture_dir().join("ingot"))
        .env("NO_COLOR", "1")
        .output()
        .expect("run fe test");
    let output = String::from_utf8_lossy(&output.stdout).into_owned();
    let stack: Vec<_> = section(&output, "total_out_of_range_fails_in_other_module")
        .into_iter()
        .skip_while(|line| !line.contains("call stack"))
        .skip(1)
        .map(str::trim)
        .collect();
    assert_eq!(stack.len(), 4, "{output}");
    assert!(stack[1].starts_with("scale  src/math.fe:2:5"), "{output}");
    assert!(
        stack[2].starts_with("scaled_total  src/math.fe:8:5"),
        "{output}"
    );
    assert!(stack[3].contains("src/lib.fe:10:12"), "{output}");
}

#[test]
fn unexpected_revert_reason_is_located_too() {
    let (output, _) = run_fe_test(&["-O", "0", "--locate-failures", "--filter", "wrong_panic"]);
    let failure = section(&output, "wrong_panic_code_expected").join("\n");
    assert!(failure.contains("Expected revert data 0x"), "{output}");
    assert!(
        failure.contains(&format!(
            "REVERT at {}",
            position_of("big - one + one + one")
        )),
        "{output}"
    );
}
