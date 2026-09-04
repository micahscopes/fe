use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WebBuildOptions, WebBundleMode, resolve_web_entry};
use hir::hir_def::HirIngot;
use url::Url;

const SECURITY_PROFILE_ASSET: &[u8] = include_bytes!(
    "fixtures/mandelbrot_proof_composition_bind_webgpu_ingot/assets/sha256/3ec28891664ccf5c1158abc053b569581000e04fdf7642102b83f84698456906.bin"
);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repository root")
        .to_path_buf()
}

#[test]
fn production_composition_binding_lowers_in_isolation() {
    let dir = repo_root()
        .join("crates/codegen/tests/fixtures/mandelbrot_proof_composition_bind_webgpu_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "composition binding fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("composition binding fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "composition binding source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Compute);
    let bundle = fe_codegen::WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::compute(entry, Some("mandelbrot_composition_binding".into()))
            .with_resource_asset(SECURITY_PROFILE_ASSET.to_vec()),
    )
    .expect("composition binding fixture should compile into a WebBundle");

    assert_eq!(bundle.manifest.passes.len(), 9);
    assert_eq!(bundle.manifest.resources.len(), 2);
    assert_eq!(bundle.manifest.passes[0].source_entry, "stage_roots");
    assert_eq!(bundle.manifest.passes[1].source_entry, "stage_statement");
    assert_eq!(bundle.manifest.passes[2].source_entry, "bind_transcript");
    assert_eq!(
        bundle.manifest.passes[3].source_entry,
        "validate_transcript"
    );
    assert_eq!(bundle.manifest.passes[4].source_entry, "validate_statement");
    assert_eq!(
        bundle.manifest.passes[5].source_entry,
        "bind_security_profile"
    );
    assert_eq!(
        bundle.manifest.passes[6].source_entry,
        "validate_security_profile"
    );
    assert_eq!(bundle.manifest.passes[7].source_entry, "bind_seed");
    assert_eq!(bundle.manifest.passes[8].source_entry, "derive_challenges");
    let mut measured_sizes = Vec::with_capacity(bundle.manifest.passes.len());
    for (pass, shader) in bundle.manifest.passes.iter().zip(&bundle.pass_wgsl) {
        let maximum_wgsl_bytes = match pass.source_entry.as_str() {
            "stage_roots" => 30_000,
            "stage_statement" => 175_000,
            "bind_transcript" => 205_000,
            "validate_transcript" => 225_000,
            "validate_statement" => 225_000,
            "bind_security_profile" => 170_000,
            "validate_security_profile" => 120_000,
            "bind_seed" => 75_000,
            "derive_challenges" => 185_000,
            other => panic!("unexpected composition binding pass `{other}`"),
        };
        eprintln!(
            "composition binding pass `{}` emitted {} WGSL bytes",
            pass.source_entry,
            shader.source.len(),
        );
        let module = naga::front::wgsl::parse_str(&shader.source)
            .unwrap_or_else(|error| panic!("{} WGSL parse failed: {error:?}", pass.source_entry));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap_or_else(|error| panic!("{} WGSL validation failed: {error:?}", pass.source_entry));
        measured_sizes.push((
            pass.source_entry.as_str(),
            shader.source.len(),
            maximum_wgsl_bytes,
        ));
    }
    for (source_entry, actual, maximum) in measured_sizes {
        assert!(
            actual <= maximum,
            "composition binding pass `{source_entry}` emitted {actual} WGSL bytes, exceeding its {maximum}-byte regression budget",
        );
    }
    if let Ok(destination) = std::env::var("MB2_COMPOSITION_BIND_BUNDLE_OUT") {
        bundle
            .write_atomic(destination)
            .expect("the explicitly requested composition bundle should persist atomically");
    }
}
