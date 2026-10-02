use dir_test::{Fixture, dir_test};
use fe_hir::{
    analysis::ty::{
        const_ty::CallableInputLayoutHoleOrigin,
        ty_check::{ReturnSource, check_func_body},
    },
    test_db::HirAnalysisTestDb,
};

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/selected_return_provenance",
    glob: "*.fe"
)]
fn selected_impl_controls_return_provenance(fixture: Fixture<&str>) {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(fixture.path().into(), fixture.content());
    let (top, _) = db.top_mod(file);
    db.assert_no_diags(top);

    for (name, param) in [("ordinary", 0), ("selected", 1)] {
        let func = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|func| {
                func.name(&db)
                    .to_opt()
                    .is_some_and(|id| id.data(&db) == name)
            })
            .expect("fixture function");
        let sources = check_func_body(&db, func).1.forwarded_return_sources(&db);
        assert_eq!(
            sources,
            [ReturnSource {
                result_projection: Vec::new(),
                origin: CallableInputLayoutHoleOrigin::ValueParam(param),
                projection: Vec::new(),
            }],
            "{name}"
        );
    }
}
