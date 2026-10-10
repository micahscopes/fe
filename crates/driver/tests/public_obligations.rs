//! Public obligation surface tests. Goal formation does not establish proof.
use common::InputDb;
use dir_test::{Fixture, dir_test};
use fe_driver::DriverDataBase;
use hir::analysis::semantic::GenericSubst;
use hir::analysis::ty::ty_def::{Kind, TyId};

#[test]
fn constraint_parameters_have_a_distinct_kind() {
    let mut db = DriverDataBase::default();
    let file = db.workspace().update(
        &mut db,
        "file:///constraint_kind.fe".parse().unwrap(),
        "fn goal<G: Constraint>() {}\nfn family<P: * -> Constraint>() {}".into(),
    );
    let module = db.top_mod(file);
    let diagnostics = db.run_on_ingot(module.ingot(&db));
    assert!(
        !diagnostics.has_errors(&db),
        "{}",
        diagnostics.format_diags(&db)
    );
    for function in module.all_funcs(&db) {
        let Kind::Abs(parameters) = TyId::func(&db, (*function).into()).kind(&db) else {
            panic!("expected a generic function constructor");
        };
        let kind = parameters.0.clone();
        let name = function.name(&db).to_opt().unwrap();
        match name.data(&db).as_str() {
            "goal" => assert_eq!(kind, Kind::Constraint),
            "family" => assert_eq!(kind, Kind::Abs(Box::new((Kind::Star, Kind::Constraint)))),
            other => panic!("unexpected function {other}"),
        }
    }
    assert!(!Kind::Constraint.does_match(&Kind::Star));
    assert!(!Kind::Star.does_match(&Kind::Constraint));
}

#[test]
fn ordinary_types_cannot_fill_constraint_parameters() {
    let mut db = DriverDataBase::default();
    let file = db.workspace().update(
        &mut db,
        "file:///constraint_kind_mismatch.fe".parse().unwrap(),
        "fn goal<G: Constraint>() {}\nfn misuse() { goal<u256>() }".into(),
    );
    let module = db.top_mod(file);
    let diagnostics = db.run_on_ingot(module.ingot(&db));
    assert!(diagnostics.has_errors(&db));
    let rendered = diagnostics.format_diags(&db);
    assert!(
        rendered.contains("error[3-0001]: invalid type argument kind"),
        "{rendered}"
    );
    assert!(
        rendered.contains("expected `Constraint` kind"),
        "{rendered}"
    );
    assert!(rendered.contains("u256"), "{rendered}");
}

fn source_diagnostics(source: &str) -> String {
    let mut db = DriverDataBase::default();
    let file = db.workspace().update(
        &mut db,
        "file:///constraint_terms.fe".parse().unwrap(),
        source.into(),
    );
    let module = db.top_mod(file);
    let diagnostics = db.run_on_ingot(module.ingot(&db));
    if diagnostics.has_errors(&db) {
        diagnostics.format_diags(&db)
    } else {
        String::new()
    }
}

#[test]
fn forming_a_trait_goal_does_not_require_or_establish_an_implementation() {
    let source = r#"
trait Produce { type Output }
struct Subject {}
struct Goal<G: Constraint> {}
fn forward<G: Constraint>(_ goal: Goal<G>) -> Goal<G> { goal }
fn form() -> Goal<Produce<Subject>> { forward(Goal<Produce<Subject>> {}) }
"#;
    let diagnostics = source_diagnostics(source);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let diagnostics = source_diagnostics(&format!(
        "{source}\nfn require<T: Produce>() {{}}\nfn misuse() {{ let goal = form()\nrequire<Subject>() }}"
    ));
    assert!(diagnostics.contains("Produce"), "{diagnostics}");
    assert!(!diagnostics.is_empty());
}

