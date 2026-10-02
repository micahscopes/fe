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
        imports::{FrozenArtifact, bind_function},
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
        BodyOwner::Func(
            output
                .database()
                .top_mod(output.file())
                .all_funcs(output.database())
                .iter()
                .copied()
                .find(|f| {
                    f.name(output.database())
                        .to_opt()
                        .is_some_and(|n| n.data(output.database()) == "probe")
                })
                .unwrap(),
        ),
        hir::analysis::semantic::GenericSubst::for_body_owner(
            output.database(),
            BodyOwner::Func(
                output
                    .database()
                    .top_mod(output.file())
                    .all_funcs(output.database())
                    .iter()
                    .copied()
                    .find(|f| {
                        f.name(output.database())
                            .to_opt()
                            .is_some_and(|n| n.data(output.database()) == "probe")
                    })
                    .unwrap(),
            ),
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
use core::meta::{FunctionEmission, ScalarBody, ScalarOp, record_field_count}
struct Packet { first: u256, second: bool }
const fn provide() -> FunctionEmission<ScalarBody<bool, 3>> {
    FunctionEmission { target: 7, value: ScalarBody {
        nodes: [ScalarOp::U256Parameter(0), ScalarOp::U256(record_field_count<Packet>() * 8), ScalarOp::Less(0, 1)],
        result: 2,
    } }
}
"#;
fn expression_request() -> FunctionEmissionRequest {
    let mut req = request("expression");
    req.grants[0].template.source="pub const fn target_7(value: u256) -> bool {}\nconst fn probe() -> bool { target_7(value: 20) }".into();
    req
}
fn generate(
    db: &DriverDataBase,
    file: File,
    request: FunctionEmissionRequest,
) -> Result<GeneratedFunction, GenerationError> {
    GenerationSession::new("expressions".into(), GenerationBudget::new(1, 16384)).emit_scalar_body(
        db,
        named(db, file),
        request,
    )
}
#[test]
fn reflected_fe_policy_authors_the_runtime_comparison_and_binds_it() {
    let mut db = database();
    let file = input(&mut db, SOURCE);
    let out = generate(&db, file, expression_request()).unwrap();
    assert_eq!(value(&out), "false");
    assert!(out.source().contains(" < "));
    assert!(!out.provenance().template_source.contains(" < "));
    let selected = FrozenArtifact::new(out).export("target_7").unwrap();
    let bound=bind_function(FunctionTemplate {url:Url::parse("file:///scalar/consumer.fe").unwrap(),source:"const fn validate(value: u256) -> bool {}\nconst fn check() -> bool { validate(value: 15) }".into(),function_name:"validate".into()},&selected,&[0],&mut GenerationBudget::new(1,16384)).unwrap();
    let db = bound.database();
    let check = db
        .top_mod(bound.file())
        .all_funcs(db)
        .iter()
        .copied()
        .find(|f| f.name(db).to_opt().is_some_and(|n| n.data(db) == "check"))
        .unwrap();
    assert_eq!(
        (match eval_body_owner_const_with_args(
            db,
            BodyOwner::Func(check),
            hir::analysis::semantic::GenericSubst::for_body_owner(
                db,
                BodyOwner::Func(check),
                vec![]
            ),
            vec![]
        ) {
            hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
            outcome => Err(outcome),
        })
        .unwrap()
        .pretty_print(db),
        "true"
    );
}
#[test]
fn field_and_operation_edits_match_fresh_generation() {
    let mut db = database();
    for (source, expected) in [
        (SOURCE.to_owned(), "false"),
        (
            SOURCE.replace("second: bool", "second: bool, third: u256"),
            "true",
        ),
        (
            SOURCE
                .replace("ScalarOp::Less(0, 1)", "ScalarOp::Equal(0, 1)")
                .replace("record_field_count<Packet>() * 8", "20"),
            "true",
        ),
        (SOURCE.to_owned(), "false"),
    ] {
        let file = input(&mut db, &source);
        let warm = generate(&db, file, expression_request()).unwrap();
        let mut fresh = database();
        let file = input(&mut fresh, &source);
        let cold = generate(&fresh, file, expression_request()).unwrap();
        assert_eq!(value(&warm), expected);
        assert_eq!(value(&warm), value(&cold));
        assert_eq!(warm.source(), cold.source());
    }
}
#[test]
fn malformed_references_and_ungranted_targets_reject() {
    for (source, kind) in [
        (
            SOURCE.replace("Less(0, 1)", "Less(2, 1)"),
            GenerationErrorKind::Protocol,
        ),
        (
            SOURCE.replace("Less(0, 1)", "Less(999, 1)"),
            GenerationErrorKind::Protocol,
        ),
        (
            SOURCE.replace("result: 2", "result: 3"),
            GenerationErrorKind::Protocol,
        ),
        (
            SOURCE.replace("U256Parameter(0)", "U256Parameter(1)"),
            GenerationErrorKind::Protocol,
        ),
        (
            SOURCE.replace("target: 7", "target: 99"),
            GenerationErrorKind::Authority,
        ),
        (
            SOURCE.replace("result: 2", "result: 18446744073709551616"),
            GenerationErrorKind::Protocol,
        ),
    ] {
        let mut db = database();
        let file = input(&mut db, &source);
        let err = error(generate(&db, file, expression_request()));
        assert_eq!(err.kind, kind, "{err}");
    }
}
#[test]
fn ordinary_checking_owns_source_and_operation_types() {
    for (source, kind) in [
        (
            SOURCE.replace("ScalarOp::Less(0, 1)", "ScalarOp::Not(0)"),
            GenerationErrorKind::Output,
        ),
        (
            SOURCE.replace("ScalarOp::U256Parameter(0)", "ScalarOp::Bool(true)"),
            GenerationErrorKind::Output,
        ),
        (
            SOURCE.replace("ScalarBody<bool, 3>", "ScalarBody<u256, 3>"),
            GenerationErrorKind::ReturnType,
        ),
        (
            SOURCE.replace(
                "FunctionEmission { target:",
                "if false { let wrong: bool = 1 }\nFunctionEmission { target:",
            ),
            GenerationErrorKind::Provider,
        ),
    ] {
        let mut db = database();
        let file = input(&mut db, &source);
        let err = error(generate(&db, file, expression_request()));
        assert_eq!(err.kind, kind, "{err}");
    }
    let mut db = database();
    let file = input(&mut db, SOURCE);
    let mut req = expression_request();
    req.grants[0].template.source = req.grants[0]
        .template
        .source
        .replace("value: u256", "value: u8");
    assert_eq!(
        error(generate(&db, file, req)).kind,
        GenerationErrorKind::Output
    );
}
#[test]
fn bounded_nodes_and_output_bytes_remain_independent_limits() {
    for n in [0, 65] {
        let source = format!(
            "use core::meta::{{FunctionEmission, ScalarBody, ScalarOp}}\nconst fn provide() -> FunctionEmission<ScalarBody<bool, {n}>> {{ FunctionEmission {{ target: 7, value: ScalarBody {{ nodes: [ScalarOp::Bool(true); {n}], result: 0 }} }} }}"
        );
        let mut db = database();
        let file = input(&mut db, &source);
        assert_eq!(
            error(generate(&db, file, expression_request())).kind,
            GenerationErrorKind::Limit
        );
    }
    let mut db = database();
    let file = input(&mut db, SOURCE);
    let req = expression_request();
    let bytes = req.grants.iter().map(|g| g.template.source.len()).sum();
    let mut session = GenerationSession::new("small".into(), GenerationBudget::new(1, bytes));
    assert_eq!(
        error(session.emit_scalar_body(&db, named(&db, file), req)).kind,
        GenerationErrorKind::Limit
    );
    assert_eq!(session.budget().used_functions(), 0);
}
#[test]
fn generated_locals_cannot_capture_parameter_names_and_boolean_ops_work() {
    let source = "use core::meta::{FunctionEmission, ScalarBody, ScalarOp}\nconst fn provide() -> FunctionEmission<ScalarBody<bool, 2>> { FunctionEmission { target: 7, value: ScalarBody { nodes: [ScalarOp::BoolParameter(0), ScalarOp::Not(0)], result: 1 } } }";
    let mut db = database();
    let file = input(&mut db, source);
    let mut req = expression_request();
    req.grants[0].template.source="pub const fn target_7(__scalar_node_0: bool) -> bool {}\nconst fn probe() -> bool { target_7(__scalar_node_0: false) }".into();
    let out = generate(&db, file, req).unwrap();
    assert_eq!(value(&out), "true");
    assert!(out.source().contains("let __scalar_node__0"));
}
#[test]
fn unused_nodes_are_checked_and_execute_in_order() {
    let source = "use core::meta::{FunctionEmission, ScalarBody, ScalarOp}\nconst fn provide() -> FunctionEmission<ScalarBody<bool, 4>> { FunctionEmission { target: 7, value: ScalarBody { nodes: [ScalarOp::U256(0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff), ScalarOp::U256(1), ScalarOp::Add(0, 1), ScalarOp::Bool(true)], result: 3 } } }";
    let mut db = database();
    let file = input(&mut db, source);
    let out = generate(&db, file, expression_request()).unwrap();
    let db = out.database();
    let probe = db
        .top_mod(out.file())
        .all_funcs(db)
        .iter()
        .copied()
        .find(|f| f.name(db).to_opt().is_some_and(|n| n.data(db) == "probe"))
        .unwrap();
    let overflow = (match eval_body_owner_const_with_args(
        db,
        BodyOwner::Func(probe),
        hir::analysis::semantic::GenericSubst::for_body_owner(db, BodyOwner::Func(probe), vec![]),
        vec![],
    ) {
        hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
        outcome => Err(outcome),
    })
    .unwrap_err();
    assert!(
        format!("{overflow:?}").contains("ArithmeticOverflow"),
        "{overflow:?}"
    );
    let invalid = source.replace("ScalarOp::U256(1)", "ScalarOp::Bool(false)");
    let mut bad = database();
    let file = input(&mut bad, &invalid);
    assert_eq!(
        error(generate(&bad, file, expression_request())).kind,
        GenerationErrorKind::Output
    );
}

#[test]
fn designated_protocol_and_maximum_program_size_are_checked() {
    let fake = SOURCE.replace("FunctionEmission, ScalarBody, ScalarOp", "FunctionEmission, ScalarOp")
        .replace("struct Packet", "struct ScalarBody<T, const N: usize> { nodes: [ScalarOp; N], result: u256 }\nstruct Packet");
    let mut db = database();
    let file = input(&mut db, &fake);
    assert_eq!(
        error(generate(&db, file, expression_request())).kind,
        GenerationErrorKind::Protocol
    );
    let source = "use core::meta::{FunctionEmission, ScalarBody, ScalarOp}\nconst fn provide() -> FunctionEmission<ScalarBody<bool, 64>> { FunctionEmission { target: 7, value: ScalarBody { nodes: [ScalarOp::Bool(true); 64], result: 63 } } }";
    let file = input(&mut db, source);
    assert_eq!(
        value(&generate(&db, file, expression_request()).unwrap()),
        "true"
    );
    let mut session = GenerationSession::new("values".into(), GenerationBudget::new(1, 16384));
    assert_eq!(
        error(session.emit_value_function(&db, named(&db, file), expression_request())).kind,
        GenerationErrorKind::Protocol
    );
}

#[test]
fn addition_and_equality_use_ordinary_scalar_semantics() {
    let source = "use core::meta::{FunctionEmission, ScalarBody, ScalarOp}\nconst fn provide() -> FunctionEmission<ScalarBody<bool, 5>> { FunctionEmission { target: 7, value: ScalarBody { nodes: [ScalarOp::U256Parameter(0), ScalarOp::U256(1), ScalarOp::Add(0, 1), ScalarOp::U256(21), ScalarOp::Equal(2, 3)], result: 4 } } }";
    let mut db = database();
    let file = input(&mut db, source);
    assert_eq!(
        value(&generate(&db, file, expression_request()).unwrap()),
        "true"
    );
}
