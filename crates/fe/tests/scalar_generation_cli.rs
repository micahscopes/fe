use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

const PROVIDER: &str = include_str!("fixtures/generation/schema.fe");
const TEMPLATE: &str = include_str!("fixtures/generation/validator.fe");

fn invoke(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fe"))
        .current_dir(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("run fe")
}
fn diagnostic(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
fn generate(root: &Path, provider: &str, template: &str, function: &str, target: &str) -> Output {
    fs::write(root.join("provider.fe"), provider).unwrap();
    fs::write(root.join("template.fe"), template).unwrap();
    invoke(
        root,
        &[
            "generate",
            "provider.fe",
            "--provider-function",
            function,
            "--template",
            "template.fe",
            "--target-function",
            "accepts",
            "--target",
            target,
            "--output",
            "output.fe",
        ],
    )
}
fn execute(root: &Path) {
    let checked = invoke(root, &["check", "output.fe"]);
    assert!(checked.status.success(), "{}", diagnostic(&checked));
    let ran = invoke(root, &["test", "--jobs", "1", "output.fe"]);
    assert!(ran.status.success(), "{}", diagnostic(&ran));
    assert!(diagnostic(&ran).contains("PASS"), "{}", diagnostic(&ran));
}

#[test]
fn users_generate_and_execute_after_record_operation_and_template_edits() {
    let root = TempDir::new().unwrap();
    let mut first = None;
    for (provider, template) in [
        (PROVIDER.to_owned(), TEMPLATE.to_owned()),
        (
            PROVIDER.replace("ready: bool", "ready: bool, third: u256"),
            TEMPLATE.replace("15", "23").replace("16", "24"),
        ),
        (
            PROVIDER.replace("ScalarOp::Less", "ScalarOp::Equal"),
            TEMPLATE
                .replace("value: 15", "value: 16")
                .replace("!accepts(value: 16)", "!accepts(value: 17)"),
        ),
        (PROVIDER.to_owned(), TEMPLATE.to_owned()),
    ] {
        let result = generate(root.path(), &provider, &template, "author", "23");
        assert!(result.status.success(), "{}", diagnostic(&result));
        execute(root.path());
        let artifact = fs::read_to_string(root.path().join("output.fe")).unwrap();
        if provider == PROVIDER && template == TEMPLATE {
            if let Some(original) = &first {
                assert_eq!(&artifact, original);
            } else {
                first = Some(artifact);
            }
        }
        fs::remove_file(root.path().join("output.fe")).unwrap();
    }
}

#[test]
fn independent_names_and_u256_result_need_no_custom_host_code() {
    let root = TempDir::new().unwrap();
    // Explicit generation does not inherit nearby project configuration.
    fs::write(root.path().join("fe.toml"), "this is not valid TOML").unwrap();
    let provider = r#"
use core::meta::{FunctionEmission, ScalarBody, ScalarOp}
const fn incrementer() -> FunctionEmission<ScalarBody<u256, 3>> {
    FunctionEmission { target: 61, value: ScalarBody {
        nodes: [ScalarOp::U256Parameter(0), ScalarOp::U256(1), ScalarOp::Add(0, 1)], result: 2,
    } }
}
"#;
    fs::write(root.path().join("math.fe"), provider).unwrap();
    fs::write(
        root.path().join("result.fe"),
        r#"
pub const fn increment(_ operand: u256) -> u256 {}
#[test]
fn math() { assert!(increment(41) == 42) }
"#,
    )
    .unwrap();
    let result = invoke(
        root.path(),
        &[
            "generate",
            "math.fe",
            "--provider-function",
            "incrementer",
            "--template",
            "result.fe",
            "--target-function",
            "increment",
            "--target",
            "61",
            "--output",
            "output.fe",
        ],
    );
    assert!(result.status.success(), "{}", diagnostic(&result));
    fs::remove_file(root.path().join("fe.toml")).unwrap();
    execute(root.path());
}

#[test]
fn failed_invocations_do_not_publish_or_replace_artifacts() {
    let root = TempDir::new().unwrap();
    let cases = [
        (
            PROVIDER.to_owned(),
            TEMPLATE.to_owned(),
            "author",
            "99",
            "Authority",
        ),
        (
            PROVIDER.replace("result: 2", "result: 99"),
            TEMPLATE.to_owned(),
            "author",
            "23",
            "Protocol",
        ),
        (
            PROVIDER.to_owned(),
            TEMPLATE.replace("-> bool", "-> u256"),
            "author",
            "23",
            "Output",
        ),
        (
            PROVIDER.replace(
                "FunctionEmission { target",
                "if false { let x: bool = 1 }\n    FunctionEmission { target",
            ),
            TEMPLATE.to_owned(),
            "author",
            "23",
            "Provider",
        ),
        (
            PROVIDER.to_owned(),
            format!("{TEMPLATE}\n//{}", "x".repeat(65536)),
            "author",
            "23",
            "Limit",
        ),
        (
            PROVIDER.to_owned(),
            TEMPLATE.to_owned(),
            "missing",
            "23",
            "no top-level function",
        ),
    ];
    for (provider, template, name, target, kind) in cases {
        let result = generate(root.path(), &provider, &template, name, target);
        assert!(!result.status.success(), "unexpected success: {kind}");
        assert!(
            diagnostic(&result).contains(kind),
            "{}",
            diagnostic(&result)
        );
        assert!(!root.path().join("output.fe").exists());
    }
    let missing = invoke(
        root.path(),
        &[
            "generate",
            "provider.fe",
            "--provider-function",
            "author",
            "--template",
            "template.fe",
            "--target-function",
            "accepts",
            "--output",
            "output.fe",
        ],
    );
    assert!(!missing.status.success());
    assert!(diagnostic(&missing).contains("--target"));
    fs::write(root.path().join("output.fe"), "preserve this artifact").unwrap();
    for target in ["23", "99"] {
        let result = generate(root.path(), PROVIDER, TEMPLATE, "author", target);
        assert!(!result.status.success());
        assert_eq!(
            fs::read_to_string(root.path().join("output.fe")).unwrap(),
            "preserve this artifact"
        );
    }
    let input_before = fs::read(root.path().join("provider.fe")).unwrap();
    let aliases_input = invoke(
        root.path(),
        &[
            "generate",
            "provider.fe",
            "--provider-function",
            "author",
            "--template",
            "template.fe",
            "--target-function",
            "accepts",
            "--target",
            "23",
            "--output",
            "provider.fe",
        ],
    );
    assert!(!aliases_input.status.success());
    assert_eq!(
        fs::read(root.path().join("provider.fe")).unwrap(),
        input_before
    );
    assert!(!fs::read_dir(root.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".fe-generate-")
    }));
}
