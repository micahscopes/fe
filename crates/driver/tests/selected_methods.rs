use common::{
    InputDb,
    file::File,
    indexmap::IndexMap,
    stdlib::{HasBuiltinCore, HasBuiltinStd},
};
use fe_driver::{
    DriverDataBase,
    selected_methods::{SelectedMethodError, evaluate_selected_const_method},
};
use hir::{
    analysis::{
        semantic::eval_body_owner_const_with_args,
        ty::{trait_def::TraitInstId, ty_check::BodyOwner},
    },
    hir_def::{Func, IdentId},
};
use url::Url;

fn database() -> DriverDataBase {
    let mut db = DriverDataBase::default();
    db.initialize_builtin_core();
    db.initialize_builtin_std();
    db
}
fn input(db: &mut DriverDataBase, source: &str) -> File {
    use salsa::Setter;
    let file = db.workspace().touch(
        db,
        Url::parse("file:///selected-methods/main.fe").unwrap(),
        Some(source.into()),
    );
    file.set_text(db).to(source.into());
    file
}
fn named<'db>(db: &'db DriverDataBase, file: File, name: &str) -> Func<'db> {
    db.top_mod(file)
        .all_funcs(db)
        .iter()
        .copied()
        .find(|f| f.name(db).to_opt().is_some_and(|id| id.data(db) == name))
        .unwrap()
}
fn evaluate(
    db: &DriverDataBase,
    file: File,
    target: &str,
    index: usize,
    method: &str,
) -> Result<String, String> {
    let top = db.top_mod(file);
    let trait_ = top
        .all_traits(db)
        .iter()
        .copied()
        .find(|t| {
            t.name(db)
                .to_opt()
                .is_some_and(|id| id.data(db) == "Compute")
        })
        .unwrap();
    let goal = TraitInstId::new(
        db,
        trait_,
        vec![named(db, file, target).return_ty(db)],
        IndexMap::new(),
    );
    evaluate_selected_const_method(
        db,
        file,
        top.all_impl_traits(db)[index],
        goal,
        IdentId::new(db, method.to_string()),
    )
    .map(|value| value.pretty_print(db))
    .map_err(|error| match error {
        SelectedMethodError::Source(message) => format!("source: {message}"),
        error => format!("{error:?}"),
    })
}
const SOURCE: &str = "trait Compute { const fn compute() -> u256 }\nstruct Subject {}\nimpl Compute for Subject { const fn compute() -> u256 { 42 } }\nfn target() -> Subject { Subject {} }\nconst fn ordinary() -> u256 { Subject::compute() }";

#[test]
fn selected_method_executes_and_matches_an_ordinary_call() {
    let mut db = database();
    let file = input(&mut db, SOURCE);
    assert_eq!(evaluate(&db, file, "target", 0, "compute"), Ok("42".into()));
    assert_eq!(
        (match eval_body_owner_const_with_args(
            &db,
            BodyOwner::Func(named(&db, file, "ordinary")),
            hir::analysis::semantic::GenericSubst::for_body_owner(
                &db,
                BodyOwner::Func(named(&db, file, "ordinary")),
                vec![]
            ),
            vec![]
        ) {
            hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
            outcome => Err(outcome),
        })
        .unwrap()
        .pretty_print(&db),
        "42"
    );
    assert_eq!(
        evaluate(&db, file, "target", 0, "missing"),
        Err("MissingMethod".into())
    );
}

#[test]
fn selected_method_substitutes_impl_arguments_and_rechecks_edits() {
    for first in [false, true] {
        let mut db = database();
        for n in [1, 7, 1] {
            let source = format!(
                "trait Compute {{ const fn compute() -> u256 }}\nstruct Subject<const N: usize> {{}}\nimpl<const N: usize> Compute for Subject<N> {{ const fn compute() -> u256 {{ N as u256 }} }}\nfn target() -> Subject<{n}> {{ Subject {{}} }}"
            );
            let file = input(&mut db, &source);
            if first {
                let _ = named(&db, file, "compute").return_ty(&db);
            }
            let warm = evaluate(&db, file, "target", 0, "compute");
            assert_eq!(warm, Ok(n.to_string()));
            let mut fresh = database();
            let fresh_file = input(&mut fresh, &source);
            assert_eq!(warm, evaluate(&fresh, fresh_file, "target", 0, "compute"));
        }
    }
}

