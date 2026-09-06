//! Resource-bearing products must retain field projection through helpers.
#![cfg(feature = "spirv-backend")]
use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WebBuildOptions, WebBundle};
use hir::hir_def::HirIngot;
use url::Url;

#[test]
fn nested_atomic_policy_projects_second_resource_field() {
    let mut db = DriverDataBase::default();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/actor_nested_atomic_policy");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    assert!(!driver::init_ingot(&mut db, &url));
    let top = db
        .workspace()
        .containing_ingot(&db, url)
        .unwrap()
        .root_mod(&db);
    let diagnostics = db.run_on_top_mod(top).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let package = mir::build_wasm_runtime_package_for_entry(&db, top, "probe").unwrap();
    let mut projected_base_count = 0;
    for function in package.functions(&db) {
        let body = function.instance(&db).body(&db);
        for stmt in body.blocks.iter().flat_map(|block| &block.stmts) {
            let mir::RStmt::Assign {
                dst,
                expr: mir::RExpr::AggregateExtract { .. },
            } = stmt
            else {
                continue;
            };
            let Some(layout) = body
                .value_class(*dst)
                .and_then(|class| class.aggregate_layout())
            else {
                continue;
            };
            let mir::Layout::Struct(layout) = layout.data(&db) else {
                continue;
            };
            if layout.source_ty.pretty_print(&db).starts_with("Base<") {
                assert_eq!(
                    body.locals[dst.as_u32() as usize].semantic_ty,
                    layout.source_ty,
                    "intermediate Base must not inherit the final buffer's type"
                );
                projected_base_count += 1;
            }
        }
    }
    assert!(
        projected_base_count > 0,
        "fixture must exercise intermediate product extraction"
    );
    let bundle = WebBundle::compile(&db, top, WebBuildOptions::compute("probe", None))
        .expect("nested resource product projection must lower");
    assert_eq!(bundle.manifest.resources.len(), 2);
    assert_eq!(bundle.manifest.passes.len(), 1);
    assert!(bundle.pass_wgsl[0].source.contains("atomicLoad"));
}
