use common::InputDb;
use dir_test::{Fixture, dir_test};
use fe_driver::DriverDataBase;
use hir::analysis::{
    semantic::{GenericSubst, eval_body_owner_const_with_args},
    ty::ty_check::BodyOwner,
};
use url::Url;

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/generated_primitive_scope", glob: "*.fe")]
fn generated_primitive_is_not_a_target_alias(fixture: Fixture<&str>) {
    let url = Url::from_file_path(fixture.path()).expect("fixture path is absolute");
    let mut db = DriverDataBase::default();
    db.workspace()
        .update(&mut db, url, fixture.content().to_string());
    assert!(
        fe_driver::elaboration::prepare(&db).is_err(),
        "generated u256 was captured by the target module's u8 alias"
    );
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/generated_primitive_scope_valid", glob: "*.fe")]
fn generated_primitive_can_leave_shadowed_module(fixture: Fixture<&str>) {
    let url = Url::from_file_path(fixture.path()).expect("fixture path is absolute");
    let mut db = DriverDataBase::default();
    db.workspace()
        .update(&mut db, url.clone(), fixture.content().to_string());
    let output = fe_driver::elaboration::prepare(&db)
        .unwrap()
        .expect("generation request");
    let file = output.workspace().get(&output, &url).unwrap();
    let function = output
        .top_mod(file)
        .all_funcs(&output)
        .iter()
        .copied()
        .find(|function| {
            function
                .name(&output)
                .to_opt()
                .is_some_and(|name| name.data(&output) == "answer")
        })
        .expect("answer function");
    let result = eval_body_owner_const_with_args(
        &output,
        BodyOwner::Func(function),
        GenericSubst::for_body_owner(&output, BodyOwner::Func(function), vec![]),
        vec![],
    )
    .into_ready()
    .expect("generated answer evaluates")
    .pretty_print(&output);
    assert_eq!(result, "7");
}