#[test]
fn selected_method_certifies_source_before_execution() {
    for body in ["true", "if false { let invalid: bool = 1 }\n42"] {
        let mut db = database();
        let source = SOURCE.replace("{ 42 }", &format!("{{ {body} }}"));
        let file = input(&mut db, &source);
        assert!(
            evaluate(&db, file, "target", 0, "compute")
                .unwrap_err()
                .starts_with("source:")
        );
    }
    let mut db = database();
    let source = SOURCE.replace("{ 42 }", "{ let zero: u256 = 0\n1 / zero }");
    let file = input(&mut db, &source);
    assert!(
        evaluate(&db, file, "target", 0, "compute")
            .unwrap_err()
            .starts_with("Execution(")
    );
}

#[test]
fn selected_method_cannot_switch_to_another_impl_or_relax_coherence() {
    let mut db = database();
    let source = "trait Compute { const fn compute() -> u256 }\nstruct A {}\nstruct B {}\nimpl Compute for A { const fn compute() -> u256 { 10 } }\nimpl Compute for B { const fn compute() -> u256 { 20 } }\nfn target_a() -> A { A {} }\nfn target_b() -> B { B {} }";
    let file = input(&mut db, source);
    assert_eq!(
        evaluate(&db, file, "target_a", 0, "compute"),
        Ok("10".into())
    );
    assert_eq!(
        evaluate(&db, file, "target_b", 1, "compute"),
        Ok("20".into())
    );
    assert_eq!(
        evaluate(&db, file, "target_b", 0, "compute"),
        Err("Evidence(NotProven)".into())
    );
    let file = input(&mut db, &source.replace("for B", "for A"));
    assert!(
        evaluate(&db, file, "target_a", 0, "compute")
            .unwrap_err()
            .starts_with("source:")
    );
}

#[test]
fn selected_method_rejects_additional_method_obligations_and_default_bodies() {
    for source in [
        "trait Compute { const fn compute<const N: usize>() -> u256 }\nstruct Subject {}\nimpl Compute for Subject { const fn compute<const N: usize>() -> u256 { 42 } }\nfn target() -> Subject { Subject {} }",
        "trait Compute { const fn compute() -> u256 { 42 } }\nstruct Subject {}\nimpl Compute for Subject {}\nfn target() -> Subject { Subject {} }",
        "trait Compute { const fn compute(self) -> u256 }\nstruct Subject {}\nimpl Compute for Subject { const fn compute(self) -> u256 { 42 } }\nfn target() -> Subject { Subject {} }",
    ] {
        let mut db = database();
        let file = input(&mut db, source);
        assert_eq!(
            evaluate(&db, file, "target", 0, "compute"),
            Err("Evidence(Unsupported)".into()),
            "{source}"
        );
    }
}

#[test]
fn selected_method_rejects_unsafe_nonconst_and_effectful_roots() {
    let base = SOURCE.split("\nconst fn ordinary").next().unwrap();
    let mut db = database();
    let file = input(&mut db, &base.replace("const fn compute", "fn compute"));
    assert_eq!(
        evaluate(&db, file, "target", 0, "compute"),
        Err("Evidence(Unsupported)".into())
    );
    let file = input(
        &mut db,
        &base.replace("const fn compute", "unsafe const fn compute"),
    );
    let error = evaluate(&db, file, "target", 0, "compute").unwrap_err();
    assert!(
        error.starts_with("source:") && error.contains("modifier is not allowed"),
        "{error}"
    );
    let source = format!(
        "struct Cap {{}}\n{}",
        base.replace("-> u256", "-> u256 uses (cap: Cap)")
    );
    let file = input(&mut db, &source);
    let error = evaluate(&db, file, "target", 0, "compute").unwrap_err();
    assert!(
        error.starts_with("source:") && error.contains("effects are not allowed"),
        "{error}"
    );
}

