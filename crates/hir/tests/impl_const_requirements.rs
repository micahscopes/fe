use dir_test::{Fixture, dir_test};
use fe_hir::test_db::{HirAnalysisTestDb, format_diagnostics};

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/impl_const_requirements/pass", glob: "*.fe")]
fn accepted(fixture: Fixture<&str>) {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(fixture.path().into(), fixture.content());
    let (top, _) = db.top_mod(file);
    let diagnostics = format_diagnostics(&db, &db.run_on_top_mod(top));
    assert!(diagnostics.is_empty(), "{diagnostics}");
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/impl_const_requirements/fail", glob: "*.fe")]
fn rejected(fixture: Fixture<&str>) {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(fixture.path().into(), fixture.content());
    let (top, _) = db.top_mod(file);
    let diagnostics = format_diagnostics(&db, &db.run_on_top_mod(top));
    assert!(
        !diagnostics.is_empty(),
        "invalid conditional impl use was accepted"
    );
    assert!(!diagnostics.contains("not supported yet"), "{diagnostics}");
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/impl_const_requirements/trait_method", glob: "*.fe")]
fn trait_method_condition(fixture: Fixture<&str>) {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(fixture.path().into(), fixture.content());
    let (top, _) = db.top_mod(file);
    let diagnostics = format_diagnostics(&db, &db.run_on_top_mod(top));
    if fixture.path().ends_with("accepted.fe") {
        assert!(diagnostics.is_empty(), "{diagnostics}");
    } else {
        assert!(
            diagnostics.contains("const requirement is not satisfied"),
            "{diagnostics}"
        );
        assert!(!diagnostics.contains("not supported"), "{diagnostics}");
    }
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/impl_const_requirements/inherent_impl", glob: "*.fe")]
fn inherent_impl_condition(fixture: Fixture<&str>) {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(fixture.path().into(), fixture.content());
    let (top, _) = db.top_mod(file);
    let diagnostics = format_diagnostics(&db, &db.run_on_top_mod(top));
    if fixture.path().ends_with("accepted.fe") {
        assert!(diagnostics.is_empty(), "{diagnostics}");
    } else {
        assert!(
            diagnostics.contains("const requirement is not satisfied"),
            "{diagnostics}"
        );
        assert!(!diagnostics.contains("not supported"), "{diagnostics}");
    }
}

#[test]
fn impl_condition_edits_and_restoration_match_fresh_checks() {
    use salsa::Setter;
    let source = include_str!("fixtures/impl_const_requirements/pass/forward.fe");
    let mut warm = HirAnalysisTestDb::default();
    let file = warm.new_stand_alone("impl_edits.fe".into(), source);
    for condition in ["N > 0", "N > 1", "N > 0"] {
        // Change only the implementation's condition. The caller retains N > 0.
        let changed = source.replacen("where N > 0", &format!("where {condition}"), 1);
        file.set_text(&mut warm).to(changed.clone());
        let (top, _) = warm.top_mod(file);
        let actual = format_diagnostics(&warm, &warm.run_on_top_mod(top));
        let mut fresh = HirAnalysisTestDb::default();
        let fresh_file = fresh.new_stand_alone("impl_edits.fe".into(), &changed);
        let (fresh_top, _) = fresh.top_mod(fresh_file);
        let expected = format_diagnostics(&fresh, &fresh.run_on_top_mod(fresh_top));
        assert_eq!(actual, expected);
        assert_eq!(actual.is_empty(), condition == "N > 0", "{actual}");
    }
}

#[test]
fn impl_condition_premises_stay_with_the_caller_in_both_query_orders() {
    use fe_hir::analysis::ty::ty_check::check_func_body;
    let source = include_str!("fixtures/impl_const_requirements/scoped_callers.fe");
    for order in [["permitted", "missing"], ["missing", "permitted"]] {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("scoped_callers.fe".into(), source);
        let (top, _) = db.top_mod(file);
        for name in order.into_iter().cycle().take(4) {
            let function = top
                .all_funcs(&db)
                .iter()
                .copied()
                .find(|function| {
                    function
                        .name(&db)
                        .to_opt()
                        .is_some_and(|id| id.data(&db) == name)
                })
                .expect("fixture caller");
            let diagnostics = &check_func_body(&db, function).0;
            assert_eq!(
                diagnostics.is_empty(),
                name == "permitted",
                "{name}: {diagnostics:?}"
            );
        }
    }
}
