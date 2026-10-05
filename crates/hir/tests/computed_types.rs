use camino::Utf8PathBuf;
use fe_hir::test_db::{HirAnalysisTestDb, format_diagnostics};
use salsa::Setter;

fn check(source: &str) {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("computed_types.fe"), source);
    let (top, _) = db.top_mod(file);
    let diags = format_diagnostics(&db, &db.run_on_top_mod(top));
    assert!(diags.is_empty(), "{diags}");
}

#[test]
fn computed_types_commute_with_explicit_associated_families() {
    check(include_str!(
        "fixtures/computed_types/gat_typefn_confluence.fe"
    ));
}

#[test]
fn computed_types_commute_with_default_associated_families() {
    check(include_str!(
        "fixtures/computed_types/gat_default_typefn_confluence.fe"
    ));
}

#[test]
fn literal_recursive_subjects_reduce_without_reentering_definition_checking() {
    check(
        r#"
recursive type fn Tree<T, const N: usize>() -> (*) {
    match N {
        0 => T
        _ => Tree<T, {0}>
    }
}

fn forward(x: Tree<bool, 3>) -> bool { x }
fn backward(x: bool) -> Tree<bool, 3> { x }
"#,
    );
}

#[test]
fn computed_types_cannot_erase_const_slots_by_partial_application() {
    let template = r#"
struct Pair<A, B> {}
recursive type fn Tree<T, const N: usize>() -> (*) {
    match N { 0 => T
        _ => Tree<T, {N - 1}> }
}
fn take<F: * -> * -> *>() {}
fn caller() { take<CANDIDATE>() }
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("partial.fe"), "");
    for (candidate, accepted) in [("Pair", true), ("Tree", false), ("Pair", true)] {
        let source = template.replace("CANDIDATE", candidate);
        file.set_text(&mut db).to(source.clone());
        let (top, _) = db.top_mod(file);
        let actual = format_diagnostics(&db, &db.run_on_top_mod(top));
        assert_eq!(actual.is_empty(), accepted, "{actual}");
        if !accepted {
            assert!(actual.contains("argument"), "{actual}");
        }
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file = fresh.new_stand_alone(Utf8PathBuf::from("partial.fe"), &source);
        let (fresh_top, _) = fresh.top_mod(fresh_file);
        assert_eq!(
            actual,
            format_diagnostics(&fresh, &fresh.run_on_top_mod(fresh_top))
        );
    }
}

#[test]
fn computed_type_payload_edits_invalidate_normal_forms_and_restore() {
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
fn consume(x: Tree<3>) -> Pair<Term<3>, Pair<Term<2>, Pair<Term<1>, bool>>> { x }
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("payload_edit.fe"), "");
    for (bias, accepted) in [("1", true), ("2", false), ("1", true)] {
        let source = template.replace("BIAS", bias);
        file.set_text(&mut db).to(source.clone());
        let (top, _) = db.top_mod(file);
        let actual = format_diagnostics(&db, &db.run_on_top_mod(top));
        assert_eq!(actual.is_empty(), accepted, "{actual}");
        assert!(accepted || actual.contains("type mismatch"), "{actual}");
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file = fresh.new_stand_alone(Utf8PathBuf::from("payload_edit.fe"), &source);
        let (fresh_top, _) = fresh.top_mod(fresh_file);
        assert_eq!(
            actual,
            format_diagnostics(&fresh, &fresh.run_on_top_mod(fresh_top))
        );
    }
}

#[test]
fn computed_type_requirements_survive_erased_parameters() {
    let template = r#"
trait Marker {}
struct Good {}
struct Bad {}
impl Marker for Good {}
recursive type fn Tree<T, const N: usize>() -> (*) where T: Marker {
    match N { 0 => bool
        _ => Tree<T, {N - 1}> }
}
fn consume(x: Tree<CANDIDATE, 2>) -> bool { x }
fn generic<TBOUND>(x: Tree<T, 2>) -> bool { x }
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("requirements.fe"), "");
    for (candidate, bound, accepted) in [
        ("Good", "T: Marker", true),
        ("Bad", "T: Marker", false),
        ("Good", "T", false),
        ("Good", "T: Marker", true),
    ] {
        let source = template
            .replace("CANDIDATE", candidate)
            .replace("TBOUND", bound);
        file.set_text(&mut db).to(source.clone());
        let (top, _) = db.top_mod(file);
        let actual = format_diagnostics(&db, &db.run_on_top_mod(top));
        assert_eq!(actual.is_empty(), accepted, "{actual}");
        if !accepted {
            assert!(actual.contains("Marker"), "{actual}");
        }
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file = fresh.new_stand_alone(Utf8PathBuf::from("requirements.fe"), &source);
        let (fresh_top, _) = fresh.top_mod(fresh_file);
        assert_eq!(
            actual,
            format_diagnostics(&fresh, &fresh.run_on_top_mod(fresh_top))
        );
    }
}