#[test]
fn selected_method_checks_unused_impl_prerequisites_and_ground_requests() {
    let mut db = database();
    let file = input(
        &mut db,
        "trait Marker {}\ntrait Compute { const fn compute() -> u256 }\nstruct Missing {}\nstruct Subject<T> {}\nimpl<T: Marker> Compute for Subject<T> { const fn compute() -> u256 { 42 } }\nfn target() -> Subject<Missing> { Subject {} }\nfn generic<T>() -> Subject<T> { Subject {} }",
    );
    assert_eq!(
        evaluate(&db, file, "target", 0, "compute"),
        Err("Evidence(NotProven)".into())
    );
    assert_eq!(
        evaluate(&db, file, "generic", 0, "compute"),
        Err("Unsupported".into())
    );
}

const BOUNDED_SOURCE: &str = "trait Gate {}\ntrait Compute { const fn compute() -> u256 where Self: Gate }\nstruct Subject {}\nimpl<T> Compute for T { const fn compute() -> u256 where T: Gate { 42 } }\nimpl Gate for Subject {}\nfn target() -> Subject { Subject {} }";

#[test]
fn selected_method_proves_local_trait_bounds_like_an_ordinary_call() {
    let mut db = database();
    let source = format!("{BOUNDED_SOURCE}\nconst fn ordinary() -> u256 {{ Subject::compute() }}");
    let file = input(&mut db, &source);
    assert_eq!(evaluate(&db, file, "target", 0, "compute"), Ok("42".into()));
    assert_eq!(
        (match eval_body_owner_const_with_args(
            &db,
            BodyOwner::Func(named(&db, file, "ordinary")),
            hir::analysis::semantic::GenericSubst::for_body_owner(
                &db,
                BodyOwner::Func(named(&db, file, "ordinary")),
                vec![]
            ),
            vec![]
        ) {
            hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
            outcome => Err(outcome),
        })
        .unwrap()
        .pretty_print(&db),
        "42"
    );
}

#[test]
fn selected_method_local_bounds_survive_edits_and_unused_bodies() {
    let mut db = database();
    for present in [true, false, true] {
        let source = if present {
            BOUNDED_SOURCE.to_owned()
        } else {
            BOUNDED_SOURCE.replace("impl Gate for Subject {}", "")
        };
        let file = input(&mut db, &source);
        let warm = evaluate(&db, file, "target", 0, "compute");
        let expected = if present {
            Ok("42".into())
        } else {
            Err("Evidence(NotProven)".into())
        };
        assert_eq!(warm, expected);
        let mut fresh = database();
        let fresh_file = input(&mut fresh, &source);
        assert_eq!(warm, evaluate(&fresh, fresh_file, "target", 0, "compute"));
    }
}

#[test]
fn selected_method_checks_trait_declaration_when_impl_omits_bound() {
    let source = BOUNDED_SOURCE.replace("where T: Gate { 42 }", "{ 42 }");
    for present in [true, false] {
        let source = if present {
            source.clone()
        } else {
            source.replace("impl Gate for Subject {}", "")
        };
        let mut db = database();
        let file = input(&mut db, &source);
        assert_eq!(
            evaluate(&db, file, "target", 0, "compute"),
            if present {
                Ok("42".into())
            } else {
                Err("Evidence(NotProven)".into())
            }
        );
    }
}

#[test]
fn selected_method_local_bound_search_remains_incomplete() {
    let source = BOUNDED_SOURCE.replace(
        "impl Gate for Subject {}",
        "struct Wrap<T> {}\nimpl<T> Gate for T where Wrap<T>: Gate {}",
    );
    let mut db = database();
    let file = input(&mut db, &source);
    assert_eq!(
        evaluate(&db, file, "target", 0, "compute"),
        Err("Evidence(Incomplete)".into())
    );
}

