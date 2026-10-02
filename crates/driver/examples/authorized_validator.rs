//! Run with `cargo run --release -p fe-driver --example authorized_validator`.
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

const PROVIDER: &str = r#"
use core::meta::FunctionEmission
trait Policy: Copy {
    const fn target(self) -> u256
    const fn limit(self) -> u256
}
struct Choice { strict: bool, maximum: u256 }
impl Copy for Choice {}
impl Policy for Choice {
    const fn target(self) -> u256 { if self.strict { 7 } else { 9 } }
    const fn limit(self) -> u256 { self.maximum }
}
const fn choose() -> FunctionEmission<u256> uses (policy: Policy) {
    FunctionEmission { target: policy.target(), value: policy.limit() }
}
const fn provide() -> FunctionEmission<u256> {
    with (Policy = Choice { strict: true, maximum: 20 }) { choose() }
}
"#;

const STRICT: &str = r#"
const fn limit() -> u256 {}
pub const fn accepts(value: u256) -> bool { value < ingot::limit() }
"#;
const INCLUSIVE: &str = r#"
const fn limit() -> u256 {}
pub const fn accepts(value: u256) -> bool { value <= ingot::limit() }
"#;

const CONSUMER: &str = r#"
pub const fn validate(value: u256) -> bool {}
const fn below() -> bool { validate(value: 19) }
const fn edge() -> bool { validate(value: 20) }
const fn above() -> bool { validate(value: 21) }
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
    [(7, STRICT), (9, INCLUSIVE)]
        .into_iter()
        .map(|(target, source)| FunctionEmissionGrant {
            target,
            // Identical URLs and export names do not identify an incarnation.
            template: template("file:///validator/package.fe", source, "limit"),
        })
        .collect()
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
    .emit_value_function(
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

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut db = database();
    let file = provider(&mut db, PROVIDER);
    let frozen = FrozenArtifact::new(generate(&db, file, grants())?);
    let selected = frozen.export("accepts")?;
    let validator = bind(&selected, CONSUMER)?;
    println!(
        "selected target: {}",
        validator
            .receipt()
            .original_provenance
            .emission_target
            .unwrap()
    );
    for name in ["below", "edge", "above"] {
        println!("{name}: {}", evaluate(&validator, name));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/support/authorized_validator_checks.rs"]
mod tests;
