use common::InputDb;
use dir_test::{Fixture, dir_test};
use fe_driver::DriverDataBase;
use hir::{
    analysis::ty::ty_check::check_func_body,
    hir_def::{Expr, Partial, Stmt},
};
use salsa::plumbing::AsId;
use url::Url;

fn assert_on_the_fly_identity(_fixture: Fixture<&str>) {
    unimplemented!("RD84 on-the-fly derivation identity")
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/derive_identity",
    glob: "*.fe",
)]
#[allow(dead_code)] // The first derivation identity fixture is still pending.
fn on_the_fly_derivation_identity(fixture: Fixture<&str>) {
    assert_on_the_fly_identity(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/derive_identity",
    glob: "pending/*.fe",
)]
#[dir_test_attr(#[ignore = "target not built yet"])]
fn on_the_fly_derivation_identity_pending(fixture: Fixture<&str>) {
    assert_on_the_fly_identity(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_generated_function.fe"
)]
fn quoted_function_has_a_call_plan(fixture: Fixture<&str>) {
    assert_quoted_function_has_call_plan(fixture, "generated_function_runs");
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_scalar_splice.fe"
)]
fn quoted_scalar_splices_have_a_call_plan(fixture: Fixture<&str>) {
    assert_quoted_function_has_call_plan(fixture, "scalar_splices_run");
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_impl_helper.fe"
)]
fn quoted_impl_method_has_a_call_plan(fixture: Fixture<&str>) {
    assert_quoted_function_has_call_plan(fixture, "quote_from_impl_method_runs");
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_impl_helper.fe"
)]
fn quoted_impl_owner_survives_sibling_insertion(fixture: Fixture<&str>) {
    let url = Url::from_file_path(fixture.path()).expect("fixture URL");
    let mut authored = DriverDataBase::default();
    authored
        .workspace()
        .update(&mut authored, url.clone(), fixture.content().to_string());
    let mut output = fe_driver::elaboration::prepare(&authored)
        .expect("generation succeeds")
        .expect("generation request");
    let frozen = hir::HirDb::generated_module(&output, &url).expect("frozen request");
    let [request] = frozen.requests(&output).as_slice() else {
        panic!("expected one generation request");
    };
    let hir::core::meta::FrozenMetaValue::Code(code) = &request.value else {
        panic!("expected quoted code");
    };
    let Some(hir::core::meta::FrozenItemLocator::ImplMember {
        trait_path,
        self_type_path,
        member_name,
        ..
    }) = code.nodes[code.root].template().map(|key| &key.owner)
    else {
        panic!("expected an impl-member quote owner");
    };
    assert_eq!(trait_path.as_deref(), Some("Helper"));
    assert_eq!(self_type_path, "Tag");
    assert_eq!(member_name, "make");
    let answer = |db: &DriverDataBase| {
        let file = db.workspace().get(db, &url).expect("fixture file");
        db.top_mod(file)
            .all_funcs(db)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(db)
                    .to_opt()
                    .is_some_and(|name| name.data(db) == "answer")
            })
            .expect("generated answer")
            .as_id()
    };
    let original = answer(&output);
    let marker = "    impl Helper for Tag {";
    assert_eq!(fixture.content().matches(marker).count(), 1);
    let earlier = "    struct Other {}\n    impl Helper for Other {\n        const fn make() -> Code<Item> { quote { pub fn other() -> u256 { 0 } } }\n    }\n\n    impl Helper for Tag {";
    output.workspace().update(
        &mut output,
        url.clone(),
        fixture.content().replace(marker, earlier),
    );
    assert_eq!(original, answer(&output), "generated identity changed");
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_array_loop_sum.fe"
)]
fn quoted_array_sum_retains_one_loop(fixture: Fixture<&str>) {
    let url = Url::from_file_path(fixture.path()).expect("fixture URL");
    let mut authored = DriverDataBase::default();
    authored
        .workspace()
        .update(&mut authored, url.clone(), fixture.content().to_string());
    let output = fe_driver::elaboration::prepare(&authored)
        .expect("generation succeeds")
        .expect("generation request");
    let file = output.workspace().get(&output, &url).expect("fixture file");
    let sum = output
        .top_mod(file)
        .all_impl_traits(&output)
        .iter()
        .flat_map(|implementation| implementation.methods(&output))
        .find(|function| {
            function
                .name(&output)
                .to_opt()
                .is_some_and(|name| name.data(&output) == "sum")
        })
        .expect("generated sum");
    let body = sum.body(&output).expect("generated body");
    let loops = body
        .stmts(&output)
        .iter()
        .filter(|(_, stmt)| matches!(stmt, Partial::Present(Stmt::While(..))))
        .count();
    assert_eq!(loops, 1, "generated sum should keep one loop");
}

fn assert_quoted_function_has_call_plan(fixture: Fixture<&str>, caller_name: &str) {
    let url = Url::from_file_path(fixture.path()).expect("fixture URL");
    let mut authored = DriverDataBase::default();
    authored
        .workspace()
        .update(&mut authored, url.clone(), fixture.content().to_string());
    let output = fe_driver::elaboration::prepare(&authored)
        .expect("generation succeeds")
        .expect("generation request");
    let file = output.workspace().get(&output, &url).expect("fixture file");
    let module = output.top_mod(file);
    let caller = module
        .all_funcs(&output)
        .iter()
        .copied()
        .find(|function| {
            function
                .name(&output)
                .to_opt()
                .is_some_and(|name| name.data(&output) == caller_name)
        })
        .expect("caller");
    let (_, typed) = check_func_body(&output, caller);
    let body = caller.body(&output).expect("caller body");
    let calls = body
        .exprs(&output)
        .iter()
        .filter_map(|(id, expr)| matches!(expr, Partial::Present(Expr::Call(..))).then_some(id))
        .collect::<Vec<_>>();
    assert!(!calls.is_empty(), "caller has no calls");
    for call in calls {
        assert!(
            typed.semantic_expr_lowering(call).is_some(),
            "generated call has no lowering plan: {call:?}, type: {}, body: {body:#?}",
            typed.expr_ty(&output, call).pretty_print(&output),
        );
    }

    let generated = module
        .all_funcs(&output)
        .iter()
        .copied()
        .find(|function| {
            function
                .name(&output)
                .to_opt()
                .is_some_and(|name| name.data(&output) == "answer")
        })
        .expect("generated function");
    let (diagnostics, typed) = check_func_body(&output, generated);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let body = generated.body(&output).expect("generated body");
    for (id, expr) in body.exprs(&output).iter() {
        let ty = typed.expr_ty(&output, id);
        assert!(
            !ty.has_invalid(&output),
            "generated expression is invalid: {id:?} {expr:?}, type: {}",
            ty.pretty_print(&output),
        );
    }
}
