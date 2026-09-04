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
fn one_invocation_workgroup_preserves_the_exact_component_shader() {
    let dir = repo_root().join(
        "crates/codegen/tests/fixtures/mandelbrot_proof_composition_component_single_thread_webgpu_ingot",
    );
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "single-thread composition component fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("single-thread composition component fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "single-thread composition component source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Compute);
    let bundle = fe_codegen::WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::compute(
            entry,
            Some("mandelbrot_composition_component_single_thread".into()),
        ),
    )
    .expect("the single-thread component should compile");

    assert_eq!(bundle.manifest.passes.len(), 1);
    assert_eq!(bundle.manifest.resources.len(), 3);
    let pass = &bundle.manifest.passes[0];
    assert_eq!(pass.source_entry, "control_all_rows");
    assert_eq!(pass.layout.workgroup_size, [1, 1, 1]);
    assert_eq!(pass.dispatch, Some([8192, 1, 1]));
    let shader = &bundle.pass_wgsl[0].source;
    let module = naga::front::wgsl::parse_str(shader)
        .unwrap_or_else(|error| panic!("single-thread component WGSL parse failed: {error:?}"));
    let mut layouter = naga::proc::Layouter::default();
    layouter
        .update(module.to_ctx())
        .expect("single-thread component types should have valid WGSL layouts");
    let private_bytes = module.entry_points[0]
        .function
        .local_variables
        .iter()
        .map(|(_, local)| layouter[local.ty].size as usize)
        .sum::<usize>();
    eprintln!(
        "single-thread composition component: {} WGSL bytes, {} declared function-local bytes",
        shader.len(),
        private_bytes,
    );
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap_or_else(|error| panic!("single-thread component WGSL validation failed: {error:?}"));

    if let Ok(destination) = std::env::var("MB2_COMPOSITION_BUNDLE_OUT") {
        bundle
            .write_atomic(destination)
            .expect("the requested single-thread browser bundle should persist atomically");
    }
}
