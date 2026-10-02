use common::{
    InputDb,
    file::File,
    stdlib::{HasBuiltinCore, HasBuiltinStd},
};
use fe_driver::{
    DriverDataBase,
    generation::{
        FunctionEmissionGrant, FunctionEmissionRequest, FunctionTemplate, GeneratedFunction,
        GenerationBudget, GenerationError, GenerationErrorKind, GenerationSession,
    },
};
use hir::{
    analysis::{semantic::eval_body_owner_const_with_args, ty::ty_check::BodyOwner},
    hir_def::Func,
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
        Url::parse("file:///emission/provider.fe").unwrap(),
        Some(source.into()),
    );
    file.set_text(db).to(source.into());
    file
}
fn named<'db>(db: &'db DriverDataBase, file: File) -> Func<'db> {
    db.top_mod(file)
        .all_funcs(db)
        .iter()
        .copied()
        .find(|f| f.name(db).to_opt().is_some_and(|n| n.data(db) == "provide"))
        .unwrap()
}
fn grants() -> Vec<FunctionEmissionGrant> {
    [7, 9]
        .into_iter()
        .map(|target| FunctionEmissionGrant {
            target,
            template: FunctionTemplate {
                url: Url::parse(&format!("file:///emission/target-{target}.fe")).unwrap(),
                source: format!("const fn target_{target}() -> u256 {{}}"),
                function_name: format!("target_{target}"),
            },
        })
        .collect()
}
fn request(key: &str) -> FunctionEmissionRequest {
    FunctionEmissionRequest {
        key: key.into(),
        grants: grants(),
    }
}
fn value(output: &GeneratedFunction) -> String {
    (match eval_body_owner_const_with_args(
        output.database(),
        BodyOwner::Func(output.function()),
        hir::analysis::semantic::GenericSubst::for_body_owner(
            output.database(),
            BodyOwner::Func(output.function()),
            vec![],
        ),
        vec![],
    ) {
        hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
        outcome => Err(outcome),
    })
    .unwrap()
    .pretty_print(output.database())
}
fn error(result: Result<GeneratedFunction, GenerationError>) -> GenerationError {
    match result {
        Ok(_) => panic!("unexpected emission"),
        Err(e) => e,
    }
}
const SOURCE: &str = r#"
use core::meta::FunctionEmission
trait Policy: Copy { const fn target(self) -> u256 }
struct Choice { strict: bool }
impl Copy for Choice {}
impl Policy for Choice {
    const fn target(self) -> u256 { if self.strict { 7 } else { 9 } }
}
const fn choose() -> FunctionEmission<u256> uses (policy: Policy) {
    FunctionEmission { target: policy.target(), value: 42 }
}
const fn provide() -> FunctionEmission<u256> {
    with (Policy = Choice { strict: true }) { choose() }
}
"#;

#[test]
fn scoped_fe_policy_selects_and_executes_an_authorized_declaration() {
    let mut db = database();
    let file = input(&mut db, SOURCE);
    let mut session = GenerationSession::new("stage".into(), GenerationBudget::new(1, 4096));
    let output = session
        .emit_value_function(&db, named(&db, file), request("select"))
        .unwrap();
    assert_eq!(value(&output), "42");
    assert!(output.source().contains("fn target_7"));
    assert!(!output.source().contains("fn target_9"));
    assert_eq!(output.provenance().emission_target, Some(7));
    assert_eq!(
        output.provenance().request_identity.as_ref().unwrap().key,
        "select"
    );
    assert_eq!(session.budget().used_functions(), 1);
}

#[test]
fn descriptor_construction_cannot_grant_authority_and_failures_reserve_keys() {
    let mut db = database();
    let file = input(&mut db, &SOURCE.replace("{ 7 }", "{ 123 }"));
    let mut session = GenerationSession::new("stage".into(), GenerationBudget::new(2, 4096));
    let err = error(session.emit_value_function(&db, named(&db, file), request("attempt")));
    assert_eq!(err.kind, GenerationErrorKind::Authority);
    assert_eq!(err.request_identity.unwrap().key, "attempt");
    assert!(err.provenance.is_none());
    assert_eq!(session.budget().used_functions(), 0);
    assert_eq!(session.budget().used_bytes(), 0);
    assert_eq!(
        error(session.emit_value_function(&db, named(&db, file), request("attempt"))).kind,
        GenerationErrorKind::DuplicateRequest
    );
    let file = input(&mut db, SOURCE);
    assert_eq!(
        value(
            &session
                .emit_value_function(&db, named(&db, file), request("retry"))
                .unwrap()
        ),
        "42"
    );
}