#[test]
fn trait_goal_subject_identity_survives_forwarding() {
    let diagnostics = source_diagnostics(
        r#"
trait Produce { type Output }
struct A {}
struct B {}
struct Goal<G: Constraint> {}
fn forward<G: Constraint>(_ goal: Goal<G>) -> Goal<G> { goal }
fn wrong() -> Goal<Produce<B>> { forward(Goal<Produce<A>> {}) }
"#,
    );
    assert!(diagnostics.contains("type mismatch"), "{diagnostics}");
}

#[test]
fn goal_term_equality_cannot_drop_associated_bindings() {
    let prefix = "trait Produce { type Output }\nstruct A {}\nstruct Goal<G: Constraint> {}\n";
    for (input, output) in [
        ("Produce<A, Output = u64>", "Produce<A>"),
        ("Produce<A>", "Produce<A, Output = u64>"),
        ("Produce<A, Output = u64>", "Produce<A, Output = bool>"),
    ] {
        let diagnostics = source_diagnostics(&format!(
            "{prefix}\nfn wrong(_ goal: Goal<{input}>) -> Goal<{output}> {{ goal }}"
        ));
        assert!(
            diagnostics.contains("type mismatch"),
            "{input} -> {output}: {diagnostics}"
        );
    }
}

#[test]
fn constraint_terms_cannot_be_runtime_tuple_elements() {
    let diagnostics =
        source_diagnostics("trait Produce {}\nstruct A {}\nfn wrong(_ value: (Produce<A>,)) {}");
    assert!(
        !diagnostics.is_empty(),
        "a goal is not a runtime value type"
    );
}

#[test]
fn goal_terms_retain_argument_and_visibility_checks() {
    for source in [
        "trait Produce {}\nstruct Goal<G: Constraint> {}\nfn bad() -> Goal<Produce<3>> { Goal<Produce<3>> {} }",
        "trait Produce { type Output }\nstruct Goal<G: Constraint> {}\nfn bad() -> Goal<Produce<u64, Missing = bool>> { Goal<Produce<u64, Missing = bool>> {} }",
        "mod hidden { trait Private {} }\nstruct Goal<G: Constraint> {}\nfn bad() -> Goal<hidden::Private<u64>> { Goal<hidden::Private<u64>> {} }",
        "trait Produce {}\nmod hidden { struct Private {} }\nstruct Goal<G: Constraint> {}\nfn bad() -> Goal<Produce<hidden::Private>> { Goal<Produce<hidden::Private>> {} }",
    ] {
        let diagnostics = source_diagnostics(source);
        assert!(
            !diagnostics.is_empty(),
            "invalid goal was accepted: {source}"
        );
    }
}

#[test]
fn ordinary_source_cannot_construct_evidence() {
    let diagnostics = source_diagnostics(
        r#"
use core::obligations::Evidence
trait Produce {}
struct Subject {}
const fn counterfeit() -> Evidence<Produce<Subject>> {
    Evidence<Produce<Subject>> { seal: true }
}

"#,
    );
    assert!(
        diagnostics.contains("`seal` is not visible"),
        "{diagnostics}"
    );
}

#[test]
fn evidence_introduction_requires_the_complete_goal() {
    use hir::analysis::{semantic::eval_body_owner_const_with_args, ty::ty_check::BodyOwner};
    for (implementation, goal) in [
        ("", "Produce<Subject>"),
        (
            "impl Produce for Subject { type Output = u64 }",
            "Produce<Subject, Output = bool>",
        ),
        (
            "impl<T: Required> Produce for Wrapper<T> { type Output = u64 }",
            "Produce<Wrapper<Subject>>",
        ),
    ] {
        let mut db = DriverDataBase::default();
        let source = format!(
            "use core::obligations::{{Evidence, prove}}\n\
             trait Produce {{ type Output }}\ntrait Required {{}}\n\
             struct Subject {{}}\nstruct Wrapper<T> {{ value: T }}\n\
             {implementation}\n\
             const fn introduce() -> Evidence<{goal}> {{ prove<{goal}>() }}"
        );
        let file = db.workspace().update(
            &mut db,
            "file:///invalid_evidence.fe".parse().unwrap(),
            source,
        );
        let module = db.top_mod(file);
        let diagnostics = db.run_on_ingot(module.ingot(&db)).format_diags(&db);
        assert!(
            diagnostics.contains("trait evidence introduction failed"),
            "{goal}: {diagnostics}"
        );
        let function = module
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(&db)
                    .to_opt()
                    .is_some_and(|name| name.data(&db) == "introduce")
            })
            .unwrap();
        let result = eval_body_owner_const_with_args(
            &db,
            BodyOwner::Func(function),
            GenericSubst::for_body_owner(&db, BodyOwner::Func(function), vec![]),
            vec![],
        );
        assert!(
            matches!(result, hir::analysis::semantic::EvalOutcome::Failed(_)),
            "unproved goal produced evidence: {goal}: {result:?}"
        );
    }
}

