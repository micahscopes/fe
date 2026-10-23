//! Recursive type functions across edits: every result must equal a fresh
//! database's, and a cycle must be reported the same way in either
//! declaration order and from either first use.

use camino::Utf8PathBuf;
use common::file::File;
use fe_hir::analysis::ty::ty_check::check_func_body;
use fe_hir::hir_def::ItemKind;
use fe_hir::test_db::{HirAnalysisTestDb, format_diagnostics};
use salsa::Setter;

const PATH: &str = "computed_types.fe";

fn fresh(source: &str) -> String {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from(PATH), source);
    let (top, _) = db.top_mod(file);
    format_diagnostics(&db, &db.run_on_top_mod(top))
}

/// Checks each source in turn on one database, comparing every result with a
/// fresh database's, and returns the results.
fn edits(sources: &[&str]) -> Vec<String> {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from(PATH), "");
    sources
        .iter()
        .map(|source| {
            file.set_text(&mut db).to((*source).to_owned());
            let (top, _) = db.top_mod(file);
            let actual = format_diagnostics(&db, &db.run_on_top_mod(top));
            assert_eq!(actual, fresh(source), "warm and fresh differ:\n{source}");
            actual
        })
        .collect()
}

#[test]
fn payload_edits_change_and_restore_the_computed_type() {
    let template = r#"
struct Pair<A, B> {}
struct Term<const I: usize> {}
const fn label(_ i: usize) -> usize { i + BIAS }
recursive type fn Tree<const N: usize>() -> (*) {
    match N {
        0 => bool
        _ => Pair<Term<{label(N - 1)}>, Tree<{N - 1}>>
    }
}
fn consume(x: own Tree<3>) -> Pair<Term<3>, Pair<Term<2>, Pair<Term<1>, bool>>> { x }
"#;
    let one = template.replace("BIAS", "1");
    let two = template.replace("BIAS", "2");
    let results = edits(&[&one, &two, &one]);
    assert!(results[0].is_empty(), "{}", results[0]);
    assert_eq!(
        results[1].matches("type mismatch").count(),
        1,
        "{}",
        results[1]
    );
    assert!(results[2].is_empty(), "{}", results[2]);
}

#[test]
fn requirements_survive_parameters_the_result_drops() {
    let template = r#"
trait Marker {}
struct Good {}
struct Bad {}
impl Marker for Good {}
recursive type fn Tree<T, const N: usize>() -> (*) where T: Marker {
    match N {
        0 => bool
        _ => Tree<T, {N - 1}>
    }
}
fn consume(x: own Tree<CANDIDATE, 2>) -> bool { x }
fn generic<TBOUND>(x: own Tree<T, 2>) -> bool { x }
"#;
    let source = |candidate: &str, bound: &str| {
        template
            .replace("CANDIDATE", candidate)
            .replace("TBOUND", bound)
    };
    let accepted = source("Good", "T: Marker");
    let results = edits(&[
        &accepted,
        &source("Bad", "T: Marker"),
        &accepted,
        &source("Good", "T"),
        &accepted,
    ]);
    for (index, expected) in [
        (0, None),
        (1, Some("`Bad`")),
        (2, None),
        (3, Some("`T`")),
        (4, None),
    ] {
        match expected {
            None => assert!(results[index].is_empty(), "{}", results[index]),
            Some(subject) => {
                assert_eq!(
                    results[index].matches("error[").count(),
                    1,
                    "{}",
                    results[index]
                );
                assert!(
                    results[index].contains(&format!("{subject} doesn't implement `Marker`")),
                    "{}",
                    results[index]
                );
            }
        }
    }
}

#[test]
fn projections_in_arms_follow_implementations_in_either_spelling() {
    let template = r#"
trait Item { type Out }
struct A {}
struct B {}
impl Item for A { type Out = A_OUTPUT }
impl Item for B { type Out = bool }
recursive type fn Tree<T, const N: usize>() -> (*) where T: Item {
    match N {
        0 => PROJECTION
        _ => Tree<T, {N - 1}>
    }
}
fn a(x: own Tree<A, 2>) -> u256 { x }
fn b(x: own Tree<B, 2>) -> bool { x }
fn generic<T: Item>(x: own Tree<T, 2>) -> <T as Item>::Out { x }
"#;
    for projection in ["T::Out", "<T as Item>::Out"] {
        let source = |output: &str| {
            template
                .replace("PROJECTION", projection)
                .replace("A_OUTPUT", output)
        };
        let results = edits(&[&source("u256"), &source("bool"), &source("u256")]);
        assert!(results[0].is_empty(), "{}", results[0]);
        assert_eq!(
            results[1].matches("type mismatch").count(),
            1,
            "{}",
            results[1]
        );
        assert!(results[2].is_empty(), "{}", results[2]);
    }
}

