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

#[test]
fn export_retains_condition_terms_and_actual_discharge() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///condition_decision_export.fe").unwrap();
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(
            r#"
fn need<const N: usize>() where N > 0 {}
fn good() { need<7>() }
fn bad() { need<0>() }
fn forward<const N: usize>() where N > 0 { need<N>() }
"#
            .into(),
        ),
    );
    let module = db.top_mod(db.workspace().get(&db, &url).unwrap());
    let diagnostics = db.run_on_top_mod(module);
    assert!(!diagnostics.is_empty(), "bad must fail its condition");
    for (name, outcome) in [
        ("good", "eval-true"),
        ("bad", "eval-false"),
        ("forward", "premise:0"),
    ] {
        let func = module
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|func| func.name(&db).to_opt().unwrap().data(&db) == name)
            .unwrap();
        let (_, body) = check_func_body(&db, func);
        let records = fe::decision_export::condition_records(&db, body).unwrap();
        assert_eq!(records.len(), 1, "{name}");
        assert_eq!(records[0]["source"], json!("fn need"));
        assert_eq!(records[0]["outcome"], json!(outcome));
        assert_eq!(records[0]["sels"], json!([]));
        assert_eq!(records[0]["pred"]["op"], json!(">"));
        assert_eq!(records[0]["pred"]["b"], json!({"nat":0}));
    }
}

#[test]
fn export_reads_the_condition_selection_used_for_discharge() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///selected_condition_export.fe").unwrap();
    db.workspace().touch(&mut db,url.clone(),Some(include_str!("../../hir/tests/fixtures/impl_const_requirements/pass/selected_body_impl_condition.fe").into()));
    let module = db.top_mod(db.workspace().get(&db, &url).unwrap());
    let diagnostics = db.run_on_top_mod(module);
    assert!(diagnostics.is_empty(), "{}", diagnostics.format_diags(&db));
    let func = module
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|func| func.name(&db).to_opt().unwrap().data(&db) == "selected")
        .unwrap();
    let (_, body) = check_func_body(&db, func);
    let records = fe::decision_export::condition_records(&db, body).unwrap();
    let condition = records
        .iter()
        .find(|row| row["source"] == "impl #2")
        .unwrap();
    assert_eq!(condition["sels"], json!([1]));
    assert_eq!(condition["outcome"], json!("eval-true"));
    assert_eq!(
        condition["pred"],
        json!({"assoc":{"con":"Subject","args":[]},"trait":"Compute","targs":[],"c":"OK"})
    );
}

#[test]
fn export_reads_successful_and_failed_named_openings() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///opening_decision_export.fe").unwrap();
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(
            r#"
trait Read { fn read() }
struct Word {}
impl Read for Word as Other { fn read() {} }
fn selected() { with (Other) {} }
fn unknown() { with (Missing) {} }
"#
            .into(),
        ),
    );
    let module = db.top_mod(db.workspace().get(&db, &url).unwrap());
    let diagnostics = db.run_on_top_mod(module);
    assert!(!diagnostics.is_empty(), "unknown opening must fail");
    for (func_name, name, outcome) in [
        ("selected", "Other", json!({"impl":0})),
        ("unknown", "Missing", json!({"error":"8-0099"})),
    ] {
        let func = module
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|func| func.name(&db).to_opt().unwrap().data(&db) == func_name)
            .unwrap();
        let (_, body) = check_func_body(&db, func);
        let records =
            fe::decision_export::with_records(&db, func.body(&db).unwrap(), body).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["name"], json!(name));
        assert_eq!(records[0]["outcome"], outcome);
        assert!(records[0]["site"]["col"].as_u64().unwrap() > 20);
    }
}

#[test]
fn cli_export_preserves_check_results_and_source_provenance() {
    use sha2::{Digest, Sha256};
    use std::process::Command;
    for (source, expected_status, outcome) in [
        (
            "fn need<const N: usize>() where N > 0 {}\nfn call() { need<7>() }\n",
            0,
            "eval-true",
        ),
        (
            "fn need<const N: usize>() where N > 0 {}\nfn call() { need<0>() }\n",
            1,
            "eval-false",
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("program.fe");
        let output = temp.path().join("decisions.jsonl");
        std::fs::write(&input, source).unwrap();
        let baseline = Command::new(env!("CARGO_BIN_EXE_fe"))
            .arg("check")
            .arg("--standalone")
            .arg(&input)
            .output()
            .unwrap();
        let exported = Command::new(env!("CARGO_BIN_EXE_fe"))
            .arg("check")
            .arg("--standalone")
            .arg("--decisions-out")
            .arg(&output)
            .arg(&input)
            .output()
            .unwrap();
        assert_eq!(baseline.status.code(), Some(expected_status));
        assert_eq!(
            exported.status.code(),
            baseline.status.code(),
            "{}",
            String::from_utf8_lossy(&exported.stderr)
        );
        assert_eq!(exported.stdout, baseline.stdout);
        assert_eq!(exported.stderr, baseline.stderr);
        let records: Vec<serde_json::Value> = std::fs::read_to_string(&output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records[0]["schema"], json!("fe-decisions/1"));
        assert_eq!(
            records[0]["source_sha256"],
            json!(format!("{:x}", Sha256::digest(source.as_bytes())))
        );
        assert_ne!(records[0]["compiler_commit"], json!("unknown"));
        assert!(
            records
                .iter()
                .any(|row| row["kind"] == "condition" && row["outcome"] == outcome)
        );
        assert!(records.iter().all(|row| row["v"] == 1));
        assert!(
            records
                .iter()
                .all(|row| row["kind"] != "select_impl" && row["kind"] != "read_impl_choice")
        );
    }
}