#[test]
fn unused_and_untaken_introductions_are_checked() {
    for body in [
        "let evidence = prove<Produce<Subject>>()",
        "if false { let evidence = prove<Produce<Subject>>() }",
    ] {
        let diagnostics = source_diagnostics(&format!(
            "use core::obligations::prove\ntrait Produce {{}}\nstruct Subject {{}}\n\
             const fn unused() {{ {body} }}"
        ));
        assert!(
            diagnostics.contains("trait evidence introduction failed"),
            "{diagnostics}"
        );
    }
}

#[test]
fn evidence_introduction_tracks_impl_edits_and_restoration() {
    let mut warm = DriverDataBase::default();
    for (support, output, accepted) in [
        (true, "u64", true),
        (false, "u64", false),
        (true, "u64", true),
        (true, "bool", false),
        (true, "u64", true),
    ] {
        let support = if support {
            "impl Required for Subject {}"
        } else {
            ""
        };
        let source = format!(
            "use core::obligations::{{Evidence, prove}}\n\
             trait Required {{}}\ntrait Produce {{ type Output }}\n\
             struct Subject {{}}\nstruct Wrapper<T> {{ value: T }}\n\
             {support}\nimpl<T: Required> Produce for Wrapper<T> {{ type Output = {output} }}\n\
             const fn introduce() -> Evidence<Produce<Wrapper<Subject>, Output = u64>> {{\n\
                 prove<Produce<Wrapper<Subject>, Output = u64>>()\n}}"
        );
        let mut fresh = DriverDataBase::default();
        for db in [&mut warm, &mut fresh] {
            let file = db.workspace().update(
                db,
                "file:///evidence_edit.fe".parse().unwrap(),
                source.clone(),
            );
            let module = db.top_mod(file);
            let diagnostics = db.run_on_ingot(module.ingot(db));
            assert_eq!(
                !diagnostics.has_errors(db),
                accepted,
                "{}",
                diagnostics.format_diags(db)
            );
        }
    }
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/public_obligations",
    glob: "evidence_forwarding.fe"
)]
fn checked_evidence_survives_ordinary_const_function_forwarding(fixture: Fixture<&str>) {
    use hir::analysis::{
        semantic::{SemConstValue, StructuralHandle, eval_body_owner_const_with_args},
        ty::ty_check::BodyOwner,
    };
    let mut db = DriverDataBase::default();
    let file = db.workspace().update(
        &mut db,
        url::Url::from_file_path(fixture.path()).unwrap(),
        fixture.content().to_string(),
    );
    let module = db.top_mod(file);
    let diagnostics = db.run_on_ingot(module.ingot(&db));
    assert!(
        !diagnostics.has_errors(&db),
        "{}",
        diagnostics.format_diags(&db)
    );
    let evaluate = |name: &str| {
        let function = module
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(&db)
                    .to_opt()
                    .is_some_and(|ident| ident.data(&db) == name)
            })
            .unwrap();
        eval_body_owner_const_with_args(
            &db,
            BodyOwner::Func(function),
            GenericSubst::for_body_owner(&db, BodyOwner::Func(function), vec![]),
            vec![],
        )
        .into_ready()
        .expect("checked evidence evaluates")
    };
    let introduced = evaluate("introduce");
    let forwarded = evaluate("answer");
    assert_eq!(introduced, forwarded);
    let SemConstValue::Structural {
        handle: StructuralHandle::Evidence(selection),
        ..
    } = forwarded.value(&db)
    else {
        panic!("expected compiler-issued evidence");
    };
    let goal = selection.goal();
    assert_eq!(goal.self_ty(&db).pretty_print(&db), "Subject");
    let checked = selection
        .revalidate(
            &db,
            hir::semantic::context_at(&db, selection.scope(), None).solver_context(&db),
            goal,
        )
        .expect("forwarded evidence must retain a valid selected application");
    assert_eq!(checked.goal(), goal);
    assert_eq!(goal.assoc_type_bindings(&db).len(), 1);
}