#[test]
fn selected_method_substitutes_distinct_trait_and_impl_parameter_slots() {
    let source = "trait Gate {}\ntrait Compute<T> { const fn compute() -> u256 where T: Gate }\nstruct Subject<A> {}\nimpl<A, B> Compute<B> for Subject<A> { const fn compute() -> u256 where B: Gate { 42 } }\nimpl Gate for u256 {}\nfn target() -> Subject<bool> { Subject {} }\nfn allowed() -> u256 { 0 }\nfn denied() -> bool { false }";
    let mut db = database();
    let file = input(&mut db, source);
    let top = db.top_mod(file);
    let compute = top
        .all_traits(&db)
        .iter()
        .copied()
        .find(|t| {
            t.name(&db)
                .to_opt()
                .is_some_and(|n| n.data(&db) == "Compute")
        })
        .unwrap();
    for (argument, expected) in [
        ("allowed", Ok("42".to_owned())),
        ("denied", Err("Evidence(NotProven)".to_owned())),
    ] {
        let goal = TraitInstId::new(
            &db,
            compute,
            vec![
                named(&db, file, "target").return_ty(&db),
                named(&db, file, argument).return_ty(&db),
            ],
            IndexMap::new(),
        );
        let actual = evaluate_selected_const_method(
            &db,
            file,
            top.all_impl_traits(&db)[0],
            goal,
            IdentId::new(&db, "compute".to_owned()),
        )
        .map(|v| v.pretty_print(&db))
        .map_err(|e| format!("{e:?}"));
        assert_eq!(actual, expected);
    }
}

#[test]
fn selected_method_does_not_silently_drop_projection_bounds() {
    let source = "trait Gate {}\ntrait Has { type Item }\ntrait Compute { const fn compute() -> u256 where Self: Has, Self::Item: Gate }\nstruct Subject {}\nimpl<T: Has> Compute for T { const fn compute() -> u256 where T::Item: Gate { 42 } }\nimpl Has for Subject { type Item = u256 }\nimpl Gate for u256 {}\nfn target() -> Subject { Subject {} }";
    let mut db = database();
    let file = input(&mut db, source);
    assert_eq!(
        evaluate(&db, file, "target", 0, "compute"),
        Err("Evidence(Unsupported)".into())
    );
}

#[test]
fn ordinary_call_rejects_the_same_missing_method_bound() {
    let source = format!(
        "{}\nconst fn ordinary() -> u256 {{ Subject::compute() }}",
        BOUNDED_SOURCE.replace("impl Gate for Subject {}", "")
    );
    let mut db = database();
    let file = input(&mut db, &source);
    let error = evaluate(&db, file, "target", 0, "compute").unwrap_err();
    assert!(
        error.starts_with("source:") && error.contains("Gate"),
        "{error}"
    );
}

const SELECTED_ASSOC_SOURCE: &str = "trait Gate {}\ntrait Compute { type Item\nconst fn compute() -> u256 where Self::Item: Gate }\nstruct Subject<T> {}\nimpl<T> Compute for Subject<T> { type Item = T\nconst fn compute() -> u256 where T: Gate { 42 } }\nimpl Gate for u256 {}\nfn target() -> Subject<u256> { Subject {} }";

#[test]
fn selected_method_uses_selected_associated_binding() {
    let mut db = database();
    let source = format!(
        "{SELECTED_ASSOC_SOURCE}\nconst fn ordinary() -> u256 {{ Subject<u256>::compute() }}"
    );
    let file = input(&mut db, &source);
    assert_eq!(evaluate(&db, file, "target", 0, "compute"), Ok("42".into()));
    assert_eq!(
        (match eval_body_owner_const_with_args(
            &db,
            BodyOwner::Func(named(&db, file, "ordinary")),
            hir::analysis::semantic::GenericSubst::for_body_owner(
                &db,
                BodyOwner::Func(named(&db, file, "ordinary")),
                vec![]
            ),
            vec![]
        ) {
            hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
            outcome => Err(outcome),
        })
        .unwrap()
        .pretty_print(&db),
        "42"
    );
}

#[test]
fn selected_method_associated_binding_edits_match_fresh() {
    let mut db = database();
    for ty in ["u256", "bool", "u256"] {
        let source = SELECTED_ASSOC_SOURCE
            .replace("type Item = T", &format!("type Item = {ty}"))
            .replace("where T: Gate { 42 }", "{ 42 }");
        let file = input(&mut db, &source);
        let result = evaluate(&db, file, "target", 0, "compute");
        assert_eq!(
            result,
            if ty == "u256" {
                Ok("42".into())
            } else {
                Err("Evidence(NotProven)".into())
            }
        );
        let mut fresh = database();
        let fresh_file = input(&mut fresh, &source);
        assert_eq!(result, evaluate(&fresh, fresh_file, "target", 0, "compute"));
    }
}

