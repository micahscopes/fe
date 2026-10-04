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
        let call = records
            .iter()
            .find(|row| row["kind"] == "item" && row["item"] == "fn" && row["name"] == "call")
            .unwrap();
        assert_eq!(call["body"][0]["use"], "call");
        assert_eq!(call["body"][0]["fn"], "need");
        assert_eq!(call["body"][0]["span"], json!({"line":2,"col":13}));
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

#[test]
fn export_program_declarations_preserve_binders_and_requirements() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///declaration_export.fe").unwrap();
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(
            r#"
trait Read<const N: usize> {
    const OK: bool
    fn read() -> u256 where N > 0
}
struct Word<const N: usize> where N > 0 {}
impl<const N: usize> Read<N> for Word<N> where N > 0 {
    const OK: bool = true
    fn read() -> u256 where N > 0 { 1 }
}
const Value: u32 = 7
struct Outside { field: u8 }
trait Base {}
trait OutsideTrait: Base {}
"#
            .into(),
        ),
    );
    let module = db.top_mod(db.workspace().get(&db, &url).unwrap());
    let diagnostics = db.run_on_top_mod(module);
    assert!(diagnostics.is_empty(), "{}", diagnostics.format_diags(&db));
    let records =
        fe::decision_export::declaration_records(&db, module, &diagnostics.complete_diags(&db));
    let requirement = json!({"op":">","a":{"var":"N"},"b":{"nat":0}});
    for kind in ["trait", "struct", "impl"] {
        let record = records
            .iter()
            .find(|row| row["item"] == kind && row["name"] != "Outside")
            .unwrap();
        assert_eq!(record["params"], json!([{ "name":"N","kind":"nat" }]));
        if kind != "trait" {
            assert_eq!(record["conds"], json!([requirement]));
        }
    }
    let trait_ = records.iter().find(|row| row["item"] == "trait").unwrap();
    assert_eq!(
        trait_["assoc_consts"],
        json!([{ "name":"OK","kind":"bool" }])
    );
    assert_eq!(
        trait_["methods"],
        json!([{ "name":"read","conds":[requirement] }])
    );
    let impl_ = records.iter().find(|row| row["item"] == "impl").unwrap();
    assert_eq!(impl_["index"], json!(0));
    assert_eq!(impl_["trait"], json!("Read"));
    assert_eq!(impl_["targs"], json!([{ "var":"N","kind":"nat" }]));
    assert_eq!(
        impl_["self"],
        json!({"con":"Word","args":[{ "var":"N","kind":"nat" }]})
    );
    assert_eq!(
        impl_["consts"],
        json!([{ "name":"OK","value":{ "bool":true } }])
    );
    assert!(
        records
            .iter()
            .any(|row| row["item"] == "value" && row["name"] == "Value")
    );
    assert!(records.iter().any(|row| row["item"] == "out_of_fragment"
        && row["reason"].as_str().unwrap().contains("struct fields")));
    assert!(records.iter().any(|row| row["item"] == "out_of_fragment"
        && row["reason"].as_str().unwrap().contains("super-traits")));
}

#[test]
fn export_program_uses_preserve_structure_terms_and_spans() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///program_use_export.fe").unwrap();
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(
            r#"
trait Read { const OK: bool
 fn read() -> u256 }
struct Word {}
impl Read for Word { const OK: bool = true
 fn read() -> u256 { 1 } }
impl Read for Word as Other { const OK: bool = true
 fn read() -> u256 { 2 } }
fn need<const B: bool>() -> u256 where B { 0 }
fn exercise() -> u256 {
    let _a: Word = Word {}
    let _b: bool = <Word as Read>::OK
    let _c: u256 = need<<Word as Read>::OK>()
    let _f: u256 = Word::read()
    let _d: u256 = with (Other) {
        let _e: u256 = <Word as Read>::read()
        0
    }
    0
}
"#
            .into(),
        ),
    );
    let module = db.top_mod(db.workspace().get(&db, &url).unwrap());
    let diagnostics = db.run_on_top_mod(module);
    assert!(diagnostics.is_empty(), "{}", diagnostics.format_diags(&db));
    let records =
        fe::decision_export::declaration_records(&db, module, &diagnostics.complete_diags(&db));
    let function = records
        .iter()
        .find(|r| r["item"] == "fn" && r["name"] == "exercise")
        .unwrap_or_else(|| panic!("{records:#?}"));
    let uses = function["body"].as_array().unwrap();
    assert_eq!(uses.len(), 5);
    assert_eq!(uses[0]["use"], "type");
    assert_eq!(uses[1]["use"], "const");
    let goal = json!({"self":{"con":"Word","args":[]},"trait":"Read","args":[]});
    assert_eq!(uses[1]["goal"], goal);
    assert_eq!(
        uses[2]["args"],
        json!([{"assoc":goal["self"],"trait":"Read","targs":[],"c":"OK"}])
    );
    assert_eq!(uses[3]["goal"], goal);
    assert_eq!(uses[3]["qualified"], false);
    assert_eq!(uses[4]["use"], "with");
    assert_eq!(uses[4]["name"], "Other");
    assert_eq!(uses[4]["construct_span"], json!({"line":15,"col":20}));
    let function = module
        .all_funcs(&db)
        .iter()
        .find(|f| f.name(&db).to_opt().unwrap().data(&db) == "exercise")
        .copied()
        .unwrap();
    let (_, typed) = check_func_body(&db, function);
    let openings =
        fe::decision_export::with_records(&db, function.body(&db).unwrap(), typed).unwrap();
    assert_eq!(uses[4]["span"], openings[0]["site"]);
    assert!(
        uses[4]["span_aliases"]
            .as_array()
            .unwrap()
            .iter()
            .all(|span| span["line"] == 15)
    );
    assert!(
        uses[4]["body"][0]["span_aliases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|span| span["line"] == 16)
    );
    assert_eq!(uses[4]["body"][0]["goal"], goal);
    assert_eq!(uses[4]["body"][0]["qualified"], true);
    assert_eq!(uses[0]["span"], json!({"line":11,"col":20}));
    assert_eq!(uses[4]["body"][0]["span"], json!({"line":16,"col":24}));
}