#[test]
fn explicit_opening_supplies_a_premise_and_executes_the_selected_method() {
    use hir::analysis::{
        semantic::{SemConstScalar, SemConstValue, eval_body_owner_const_with_args},
        ty::ty_check::BodyOwner,
    };
    let mut db = DriverDataBase::default();
    let file = db.workspace().update(
        &mut db,
        "file:///open_evidence.fe".parse().unwrap(),
        r#"
use core::obligations::{Evidence, prove}
trait Produce {
    type Output
    const fn produce() -> Self::Output
}
struct Subject {}
impl Produce for Subject {
    type Output = u64
    const fn produce() -> u64 { 42 }
}
const fn forward<G: Constraint>(_ evidence: Evidence<G>) -> Evidence<G> { evidence }
const fn consume<T, Output>(_ evidence: Evidence<Produce<T, Output = Output>>) -> Output {
    with (evidence) { T::produce() }
}
const fn answer() -> u64 {
    consume(forward(prove<Produce<Subject, Output = u64>>()))
}
"#
        .into(),
    );
    let module = db.top_mod(file);
    let diagnostics = db.run_on_ingot(module.ingot(&db));
    assert!(
        !diagnostics.has_errors(&db),
        "{}",
        diagnostics.format_diags(&db)
    );
    let function = module
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|function| {
            function
                .name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "answer")
        })
        .unwrap();
    let value = eval_body_owner_const_with_args(
        &db,
        BodyOwner::Func(function),
        GenericSubst::for_body_owner(&db, BodyOwner::Func(function), vec![]),
        vec![],
    )
    .into_ready()
    .expect("opened evidence evaluates");
    let SemConstValue::Scalar {
        value: SemConstScalar::Int { value },
        ..
    } = value.value(&db)
    else {
        panic!("expected integer payload")
    };
    assert_eq!(value.to_string(), "42");
}

#[test]
fn evidence_premises_do_not_escape_the_opening_scope() {
    let diagnostics = source_diagnostics(
        r#"
use core::obligations::Evidence
trait Produce { const fn produce() -> u64 }
const fn outside<T>(_ evidence: Evidence<Produce<T>>) -> u64 {
    with (evidence) { let value = T::produce() }
    T::produce()
}
"#,
    );
    assert!(!diagnostics.is_empty(), "opening leaked a trait premise");
    assert!(diagnostics.contains("produce"), "{diagnostics}");
}

#[test]
fn opening_does_not_prove_method_specific_requirements() {
    let diagnostics = source_diagnostics(
        r#"
use core::obligations::Evidence
trait Gate {}
trait Produce { const fn produce() -> u64 where Self: Gate }
const fn consume<T>(_ evidence: Evidence<Produce<T>>) -> u64 {
    with (evidence) { T::produce() }
}
"#,
    );
    assert!(
        !diagnostics.is_empty(),
        "trait evidence supplied a missing method requirement"
    );
    assert!(diagnostics.contains("Gate"), "{diagnostics}");
}