#[test]
fn selected_method_projection_substitution_preserves_trait_identity() {
    let source = "trait Gate {}\ntrait Has { type Item }\ntrait Compute { type Item\nconst fn compute() -> u256 where Self: Has, <Self as Has>::Item: Gate }\nstruct Subject {}\nimpl<T: Has> Compute for T { type Item = u256\nconst fn compute() -> u256 where <T as Has>::Item: Gate { 42 } }\nimpl Has for Subject { type Item = bool }\nimpl Gate for u256 {}\nfn target() -> Subject { Subject {} }";
    let mut db = database();
    let file = input(&mut db, source);
    assert_eq!(
        evaluate(&db, file, "target", 0, "compute"),
        Err("Evidence(Unsupported)".into())
    );
}

#[test]
fn selected_method_substitutes_nested_selected_projections() {
    let source = SELECTED_ASSOC_SOURCE
        .replace("Self::Item: Gate", "Wrap<Self::Item>: Gate")
        .replace("where T: Gate", "where Wrap<T>: Gate")
        .replace(
            "impl Gate for u256 {}",
            "struct Wrap<T> {}\nimpl Gate for Wrap<u256> {}",
        );
    let mut db = database();
    let file = input(&mut db, &source);
    assert_eq!(evaluate(&db, file, "target", 0, "compute"), Ok("42".into()));
    let file = input(&mut db, &source.replace("Subject<u256>", "Subject<bool>"));
    assert_eq!(
        evaluate(&db, file, "target", 0, "compute"),
        Err("Evidence(NotProven)".into())
    );
}

#[test]
fn selected_method_projection_substitution_preserves_trait_arguments() {
    let source = "trait Gate {}\ntrait Compute<T> { type Item\nconst fn compute() -> u256 where Self: Compute<bool>, <Self as Compute<bool>>::Item: Gate }\nstruct Subject<A> {}\nimpl<A, T> Compute<T> for Subject<A> { type Item = T\nconst fn compute() -> u256 where Self: Compute<bool>, <Self as Compute<bool>>::Item: Gate { 42 } }\nimpl Gate for u256 {}\nfn target() -> Subject<u256> { Subject {} }\nfn argument() -> u256 { 0 }";
    let mut db = database();
    let file = input(&mut db, source);
    let top = db.top_mod(file);
    let compute = top
        .all_traits(&db)
        .iter()
        .copied()
        .find(|t| {
            t.name(&db)
                .to_opt()
                .is_some_and(|n| n.data(&db) == "Compute")
        })
        .unwrap();
    let goal = TraitInstId::new(
        &db,
        compute,
        vec![
            named(&db, file, "target").return_ty(&db),
            named(&db, file, "argument").return_ty(&db),
        ],
        IndexMap::new(),
    );
    let result = evaluate_selected_const_method(
        &db,
        file,
        top.all_impl_traits(&db)[0],
        goal,
        IdentId::new(&db, "compute".to_owned()),
    )
    .map(|v| v.pretty_print(&db))
    .map_err(|e| format!("{e:?}"));
    assert_eq!(result, Err("Evidence(Unsupported)".into()));
}

#[test]
fn selected_method_does_not_search_through_unresolved_selected_binding() {
    let source = "trait Gate {}\ntrait Has { type Item }\ntrait Compute { type Item\nconst fn compute() -> u256 where Self::Item: Gate }\nstruct Subject<T> {}\nstruct Provided {}\nimpl<T: Has> Compute for Subject<T> { type Item = <T as Has>::Item\nconst fn compute() -> u256 { 42 } }\nimpl Has for Provided { type Item = u256 }\nimpl Gate for u256 {}\nfn target() -> Subject<Provided> { Subject {} }";
    let mut db = database();
    let file = input(&mut db, source);
    assert_eq!(
        evaluate(&db, file, "target", 0, "compute"),
        Err("Evidence(Unsupported)".into())
    );
}
