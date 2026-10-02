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
use core::meta::{FunctionEmission, record_field_count}
struct Packet { first: u256, second: bool }
type Alias = Packet
trait Shape: Copy { const fn field_count(self) -> u256 }
struct Owner {}
impl Copy for Owner {}
impl Shape for Owner {
    const fn field_count(self) -> u256 { record_field_count<Alias>() }
}
const fn limit() -> u256 uses (shape: Shape) { shape.field_count() * 8 }
const fn provide() -> FunctionEmission<u256> {
    with (Shape = Owner {}) { FunctionEmission { target: 7, value: limit() } }
}
"#;
fn downstream(db: &DriverDataBase, file: File) -> (String, String) {
    let mut session = GenerationSession::new("reflection".into(), GenerationBudget::new(1, 4096));
    let mut req = request("shape");
    req.grants[0].template.source = "const fn target_7() -> u256 {}\npub const fn accepts(value: u256) -> bool { value < target_7() }".into();
    let out = session
        .emit_value_function(db, named(db, file), req)
        .unwrap();
    let limit = value(&out);
    let frozen = FrozenArtifact::new(out);
    let bound=bind_function(FunctionTemplate {
        url: Url::parse("file:///reflection/consumer.fe").unwrap(),
        source:"const fn validate(value: u256) -> bool {}\nconst fn check() -> bool { validate(value: 20) }".into(),
        function_name:"validate".into(),
    }, &frozen.export("accepts").unwrap(), &[0], &mut GenerationBudget::new(1,8192)).unwrap();
    let db = bound.database();
    let check = db
        .top_mod(bound.file())
        .all_funcs(db)
        .iter()
        .copied()
        .find(|f| f.name(db).to_opt().is_some_and(|n| n.data(db) == "check"))
        .unwrap();
    let result = (match eval_body_owner_const_with_args(
        db,
        BodyOwner::Func(check),
        hir::analysis::semantic::GenericSubst::for_body_owner(db, BodyOwner::Func(check), vec![]),
        vec![],
    ) {
        hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
        outcome => Err(outcome),
    })
    .unwrap()
    .pretty_print(db);
    (limit, result)
}
#[test]
fn acquired_shape_drives_policy_and_field_edits_match_fresh_consumers() {
    let mut db = database();
    for (fields, expected) in [
        ("first: u256, second: bool", ("16", "false")),
        ("first: u256, second: bool, third: u256", ("24", "true")),
        ("first: u256, second: bool", ("16", "false")),
    ] {
        let source = SOURCE.replace("first: u256, second: bool", fields);
        let file = input(&mut db, &source);
        let warm = downstream(&db, file);
        assert_eq!(warm, (expected.0.into(), expected.1.into()));
        let mut fresh = database();
        let file = input(&mut fresh, &source);
        assert_eq!(warm, downstream(&fresh, file));
    }
}
#[test]
fn acquisition_is_owner_scoped_and_rejects_unsupported_types_even_when_untaken() {
    for replacement in [
        "record_field_count<u256>()".to_owned(),
        "record_field_count<(u256, bool)>()".to_owned(),
        "record_field_count<other::Packet>()".to_owned(),
        "if false { record_field_count<other::Packet>() } else { 2 }".to_owned(),
        "record_field_count<Generic<u256>>()".to_owned(),
        "record_field_count<Choice>()".to_owned(),
    ] {
        let source = format!(
            "{}\nmod other {{ pub struct Packet {{ pub a: u256, pub b: bool }} }}\nstruct Generic<T> {{ item: T }}\nenum Choice {{ Empty }}",
            SOURCE.replace("record_field_count<Alias>()", &replacement)
        );
        let mut db = database();
        let file = input(&mut db, &source);
        let mut session =
            GenerationSession::new("reflection".into(), GenerationBudget::new(1, 4096));
        let err = error(session.emit_value_function(&db, named(&db, file), request("denied")));
        assert_eq!(err.kind, GenerationErrorKind::Provider, "{err}");
        assert!(err.message.contains("8-0094"), "{err}");
    }
}
#[test]
fn moving_acquisition_out_of_owner_scope_invalidates_warm_results() {
    let mut db = database();
    let file = input(&mut db, SOURCE);
    assert_eq!(downstream(&db, file).0, "16");
    let source = SOURCE.replace("record_field_count<Alias>()", "outsider::count()");
    let source = format!(
        "{source}\nmod outsider {{ pub const fn count() -> u256 {{ core::meta::record_field_count<super::Packet>() }} }}"
    );
    let file = input(&mut db, &source);
    let diags = db
        .run_on_ingot(db.top_mod(file).ingot(&db))
        .format_diags(&db);
    assert!(diags.contains("8-0094"), "{diags}");
    let mut fresh = database();
    let file = input(&mut fresh, &source);
    assert!(
        fresh
            .run_on_ingot(fresh.top_mod(file).ingot(&fresh))
            .format_diags(&fresh)
            .contains("8-0094")
    );
    let file = input(&mut db, SOURCE);
    assert_eq!(downstream(&db, file).0, "16");
}
#[test]
fn names_do_not_confer_intrinsic_authority_and_source_is_still_checked() {
    let mut db = database();
    let fake = SOURCE
        .replace("{FunctionEmission, record_field_count}", "FunctionEmission")
        .replace(
            "type Alias = Packet",
            "type Alias = Packet\nconst fn record_field_count<T>() -> u256 { 99 }",
        );
    let file = input(&mut db, &fake);
    assert_eq!(downstream(&db, file).0, "792");
    let malformed = format!("{SOURCE}\nfn invalid() {{ if false {{ let bad: bool = 1 }} }}");
    let file = input(&mut db, &malformed);
    let mut session = GenerationSession::new("reflection".into(), GenerationBudget::new(1, 4096));
    assert_eq!(
        error(session.emit_value_function(&db, named(&db, file), request("bad"))).kind,
        GenerationErrorKind::Provider
    );
}

#[test]
fn record_owner_can_publish_a_count_but_generic_acquisition_is_not_admitted() {
    let source = SOURCE.replace("record_field_count<Alias>()", "owner::count()");
    let source = format!(
        "{source}\nmod owner {{ struct Hidden {{ first: u256, second: bool, third: u256 }}\npub const fn count() -> u256 {{ core::meta::record_field_count<Hidden>() }} }}"
    );
    let mut db = database();
    let file = input(&mut db, &source);
    assert_eq!(downstream(&db, file), ("24".into(), "true".into()));
    let generic = format!(
        "{SOURCE}\nconst fn generic<T>() -> u256 {{ core::meta::record_field_count<T>() }}"
    );
    let file = input(&mut db, &generic);
    let errors = db
        .run_on_ingot(db.top_mod(file).ingot(&db))
        .format_diags(&db);
    assert!(errors.contains("8-0094"), "{errors}");
    let empty = SOURCE.replace("first: u256, second: bool", "");
    let file = input(&mut db, &empty);
    assert_eq!(downstream(&db, file), ("0".into(), "false".into()));
}