#[test]
fn unrelated_source_types_cannot_open_trait_premises() {
    let diagnostics = source_diagnostics(
        r#"
mod fake { pub struct Evidence<G: Constraint> {} }
trait Produce { const fn produce() -> u64 }
const fn consume<T>(_ evidence: fake::Evidence<Produce<T>>) -> u64 {
    with (evidence) { T::produce() }
}
"#,
    );
    assert!(
        !diagnostics.is_empty(),
        "source marker was treated as checked evidence"
    );
    assert!(diagnostics.contains("produce"), "{diagnostics}");
}

#[test]
fn opening_cannot_silently_dispatch_an_inherited_trait() {
    let diagnostics = source_diagnostics(
        r#"
use core::obligations::Evidence
trait Base { const fn base() -> u64 }
trait Derived: Base {}
const fn consume<T>(_ evidence: Evidence<Derived<T>>) -> u64 {
    with (evidence) { T::base() }
}
"#,
    );
    assert!(
        diagnostics.contains("derived selection transport"),
        "{diagnostics}"
    );
}

fn opening_answer(db: &mut DriverDataBase, source: &str) -> Result<String, String> {
    use hir::analysis::{
        semantic::{SemConstScalar, SemConstValue, eval_body_owner_const_with_args},
        ty::ty_check::BodyOwner,
    };
    let file = db.workspace().update(
        db,
        "file:///opening_edits.fe".parse().unwrap(),
        source.into(),
    );
    let module = db.top_mod(file);
    let diagnostics = db.run_on_ingot(module.ingot(db));
    if diagnostics.has_errors(db) {
        return Err(diagnostics.format_diags(db));
    }
    let function = module
        .all_funcs(db)
        .iter()
        .copied()
        .find(|func| {
            func.name(db)
                .to_opt()
                .is_some_and(|name| name.data(db) == "answer")
        })
        .unwrap();
    let result = match eval_body_owner_const_with_args(
        db,
        BodyOwner::Func(function),
        GenericSubst::for_body_owner(db, BodyOwner::Func(function), vec![]),
        vec![],
    ) {
        hir::analysis::semantic::EvalOutcome::Ready(value) => value,
        other => return Err(format!("{other:?}")),
    };
    match result.value(db) {
        SemConstValue::Scalar {
            value: SemConstScalar::Int { value },
            ..
        } => Ok(value.to_string()),
        other => Err(format!("expected concrete integer, got {other:?}")),
    }
}

#[test]
fn opened_dispatch_tracks_body_goal_and_prerequisite_edits() {
    let source = include_str!("../../fe/tests/fixtures/fe_test/public_obligations.fe");
    let bounded = source.replace(
        "impl Produce for Subject {",
        "trait Gate {}\nimpl Gate for Subject {}\nimpl<T: Gate> Produce for T {",
    );
    let mut warm = DriverDataBase::default();
    for (source, expected) in [
        (source.to_owned(), Some("42")),
        (source.replace("{ 42 }", "{ 73 }"), Some("73")),
        (source.to_owned(), Some("42")),
        (bounded.clone(), Some("42")),
        (bounded.replace("impl Gate for Subject {}", ""), None),
        (bounded, Some("42")),
        (
            source
                .replace("type Output = u64", "type Output = bool")
                .replace(
                    "const fn produce() -> u64 { 42 }",
                    "const fn produce() -> bool { true }",
                ),
            None,
        ),
        (source.to_owned(), Some("42")),
        (source.replace("Subject", "RenamedSubject"), Some("42")),
        (source.to_owned(), Some("42")),
    ] {
        let observed = opening_answer(&mut warm, &source);
        let fresh = opening_answer(&mut DriverDataBase::default(), &source);
        assert_eq!(observed, fresh, "warm/fresh mismatch");
        match expected {
            Some(expected) => assert_eq!(observed.as_deref(), Ok(expected)),
            None => assert!(
                observed.is_err(),
                "invalid selected application was accepted"
            ),
        }
    }
}