#[test]
fn computed_projection_bodies_preserve_context_and_spelling_equivalence() {
    let template = r#"
trait Item { type Out }
struct A {}
struct B {}
impl Item for A { type Out = A_OUTPUT }
impl Item for B { type Out = bool }
recursive type fn Tree<T, const N: usize>() -> (*) where T: Item {
    match N { 0 => PROJECTION
        _ => Tree<T, {N - 1}> }
}
fn a(x: Tree<A, 2>) -> u256 { x }
fn b(x: Tree<B, 2>) -> bool { x }
fn generic<T: Item>(x: Tree<T, 2>) -> <T as Item>::Out { x }
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("projection_body.fe"), "");
    for projection in ["T::Out", "<T as Item>::Out"] {
        for (output, accepted) in [("u256", true), ("bool", false), ("u256", true)] {
            let source = template
                .replace("PROJECTION", projection)
                .replace("A_OUTPUT", output);
            file.set_text(&mut db).to(source.clone());
            let (top, _) = db.top_mod(file);
            let actual = format_diagnostics(&db, &db.run_on_top_mod(top));
            assert_eq!(actual.is_empty(), accepted, "{actual}");
            assert!(accepted || actual.contains("type mismatch"), "{actual}");
            let mut fresh = HirAnalysisTestDb::default();
            let fresh_file =
                fresh.new_stand_alone(Utf8PathBuf::from("projection_body.fe"), &source);
            let (fresh_top, _) = fresh.top_mod(fresh_file);
            assert_eq!(
                actual,
                format_diagnostics(&fresh, &fresh.run_on_top_mod(fresh_top))
            );
        }
    }
}

#[test]
fn computed_types_cannot_define_distinct_implementation_identities() {
    let template = r#"
struct Pair<A, B> {}
struct Wrap<T> {}
recursive type fn Tree<const N: usize>() -> (*) {
    match N { 0 => bool
        _ => Pair<bool, Tree<{N - 1}>> }
}
type Alias = Tree<1>
trait Mark { fn value() -> u256 }
trait Other<T> {}
IMPLEMENTATION
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("computed_impl.fe"), "");
    for (implementation, accepted) in [
        (
            "impl Mark for Pair<bool, bool> { fn value() -> u256 { 11 } }",
            true,
        ),
        ("impl Mark for Tree<1> { fn value() -> u256 { 11 } }", false),
        ("impl Mark for Alias { fn value() -> u256 { 11 } }", false),
        (
            "impl<const N: usize> Mark for Wrap<Tree<N>> { fn value() -> u256 { 11 } }",
            false,
        ),
        ("impl Other<Tree<1>> for Wrap<bool> {}", false),
        (
            "impl Mark for Pair<bool, bool> { fn value() -> u256 { 11 } }",
            true,
        ),
    ] {
        let source = template.replace("IMPLEMENTATION", implementation);
        file.set_text(&mut db).to(source.clone());
        let (top, _) = db.top_mod(file);
        let actual = format_diagnostics(&db, &db.run_on_top_mod(top));
        assert_eq!(actual.is_empty(), accepted, "{actual}");
        if !accepted {
            assert!(
                actual.contains("computed types are not allowed in implementation headers"),
                "{actual}"
            );
        }
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file = fresh.new_stand_alone(Utf8PathBuf::from("computed_impl.fe"), &source);
        let (fresh_top, _) = fresh.top_mod(fresh_file);
        assert_eq!(
            actual,
            format_diagnostics(&fresh, &fresh.run_on_top_mod(fresh_top))
        );
    }
}

