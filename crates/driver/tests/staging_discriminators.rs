//! Generation staging discriminators (SIX-AREA-PLAN §4). The staging contract
//! is undecided: providers may see only authored input, or may execute
//! generated output through explicit dependencies. These tests assert only
//! what both contracts require, comparing outcomes rather than choosing one.
use common::InputDb;
use fe_driver::{DriverDataBase, generation::prepare_file};
use hir::analysis::{
    semantic::{EvalOutcome, GenericSubst, SemConstScalar, SemConstValue, eval_body_owner_const},
    ty::ty_check::BodyOwner,
};
use url::Url;

const URL: &str = "file:///tmp/staging-discriminators.fe";

/// The observable outcome of preparing a program: the value of `run`, or the
/// refusal's cause.
fn outcome(db: &mut DriverDataBase, source: &str) -> Result<String, String> {
    let url = Url::parse(URL).unwrap();
    db.workspace().update(db, url.clone(), source.into());
    let db = &*db;
    let file = db.workspace().get(db, &url).unwrap();
    let output = prepare_file(db, file)
        .map_err(|error| error.cause)?
        .ok_or("nothing was generated")?;
    let file = output.workspace().get(&output, &url).unwrap();
    let function = output
        .top_mod(file)
        .all_funcs(&output)
        .iter()
        .copied()
        .find(|function| {
            function
                .name(&output)
                .to_opt()
                .is_some_and(|ident| ident.data(&output) == "run")
        })
        .ok_or("missing `run`")?;
    let owner = BodyOwner::Func(function, None);
    match eval_body_owner_const(
        &output,
        owner,
        GenericSubst::for_body_owner(&output, owner, vec![]),
    ) {
        EvalOutcome::Ready(value) => match value.value(&output) {
            SemConstValue::Scalar {
                value: SemConstScalar::Int { value },
                ..
            } => Ok(value.to_string()),
            other => Err(format!("`run` returned {other:?}")),
        },
        other => Err(format!("`run` did not evaluate: {other:?}")),
    }
}

fn fresh(source: &str) -> Result<String, String> {
    outcome(&mut DriverDataBase::default(), source)
}

const PRELUDE: &str = "use core::derive::Derive\nuse core::meta::{Record, Code, Item}\ntrait Score { const fn score() -> u256 }\ntrait Seed { const fn seed() -> u256 }\nstruct Pair {}\nstruct Token {}\n";
const A_USES_B: &str = "struct A {}\nimpl Derive<Score> for A {\n    const fn derive(_ record: Record) -> Code<Item> {\n        let n = helper()\n        quote { const fn score() -> u256 { ${n} } }\n    }\n}\nconst fn helper() -> u256 { with (B) { Token::seed() } }\n";
const B: &str = "struct B {}\nimpl Derive<Seed> for B {\n    const fn derive(_ record: Record) -> Code<Item> {\n        quote { const fn seed() -> u256 { 42 } }\n    }\n}\n";
const RUN: &str = "const fn run() -> u256 { with (A) { Pair::score() } }\n";

/// S-T3: declaration order is not an input. Every permutation of the
/// provider-to-provider program has the same outcome, whatever it is.
#[test]
fn provider_dependency_outcome_is_independent_of_declaration_order() {
    let parts = [A_USES_B, B, RUN];
    let orders = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let outcomes = orders
        .iter()
        .map(|order| {
            let source = std::iter::once(PRELUDE)
                .chain(order.iter().map(|index| parts[*index]))
                .collect::<String>();
            fresh(&source)
        })
        .collect::<Vec<_>>();
    // Causes name ranges, which move with the text, so a refusal is compared
    // by the presence of an error and a value by equality.
    let first = &outcomes[0];
    for (order, outcome) in orders.iter().zip(&outcomes) {
        match (first, outcome) {
            (Ok(expected), Ok(value)) => assert_eq!(value, expected, "{order:?}"),
            (Err(_), Err(_)) => {}
            _ => panic!("order {order:?} gave {outcome:?}, first order gave {first:?}"),
        }
    }
}

/// S-T8: an authored database reused across edits prepares exactly what a
/// fresh database prepares, for each revision, including the restore.
#[test]
fn provider_dependency_edits_agree_with_fresh_preparation() {
    let original = [PRELUDE, A_USES_B, B, RUN].concat();
    let edited = original.replace("{ 42 }", "{ 43 }");
    let mut warm = DriverDataBase::default();
    for source in [&original, &edited, &original] {
        assert_eq!(outcome(&mut warm, source), fresh(source));
    }
}

/// S-T4: the two generation channels see the same thing. A scoped provider
/// reading output of `#[generate]`, and a `#[generate]` provider reading
/// output of a scoped opening, are either both admitted (an explicit staged
/// dependency) or both refused (authored input only).
///
/// Predicted, unexecuted: today scoped providers are evaluated in a view that
/// already contains whole-item output, while whole-item providers are
/// evaluated against authored input, so the first is admitted and the second
/// refused. Run with `--include-ignored` to record the observation.
#[test]
#[ignore = "S1: predicted channel asymmetry; the staging contract is undecided"]
fn generation_channels_see_each_other_symmetrically() {
    let scoped_reads_whole_item = "use core::derive::Derive\nuse core::meta::{Record, Code, Item}\ntrait Score { const fn score() -> u256 }\ntrait Seed { const fn seed() -> u256 }\npub const fn seed_impl(_ record: Record) -> Code<Item> {\n    quote { impl Seed for ${record} { const fn seed() -> u256 { 42 } } }\n}\n#[generate(seed_impl)]\nstruct Token {}\nstruct Pair {}\nstruct A {}\nimpl Derive<Score> for A {\n    const fn derive(_ record: Record) -> Code<Item> {\n        let n = Token::seed()\n        quote { const fn score() -> u256 { ${n} } }\n    }\n}\nconst fn run() -> u256 { with (A) { Pair::score() } }\n";
    let whole_item_reads_scoped = "use core::derive::Derive\nuse core::meta::{Record, Code, Item}\ntrait Seed { const fn seed() -> u256 }\nstruct Token {}\nstruct B {}\nimpl Derive<Seed> for B {\n    const fn derive(_ record: Record) -> Code<Item> {\n        quote { const fn seed() -> u256 { 42 } }\n    }\n}\npub const fn make(_ record: Record) -> Code<Item> {\n    let n = with (B) { Token::seed() }\n    quote { const fn run() -> u256 { ${n} } }\n}\n#[generate(make)]\nstruct Pair {}\n";
    let scoped = fresh(scoped_reads_whole_item);
    let whole_item = fresh(whole_item_reads_scoped);
    assert_eq!(
        scoped.is_ok(),
        whole_item.is_ok(),
        "scoped provider: {scoped:?}; whole-item provider: {whole_item:?}"
    );
}
