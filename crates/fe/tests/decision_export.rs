use common::InputDb;
use driver::DriverDataBase;
use hir::analysis::ty::ty_check::check_func_body;
use serde_json::json;
use url::Url;

#[test]
fn export_reads_the_retained_equivalent_clone_answer() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///decision_export.fe").unwrap();
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some("fn dup<T>(_ x: T) -> T where T: Copy + Clone { x.clone() }".into()),
    );
    let module = db.top_mod(db.workspace().get(&db, &url).unwrap());
    let diagnostics = db.run_on_top_mod(module);
    assert!(diagnostics.is_empty(), "{}", diagnostics.format_diags(&db));
    let func = module.all_funcs(&db)[0];
    let (_, body) = check_func_body(&db, func);
    let records = fe::decision_export::goal_records(&db, body).unwrap();
    let clone = records
        .iter()
        .find(|row| row["goal"]["trait"] == "Clone")
        .unwrap();
    assert_eq!(clone["via"], json!("bound"));
    assert_eq!(clone["goal"]["self"], json!({"var": "T", "kind": "type"}));
    assert_eq!(clone["goal"]["args"], json!([]));
    assert_eq!(clone["site"]["line"], json!(1));
    assert!(clone["site"]["col"].as_u64().unwrap() > 40);
}

#[test]
fn export_distinguishes_ordinary_and_selected_answers() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///selected_decision_export.fe").unwrap();
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(
            r#"
trait Read<T> { fn read(self) -> u8 }
struct Word {}
impl Read<u8> for Word { fn read(self) -> u8 { 1 } }
impl Read<u8> for Word as Other { fn read(self) -> u8 { 2 } }
fn ordinary(_ x: Word) -> u8 { x.read() }
fn selected(_ x: Word) -> u8 { with (Other) { x.read() } }
"#
            .into(),
        ),
    );
    let module = db.top_mod(db.workspace().get(&db, &url).unwrap());
    let diagnostics = db.run_on_top_mod(module);
    assert!(diagnostics.is_empty(), "{}", diagnostics.format_diags(&db));
    for (name, via) in [
        ("ordinary", json!({"impl": 0})),
        ("selected", json!({"selected": 1})),
    ] {
        let func = module
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|func| func.name(&db).to_opt().unwrap().data(&db) == name)
            .unwrap();
        let (_, body) = check_func_body(&db, func);
        let records = fe::decision_export::goal_records(&db, body).unwrap();
        let answer = records
            .iter()
            .find(|row| row["goal"]["trait"] == "Read")
            .unwrap();
        assert_eq!(answer["via"], via);
        assert_eq!(answer["goal"]["self"], json!({"con": "Word", "args": []}));
        assert_eq!(answer["goal"]["args"], json!([{ "con": "u8", "args": [] }]));
    }
}

#[test]
fn export_preserves_boolean_parameter_kind() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///boolean_decision_export.fe").unwrap();
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(
            r#"
trait Test<const B: bool> { fn test(self) }
fn call<T, const B: bool>(_ x: T) where T: Test<B> { x.test() }
"#
            .into(),
        ),
    );
    let module = db.top_mod(db.workspace().get(&db, &url).unwrap());
    let diagnostics = db.run_on_top_mod(module);
    assert!(diagnostics.is_empty(), "{}", diagnostics.format_diags(&db));
    let func = module
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|func| func.name(&db).to_opt().unwrap().data(&db) == "call")
        .unwrap();
    let (_, body) = check_func_body(&db, func);
    let records = fe::decision_export::goal_records(&db, body).unwrap();
    assert_eq!(
        records[0]["goal"]["args"],
        json!([{ "var": "B", "kind": "bool" }])
    );
}

#[test]
fn export_does_not_number_external_impls_as_local() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///external_decision_export.fe").unwrap();
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some("fn call(_ x: u8) -> u8 { x.clone() }".into()),
    );
    let module = db.top_mod(db.workspace().get(&db, &url).unwrap());
    let diagnostics = db.run_on_top_mod(module);
    assert!(diagnostics.is_empty(), "{}", diagnostics.format_diags(&db));
    let (_, body) = check_func_body(&db, module.all_funcs(&db)[0]);
    assert!(
        fe::decision_export::goal_records(&db, body)
            .unwrap_err()
            .contains("another file")
    );
}