#[test]
fn computed_inherent_targets_share_method_identity_with_expanded_types() {
    let source = r#"
struct Pair<A, B> {}
recursive type fn Tree<const N: usize>() -> (*) {
    match N { 0 => bool
        _ => Pair<bool, Tree<{N - 1}>> }
}
impl Tree<1> { fn value(self) -> u256 { 11 } }
fn caller() -> u256 { Pair<bool, bool> {}.value() }
"#;
    check(source);
    let duplicate =
        format!("{source}\nimpl Pair<bool, bool> {{ fn value(self) -> u256 {{ 22 }} }}");
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("inherent.fe"), &duplicate);
    let (top, _) = db.top_mod(file);
    let diags = format_diagnostics(&db, &db.run_on_top_mod(top));
    assert!(
        diags.contains("conflicting method implementations"),
        "{diags}"
    );
}

#[test]
fn symbolic_computed_membership_requires_component_and_constructor_premises() {
    let source = include_str!("fixtures/computed_types/symbolic_membership.fe");
    let missing_component = source.replace("<F: Reduce, const N", "<F, const N");
    let missing_constructor =
        source.replace("impl<A: Reduce, B: Reduce> Reduce for Comp<A, B> {}", "");
    let missing_base = source.replace("impl Reduce for Par {}", "");
    let diagonal_only = source.replace(
        "impl<A: Reduce, B: Reduce> Reduce for Comp<A, B> {}",
        "impl<A: Reduce> Reduce for Comp<A, A> {}",
    );
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("symbolic_membership.fe"), "");
    for (text, accepted) in [
        (source, true),
        (missing_component.as_str(), false),
        (source, true),
        (missing_constructor.as_str(), false),
        (source, true),
        (missing_base.as_str(), false),
        (source, true),
        (diagonal_only.as_str(), false),
        (source, true),
    ] {
        file.set_text(&mut db).to(text.to_owned());
        let (top, _) = db.top_mod(file);
        let actual = format_diagnostics(&db, &db.run_on_top_mod(top));
        assert_eq!(actual.is_empty(), accepted, "{actual}");
        assert!(
            accepted || actual.contains("doesn't implement `Reduce`"),
            "{actual}"
        );
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file = fresh.new_stand_alone(Utf8PathBuf::from("symbolic_membership.fe"), text);
        let (fresh_top, _) = fresh.top_mod(fresh_file);
        assert_eq!(
            actual,
            format_diagnostics(&fresh, &fresh.run_on_top_mod(fresh_top))
        );
    }
}

#[test]
fn symbolic_induction_cannot_reuse_a_subject_specific_premise_for_smaller_sizes() {
    let source = r#"
struct Par {}
struct Stamp<const N: usize> {}
struct Comp<A, B> {}
trait Reduce {}
impl Reduce for Par {}
impl<A: Reduce, B: Reduce> Reduce for Comp<A, B> {}
struct Reducer<S> where S: Reduce {}
recursive type fn Tree<const N: usize>() -> (*) {
    match N {
        0 => Par
        _ => Comp<Tree<{N - 1}>, Stamp<N>>
    }
}
fn use_tree<const N: usize>(x: Reducer<Tree<N>>) where Stamp<N>: Reduce {}
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("subject_premise.fe"), source);
    let (top, _) = db.top_mod(file);
    let diagnostics = format_diagnostics(&db, &db.run_on_top_mod(top));
    assert!(
        diagnostics.contains("trait bound is not satisfied"),
        "{diagnostics}"
    );
    // An explicit promise about the complete shape remains sufficient.
    check(&source.replace("where Stamp<N>: Reduce {}", "where Tree<N>: Reduce {}"));
}

