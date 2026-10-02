//! Run with `cargo run --release -p fe-driver --example authored_validator`.
//! All source stages are held in memory; the example writes no generated files.

use common::{
    InputDb,
    file::File,
    stdlib::{HasBuiltinCore, HasBuiltinStd},
};
use fe_driver::{
    DriverDataBase,
    generation::{
        FunctionEmissionGrant, FunctionEmissionRequest, FunctionTemplate, GeneratedFunction,
        GenerationBudget, GenerationError, GenerationSession,
        imports::{BoundFunction, FrozenArtifact, FrozenFunction, ImportError, bind_function},
    },
};
use hir::{
    analysis::{semantic::eval_body_owner_const_with_args, ty::ty_check::BodyOwner},
    hir_def::Func,
};
use url::Url;

pub const PROVIDER: &str = r#"
use core::meta::{FunctionEmission, ScalarBody, ScalarOp, record_field_count}
struct Packet { first: u256, second: bool }
const fn provide() -> FunctionEmission<ScalarBody<bool, 3>> {
    FunctionEmission { target: 7, value: ScalarBody {
        nodes: [ScalarOp::U256Parameter(0),
                ScalarOp::U256(record_field_count<Packet>() * 8),
                ScalarOp::Less(0, 1)],
        result: 2,
    } }
}
"#;

const CONSUMER: &str = r#"
pub const fn validate(value: u256) -> bool {}
const fn below() -> bool { validate(value: 15) }
const fn edge() -> bool { validate(value: 16) }
const fn above() -> bool { validate(value: 17) }
"#;

fn database() -> DriverDataBase {
    let mut db = DriverDataBase::default();
    db.initialize_builtin_core();
    db.initialize_builtin_std();
    db
}

fn provider(db: &mut DriverDataBase, source: &str) -> File {
    db.workspace().update(
        db,
        "file:///validator/schema.fe".parse().unwrap(),
        source.to_owned(),
    )
}

fn named<'db>(db: &'db DriverDataBase, file: File, name: &str) -> Func<'db> {
    db.top_mod(file)
        .all_funcs(db)
        .iter()
        .copied()
        .find(|func| {
            func.name(db)
                .to_opt()
                .is_some_and(|ident| ident.data(db) == name)
        })
        .expect("example contains its named function")
}

fn template(url: &str, source: &str, name: &str) -> FunctionTemplate {
    FunctionTemplate {
        url: Url::parse(url).unwrap(),
        source: source.to_owned(),
        function_name: name.to_owned(),
    }
}

fn grants() -> Vec<FunctionEmissionGrant> {
    vec![FunctionEmissionGrant {
        target: 7,
        template: template(
            "file:///validator/package.fe",
            "pub const fn accepts(value: u256) -> bool {}",
            "accepts",
        ),
    }]
}

fn generate(
    db: &DriverDataBase,
    file: File,
    grants: Vec<FunctionEmissionGrant>,
) -> Result<GeneratedFunction, GenerationError> {
    GenerationSession::new(
        "validator-policy".to_owned(),
        GenerationBudget::new(1, 65536),
    )
    .emit_scalar_body(
        db,
        named(db, file, "provide"),
        FunctionEmissionRequest {
            key: "validator".to_owned(),
            grants,
        },
    )
}

fn bind(target: &FrozenFunction, consumer: &str) -> Result<BoundFunction, ImportError> {
    bind_function(
        template("file:///validator/consumer.fe", consumer, "validate"),
        target,
        &[0],
        &mut GenerationBudget::new(1, 65536),
    )
}

fn evaluate(bound: &BoundFunction, name: &str) -> String {
    let db = bound.database();
    (match eval_body_owner_const_with_args(
        db,
        BodyOwner::Func(named(db, bound.file(), name)),
        hir::analysis::semantic::GenericSubst::for_body_owner(
            db,
            BodyOwner::Func(named(db, bound.file(), name)),
            Vec::new(),
        ),
        Vec::new(),
    ) {
        hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
        outcome => Err(outcome),
    })
    .expect("checked example caller evaluates")
    .pretty_print(db)
}

/// Build a checked consumer of the frozen, Fe-authored validator.
pub fn build(source: &str, consumer: &str) -> Result<BoundFunction, Box<dyn std::error::Error>> {
    let mut db = database();
    let file = provider(&mut db, source);
    let frozen = FrozenArtifact::new(generate(&db, file, grants())?);
    bind(&frozen.export("accepts")?, consumer).map_err(Into::into)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let validator = build(PROVIDER, CONSUMER)?;
    for name in ["below", "edge", "above"] {
        println!("{name}: {}", evaluate(&validator, name));
    }
    Ok(())
}