#[test]
fn export_overlap_uses_reported_pairs_and_keeps_disjoint_pairs() {
    for (second, conflict) in [("Word", true), ("Other", false)] {
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///overlap_export.fe").unwrap();
        let source = format!(
            "trait Read {{}}\nstruct Word {{}}\nstruct Other {{}}\nimpl Read for Word {{}}\nimpl Read for {second} {{}}\n"
        );
        db.workspace().touch(&mut db, url.clone(), Some(source));
        let module = db.top_mod(db.workspace().get(&db, &url).unwrap());
        let completed = db.run_on_top_mod(module).complete_diags(&db);
        let pairs = fe::decision_export::overlap_records(&db, module, &completed).unwrap();
        assert_eq!(
            pairs,
            json!([{"v":1,"kind":"overlap","a":0,"b":1,"conflict":conflict}])
                .as_array()
                .unwrap()
                .clone()
        );
        let errors = fe::decision_export::diagnostic_records(&db, &completed).unwrap();
        assert_eq!(errors.iter().any(|e| e["code"] == "5-0001"), conflict);
        if conflict {
            assert_eq!(errors[0]["site"]["line"], 4);
        }
    }
}

#[test]
fn export_keeps_written_type_conditions_in_generic_const_paths() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///written_condition_export.fe").unwrap();
    db.workspace().touch(&mut db,url.clone(),Some("trait HasL { const L: usize }\nstruct Bounded<const N: usize> where N > 0 {}\nimpl<T> HasL for T { const L: usize = 1 }\nfn g<const M: usize>() {}\nfn failing() { g<<Bounded<0> as HasL>::L>() }\nfn passing() { g<<Bounded<1> as HasL>::L>() }\n".into()));
    let module = db.top_mod(db.workspace().get(&db, &url).unwrap());
    let diagnostics = db.run_on_top_mod(module);
    assert!(diagnostics.has_errors(&db));
    for (name, outcome) in [("failing", "eval-false"), ("passing", "eval-true")] {
        let function = module
            .all_funcs(&db)
            .iter()
            .find(|f| f.name(&db).to_opt().unwrap().data(&db) == name)
            .copied()
            .unwrap();
        let owner = function.body(&db).unwrap();
        let written = hir::analysis::ty::ty_check::written_condition_uses(
            &db,
            hir::hir_def::ItemKind::Body(owner),
        )
        .count();
        assert!(
            written > 0,
            "authored check did not retain a condition for {name}"
        );
        let (_, typed) = check_func_body(&db, function);
        let conditions = fe::decision_export::condition_records(&db, typed).unwrap();
        assert_eq!(
            conditions.len(),
            1,
            "one authored type application should retain one check, {name}: {conditions:#?}"
        );
        let program =
            fe::decision_export::declaration_records(&db, module, &diagnostics.complete_diags(&db));
        let function = program
            .iter()
            .find(|r| r["item"] == "fn" && r["name"] == name)
            .unwrap();
        let aliases = function["body"][0]["span_aliases"].as_array().unwrap();
        assert!(
            conditions
                .iter()
                .all(|condition| aliases.contains(&condition["site"])),
            "nested condition coordinate was not paired with its authored call"
        );
        assert!(
            conditions
                .iter()
                .any(|r| r["source"] == "struct Bounded" && r["outcome"] == outcome),
            "{name}: {conditions:#?}"
        );
    }
}