#[test]
fn symbolic_induction_checks_constructor_bounds_in_recursive_arms() {
    let source = include_str!("fixtures/computed_types/symbolic_membership.fe")
        .replace(
            "struct Comp<F, G> {}",
            "struct Comp<F: Reduce, G: Reduce> {}",
        )
        .replace("type fn RPow<F,", "type fn RPow<F: Reduce,")
        .replace("type fn LPow<F,", "type fn LPow<F: Reduce,");
    let missing_base = source.replace("impl Reduce for Par {}", "");
    let missing_premise = source.replace("type fn RPow<F: Reduce,", "type fn RPow<F,");
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone("constructor.fe".into(), "");
    for (text, accepted) in [
        (source.as_str(), true),
        (missing_base.as_str(), false),
        (source.as_str(), true),
        (missing_premise.as_str(), false),
        (source.as_str(), true),
    ] {
        file.set_text(&mut db).to(text.to_owned());
        let (top, _) = db.top_mod(file);
        let actual = format_diagnostics(&db, &db.run_on_top_mod(top));
        assert_eq!(actual.is_empty(), accepted, "{actual}");
        assert!(
            accepted || actual.contains("doesn't implement `Reduce`"),
            "{actual}"
        );
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file = fresh.new_stand_alone("constructor.fe".into(), text);
        let (fresh_top, _) = fresh.top_mod(fresh_file);
        assert_eq!(
            actual,
            format_diagnostics(&fresh, &fresh.run_on_top_mod(fresh_top))
        );
    }
}

#[test]
fn a_const_that_faults_after_substitution_is_reported() {
    for source in [
        // An associated type's output const, at the projection.
        "struct Slot<const N: usize> {}\nstruct S<const N: usize> {}\ntrait Step { type Out }\nimpl<const N: usize> Step for S<N> { type Out = Slot<{ N - 5 }> }\nfn b(x: <S<2> as Step>::Out) -> <S<2> as Step>::Out { x }",
        // A generic function's return type, at the call.
        "pub struct S<const N: usize> {}\nfn h<const N: usize>(_ b: bool) -> S<{N - 5}> { h<N>(b) }\nfn call_h(_ b: bool) -> u256 { if b { return 0 }\n h<2>(b) }",
    ] {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("const_fault.fe"), source);
        let (top, _) = db.top_mod(file);
        let result = fe_hir::test_db::format_diagnostics(&db, &db.run_on_top_mod(top));
        assert!(
            result.contains("arithmetic overflow in const context"),
            "{source}\n{result}"
        );
    }
}

#[test]
fn one_mistake_in_a_computed_type_is_reported_once() {
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
fn use_it(x: Tree<1, 2>) -> Tree<1, 2> {{ x }}"
        );
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("once.fe"), &source);
        let (top, _) = db.top_mod(file);
        let result = format_diagnostics(&db, &db.run_on_top_mod(top));
        assert!(result.contains(reason), "{source}\n{result}");
        assert_eq!(result.matches("error[").count(), 1, "{source}\n{result}");
    }
}

#[test]
fn a_bound_proved_by_induction_explains_why_a_call_still_needs_it() {
    let source = r#"
struct Par {}
struct Comp<A, B> {}
trait Reduce {}
impl Reduce for Par {}
impl<A: Reduce, B: Reduce> Reduce for Comp<A, B> {}
struct Reducer<S> where S: Reduce {}
recursive type fn RPow<F, const N: usize>() -> (*) {
    match N {
        0 => Par
        _ => Comp<RPow<F, {N - 1}>, F>
    }
}
fn need<T: Reduce>() {}
fn admitted<F: Reduce, const N: usize>(_ x: Reducer<RPow<F, N>>) {}
fn call<F: Reduce, const N: usize>() { need<RPow<F, N>>() }
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("induction_call.fe"), source);
    let (top, _) = db.top_mod(file);
    let result = format_diagnostics(&db, &db.run_on_top_mod(top));
    assert_eq!(result.matches("error[").count(), 1, "{result}");
    assert!(
        result.contains("add this bound to the function's `where` clause"),
        "{result}"
    );
    check(&source.replace(
        "fn call<F: Reduce, const N: usize>() {",
        "fn call<F: Reduce, const N: usize>() where RPow<F, N>: Reduce {",
    ));
}

#[test]
fn a_computed_return_type_that_faults_at_a_call_is_reported() {
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
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("computed_return.fe"), source);
    let (top, _) = db.top_mod(file);
    let result = format_diagnostics(&db, &db.run_on_top_mod(top));
    assert!(
        result.contains("the return type cannot be computed for these generic arguments"),
        "{result}"
    );
    check(
        &source
            .replace("h<2>(b)", "h<7>(b)")
            .replace("-> u256", "-> Tree<2>")
            .replace("return 0", "return h<7>(b)"),
    );
}
