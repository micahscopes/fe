use std::{fs, process::Command};

fn execute(source: &str) {
    let root = tempfile::TempDir::new().unwrap();
    let path = root.path().join("selected.fe");
    fs::write(&path, source).unwrap();
    for args in [
        vec!["check"],
        vec!["build", "-O", "0"],
        vec!["test", "-O", "0", "--jobs", "1"],
        vec!["test", "-O", "2", "--jobs", "1"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_fe"))
            .current_dir(root.path())
            .args(&args)
            .arg(&path)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if args[0] == "test" {
            assert!(String::from_utf8_lossy(&output.stdout).contains("PASS"));
        }
    }
}

#[test]
fn lexical_selection_executes_receiver_calls_through_helpers_at_o0_and_o2() {
    let source =
        include_str!("../../driver/tests/fixtures/selected_helpers.fe").replace("const fn", "fn");
    for (extra, offset) in [("", 121), ("const Alternative: u256 = 999", 103)] {
        // RD85 requires an explicit value path when a value and named impl collide.
        let source = if extra.is_empty() {
            source.clone()
        } else {
            source.replace("with (Alternative)", "with (ingot::Alternative)")
        };
        execute(&format!(
            r#"
use core::abi::ByteInput
use std::evm::CallData
{source}
{extra}
pub fn main() -> u256 {{ selected(7) }}
#[test]
fn selected_receiver_calls() {{
    let x = CallData::new().word_at(0) + 7
    assert!(selected(x) == 4 * x + {offset})
    assert!(first(Subject {{}}, x) == x + 1)
}}
"#
        ));
    }
    let outputs =
        include_str!("../../driver/tests/fixtures/selected_outputs.fe").replace("const fn", "fn");
    execute(&format!(
        r#"
{outputs}
pub fn main() -> u256 {{ selected() }}
#[test]
fn distinct_returned_outputs() {{
    let flag = selected()
    let number = ordinary()
    assert!(flag == 1)
    assert!(selected_direct() == 1)
    assert!(number == 42)
    assert!(projected_selected() == 1)
    assert!(projected_ordinary() == 42)
    assert!(unrelated_ordinary() == 42)
    assert!(unrelated_selected() == 1)
}}
"#
    ));
}

#[test]
fn selected_parent_and_child_defaults_execute_through_inherited_requirements() {
    let source =
        include_str!("../../driver/tests/fixtures/selected_inherited.fe").replace("const fn", "fn");
    execute(&format!(
        r#"
{source}
pub fn main() -> u256 {{ selected() }}
#[test]
fn inherited_choices() {{
    assert!(selected() == 129)
    assert!(both() == 61)
    assert!(ordinary() == 3)
}}
"#
    ));
}

#[test]
fn associated_families_execute_defaults_overrides_and_nested_binders() {
    execute(
        r#"
trait Factory {
    type Pair<T> = (T, bool)
    fn wrap<T>(_ x: T) -> Self::Pair<T>
}
struct Forward {}
impl Factory for Forward {
    fn wrap<T>(_ x: T) -> Self::Pair<T> { (x, true) }
}
struct Reverse {}
impl Factory for Reverse {
    type Pair<T> = (bool, T)
    fn wrap<T>(_ x: T) -> Self::Pair<T> { (false, x) }
}
fn helper<F: Factory, T>(_ x: T) -> F::Pair<T> { F::wrap<T>(x) }
fn constructor<G: * -> *>(_ x: G<u256>) -> G<u256> { x }
trait Inner<V> { type Pair<U> }
struct S {}
impl<V> Inner<V> for S { type Pair<U> = (V, U) }
trait Outer { type Pair<T> }
struct O {}
impl Outer for O { type Pair<T> = <S as Inner<T>>::Pair<bool> }
fn nested(_ x: u256) -> O::Pair<u256> { (x, true) }
pub fn main() -> u256 { helper<Forward, u256>(41).0 }
#[test]
fn family_values() {
    let forward = helper<Forward, u256>(41)
    let reverse = helper<Reverse, u256>(73)
    let captured = nested(97)
    let higher = constructor<Forward::Pair>((101, true))
    assert!(forward.0 == 41)
    assert!(forward.1)
    assert!(!reverse.0)
    assert!(reverse.1 == 73)
    assert!(captured.0 == 97)
    assert!(captured.1)
    assert!(higher.0 == 101)
    assert!(higher.1)
}

"#,
    );
}

#[test]
fn computed_types_execute_one_scan_at_four_and_eight_leaves() {
    let source = include_str!("fixtures/fe_test/computed_scan.fe");
    execute(&format!(
        "{source}\npub fn main() -> u256 {{ scan_tree2(build4(3, 9, 27, 81)).1 }}\n"
    ));
}

#[test]
fn invalid_trait_holes_in_call_arguments_report_errors_without_panicking() {
    let root = tempfile::TempDir::new().unwrap();
    let path = root.path().join("invalid_call_argument.fe");
    for (member, projection) in [("Out", "Out"), ("Out<T>", "Out<bool>")] {
        fs::write(
            &path,
            format!(
                r#"
trait F<const N: usize> {{ type {member} = bool }}
struct S {{}}
impl<const N: usize> F<N> for S {{}}
fn consume<T>() {{}}
fn caller() {{ consume< <S as F<_>>::{projection} >() }}
"#
            ),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_fe"))
            .current_dir(root.path())
            .args(["check"])
            .arg(&path)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let message = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.status.code(), Some(1), "{message}");
        assert!(
            message.contains("layout hole `_` is not allowed in trait generic arguments"),
            "{message}"
        );
        assert!(!message.contains("panicked"), "{message}");
    }
}

#[test]
fn computed_fields_normalize_when_generic_storage_is_instantiated() {
    execute(
        r#"
struct Pair<A, B> { a: A, b: B }
recursive type fn Tree<const N: usize>() -> (*) {
    match N { 0 => bool
        _ => Pair<bool, Tree<{N - 1}>> }
}
struct Wrap<const N: usize> { value: Tree<N> }
fn wrap<const N: usize>(_ value: Tree<N>) -> Wrap<N> { Wrap<N> { value } }
pub fn main() -> bool {
    let scalar = wrap<0>(true)
    let nested = wrap<2>(Pair { a: true, b: Pair { a: false, b: true } })
    scalar.value && nested.value.a && !nested.value.b.a && nested.value.b.b
}
#[test]
fn stored_values() { assert!(main()) }
"#,
    );
}

#[test]
fn symbolic_computed_membership_preserves_concrete_dispatch() {
    execute(include_str!("fixtures/fe_test/computed_membership.fe"));
}

#[test]
fn generic_generated_implementation_preserves_runtime_values() {
    execute(include_str!(
        "../../driver/tests/fixtures/generic_derive/owner_binders.fe"
    ));
}

#[test]
fn bounded_family_constructor_results_execute_at_o0_and_o2() {
    execute(
        r#"trait Allowed {}
impl Allowed for bool {}
struct NotAllowed {}
struct Box<T> { value: T }
trait Factory { type Out<T: Allowed>: * -> * }
struct Provider {}
impl Factory for Provider { type Out<U> = Box }
fn pass<F: Factory>(x: own <F as Factory>::Out<bool, u256>) -> <F as Factory>::Out<bool, u256> { x }
pub fn main() -> u256 { pass<Provider>(x: Box { value: 42 }).value }
#[test]
fn run() { let x: Box<u256> = Box { value: 42 }
 let y = pass<Provider>(x)
 assert(y.value == 42) }
"#,
    );
}