#[test]
fn export_pairs_annotation_conditions_with_their_initializer_use() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///annotation_export.fe").unwrap();
    db.workspace().touch(&mut db,url.clone(),Some("struct Bounded<const N: usize> where N > 0 {}\nfn bad() { let _x: Bounded<0> = Bounded<0> {} }\nfn good() { let _x: Bounded<1> = Bounded<1> {} }\n".into()));
    let module = db.top_mod(db.workspace().get(&db, &url).unwrap());
    let diagnostics = db.run_on_top_mod(module);
    let completed = diagnostics.complete_diags(&db);
    let program = fe::decision_export::declaration_records(&db, module, &completed);
    for (name, outcome) in [("bad", "eval-false"), ("good", "eval-true")] {
        let function = module
            .all_funcs(&db)
            .iter()
            .find(|f| f.name(&db).to_opt().unwrap().data(&db) == name)
            .copied()
            .unwrap();
        let (_, typed) = check_func_body(&db, function);
        let conditions = fe::decision_export::condition_records(&db, typed).unwrap();
        let record = program
            .iter()
            .find(|r| r["item"] == "fn" && r["name"] == name)
            .unwrap();
        let use_ = &record["body"][0];
        assert_eq!(use_["use"], "type");
        assert!(conditions.iter().any(|c| c["outcome"] == outcome));
        assert!(conditions.iter().all(|c| {
            use_["span_aliases"]
                .as_array()
                .unwrap()
                .contains(&c["site"])
        }));
        assert!(
            conditions
                .iter()
                .any(|c| c["site"]["col"].as_u64() < use_["span"]["col"].as_u64()),
            "fixture must check the annotation before the initializer"
        );
    }
}

#[test]
fn export_pairs_override_errors_with_impl_without_capturing_body_errors() {
    use hir::span::LazySpan;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///override_alias_export.fe").unwrap();
    db.workspace().touch(&mut db,url.clone(),Some("trait Read { fn read() -> u256 }\nstruct Word {}\nimpl Read for Word { fn read() -> u256 where false { missing } }\n".into()));
    let module = db.top_mod(db.workspace().get(&db, &url).unwrap());
    let completed = db.run_on_top_mod(module).complete_diags(&db);
    let records = fe::decision_export::declaration_records(&db, module, &completed);
    let impl_ = records.iter().find(|r| r["item"] == "impl").unwrap();
    let aliases = impl_["span_aliases"].as_array().unwrap();
    let override_ = completed
        .iter()
        .find(|d| d.error_code.to_string() == "6-0024")
        .unwrap();
    let errors =
        fe::decision_export::diagnostic_records(&db, std::slice::from_ref(override_)).unwrap();
    assert!(aliases.contains(&errors[0]["site"]));
    let function = module
        .all_funcs(&db)
        .iter()
        .find(|f| f.body(&db).is_some())
        .copied()
        .unwrap();
    let body_span = function.body(&db).unwrap().span().resolve(&db).unwrap();
    let body_error = completed
        .iter()
        .find(|d| {
            d.primary_span()
                .is_some_and(|span| body_span.range.contains(span.range.start()))
        })
        .unwrap();
    let errors =
        fe::decision_export::diagnostic_records(&db, std::slice::from_ref(body_error)).unwrap();
    assert!(!aliases.contains(&errors[0]["site"]));
}

#[test]
fn export_keeps_successful_path_receiver_conditions() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///path_receiver_condition.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some("trait Helper { const X: usize\nfn helper() -> u256 }\nstruct Bounded<const N: usize> where N > 0 {}\nimpl<T> Helper for T { const X: usize = 1\nfn helper() -> u256 { 1 } }\nfn allowed() -> u256 { let _first: u256 = Bounded<1>::helper()\nlet _second: usize = <Bounded<1> as Helper>::X\n0 }\n".into()));
    let module = db.top_mod(db.workspace().get(&db, &url).unwrap());
    let diagnostics = db.run_on_top_mod(module);
    assert!(
        !diagnostics.has_errors(&db),
        "{}",
        diagnostics.format_diags(&db)
    );
    let function = module
        .all_funcs(&db)
        .iter()
        .find(|f| f.name(&db).to_opt().unwrap().data(&db) == "allowed")
        .copied()
        .unwrap();
    let (_, typed) = check_func_body(&db, function);
    let conditions = fe::decision_export::condition_records(&db, typed).unwrap();
    assert_eq!(
        conditions
            .iter()
            .filter(|r| r["source"] == "struct Bounded" && r["outcome"] == "eval-true")
            .count(),
        2,
        "each receiver site must retain its check: {conditions:#?}"
    );
}
