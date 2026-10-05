use camino::Utf8PathBuf;
use fe_hir::test_db::{HirAnalysisTestDb, format_diagnostics};
use salsa::Setter;

const EXPLICIT: &str = r#"
trait Inner<V> { type Pair<U> }
struct S {}
impl<V> Inner<V> for S { type Pair<U> = (V, U) }
trait Outer { type Pair<T> }
struct O {}
impl Outer for O { type Pair<T> = <S as Inner<T>>::Pair<bool> }
fn direct(x: <S as Inner<u256>>::Pair<bool>) -> (u256, bool) { x }
fn nested(x: O::Pair<u256>) -> RESULT { x }
"#;

fn diagnostics(db: &HirAnalysisTestDb, file: common::file::File) -> String {
    let (top, _) = db.top_mod(file);
    format_diagnostics(db, &db.run_on_top_mod(top))
}

#[test]
fn nested_families_preserve_outer_parameters_in_explicit_and_default_bodies() {
    let default = EXPLICIT
        .replace(
            "trait Inner<V> { type Pair<U> }",
            "trait Inner<V> { type Pair<U> = (V, U) }",
        )
        .replace(
            "impl<V> Inner<V> for S { type Pair<U> = (V, U) }",
            "impl<V> Inner<V> for S {}",
        );
    for template in [EXPLICIT, default.as_str()] {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("nested_families.fe"), "");
        for (output, valid) in [
            ("(u256, bool)", true),
            ("(bool, bool)", false),
            ("(u256, bool)", true),
        ] {
            let source = template.replace("RESULT", output);
            file.set_text(&mut db).to(source.clone());
            let result = diagnostics(&db, file);
            if valid {
                assert!(result.is_empty(), "{result}");
            } else {
                assert!(result.contains("type mismatch"), "{result}");
            }
            let mut fresh = HirAnalysisTestDb::default();
            let fresh_file =
                fresh.new_stand_alone(Utf8PathBuf::from("nested_families.fe"), &source);
            assert_eq!(result, diagnostics(&fresh, fresh_file));
        }
    }
}

fn check_fixture(name: &str, source: &str) {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from(format!("{name}.fe")), source);
    let result = diagnostics(&db, file);
    assert!(result.is_empty(), "{name}: {result}");
}

#[test]
fn historical_family_parameter_scope() {
    check_fixture(
        "scope",
        include_str!("fixtures/associated_families/gat_param_scope_resolves.fe"),
    );
}

#[test]
fn historical_family_projection() {
    check_fixture(
        "projection",
        include_str!("fixtures/associated_families/gat_projection_resolves.fe"),
    );
}

#[test]
fn historical_family_defaults_and_override() {
    check_fixture(
        "defaults",
        include_str!("fixtures/associated_families/gat_default_merge.fe"),
    );
}

#[test]
fn qualified_family_results_and_symbolic_method_clients() {
    check_fixture(
        "qualified",
        include_str!("fixtures/associated_families/gat_qualified_path_out.fe"),
    );
}

