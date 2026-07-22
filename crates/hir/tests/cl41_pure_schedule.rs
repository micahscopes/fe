use fe_hir::test_db::HirAnalysisTestDb;

#[test]
fn pure_fe_cl41_sandwich_normalizes_80_sources_to_exact_32_term_schedule() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "type_schedule_cl41_pure.fe".into(),
        include_str!("fixtures/type_schedule_cl41_pure.fe"),
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}
