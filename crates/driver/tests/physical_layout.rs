use common::InputDb;
use dir_test::{Fixture, dir_test};
use fe_driver::DriverDataBase;
use hir::analysis::semantic::GenericSubst;
use hir::analysis::{semantic::eval_body_owner_const_with_args, ty::ty_check::BodyOwner};

fn size(db: &mut DriverDataBase, source: &str) -> Result<String, String> {
    let file = db.workspace().update(
        db,
        "file:///physical_layout.fe".parse().unwrap(),
        source.to_owned(),
    );
    let module = db.top_mod(file);
    let diagnostics = db.run_on_ingot(module.ingot(db));
    if diagnostics.has_errors(db) {
        return Err(diagnostics.format_diags(db));
    }
    let function = module
        .all_funcs(db)
        .iter()
        .copied()
        .find(|function| {
            function
                .name(db)
                .to_opt()
                .is_some_and(|name| name.data(db) == "observe")
        })
        .unwrap();
    match eval_body_owner_const_with_args(
        db,
        BodyOwner::Func(function),
        GenericSubst::for_body_owner(db, BodyOwner::Func(function), vec![]),
        vec![],
    ) {
        hir::analysis::semantic::EvalOutcome::Ready(value) => {
            Ok(value.pretty_print(db).to_string())
        }
        other => Err(format!("{other:?}")),
    }
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/physical_layout",
    glob: "*.fe"
)]
fn source_edits_recompute_packed_size_like_fresh_compilation(fixture: Fixture<&str>) {
    const BASELINE: &str = include_str!("fixtures/physical_layout/baseline.fe");
    let expected = fixture
        .content()
        .lines()
        .next()
        .unwrap()
        .strip_prefix("// expected-size: ")
        .unwrap();

    let mut warm = DriverDataBase::default();
    assert_eq!(size(&mut warm, BASELINE).unwrap(), "11");
    let edited = size(&mut warm, fixture.content()).unwrap();
    let mut fresh = DriverDataBase::default();
    assert_eq!(edited, size(&mut fresh, fixture.content()).unwrap());
    assert_eq!(edited, expected);
    assert_eq!(size(&mut warm, BASELINE).unwrap(), "11");
}