#[test]
fn grants_are_request_local_unique_and_bounded() {
    let mut db = database();
    let file = input(&mut db, SOURCE);
    let mut session = GenerationSession::new("stage".into(), GenerationBudget::new(5, 4096));
    session
        .emit_value_function(&db, named(&db, file), request("first"))
        .unwrap();
    for (key, grants) in [
        ("empty", vec![]),
        ("wrong-scope", vec![grants().remove(1)]),
        ("duplicate", vec![grants().remove(0), grants().remove(0)]),
        (
            "too-many",
            (0..65)
                .map(|n| {
                    let mut g = grants().remove(0);
                    g.target = n;
                    g
                })
                .collect(),
        ),
    ] {
        let err = error(session.emit_value_function(
            &db,
            named(&db, file),
            FunctionEmissionRequest {
                key: key.into(),
                grants,
            },
        ));
        assert_eq!(err.kind, GenerationErrorKind::Authority, "{key}: {err}");
    }
    assert_eq!(session.budget().used_functions(), 1);
    let mut small = GenerationSession::new("small".into(), GenerationBudget::new(1, 40));
    assert_eq!(
        error(small.emit_value_function(&db, named(&db, file), request("oversize"))).kind,
        GenerationErrorKind::Limit
    );
}

#[test]
fn source_protocol_and_output_are_independent_checks() {
    let cases = [
        (
            SOURCE.replace("value: 42", "value: true"),
            GenerationErrorKind::Provider,
        ),
        (
            SOURCE.replace(
                "{ choose() }",
                "{ if false { let wrong: bool = 1 }\nchoose() }",
            ),
            GenerationErrorKind::Provider,
        ),
        (
            SOURCE.replace(
                "use core::meta::FunctionEmission",
                "struct FunctionEmission<T> { target: u256, value: T }",
            ),
            GenerationErrorKind::Protocol,
        ),
        (
            SOURCE.replace("value: 42", "value: 1 / 0"),
            GenerationErrorKind::Execution,
        ),
    ];
    for (source, expected) in cases {
        let mut db = database();
        let file = input(&mut db, &source);
        let mut session = GenerationSession::new("stage".into(), GenerationBudget::new(1, 4096));
        let err = error(session.emit_value_function(&db, named(&db, file), request("check")));
        assert_eq!(err.kind, expected, "{err}");
        assert_eq!(session.budget().used_functions(), 0);
    }
    let mut db = database();
    let file = input(&mut db, SOURCE);
    let mut session = GenerationSession::new("stage".into(), GenerationBudget::new(1, 4096));
    let mut req = request("bad-output");
    req.grants[0]
        .template
        .source
        .push_str("\nfn invalid() -> bool { 42 }");
    let err = error(session.emit_value_function(&db, named(&db, file), req));
    assert_eq!(err.kind, GenerationErrorKind::Output);
    assert_eq!(err.provenance.unwrap().emission_target, Some(7));
    assert_eq!(session.budget().used_functions(), 1);
}

#[test]
fn target_and_value_edits_match_fresh_generation() {
    let mut db = database();
    for (strict, n, target) in [(true, 42, 7), (false, 17, 9), (true, 42, 7)] {
        let source = SOURCE
            .replace("strict: true", &format!("strict: {strict}"))
            .replace("value: 42", &format!("value: {n}"));
        let file = input(&mut db, &source);
        let run = |db: &DriverDataBase, file| {
            let mut session =
                GenerationSession::new("stage".into(), GenerationBudget::new(1, 4096));
            let out = session
                .emit_value_function(db, named(db, file), request("same-key"))
                .unwrap();
            (
                out.source().to_owned(),
                value(&out),
                out.provenance().emission_target,
            )
        };
        let warm = run(&db, file);
        assert_eq!(warm.1, n.to_string());
        assert_eq!(warm.2, Some(target));
        let mut fresh = database();
        let file = input(&mut fresh, &source);
        assert_eq!(warm, run(&fresh, file));
    }
}

#[test]
fn authorization_does_not_certify_return_type_or_truncate_target_identity() {
    let mut db = database();
    let file = input(&mut db, SOURCE);
    let mut session = GenerationSession::new("stage".into(), GenerationBudget::new(1, 4096));
    let mut req = request("wrong-type");
    req.grants[0].template.source = req.grants[0].template.source.replace("u256", "u8");
    let err = error(session.emit_value_function(&db, named(&db, file), req));
    assert_eq!(err.kind, GenerationErrorKind::ReturnType);
    assert_eq!(err.provenance.unwrap().emission_target, Some(7));
    let file = input(
        &mut db,
        &SOURCE.replace("{ 7 }", "{ 18446744073709551623 }"),
    );
    let mut session = GenerationSession::new("stage".into(), GenerationBudget::new(1, 4096));
    assert_eq!(
        error(session.emit_value_function(&db, named(&db, file), request("overflow"))).kind,
        GenerationErrorKind::Authority
    );
    assert_eq!(session.budget().used_functions(), 0);
}
