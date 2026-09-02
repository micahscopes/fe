//! Independent execution gate for FCO-derived portable GPU layouts.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::{Path, PathBuf};
use url::Url;

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/gpu_layout_oracle_ingot")
        .canonicalize()
        .unwrap()
}

fn rejected_fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/gpu_layout_rejected_ingot")
        .canonicalize()
        .unwrap()
}

#[test]
fn reflected_storage_layout_matches_the_independent_wgsl_record_oracle() {
    let url = Url::from_directory_path(fixture_path()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "GPU layout fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("GPU layout fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected GPU layout diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("GPU layout fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("GPU layout Wasm should validate");

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("GPU layout module should load");
    assert_eq!(module.imports().len(), 0, "fixture must remain zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("GPU layout module should instantiate");
    let receipt = instance
        .get_typed_func::<(), (u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32)>(
            &mut store,
            "layout_receipt",
        )
        .expect("derived layout receipt export")
        .call(&mut store, ())
        .expect("derived layout receipt should execute");

    // WGSL storage records of three 32-bit scalar fields have 4-byte alignment,
    // declaration-order offsets 0/4/8, a 12-byte size, and a 12-byte array stride.
    assert_eq!(receipt, (12, 4, 12, 3, 1, 0, 4, 1, 4, 4, 8, 4));
}

#[test]
fn non_host_shareable_fields_fail_closed_during_layout_derivation() {
    let url = Url::from_directory_path(rejected_fixture_path()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "GPU layout rejection fixture should initialize",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("GPU layout rejection fixture ingot");
    let diagnostics = db.run_on_top_mod(ingot.root_mod(&db)).format_diags(&db);
    assert!(
        diagnostics.contains("GpuLayout")
            && (diagnostics.contains("trait bound is not satisfied")
                || diagnostics.contains("failed to derive")
                || diagnostics.contains("doesn't implement")),
        "bool fields must not acquire invented storage-layout evidence:\n{diagnostics}",
    );
}