#[test]
fn qualified_opening_retains_lexical_premises_through_deferred_checking() {
    let source = r#"
use core::obligations::{Evidence, prove}
trait Produce<Mode> {
    type Output
    const fn produce() -> <Self as Produce<Mode>>::Output
}
struct Subject {}
struct Number {}
struct Flag {}
impl Produce<Number> for Subject {
    type Output = u64
    const fn produce() -> u64 { 42 }
}
impl Produce<Flag> for Subject {
    type Output = bool
    const fn produce() -> bool { true }
}
const fn inner<T, M, O>(_ evidence: Evidence<Produce<T, M, Output = O>>) -> O {
    with (evidence) { <T as Produce<M>>::produce() }
}
const fn outer<T, M, O>(_ evidence: Evidence<Produce<T, M, Output = O>>) -> O {
    inner(evidence)
}
const fn answer() -> u64 {
    let number = outer(prove<Produce<Subject, Number, Output = u64>>())
    let flag = outer(prove<Produce<Subject, Flag, Output = bool>>())
    if flag { number } else { 0 }
}
"#;
    let mut db = DriverDataBase::default();
    assert_eq!(opening_answer(&mut db, source).unwrap(), "42");

    // A queued use outside the opening cannot borrow an earlier use's premises.
    let escaped = source.replace(
        "with (evidence) { <T as Produce<M>>::produce() }",
        "let inside = with (evidence) { <T as Produce<M>>::produce() }\n<T as Produce<M>>::produce()",
    );
    let diagnostics = source_diagnostics(&escaped);
    assert!(
        diagnostics.contains("trait bound is not satisfied"),
        "the opened premise escaped its lexical scope: {diagnostics}"
    );
    let edited = opening_answer(&mut db, &escaped).unwrap_err();
    assert!(edited.contains("trait bound is not satisfied"), "{edited}");
    assert_eq!(opening_answer(&mut db, source).unwrap(), "42");
}