#[test]
fn family_output_bounds_are_proved_for_arbitrary_arguments() {
    let template = r#"
trait Wrap<T> {}
struct Box<T> {}
SUPPORT
trait Factory { type Out<T>: Wrap<T> }
struct Provider {}
impl Factory for Provider { type Out<U> = Box<U> }
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("family_bounds.fe"), "");
    for (support, valid) in [
        ("impl<T> Wrap<T> for Box<T> {}", true),
        ("impl Wrap<u256> for Box<u256> {}", false),
        ("impl<T> Wrap<bool> for Box<T> {}", false),
        ("impl<T> Wrap<T> for Box<T> {}", true),
    ] {
        let source = template.replace("SUPPORT", support);
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        assert_eq!(result.is_empty(), valid, "{result}");
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file = fresh.new_stand_alone(Utf8PathBuf::from("family_bounds.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn family_output_defaults_and_overrides_preserve_the_declared_bound() {
    let template = r#"
trait Wrap<T> {}
struct Box<V, T> {}
struct Bad<T> {}
impl<V, T> Wrap<T> for Box<V, T> {}
trait Factory<V> { type Out<T>: Wrap<T> = Box<V, T> }
struct Provider {}
impl Factory<bool> for Provider { BODY }
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("family_default_bounds.fe"), "");
    for (body, valid) in [("", true), ("type Out<U> = Bad<U>", false), ("", true)] {
        let source = template.replace("BODY", body);
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        assert_eq!(result.is_empty(), valid, "{result}");
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file =
            fresh.new_stand_alone(Utf8PathBuf::from("family_default_bounds.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn unused_default_family_bounds_require_a_completed_proof() {
    let template = r#"
trait Wrap<T> {}
struct Box<T> {}
impl<T> Wrap<T> for Box<T> REQUIREMENT {}
trait Factory { type Out<T>: Wrap<T> = Box<T> }
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("default_family_proof.fe"), "");
    for (requirement, valid) in [
        ("", true),
        ("where Box<Box<T>>: Wrap<Box<T>>", false),
        ("", true),
    ] {
        let source = template.replace("REQUIREMENT", requirement);
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        assert_eq!(result.is_empty(), valid, "{requirement}: {result}");
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file =
            fresh.new_stand_alone(Utf8PathBuf::from("default_family_proof.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn family_parameter_guards_validate_their_trait_arguments() {
    let template = r#"
trait Allowed {}
impl Allowed for bool {}
trait Needs<V: Allowed> {}
trait Factory { type Out<T: Needs<ARG>> }
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("family_guard_arguments.fe"), "");
    for (arg, valid) in [
        ("bool", true),
        ("u256", false),
        ("Missing", false),
        ("bool", true),
    ] {
        let source = template.replace("ARG", arg);
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        assert_eq!(result.is_empty(), valid, "{arg}: {result}");
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file =
            fresh.new_stand_alone(Utf8PathBuf::from("family_guard_arguments.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn family_output_bounds_validate_names_arguments_and_prerequisites() {
    let template = r#"
trait Allowed {}
impl Allowed for bool {}
trait Needs<V: Allowed> {}
trait Parent {}
trait Child: Parent {}
trait F { type Out<T: Allowed>: BOUND }
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("family_output_bounds.fe"), "");
    for (bound, valid) in [
        ("Allowed", true),
        ("Missing", false),
        ("Needs<bool>", true),
        ("Needs<u256>", false),
        ("Needs<Missing>", false),
        ("Needs<T>", true),
        ("Child", true),
        ("Allowed", true),
    ] {
        let source = template.replace("BOUND", bound);
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        assert_eq!(result.is_empty(), valid, "{bound}: {result}");
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file =
            fresh.new_stand_alone(Utf8PathBuf::from("family_output_bounds.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn family_guard_prerequisites_can_come_from_sibling_parameters() {
    for params in ["T: Needs<U>, UBOUND", "UBOUND, T: Needs<U>"] {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("family_sibling_guards.fe"), "");
        for (bound, valid) in [("U: Allowed", true), ("U", false), ("U: Allowed", true)] {
            let source = format!(
                "trait Allowed {{}}\ntrait Needs<V: Allowed> {{}}\ntrait Factory {{ type Out<{}> }}",
                params.replace("UBOUND", bound),
            );
            file.set_text(&mut db).to(source.clone());
            let result = diagnostics(&db, file);
            assert_eq!(result.is_empty(), valid, "{params}, {bound}: {result}");
            let mut fresh = HirAnalysisTestDb::default();
            let fresh_file =
                fresh.new_stand_alone(Utf8PathBuf::from("family_sibling_guards.fe"), &source);
            assert_eq!(result, diagnostics(&fresh, fresh_file));
        }
    }
}

#[test]
fn recursive_family_definitions_do_not_silently_accept_arbitrary_values() {
    let template = r#"
trait F { type Out<T> }
struct S {}
impl F for S { type Out<T> = BODY }
pub fn value() -> S::Out<u256> { true }
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("family_recursion.fe"), "");
    for (body, valid) in [
        ("bool", true),
        ("<S as F>::Out<T>", false),
        ("<S as F>::Out<(T,)>", false),
        ("(<S as F>::Out<T>, bool)", false),
        ("bool", true),
    ] {
        let source = template.replace("BODY", body);
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        assert_eq!(result.is_empty(), valid, "{body}: {result}");
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file = fresh.new_stand_alone(Utf8PathBuf::from("family_recursion.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn inherited_and_mutual_family_recursion_preserve_rejection_and_restoration() {
    for declaration in [
        "trait F { type Out<T> = BODY }\nimpl F for S {}",
        "trait F { type Out<T>\n type Other<T> }\nimpl F for S { type Out<T> = Self::Other<T>\n type Other<T> = BODY }",
    ] {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("family_recursive_defaults.fe"), "");
        for (body, valid) in [("bool", true), ("Self::Out<T>", false), ("bool", true)] {
            let source = format!(
                "struct S {{}}\n{}\npub fn value() -> S::Out<u256> {{ true }}",
                declaration.replace("BODY", body),
            );
            file.set_text(&mut db).to(source.clone());
            let result = diagnostics(&db, file);
            assert_eq!(result.is_empty(), valid, "{source}\n{result}");
            let mut fresh = HirAnalysisTestDb::default();
            let fresh_file =
                fresh.new_stand_alone(Utf8PathBuf::from("family_recursive_defaults.fe"), &source);
            assert_eq!(result, diagnostics(&fresh, fresh_file));
        }
    }
    check_fixture(
        "overridden_recursive_default",
        r#"
trait F { type Out<T> = Self::Out<T> }
struct S {}
impl F for S { type Out<T> = bool }
fn value() -> S::Out<u256> { true }
"#,
    );
    check_fixture(
        "finite_family_nesting",
        r#"
trait F { type Out<T> = (T, bool) }
struct S {}
impl F for S {}
fn identity(_ x: S::Out<S::Out<u256>>) -> ((u256, bool), bool) { x }
"#,
    );
}

#[test]
fn symbolic_family_clients_use_only_declared_bounds() {
    let template = r#"
trait Show<T> { fn show(self) -> u256 { 1 } }
trait Draw<T> { fn draw(self) -> u256 { 2 } }
trait Factory { type Out<T>: Show<T> }
struct Box<T> {}
struct Provider {}
impl<T> Show<T> for Box<T> {}
impl<T> Draw<T> for Box<T> {}
impl Factory for Provider { type Out<T> = Box<T> }
fn generic<B: Factory, T>(x: B::Out<T>) -> u256 { x.METHOD() }
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("family_declared_bounds.fe"), "");
    for (method, valid) in [("show", true), ("draw", false), ("show", true)] {
        let source = template.replace("METHOD", method);
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        assert_eq!(result.is_empty(), valid, "{result}");
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file =
            fresh.new_stand_alone(Utf8PathBuf::from("family_declared_bounds.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn symbolic_family_bound_consumption_requires_parameter_guards() {
    let template = r#"
trait Allowed {}
trait Show<T> { fn show(self) -> u256 { 1 } }
trait Factory { type Out<T: Allowed>: Show<T> }
fn generic<B: Factory, T GUARD>(x: B::Out<T>) -> u256 { x.show() }
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("family_guarded_bounds.fe"), "");
    for (guard, valid) in [(": Allowed", true), ("", false), (": Allowed", true)] {
        let source = template.replace("GUARD", guard);
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        assert_eq!(result.is_empty(), valid, "{result}");
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file =
            fresh.new_stand_alone(Utf8PathBuf::from("family_guarded_bounds.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn symbolic_family_bounds_keep_outer_arguments_and_supertraits() {
    let template = r#"
trait Parent<A, B> { fn accept(self, _ a: A, _ b: B) {} }
trait Child<A, B>: Parent<A, B> {}
trait Factory<V> { type Out<T>: Child<V, T> }
fn generic<V, B: Factory<V>, T>(x: B::Out<T>, _ v: V, _ t: T) {
    x.accept(ARG, t)
}

"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("family_outer_bounds.fe"), "");
    for (arg, valid) in [("v", true), ("true", false), ("v", true)] {
        let source = template.replace("ARG", arg);
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        assert_eq!(result.is_empty(), valid, "{result}");
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file =
            fresh.new_stand_alone(Utf8PathBuf::from("family_outer_bounds.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn forming_a_symbolic_family_requires_guards_without_method_calls() {
    let template = r#"
trait Allowed {}
trait Factory { type Out<T: Allowed> }
fn generic<B: Factory, T GUARD>(_ x: B::Out<T>) {}
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("family_formation.fe"), "");
    for (guard, valid) in [(": Allowed", true), ("", false), (": Allowed", true)] {
        let source = template.replace("GUARD", guard);
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        assert_eq!(result.is_empty(), valid, "{result}");
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file = fresh.new_stand_alone(Utf8PathBuf::from("family_formation.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn normalizing_a_concrete_family_does_not_erase_parameter_requirements() {
    let template = r#"
trait Allowed {}
impl Allowed for bool {}
trait Factory { type Out<T: Allowed> }
struct Provider {}
impl Factory for Provider { type Out<T> = u256 }
fn concrete(_ x: Provider::Out<ARG>) {}
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("family_concrete_formation.fe"), "");
    for (arg, valid) in [("bool", true), ("u256", false), ("bool", true)] {
        let source = template.replace("ARG", arg);
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        assert_eq!(result.is_empty(), valid, "{result}");
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file =
            fresh.new_stand_alone(Utf8PathBuf::from("family_concrete_formation.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn explicit_call_type_arguments_enforce_adt_requirements() {
    check_call_type_argument_requirements("Box<ARG>");
}

#[test]
fn explicit_call_arguments_report_invalid_source_before_semantic_lowering() {
    for (member, projection) in [("Out", "Out"), ("Out<T>", "Out<bool>")] {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("call_argument_source.fe"), "");
        for (arg, valid) in [("3", true), ("_", false), ("3", true)] {
            let source = format!(
                r#"
trait F<const N: usize> {{ type {member} = bool }}
struct S {{}}
impl<const N: usize> F<N> for S {{}}
fn consume<T>() {{}}
fn caller() {{ consume< <S as F<{arg}>>::{projection} >() }}
"#
            );
            file.set_text(&mut db).to(source.clone());
            let result = diagnostics(&db, file);
            if valid {
                assert!(result.is_empty(), "{result}");
            } else {
                assert!(
                    result.contains("layout hole `_` is not allowed in trait generic arguments"),
                    "{result}"
                );
            }
            let mut fresh = HirAnalysisTestDb::default();
            let fresh_file =
                fresh.new_stand_alone(Utf8PathBuf::from("call_argument_source.fe"), &source);
            assert_eq!(result, diagnostics(&fresh, fresh_file));
        }
    }
    check_fixture(
        "ordinary_const_hole",
        "fn consume<const N: usize>() {}\nfn caller() { consume<_>() }",
    );
}

#[test]
fn unused_family_defaults_check_the_body_without_output_bounds() {
    let template = r#"
trait Allowed {}
trait Extra {}
struct Box<T: REQUIREMENT> {}
trait Factory { type Out<T: Allowed> = BODY }
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("unused_family_default.fe"), "");
    for (requirement, body, valid) in [
        ("Allowed", "Box<T>", true),
        ("Extra", "Box<T>", false),
        ("Allowed", "Missing<T>", false),
        ("Allowed", "Box<T>", true),
    ] {
        let source = template
            .replace("REQUIREMENT", requirement)
            .replace("BODY", body);
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        assert_eq!(result.is_empty(), valid, "{requirement}: {result}");
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file =
            fresh.new_stand_alone(Utf8PathBuf::from("unused_family_default.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn explicit_call_type_arguments_enforce_family_requirements() {
    check_call_type_argument_requirements("Provider::Out<ARG>");
    check_call_type_argument_requirements("(bool, <Provider as Factory>::Out<ARG>)");
}

fn check_call_type_argument_requirements(argument: &str) {
    let template = r#"
trait Allowed {}
impl Allowed for bool {}
struct Box<T: Allowed> {}
trait Factory { type Out<T: Allowed> }
struct Provider {}
impl Factory for Provider { type Out<T> = u256 }
fn consume<T>() {}
struct Receiver {}
impl Receiver { fn consume<T>(self) {} }
CALLER
"#;
    for caller in [
        "fn caller() { consume<TYPE>() }",
        "fn caller(_ receiver: Receiver) { receiver.consume<TYPE>() }",
        "fn caller<T: Allowed>() { consume<TYPE>() }",
    ] {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("call_type_requirements.fe"), "");
        let valid_arg = if caller.contains("caller<T") {
            "T"
        } else {
            "bool"
        };
        for (arg, valid) in [(valid_arg, true), ("u256", false), (valid_arg, true)] {
            let source = template
                .replace("CALLER", caller)
                .replace("TYPE", &argument.replace("ARG", arg));
            file.set_text(&mut db).to(source.clone());
            let result = diagnostics(&db, file);
            assert_eq!(
                result.is_empty(),
                valid,
                "{caller}, {argument}, {arg}: {result}"
            );
            let mut fresh = HirAnalysisTestDb::default();
            let fresh_file =
                fresh.new_stand_alone(Utf8PathBuf::from("call_type_requirements.fe"), &source);
            assert_eq!(result, diagnostics(&fresh, fresh_file));
        }
    }
}

#[test]
fn concrete_family_guards_keep_unused_enclosing_arguments() {
    let template = r#"
trait Allowed<V> {}
impl Allowed<bool> for bool {}
trait Factory<V> { type Out<T: Allowed<V>> DEFAULT }
struct Provider {}
impl<V> Factory<V> for Provider { BODY }
fn concrete(_ x: <Provider as Factory<CONTEXT>>::Out<bool>) {}
"#;
    for (default, body) in [("", "type Out<U> = u256"), ("= u256", "")] {
        let template = template.replace("DEFAULT", default).replace("BODY", body);
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("family_captured_guards.fe"), "");
        for (context, valid) in [("bool", true), ("u256", false), ("bool", true)] {
            let source = template.replace("CONTEXT", context);
            file.set_text(&mut db).to(source.clone());
            let result = diagnostics(&db, file);
            assert_eq!(result.is_empty(), valid, "{result}");
            let mut fresh = HirAnalysisTestDb::default();
            let fresh_file =
                fresh.new_stand_alone(Utf8PathBuf::from("family_captured_guards.fe"), &source);
            assert_eq!(result, diagnostics(&fresh, fresh_file));
        }
    }
}

#[test]
fn family_bodies_use_only_declared_guards_for_bounds_and_well_formedness() {
    let template = r#"
trait Allowed {}
trait Extra {}
trait Show<T> {}
struct Box<T: REQUIREMENT> {}
impl<U: REQUIREMENT> Show<U> for Box<U> {}
trait Factory { type Out<T: Allowed>: Show<T> DEFAULT }
struct Provider {}
impl Factory for Provider { BODY }
"#;
    for (default, body) in [("", "type Out<U> = Box<U>"), ("= Box<T>", "")] {
        let template = template.replace("DEFAULT", default).replace("BODY", body);
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("family_body_guards.fe"), "");
        for (requirement, valid) in [("Allowed", true), ("Extra", false), ("Allowed", true)] {
            let source = template.replace("REQUIREMENT", requirement);
            file.set_text(&mut db).to(source.clone());
            let result = diagnostics(&db, file);
            assert_eq!(result.is_empty(), valid, "{result}");
            let mut fresh = HirAnalysisTestDb::default();
            let fresh_file =
                fresh.new_stand_alone(Utf8PathBuf::from("family_body_guards.fe"), &source);
            assert_eq!(result, diagnostics(&fresh, fresh_file));
        }
    }
}

#[test]
fn declaration_guards_do_not_leak_to_sibling_families() {
    let template = r#"
trait Allowed {}
trait Show<T> {}
struct Box<T> {}
impl<U: Allowed> Show<U> for Box<U> {}
trait Factory {
    type First<T: Allowed>: Show<T>
    type Second<T GUARD>: Show<T>
}
struct Provider {}
impl Factory for Provider {
    type First<U> = Box<U>
    type Second<U> = Box<U>
}
"#;
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("family_guard_scopes.fe"), "");
    for (guard, valid) in [(": Allowed", true), ("", false), (": Allowed", true)] {
        let source = template.replace("GUARD", guard);
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        assert_eq!(result.is_empty(), valid, "{result}");
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file =
            fresh.new_stand_alone(Utf8PathBuf::from("family_guard_scopes.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn family_signatures_are_checked_without_a_use_site() {
    let cases = [
        ("type Out<T>", "type Out<U> = u256", None),
        ("type Out<F: * -> *>", "type Out<G: * -> *> = u256", None),
        ("type Out<T>", "type Out = u256", Some("parameter count")),
        (
            "type Out<T>",
            "type Out<U, V> = u256",
            Some("parameter count"),
        ),
        (
            "type Out<F: * -> *>",
            "type Out<U> = u256",
            Some("parameter kind"),
        ),
        (
            "type Out<T>",
            "type Out<U: * -> *> = u256",
            Some("parameter kind"),
        ),
        (
            "type Out<T>",
            "type Out<U: Allowed> = u256",
            Some("implementation parameter bounds"),
        ),
        (
            "type Out<T: Missing>",
            "type Out<U> = u256",
            Some("`Missing` is not found"),
        ),
        (
            "type Out<T, T>",
            "type Out<U, V> = u256",
            Some("duplicate associated type parameter"),
        ),
        (
            "type Out<T>",
            "type Out<U> = u256\n type Extra<V> = u256",
            Some("not defined in trait"),
        ),
        (
            "type Out<const N: usize>",
            "type Out<const N: usize> = u256",
            Some("const associated type parameters"),
        ),
        (
            "type Out<T>",
            "type Out<const N: usize> = u256",
            Some("parameter sort"),
        ),
        ("type Out<T>", "type Out<U> = u256", None),
    ];
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("family_signature.fe"), "");
    for (decl, definition, expected) in cases {
        let source = format!(
            "trait Allowed {{}}\ntrait Family {{ {decl} }}\nstruct P {{}}\nimpl Family for P {{ {definition} }}"
        );
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        match expected {
            Some(message) => assert!(result.contains(message), "{source}\n{result}"),
            None => assert!(result.is_empty(), "{result}"),
        }
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file = fresh.new_stand_alone(Utf8PathBuf::from("family_signature.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn trait_bindings_do_not_erase_family_parameters() {
    for consumer in [
        "fn unused<B: Family<Out = u256>>() {}",
        "fn unused<B>() where B: Family<Out = u256> {}",
        "struct Goal<G: Constraint> {}\nstruct P {}\nfn unused(_ g: Goal<Family<P, Out = u256>>) {}",
    ] {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("family_binding.fe"), "");
        for (params, valid) in [("", true), ("<T>", false), ("", true)] {
            let source = format!("trait Family {{ type Out{params} }}\n{consumer}");
            file.set_text(&mut db).to(source.clone());
            let result = diagnostics(&db, file);
            if valid {
                assert!(result.is_empty(), "{source}\n{result}");
            } else {
                assert!(
                    result.contains("associated family binding"),
                    "{source}\n{result}"
                );
            }
            let mut fresh = HirAnalysisTestDb::default();
            let fresh_file = fresh.new_stand_alone(Utf8PathBuf::from("family_binding.fe"), &source);
            assert_eq!(result, diagnostics(&fresh, fresh_file));
        }
    }
}

#[test]
fn guarded_families_cannot_escape_through_constructor_parameters() {
    for (declaration, definition, argument) in [
        ("type Out<T GUARD>", "type Out<U> = u256", ""),
        ("type Out<T GUARD> = u256", "", ""),
        ("type Out<T, U GUARD>", "type Out<A, B> = u256", "<bool>"),
    ] {
        for use_site in [
            "fn generic<B: Factory>(_ x: HCell<B::Out>) {}",
            "fn concrete(_ x: HCell<Provider::Out>) {}",
            "struct Holder<B: Factory> { value: HCell<B::Out> }",
            "type Alias = HCell<Provider::Out>",
        ] {
            let use_site = use_site.replace("::Out", &format!("::Out{argument}"));
            let template = format!(
                "trait Allowed {{}}\ntrait Factory {{ {declaration} }}\nstruct HCell<F: * -> *> {{}}\nstruct Provider {{}}\nimpl Factory for Provider {{ {definition} }}\n{use_site}"
            );
            let mut db = HirAnalysisTestDb::default();
            let file = db.new_stand_alone(Utf8PathBuf::from("family_constructor_escape.fe"), "");
            for (guard, valid) in [("", true), (": Allowed", false), ("", true)] {
                let source = template.replace("GUARD", guard);
                file.set_text(&mut db).to(source.clone());
                let result = diagnostics(&db, file);
                if valid {
                    assert!(result.is_empty(), "{source}\n{result}");
                } else {
                    assert!(
                        result.contains("guarded associated family must be fully applied"),
                        "{source}\n{result}"
                    );
                }
                let mut fresh = HirAnalysisTestDb::default();
                let fresh_file = fresh
                    .new_stand_alone(Utf8PathBuf::from("family_constructor_escape.fe"), &source);
                assert_eq!(result, diagnostics(&fresh, fresh_file));
            }
        }
    }
}

#[test]
fn associated_const_captures_follow_impl_arguments_without_crossing_instances() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("associated_const_capture.fe"), "");
    for (argument, output, valid) in [(2, 3, true), (5, 6, true), (2, 6, false), (2, 3, true)] {
        let source = format!(
            "struct Slot<const N: usize> {{}}\n\
             struct S<const N: usize> {{}}\n\
             trait Step {{ type Out }}\n\
             impl<const N: usize> Step for S<N> {{ type Out = Slot<{{ N + 1 }}> }}\n\
             fn probe(x: <S<{argument}> as Step>::Out) -> Slot<{output}> {{ x }}"
        );
        file.set_text(&mut db).to(source);
        let diags = diagnostics(&db, file);
        if valid {
            assert!(diags.is_empty(), "{diags}");
        } else {
            assert!(diags.contains("type mismatch"), "{diags}");
        }
    }
}

#[test]
fn composed_projection_constraints_use_the_full_trait_instance() {
    let template = include_str!("fixtures/associated_families/composed_projection_constraints.fe");
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("composed_projection.fe"), "");
    for (input, output, expected_error) in [
        ("u256", "u256", None),
        ("u256", "bool", Some("type mismatch")),
        ("u8", "u256", Some("trait bound is not satisfied")),
        ("u256", "u256", None),
    ] {
        let source = template
            .replace(
                "<Program as Eval<u256>>",
                &format!("<Program as Eval<{input}>>"),
            )
            .replace("-> u256 { x }", &format!("-> {output} {{ x }}"));
        file.set_text(&mut db).to(source.clone());
        let result = diagnostics(&db, file);
        if let Some(message) = expected_error {
            assert!(result.contains(message), "{result}");
        } else {
            assert!(result.is_empty(), "{result}");
        }
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file =
            fresh.new_stand_alone(Utf8PathBuf::from("composed_projection.fe"), &source);
        assert_eq!(result, diagnostics(&fresh, fresh_file));
    }
}

/// Checks each source in a fresh database and in one edited database, and
/// expects no diagnostics (`None`) or a diagnostic containing the text.
fn check_cases(cases: &[(&str, Option<&str>)]) {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("cases.fe"), "");
    for &(source, expected) in cases {
        file.set_text(&mut db).to(source.to_owned());
        let result = diagnostics(&db, file);
        match expected {
            None => assert!(result.is_empty(), "{source}\n{result}"),
            Some(text) => assert!(
                result.contains(text),
                "expected `{text}`\n{source}\n{result}"
            ),
        }
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file = fresh.new_stand_alone(Utf8PathBuf::from("cases.fe"), source);
        assert_eq!(result, diagnostics(&fresh, fresh_file), "{source}");
    }
}

#[test]
fn family_bounds_hold_on_another_instance_of_the_trait() {
    check_cases(&[
        (
            r#"
trait Show { fn show(self) -> u8 }
trait Tr {
    type Next: Tr
    type Out<T>: Show
    fn get<T>(_ x: <Self::Next as Tr>::Out<T>) -> u8 { x.show() }
}
"#,
            None,
        ),
        (
            r#"
trait Show { fn show(self) -> u8 }
trait Tr {
    type Next: Tr
    type Out<T: Show>: Show = <Self::Next as Tr>::Out<T>
}
"#,
            None,
        ),
        (
            r#"
trait Show { fn show(self) -> u8 }
trait Tr {
    type Next: Tr
    type Out<T: Show>: Show = <Self::Next as Tr>::Out<bool>
}
"#,
            Some("trait bound is not satisfied"),
        ),
    ]);
}

#[test]
fn family_parameter_bounds_hold_for_types_written_in_bodies() {
    let template = r#"
trait Allowed {}
impl Allowed for bool {}
trait Factory { type Out<T: Allowed> }
struct Provider {}
impl Factory for Provider { type Out<T> = u256 }
USE
"#;
    for use_site in [
        "fn local() { let _x: Provider::Out<ARG> = 1 }",
        "fn cast() -> u256 { 1 as Provider::Out<ARG> }",
        "fn generic_local<B: Factory>(x: B::Out<bool>) { let _y: B::Out<ARG> = x }",
    ] {
        let template = template.replace("USE", use_site);
        let valid = template.replace("ARG", "bool");
        let invalid = template.replace("ARG", "u256");
        check_cases(&[
            (&valid, None),
            (&invalid, Some("trait bound is not satisfied")),
            (&valid, None),
        ]);
    }
}

#[test]
fn growing_projections_stop_at_the_normalization_limit() {
    for source in [
        "trait Tr { type Out }\nstruct W<T> { t: T }\nimpl<T> Tr for W<T> { type Out = <W<(T, T)> as Tr>::Out }\nfn f(x: <W<u8> as Tr>::Out) {}",
        "trait Tr { type Out }\nstruct W<T> { t: T }\nimpl<T> Tr for W<T> { type Out = <W<(T,)> as Tr>::Out }\nfn f(x: <W<u8> as Tr>::Out) {}",
    ] {
        check_cases(&[(source, Some("type normalization limit exceeded"))]);
    }
}

#[test]
fn associated_type_bounds_name_a_missing_trait() {
    check_cases(&[
        ("trait T1 { type Bar: T2 }", Some("`T2` is not found")),
        ("trait T1 { type Bar<T>: T2 }", Some("`T2` is not found")),
        ("trait T1 { type Bar<T: T2> }", Some("`T2` is not found")),
    ]);
}
