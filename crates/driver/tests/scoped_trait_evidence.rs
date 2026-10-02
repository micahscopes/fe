use common::{
    InputDb,
    diagnostics::Severity,
    file::File,
    stdlib::{HasBuiltinCore, HasBuiltinStd},
};
use fe_driver::DriverDataBase;
use url::Url;

fn database() -> DriverDataBase {
    let mut db = DriverDataBase::default();
    db.initialize_builtin_core();
    db.initialize_builtin_std();
    db
}

fn input(db: &mut DriverDataBase, name: &str, source: &str) -> File {
    use salsa::Setter;
    let file = db.workspace().touch(
        db,
        Url::parse(&format!("file:///scoped-evidence/{name}.fe")).unwrap(),
        Some(source.to_owned()),
    );
    // touch reuses an existing input without replacing its source.
    file.set_text(db).to(source.to_owned());
    file
}

fn diagnostics(db: &DriverDataBase, file: File) -> String {
    let ingot = db.top_mod(file).ingot(db);
    let hir = db.run_on_ingot(ingot);
    let mut errors = if hir.has_errors(db) {
        hir.format_diags(db)
    } else {
        String::new()
    };
    let semantic = db.mir_diagnostics_for_ingot(ingot);
    if semantic
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        errors.push_str(&db.format_complete_diagnostics(&semantic));
    }
    errors
}

const SOURCE: &str = include_str!("../../fe/tests/fixtures/fe_test/scoped_trait_evidence.fe");

#[test]
fn scoped_witness_source_checks_with_associated_outputs() {
    let mut db = database();
    let file = input(&mut db, "consumer", SOURCE);
    let errors = diagnostics(&db, file);
    assert!(errors.is_empty(), "{errors}");
}

#[test]
fn missing_or_escaped_witness_is_rejected() {
    for call in [
        "fn absent() -> u256 { consume<u256>() }",
        "fn escaped() -> u256 { with (Compute<Item = u256> = Number { value: 1 }) { let value = consume<u256>() }\nconsume<u256>() }",
    ] {
        let mut db = database();
        let file = input(&mut db, "missing", &format!("{SOURCE}\n{call}"));
        let errors = diagnostics(&db, file);
        assert!(errors.contains("missing effect"), "{errors}");
    }
}

#[test]
fn provider_associated_output_cannot_be_relabelled() {
    let mut db = database();
    let source = SOURCE.replace("Compute<Item = bool> = Flag", "Compute<Item = u256> = Flag");
    let file = input(&mut db, "wrong-output", &source);
    let errors = diagnostics(&db, file);
    assert!(
        !errors.is_empty(),
        "a bool provider must not supply u256 evidence"
    );
    assert!(
        errors.contains("bool") && errors.contains("u256"),
        "{errors}"
    );
}

#[test]
fn scoped_witness_does_not_make_effectful_ctfe_legal() {
    let mut db = database();
    let source = SOURCE.replace("fn consume<T>", "const fn consume<T>");
    let file = input(&mut db, "const-effects", &source);
    let errors = diagnostics(&db, file);
    assert!(errors.contains("non-const call in `const fn`"), "{errors}");
}

#[test]
fn provider_key_edits_match_fresh_databases() {
    let mut db = database();
    for valid in [true, false, true] {
        let source = if valid {
            SOURCE.to_owned()
        } else {
            SOURCE.replace("Compute<Item = bool> = Flag", "Compute<Item = u256> = Flag")
        };
        let file = input(&mut db, "edits", &source);
        let warm = diagnostics(&db, file);
        assert_eq!(warm.is_empty(), valid, "{warm}");
        let mut fresh = database();
        let fresh_file = input(&mut fresh, "edits", &source);
        assert_eq!(warm, diagnostics(&fresh, fresh_file));
    }
}

#[test]
fn scoped_trait_provider_calls_borrow_noncopy_root() {
    let mut db = database();
    // A lexical provider is a stable root. Repeated implicit calls borrow it;
    // explicit moves out of a provider binding are checked separately.
    let source = SOURCE
        .replace("trait Compute: Copy", "trait Compute")
        .replace("impl Copy for Number {}", "");
    let file = input(&mut db, "noncopy", &source);
    let errors = diagnostics(&db, file);
    assert!(errors.is_empty(), "{errors}");
}
