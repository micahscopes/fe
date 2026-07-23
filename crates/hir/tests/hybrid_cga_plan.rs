use fe_hir::test_db::HirAnalysisTestDb;

#[test]
fn fe_support_scan_materializes_a_direct_32_term_schedule() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "cga_schedule_ctfe_specialized_render.fe".into(),
        include_str!("../../codegen/tests/fixtures/spirv/cga_schedule_ctfe_specialized_render.fe"),
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = db.run_on_top_mod(top_mod);
    assert!(
        diags.is_empty(),
        "unexpected diagnostic count: {}",
        diags.len()
    );
}