#[test]
fn implementation_headers_cannot_name_computed_types() {
    let template = r#"
struct Pair<A, B> {}
struct Wrap<T> {}
recursive type fn Tree<const N: usize>() -> (*) {
    match N {
        0 => bool
        _ => Pair<bool, Tree<{N - 1}>>
    }
}
type Alias = Tree<1>
trait Mark { fn value() -> u256 }
trait Other<T> {}
IMPLEMENTATION
"#;
    let accepted = "impl Mark for Pair<bool, bool> { fn value() -> u256 { 11 } }";
    for rejected in [
        "impl Mark for Tree<1> { fn value() -> u256 { 11 } }",
        "impl Mark for Alias { fn value() -> u256 { 11 } }",
        "impl<const N: usize> Mark for Wrap<Tree<N>> { fn value() -> u256 { 11 } }",
        "impl Other<Tree<1>> for Wrap<bool> {}",
        "impl Tree<1> { fn value(self) -> u256 { 11 } }",
    ] {
        let results = edits(&[
            &template.replace("IMPLEMENTATION", accepted),
            &template.replace("IMPLEMENTATION", rejected),
            &template.replace("IMPLEMENTATION", accepted),
        ]);
        assert!(results[0].is_empty(), "{}", results[0]);
        assert_eq!(
            results[1].matches("error[").count(),
            1,
            "{rejected}\n{}",
            results[1]
        );
        assert!(
            results[1].contains("computed types are not allowed in implementation headers"),
            "{}",
            results[1]
        );
        assert!(results[2].is_empty(), "{}", results[2]);
    }
}

#[test]
fn a_literal_recursive_subject_reduces_without_rechecking_the_definition() {
    let source = r#"
recursive type fn Tree<T, const N: usize>() -> (*) {
    match N {
        0 => T
        _ => Tree<T, {0}>
    }
}
fn forward(x: own Tree<bool, 3>) -> bool { x }
fn backward(x: own bool) -> Tree<bool, 3> { x }
"#;
    assert!(fresh(source).is_empty(), "{}", fresh(source));
}

#[test]
fn one_mistake_in_a_definition_is_reported_once() {
    for (step, reason) in [
        (
            "Node<Tree<O, {N + 1}>>",
            "the recursive subject must strictly decrease",
        ),
        (
            "Node<Tree<{O + 1}, {N - 1}>>",
            "recursive calls must forward invariant parameters unchanged and in order",
        ),
    ] {
        let source = format!(
            "struct Node<T> {{}}
recursive type fn Tree<const O: usize, const N: usize>() -> (*) {{
    match N {{
        0 => bool
        _ => {step}
    }}
}}
fn use_it(x: own Tree<1, 2>) -> Tree<1, 2> {{ x }}"
        );
        let actual = fresh(&source);
        assert!(actual.contains(reason), "{source}\n{actual}");
        assert_eq!(actual.matches("error[").count(), 1, "{source}\n{actual}");
    }
}

#[test]
fn a_return_type_that_faults_only_at_a_call_is_reported_at_the_call() {
    let source = r#"
struct Node<T> {}
recursive type fn Tree<const N: usize>() -> (*) {
    match N {
        0 => bool
        _ => Node<Tree<{N - 1}>>
    }
}
fn h<const N: usize>(_ b: bool) -> Tree<{N - 5}> { h<N>(b) }
fn call_h(_ b: bool) -> u256 {
    if b { return 0 }
    h<2>(b)
}
"#;
    let actual = fresh(source);
    assert_eq!(actual.matches("error[").count(), 1, "{actual}");
    assert!(
        actual.contains("in the return type of this call"),
        "{actual}"
    );
    let accepted = source
        .replace("h<2>(b)", "h<7>(b)")
        .replace("-> u256", "-> Tree<2>")
        .replace("return 0", "return h<7>(b)");
    assert!(fresh(&accepted).is_empty(), "{}", fresh(&accepted));
}

/// The cycle diagnostics of a source, as the text each one points at with its
/// message and labels, sorted: declaration order changes positions, not this.
fn cycle_signature(db: &HirAnalysisTestDb, file: File, first: &str) -> Vec<(String, String)> {
    let (top, _) = db.top_mod(file);
    let function = top
        .all_items(db)
        .iter()
        .find_map(|item| match item {
            ItemKind::Func(function)
                if function
                    .name(db)
                    .to_opt()
                    .is_some_and(|name| name.data(db) == first) =>
            {
                Some(*function)
            }
            _ => None,
        })
        .expect("the function to check first");
    // Demand this use before module analysis, so the order in which the
    // module's passes reach the cycle cannot hide a dependence on it.
    let _ = check_func_body(db, function);
    let text = file.text(db);
    let mut signature: Vec<_> = db
        .run_on_top_mod(top)
        .iter()
        .map(|voucher| {
            let diagnostic = voucher.to_complete(db);
            let primary = diagnostic
                .sub_diagnostics
                .iter()
                .find_map(|label| label.span.as_ref())
                .map(|span| text[span.range].to_string())
                .unwrap_or_default();
            let labels: Vec<_> = diagnostic
                .sub_diagnostics
                .into_iter()
                .map(|label| label.message)
                .collect();
            (
                primary,
                format!("{}: {}", diagnostic.message, labels.join("; ")),
            )
        })
        .collect();
    signature.sort();
    signature
}