#[test]
fn nested_openings_retain_each_goal_and_restore_outer_dispatch() {
    let source = include_str!("../../fe/tests/fixtures/fe_test/public_obligations.fe").replace(
        "consume(forward(prove<Produce<Subject, Output = u64>>()))",
        "pair(prove<Produce<Subject, Output = u64>>(), prove<Produce<Flag, Output = bool>>())",
    );
    let source = format!(
        r#"{source}
struct Flag {{}}
impl Produce for Flag {{
    type Output = bool
    const fn produce() -> bool {{ true }}
}}
const fn pair<T, U>(_ first: Evidence<Produce<T, Output = u64>>, _ second: Evidence<Produce<U, Output = bool>>) -> u64 {{
    with (first) {{
        let before = T::produce()
        let flag = with (second) {{ U::produce() }}
        let after = T::produce()
        if flag {{ before + after }} else {{ 0 }}
    }}
}}
"#
    );
    assert_eq!(
        opening_answer(&mut DriverDataBase::default(), &source).as_deref(),
        Ok("84")
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/public_obligations",
    glob: "evidence_wrong_goal.fe"
)]
fn opening_rejects_mismatched_evidence_goal(fixture: Fixture<&str>) {
    use hir::analysis::{
        semantic::{
            SelectedApplication, SemConstId, SemConstValue, StructuralHandle,
            eval_body_owner_const_with_args,
        },
        ty::{trait_def::prove_selected_hir_impl_evidence, ty_check::BodyOwner},
    };
    let mut db = DriverDataBase::default();
    let file = db.workspace().update(
        &mut db,
        url::Url::from_file_path(fixture.path()).unwrap(),
        fixture.content().to_string(),
    );
    let module = db.top_mod(file);
    let diagnostics = db.run_on_ingot(module.ingot(&db));
    assert!(
        !diagnostics.has_errors(&db),
        "{}",
        diagnostics.format_diags(&db)
    );
    let named = |name: &str| {
        module
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|func| {
                func.name(&db)
                    .to_opt()
                    .is_some_and(|ident| ident.data(&db) == name)
            })
            .unwrap()
    };
    let obtain = |name| {
        eval_body_owner_const_with_args(
            &db,
            BodyOwner::Func(named(name)),
            GenericSubst::for_body_owner(&db, BodyOwner::Func(named(name)), vec![]),
            vec![],
        )
        .into_ready()
        .expect("evidence provider evaluates")
    };
    let evidence_a = obtain("evidence_a");
    let evidence_b = obtain("evidence_b");
    let SemConstValue::Structural {
        ty,
        handle: StructuralHandle::Evidence(selection),
    } = evidence_a.value(&db)
    else {
        panic!("evidence A")
    };
    let SemConstValue::Structural {
        handle: StructuralHandle::Evidence(other),
        ..
    } = evidence_b.value(&db)
    else {
        panic!("evidence B")
    };
    assert_ne!(selection.goal(), other.goal());
    let goal = selection.goal();
    let output = *goal.assoc_type_bindings(&db).values().next().unwrap();
    let invoke = |value| {
        eval_body_owner_const_with_args(
            &db,
            BodyOwner::Func(named("consume")),
            GenericSubst::for_body_owner(
                &db,
                BodyOwner::Func(named("consume")),
                vec![goal.self_ty(&db), output],
            ),
            vec![value],
        )
    };
    assert!(matches!(
        invoke(evidence_a),
        hir::analysis::semantic::EvalOutcome::Ready(_)
    ));
    let selected = prove_selected_hir_impl_evidence(
        &db,
        hir::semantic::context_at(&db, module.scope(), None).solver_context(&db),
        module.all_impl_traits(&db)[1],
        goal,
    )
    .expect("named alternative should prove the same goal");
    let alternative = SemConstId::new(
        &db,
        SemConstValue::Structural {
            ty: *ty,
            handle: StructuralHandle::Evidence(SelectedApplication::from_evidence(&selected)),
        },
    );
    assert_eq!(
        invoke(alternative)
            .into_ready()
            .expect("selected alternative evaluates")
            .pretty_print(&db),
        "77"
    );
    // The selected implementation and its goal travel together. This value
    // has the expected source type, but carries evidence for another goal.
    let forged = SemConstId::new(
        &db,
        SemConstValue::Structural {
            ty: *ty,
            handle: StructuralHandle::Evidence(*other),
        },
    );
    let error = invoke(forged);
    assert!(matches!(
        &error,
        hir::analysis::semantic::EvalOutcome::Failed(_)
    ));
    assert!(
        format!("{error:?}").contains("opened evidence does not prove"),
        "{error:?}"
    );
}

#[test]
fn selected_evidence_revalidates_after_consumer_context_moves() {
    let source = include_str!("../../fe/tests/fixtures/fe_test/public_obligations.fe");
    let consume = "const fn consume<T, Output>(_ evidence: Evidence<Produce<T, Output = Output>>) -> Output {\n    with (evidence) { T::produce() }\n}";
    assert!(source.contains(consume));
    let moved = source.replace("trait Produce {", "pub trait Produce {")
        .replace(consume, &format!("mod consumer {{\nuse core::obligations::Evidence\nuse ingot::Produce\npub {consume}\n}}"))
        .replace("    consume(forward(", "    consumer::consume(forward(");
    let mut warm = DriverDataBase::default();
    for source in [
        source.to_owned(),
        moved.clone(),
        moved.replace("consumer", "receiver"),
        source.to_owned(),
    ] {
        let observed = opening_answer(&mut warm, &source);
        let fresh = opening_answer(&mut DriverDataBase::default(), &source);
        assert_eq!(observed, fresh);
        assert_eq!(observed.as_deref(), Ok("42"));
    }
}
