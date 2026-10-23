//! Generation in packages: one prepared view per package revision, with
//! providers imported across files and from dependencies.
use common::{InputDb, dependencies::DependencyArguments};
use fe_driver::{DriverDataBase, generation::prepare_ingots};
use hir::{
    analysis::{
        semantic::{
            EvalOutcome, GenericSubst, SemConstScalar, SemConstValue, eval_body_owner_const,
        },
        ty::ty_check::BodyOwner,
    },
    hir_def::HirIngot,
};
use url::Url;

const ROOT: &str = "file:///tmp/fe-package-generation/";

fn url(path: &str) -> Url {
    Url::parse(&format!("{ROOT}{path}")).unwrap()
}

fn project(files: &[(&str, &str)]) -> DriverDataBase {
    let mut db = DriverDataBase::default();
    for (path, text) in files {
        db.workspace()
            .update(&mut db, url(path), (*text).to_string());
    }
    db
}

fn prepare(db: &DriverDataBase, root: &str) -> DriverDataBase {
    prepare_ingots(db, &[url(root)])
        .unwrap_or_else(|error| panic!("package preparation failed: {error}"))
        .expect("the package requests generation")
}

/// Evaluate a const function of one module to its integer value.
fn eval(db: &DriverDataBase, path: &str, name: &str) -> String {
    let file = db
        .workspace()
        .get(db, &url(path))
        .unwrap_or_else(|| panic!("missing file {path}"));
    let function = db
        .top_mod(file)
        .all_funcs(db)
        .iter()
        .copied()
        .find(|function| {
            function
                .name(db)
                .to_opt()
                .is_some_and(|ident| ident.data(db) == name)
        })
        .unwrap_or_else(|| panic!("missing function `{name}` in {path}"));
    let owner = BodyOwner::Func(function, None);
    let outcome = eval_body_owner_const(db, owner, GenericSubst::for_body_owner(db, owner, vec![]));
    let EvalOutcome::Ready(value) = outcome else {
        panic!("`{name}` did not evaluate: {outcome:?}")
    };
    let SemConstValue::Scalar {
        value: SemConstScalar::Int { value },
        ..
    } = value.value(db)
    else {
        panic!("`{name}` did not return an integer")
    };
    value.to_string()
}

/// All diagnostics of an ingot, formatted; empty when it checks cleanly.
fn ingot_errors(db: &DriverDataBase, root: &str) -> String {
    let ingot = db
        .workspace()
        .containing_ingot(db, url(root))
        .expect("indexed ingot");
    let diagnostics = db.run_on_ingot(ingot);
    if diagnostics.has_errors(db) {
        diagnostics.format_diags(db)
    } else {
        String::new()
    }
}

fn assert_authored_unchanged(db: &DriverDataBase, files: &[(&str, &str)]) {
    assert!(db.generation_inputs().functions(db).is_empty());
    assert!(db.generation_inputs().requests(db).is_empty());
    for (path, text) in files {
        let file = db.workspace().get(db, &url(path)).unwrap();
        assert_eq!(file.text(db).as_str(), *text, "{path} was rewritten");
    }
}

const APP_TOML: &str = "[ingot]\nname = \"app\"\nversion = \"0.1.0\"\n";
const APP_WITH_LIB_TOML: &str =
    "[ingot]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nlib = { path = \"../lib\" }\n";
const LIB_TOML: &str = "[ingot]\nname = \"lib\"\nversion = \"0.1.0\"\n";

const SCORE: &str = "pub trait Score {\n    const fn score() -> usize\n}\n";
const MODEL: &str = "pub struct Pair {\n    pub a: u256,\n    pub b: u256,\n}\n";
const PROVIDERS: &str = r#"use core::derive::Derive
use core::meta::{Code, Item, Record, field_count}
use ingot::score::Score

pub struct FieldCount {}

impl Derive<Score> for FieldCount {
    const fn derive(_ record: Record) -> Code<Item> {
        let n = field_count(record)
        quote { const fn score() -> usize { ${n} } }
    }
}
"#;
const REQUEST: &str = r#"use ingot::model::Pair
use ingot::providers::FieldCount
use ingot::score::Score

impl Score for Pair with (FieldCount)

pub const fn total() -> usize {
    Pair::score()
}
"#;