#[test]
fn a_payload_cycle_reads_the_same_in_either_order_and_after_edits() {
    let order_a = r#"
struct Cell<const N: u256> {}
const fn size_beta() -> u256 { core::size_of<Beta<0>>() }
const fn size_alpha() -> u256 { core::size_of<Alpha<0>>() }
recursive type fn Alpha<const N: usize>() -> (*) {
    match N {
        0 => Cell<{size_beta()}>
        _ => Alpha<{N - 1}>
    }
}
recursive type fn Beta<const N: usize>() -> (*) {
    match N {
        0 => Cell<{size_alpha()}>
        _ => Beta<{N - 1}>
    }
}
fn use_alpha(x: own Alpha<0>) {}
fn use_beta(x: own Beta<0>) {}
"#;
    let order_b = r#"
struct Cell<const N: u256> {}
const fn size_alpha() -> u256 { core::size_of<Alpha<0>>() }
const fn size_beta() -> u256 { core::size_of<Beta<0>>() }
recursive type fn Beta<const N: usize>() -> (*) {
    match N {
        0 => Cell<{size_alpha()}>
        _ => Beta<{N - 1}>
    }
}
recursive type fn Alpha<const N: usize>() -> (*) {
    match N {
        0 => Cell<{size_beta()}>
        _ => Alpha<{N - 1}>
    }
}
fn use_beta(x: own Beta<0>) {}
fn use_alpha(x: own Alpha<0>) {}
"#;
    let mut expected = None;
    for source in [order_a, order_b] {
        let acyclic = source.replace("core::size_of<Alpha<0>>()", "0");
        for first in ["use_alpha", "use_beta", "size_alpha", "size_beta"] {
            let mut db = HirAnalysisTestDb::default();
            let file = db.new_stand_alone(Utf8PathBuf::from(PATH), "");
            for (text, cyclic) in [(source, true), (acyclic.as_str(), false), (source, true)] {
                file.set_text(&mut db).to(text.to_owned());
                let actual = cycle_signature(&db, file, first);
                let mut fresh_db = HirAnalysisTestDb::default();
                let fresh_file = fresh_db.new_stand_alone(Utf8PathBuf::from(PATH), text);
                assert_eq!(
                    actual,
                    cycle_signature(&fresh_db, fresh_file, first),
                    "warm and fresh differ, {first} first"
                );
                if !cyclic {
                    assert!(actual.is_empty(), "{actual:?}");
                    continue;
                }
                assert!(
                    actual.iter().all(|(_, message)| message
                        == "computed type depends on itself: computing this type requires its own result"),
                    "{actual:?}"
                );
                let written: Vec<_> = actual.iter().map(|(text, _)| text.as_str()).collect();
                assert_eq!(
                    written,
                    ["Alpha<0>", "Alpha<0>", "Beta<0>", "Beta<0>"],
                    "{actual:?}"
                );
                match &expected {
                    Some(expected) => assert_eq!(&actual, expected, "{first} first"),
                    None => expected = Some(actual),
                }
            }
        }
    }
}

#[test]
fn an_explicit_return_meets_the_declared_type_it_resolves_to() {
    let source = r#"
trait Tr { type Out }
struct A {}
impl Tr for A { type Out = u256 }
struct Node<T> {}
recursive type fn Tree<const N: usize>() -> (*) {
    match N {
        0 => u256
        _ => Node<Tree<{N - 1}>>
    }
}
fn projected(_ b: bool) -> <A as Tr>::Out {
    if b { return 1 }
    2
}
fn computed(_ b: bool) -> Tree<0> {
    if b { return 1 }
    2
}
fn computed_node(_ b: bool, x: own Node<u256>) -> Tree<1> {
    if b { return x }
    Node<u256> {}
}
"#;
    assert!(fresh(source).is_empty(), "{}", fresh(source));
    let mismatch = fresh(&source.replace(
        "if b { return 1 }\n    2\n}\nfn computed_node",
        "if b { return true }\n    2\n}\nfn computed_node",
    ));
    assert_eq!(
        mismatch.matches("returned type mismatch").count(),
        1,
        "{mismatch}"
    );
}
