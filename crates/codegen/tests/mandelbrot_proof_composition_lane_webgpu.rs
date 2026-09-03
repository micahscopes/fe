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
fn production_composition_lane_fits_browser_private_storage() {
    let dir = repo_root()
        .join("crates/codegen/tests/fixtures/mandelbrot_proof_composition_lane_webgpu_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "composition lane fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("composition lane fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "composition lane source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Compute);
    let bundle = fe_codegen::WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::compute(entry, Some("mandelbrot_composition_lane".into())),
    )
    .expect("composition lane should fit the browser shader profile");

    assert_eq!(bundle.manifest.passes.len(), 1);
    assert_eq!(bundle.manifest.resources.len(), 4);
    assert_eq!(bundle.manifest.passes[0].source_entry, "write");
    let shader = &bundle.pass_wgsl[0].source;
    let module = naga::front::wgsl::parse_str(shader)
        .unwrap_or_else(|error| panic!("composition lane WGSL parse failed: {error:?}"));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap_or_else(|error| panic!("composition lane WGSL validation failed: {error:?}"));
}