fn same_ingot_files() -> Vec<(&'static str, String)> {
    vec![
        ("app/fe.toml", APP_TOML.to_string()),
        ("app/src/score.fe", SCORE.to_string()),
        ("app/src/model.fe", MODEL.to_string()),
        ("app/src/providers.fe", PROVIDERS.to_string()),
        ("app/src/lib.fe", REQUEST.to_string()),
    ]
}

fn borrow(files: &[(&'static str, String)]) -> Vec<(&'static str, &str)> {
    files
        .iter()
        .map(|(path, text)| (*path, text.as_str()))
        .collect()
}

#[test]
fn explicit_derive_uses_a_provider_imported_from_another_file() {
    let files = same_ingot_files();
    let files = borrow(&files);
    let db = project(&files);
    let prepared = prepare(&db, "app/");
    // `Pair` has two fields; the request, provider, trait and target are in
    // four different files of one ingot.
    assert_eq!(eval(&prepared, "app/src/lib.fe", "total"), "2");
    assert_eq!(ingot_errors(&prepared, "app/"), "");
    assert_authored_unchanged(&db, &files);
    // Negative control: without preparation the header is not an
    // implementation, so the output above came from the prepared view.
    let authored = ingot_errors(&db, "app/");
    assert!(
        authored.contains("native derive request was not prepared"),
        "{authored}"
    );
}

const LIB_GENERATE: &str = r#"use core::meta::{Code, Item, Record, field_count}

pub trait Score {
    const fn score() -> usize
}

pub const fn field_score(_ record: Record) -> Code<Item> {
    let n = field_count(record)
    quote {
        impl Score for ${record} {
            const fn score() -> usize { ${n} }
        }
    }
}
"#;
const APP_GENERATE_MODEL: &str =
    "#[generate(lib::field_score)]\npub struct Pair {\n    pub a: u256,\n    pub b: u256,\n}\n";
const APP_GENERATE_LIB: &str = r#"use ingot::model::Pair
use lib::Score

pub const fn total() -> usize {
    Pair::score()
}
"#;

#[test]
fn dependency_provider_generates_an_impl_of_its_trait_for_an_app_record() {
    let files = [
        ("lib/fe.toml", LIB_TOML),
        ("lib/src/lib.fe", LIB_GENERATE),
        ("app/fe.toml", APP_WITH_LIB_TOML),
        ("app/src/model.fe", APP_GENERATE_MODEL),
        ("app/src/lib.fe", APP_GENERATE_LIB),
    ];
    let db = project(&files);
    let prepared = prepare(&db, "app/");
    assert_eq!(eval(&prepared, "app/src/lib.fe", "total"), "2");
    // `Score` in the quote resolves where the provider wrote it; `model.fe`
    // imports nothing.
    assert_eq!(ingot_errors(&prepared, "app/"), "");
    assert_authored_unchanged(&db, &files);
    // Before this change an ingot silently ignored `#[generate]`; the
    // authored view still has no implementation.
    assert!(!ingot_errors(&db, "app/").is_empty());
}

const LIB_OWN_DERIVE: &str = r#"use core::derive::Derive
use core::meta::{Code, Item, Record, field_count}

pub trait Score {
    const fn score() -> usize
}

pub struct Counted {}

impl Derive<Score> for Counted {
    const fn derive(_ record: Record) -> Code<Item> {
        let n = field_count(record)
        quote { const fn score() -> usize { ${n} } }
    }
}

pub struct Unit {
    pub x: u8,
    pub y: u8,
    pub z: u8,
}

impl Score for Unit with (Counted)

pub const fn unit_score() -> usize {
    Unit::score()
}
"#;
const APP_USES_LIB: &str = r#"use lib::{Score, Unit}

pub const fn from_dependency() -> usize {
    lib::unit_score()
}

pub const fn direct() -> usize {
    Unit::score()
}
"#;

#[test]
fn a_dependency_s_own_generation_is_prepared_with_the_app() {
    let files = [
        ("lib/fe.toml", LIB_TOML),
        ("lib/src/lib.fe", LIB_OWN_DERIVE),
        ("app/fe.toml", APP_WITH_LIB_TOML),
        ("app/src/lib.fe", APP_USES_LIB),
    ];
    let db = project(&files);
    let prepared = prepare(&db, "app/");
    assert_eq!(eval(&prepared, "app/src/lib.fe", "from_dependency"), "3");
    assert_eq!(eval(&prepared, "app/src/lib.fe", "direct"), "3");
    assert_eq!(ingot_errors(&prepared, "app/"), "");
    assert_eq!(ingot_errors(&prepared, "lib/"), "");
}

#[test]
fn a_faulting_dependency_provider_refuses_the_package_without_publishing() {
    let faulting = LIB_GENERATE.replace(
        "    let n = field_count(record)\n",
        "    let zero: usize = 0\n    let n = field_count(record) / zero\n",
    );
    let files = [
        ("lib/fe.toml", LIB_TOML),
        ("lib/src/lib.fe", faulting.as_str()),
        ("app/fe.toml", APP_WITH_LIB_TOML),
        ("app/src/model.fe", APP_GENERATE_MODEL),
        ("app/src/lib.fe", APP_GENERATE_LIB),
    ];
    let db = project(&files);
    let error = prepare_ingots(&db, &[url("app/")])
        .err()
        .expect("a provider fault refuses preparation");
    assert!(error.cause.contains("division by zero"), "{error}");
    assert_eq!(error.file, url("lib/src/lib.fe"), "{error}");
    assert_authored_unchanged(&db, &files);
}

#[test]
fn a_prepared_package_view_refuses_a_second_preparation() {
    let files = same_ingot_files();
    let db = project(&borrow(&files));
    let prepared = prepare(&db, "app/");
    let error = prepare_ingots(&prepared, &[url("app/")])
        .err()
        .expect("output is checked once");
    assert!(error.cause.contains("authored input view"), "{error}");
}

#[test]
fn a_package_without_requests_keeps_its_authored_view() {
    // An ordinary named choice is lexical selection, not a generation demand.
    let files = [
        ("app/fe.toml", APP_TOML),
        (
            "app/src/lib.fe",
            "trait Score { const fn score() -> u256 }\nstruct Pair {}\nimpl Score for Pair as Named { const fn score() -> u256 { 7 } }\nconst fn run() -> u256 { with (Named) { Pair::score() } }\n",
        ),
    ];
    let db = project(&files);
    assert!(prepare_ingots(&db, &[url("app/")]).unwrap().is_none());
}

/// Warm edits of one authored database against fresh databases built from
/// the same text: field, provider, and import alias edits, then restores.
#[test]
fn edits_and_restores_agree_with_fresh_preparation() {
    let original = same_ingot_files();
    let mut warm = project(&borrow(&original));
    let edit = |files: &[(&'static str, String)], path: &str, from: &str, to: &str| {
        files
            .iter()
            .map(|(name, text)| {
                let text = if *name == path {
                    assert!(text.contains(from), "{path} lacks {from:?}");
                    text.replace(from, to)
                } else {
                    text.clone()
                };
                (*name, text)
            })
            .collect::<Vec<_>>()
    };
    let steps = [
        (original.clone(), "2"),
        (
            edit(
                &original,
                "app/src/model.fe",
                "    pub b: u256,\n",
                "    pub b: u256,\n    pub c: bool,\n",
            ),
            "3",
        ),
        (original.clone(), "2"),
        (
            edit(
                &original,
                "app/src/providers.fe",
                "{ ${n} }",
                "{ ${n} + 10 }",
            ),
            "12",
        ),
        (original.clone(), "2"),
        (
            edit(
                &edit(
                    &original,
                    "app/src/lib.fe",
                    "use ingot::providers::FieldCount\n",
                    "use ingot::providers::FieldCount as Counter\n",
                ),
                "app/src/lib.fe",
                "with (FieldCount)",
                "with (Counter)",
            ),
            "2",
        ),
        (original.clone(), "2"),
    ];
    for (index, (files, expected)) in steps.iter().enumerate() {
        for (path, text) in files {
            warm.workspace().update(&mut warm, url(path), text.clone());
        }
        let warm_view = prepare(&warm, "app/");
        let fresh = project(&borrow(files));
        let fresh_view = prepare(&fresh, "app/");
        let warm_value = eval(&warm_view, "app/src/lib.fe", "total");
        let fresh_value = eval(&fresh_view, "app/src/lib.fe", "total");
        assert_eq!(warm_value, *expected, "warm value at step {index}");
        assert_eq!(fresh_value, *expected, "fresh value at step {index}");
        assert_eq!(
            ingot_errors(&warm_view, "app/"),
            ingot_errors(&fresh_view, "app/"),
            "diagnostics at step {index}"
        );
        assert_eq!(
            warm_view.generation_inputs().requests(&warm_view),
            fresh_view.generation_inputs().requests(&fresh_view),
            "frozen requests at step {index}"
        );
    }
}

#[test]
fn forked_inputs_keep_trust_roots_removals_and_settings() {
    let files = [
        ("lib/fe.toml", LIB_TOML),
        ("lib/src/lib.fe", "pub fn f() {}\n"),
        ("app/fe.toml", APP_WITH_LIB_TOML),
        ("app/src/lib.fe", "pub fn g() {}\n"),
        ("app/src/gone.fe", "pub fn h() {}\n"),
    ];
    let mut db = project(&files);
    let graph = db.dependency_graph();
    graph.register_root_ingot(&mut db, &url("app/"));
    graph.add_dependency(
        &mut db,
        &url("app/"),
        &url("lib/"),
        "lib".into(),
        DependencyArguments {
            allow_unsafe: true,
            ..DependencyArguments::default()
        },
    );
    db.workspace().remove(&mut db, &url("app/src/gone.fe"));
    let fork = db.fork_inputs();
    let trusted = |db: &DriverDataBase| {
        let mut trusted = db
            .dependency_graph()
            .trusted_ingots(db)
            .into_iter()
            .collect::<Vec<_>>();
        trusted.sort();
        trusted
    };
    assert_eq!(trusted(&fork), trusted(&db));
    assert!(trusted(&fork).contains(&url("lib/")));
    for root in ["app/", "lib/"] {
        assert_eq!(
            fork.dependency_graph().is_trust_root(&fork, &url(root)),
            db.dependency_graph().is_trust_root(&db, &url(root)),
            "{root}"
        );
    }
    assert!(
        fork.workspace()
            .get(&fork, &url("app/src/gone.fe"))
            .is_none()
    );
    let urls = |db: &DriverDataBase| {
        db.workspace()
            .all_files(db)
            .iter()
            .map(|(url, file)| (url.clone(), file.text(db).clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(urls(&fork), urls(&db));
    let app = fork
        .workspace()
        .containing_ingot(&fork, url("app/"))
        .expect("app ingot");
    assert_eq!(
        app.resolved_external_ingots(&fork)
            .iter()
            .map(|(_, ingot)| ingot.base(&fork).clone())
            .filter(|base| base.scheme() == "file")
            .collect::<Vec<_>>(),
        vec![url("lib/")]
    );
}

/// Two files that derive with each other's providers. Lowering one file's
/// output reads the other file's producer scope, which currently requires the
/// other file's lowered scope graph (`resolve_producer_scope`). Predicted to
/// fail with a query cycle until producer scopes are resolved lazily
/// (SIX-AREA-PLAN step A4). Expected once fixed: both totals are 1.
#[test]
#[ignore = "A4: cross-file producer scopes form a lowering cycle (predicted, unexecuted)"]
fn mutually_deriving_files_prepare_without_a_cycle() {
    let first = r#"use core::derive::Derive
use core::meta::{Code, Item, Record, field_count}
use ingot::score::Score
use ingot::second::SecondProvider

pub struct FirstProvider {}
impl Derive<Score> for FirstProvider {
    const fn derive(_ record: Record) -> Code<Item> {
        let n = field_count(record)
        quote { const fn score() -> usize { ${n} } }
    }
}
pub struct Left { pub only: u8 }
impl Score for Left with (SecondProvider)
pub const fn left_total() -> usize { Left::score() }
"#;
    let second = r#"use core::derive::Derive
use core::meta::{Code, Item, Record, field_count}
use ingot::score::Score
use ingot::first::FirstProvider

pub struct SecondProvider {}
impl Derive<Score> for SecondProvider {
    const fn derive(_ record: Record) -> Code<Item> {
        let n = field_count(record)
        quote { const fn score() -> usize { ${n} } }
    }
}
pub struct Right { pub only: u8 }
impl Score for Right with (FirstProvider)
pub const fn right_total() -> usize { Right::score() }
"#;
    let files = [
        ("app/fe.toml", APP_TOML),
        ("app/src/lib.fe", ""),
        ("app/src/score.fe", SCORE),
        ("app/src/first.fe", first),
        ("app/src/second.fe", second),
    ];
    let db = project(&files);
    let prepared = prepare(&db, "app/");
    assert_eq!(eval(&prepared, "app/src/first.fe", "left_total"), "1");
    assert_eq!(eval(&prepared, "app/src/second.fe", "right_total"), "1");
}
