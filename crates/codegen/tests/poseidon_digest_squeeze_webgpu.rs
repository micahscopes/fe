use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WebBuildOptions, WebBundleMode, resolve_web_entry};
use hir::hir_def::HirIngot;
use url::Url;

#[test]
fn digest_squeeze_stage_lowers_to_browser_webgpu() {
    let dir = repo_root()
        .join("crates/codegen/tests/fixtures/poseidon_digest_squeeze_webgpu_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "digest squeeze fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("digest squeeze fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "digest squeeze source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    let bundle = fe_codegen::WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("poseidon_digest_squeeze".into())),
    )
    .expect("digest squeeze fixture should compile into a WebBundle");

    assert_eq!(bundle.manifest.passes.len(), 2);
    assert_eq!(bundle.manifest.passes[0].source_entry, "advance");
    let shader = &bundle.pass_wgsl[0].source;
    let module = naga::front::wgsl::parse_str(shader)
        .unwrap_or_else(|error| panic!("digest squeeze WGSL parse failed: {error:?}"));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap_or_else(|error| panic!("digest squeeze WGSL validation failed: {error:?}"));
}
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repository root")
        .to_path_buf()
}
