#![cfg(feature = "spirv-backend")]

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WebBuildOptions, WebBundle, compile_actor_shader_stage};
use hir::hir_def::HirIngot;
use url::Url;

#[test]
fn isolated_raster_unit_preserves_paired_stages_and_partitioned_bindings() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/actor_raster_stage_partition").canonicalize().unwrap();
    let url = Url::from_directory_path(path).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let top = db.workspace().containing_ingot(&db, url).unwrap().root_mod(&db);
    let bundle = WebBundle::compile(&db, top, WebBuildOptions::render("shade", None)).unwrap();
    assert_eq!(bundle.manifest.resources.len(), 10);
    assert_eq!(bundle.pass_wgsl.len(), 1);
    let raw = compile_actor_shader_stage(&db, top, "shade").unwrap().wgsl.unwrap();
    assert_eq!(raw.lines().map(str::trim_end).collect::<Vec<_>>(),
        bundle.pass_wgsl[0].source.lines().collect::<Vec<_>>());
    assert!(raw.contains("@vertex") && raw.contains("@fragment"));
    assert!(compile_actor_shader_stage(&db, top, "vertices").is_err(),
        "a vertex half must not be emitted separately from its fragment pair");
}

#[test]
fn isolated_actor_stages_match_complete_bundle_shaders() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/actor_nested_cycles")
        .canonicalize()
        .unwrap();
    let url = Url::from_directory_path(path).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let top = db
        .workspace()
        .containing_ingot(&db, url)
        .unwrap()
        .root_mod(&db);
    let bundle = WebBundle::compile(&db, top, WebBuildOptions::compute("finish", None)).unwrap();
    assert_eq!(bundle.manifest.passes.len(), 6);
    for (pass, published) in bundle.manifest.passes.iter().zip(&bundle.pass_wgsl) {
        let artifact = compile_actor_shader_stage(&db, top, &pass.source_entry).unwrap();
        // Bundle publication trims trailing whitespace only. Compare every
        // emitted line, including entry points, bindings and helper bodies.
        let raw = artifact.wgsl.unwrap();
        assert_eq!(
            raw.lines().map(str::trim_end).collect::<Vec<_>>(),
            published.source.lines().collect::<Vec<_>>(),
            "{}",
            pass.source_entry
        );
        assert!(!artifact.words.is_empty());
    }
    let error = compile_actor_shader_stage(&db, top, "missing_stage").err().expect("unknown stage must fail");
    assert!(error.to_string().contains("found 0"));
    // An ordinary helper cannot be selected as though it were an actor stage.
    let error = compile_actor_shader_stage(&db, top, "append").err().expect("ordinary helper must fail");
    assert!(error.to_string().contains("found 0"));
}
