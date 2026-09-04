use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WebBuildOptions, WebBundleMode, resolve_web_entry};
use hir::hir_def::HirIngot;
use url::Url;

const BROWSER_SHADER_RISK_BUDGET_BYTES: usize = 1_100_000;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repository root")
        .to_path_buf()
}

#[test]
fn typed_composition_schedule_replaces_the_monolithic_browser_shader() {
    let dir = repo_root()
        .join("crates/codegen/tests/fixtures/mandelbrot_proof_composition_schedule_webgpu_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "composition schedule fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("composition schedule fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "composition schedule source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Compute);
    let bundle = fe_codegen::WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::compute(entry, Some("mandelbrot_composition_schedule".into())),
    )
    .expect("typed composition schedule should fit the browser shader profile");

    let expected = [
        "control_all_rows",
        "control_phase_plan",
        "control_middle_plan",
        "control_tail_plan",
        "control_relation",
        "arithmetic",
        "product",
        "round_plan_first",
        "round_plan_second",
        "round_ports",
        "round_boundary",
        "linear_plan_first",
        "linear_plan_second",
        "linear_plan_third",
        "linear_plan_fourth",
        "linear_ports",
        "linear_boundary",
        "boundary",
        "reduce_composition",
    ];
    assert_eq!(bundle.manifest.passes.len(), expected.len());
    assert_eq!(bundle.manifest.resources.len(), 4);
    let mut oversized = Vec::new();
    for ((pass, shader), expected_entry) in bundle
        .manifest
        .passes
        .iter()
        .zip(&bundle.pass_wgsl)
        .zip(expected)
    {
        assert_eq!(pass.source_entry, expected_entry);
        assert_eq!(pass.layout.workgroup_size, [64, 1, 1]);
        assert_eq!(pass.dispatch, Some([128, 1, 1]));
        assert_eq!(
            pass.cooperation,
            Some(fe_codegen::WebDispatchCooperation { repeat_batch: 1 }),
        );
        let module = naga::front::wgsl::parse_str(&shader.source)
            .unwrap_or_else(|error| panic!("`{expected_entry}` WGSL parse failed: {error:?}"));
        let mut layouter = naga::proc::Layouter::default();
        layouter
            .update(module.to_ctx())
            .unwrap_or_else(|error| panic!("`{expected_entry}` WGSL layouts failed: {error:?}"));
        let private_bytes = module.entry_points[0]
            .function
            .local_variables
            .iter()
            .map(|(_, local)| layouter[local.ty].size as usize)
            .sum::<usize>();
        eprintln!(
            "composition `{expected_entry}`: {} WGSL bytes, {} declared function-local bytes",
            shader.source.len(),
            private_bytes,
        );
        if shader.source.len() >= BROWSER_SHADER_RISK_BUDGET_BYTES {
            oversized.push((expected_entry, shader.source.len()));
        }
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap_or_else(|error| panic!("`{expected_entry}` WGSL validation failed: {error:?}"));
    }
    if let Ok(destination) = std::env::var("MB2_COMPOSITION_BUNDLE_OUT") {
        bundle
            .write_atomic(destination)
            .expect("the explicitly requested browser bundle should persist atomically");
    }
    assert!(
        oversized.is_empty(),
        "oversized composition shaders: {oversized:?}"
    );
}
