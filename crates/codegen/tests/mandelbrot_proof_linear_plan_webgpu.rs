//! Focused lowering gate for the largest sparse base-plan pass.

use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WebBuildOptions, WebBundleMode, resolve_web_entry};
use hir::hir_def::HirIngot;
use url::Url;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repository root")
        .to_path_buf()
}

#[test]
fn production_sparse_linear_plan_lowers_in_isolation() {
    let dir =
        repo_root().join("crates/codegen/tests/fixtures/mandelbrot_proof_linear_plan_webgpu_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "linear-plan fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("linear-plan fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "linear-plan source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    let bundle = fe_codegen::WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("mandelbrot_sparse_linear_plan".into())),
    )
    .expect("the isolated linear-plan pass should lower");

    assert_eq!(bundle.manifest.passes.len(), 2);
    let pass = &bundle.manifest.passes[0];
    assert_eq!(pass.source_entry, "write_linear_plan");
    assert_eq!(pass.layout.workgroup_size, [64, 1, 1]);
    assert_eq!(pass.dispatch, Some([64, 1, 1]));
    let module = naga::front::wgsl::parse_str(&bundle.pass_wgsl[0].source)
        .expect("linear-plan WGSL should parse");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("linear-plan WGSL should validate");
}
