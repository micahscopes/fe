//! Executable gate for the staged production BabyBear receipt codec.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;

fn compile_gate() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "tests/fixtures/mandelbrot_baby_bear_production_receipt_codec_oracle_ingot",
    );
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "production receipt codec fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("production receipt codec fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected production receipt codec diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O2)
        .expect("production receipt codec should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("production receipt codec Wasm should validate");
    bytes
}

#[test]
fn staged_production_receipt_codec_roundtrips_and_rejects_malformed_inputs() {
    let wasm = compile_gate();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).expect("Wasm module should load");
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("production receipt codec Wasm should instantiate");
    let status = instance
        .get_typed_func::<i32, (i32, i32, i32, i32)>(
            &mut store,
            "production_sparse_receipt_codec_status",
        )
        .expect("production receipt codec status export");

    let clean = status.call(&mut store, 0).expect("clean codec roundtrip runs");
    assert_eq!(clean.0, 1, "canonical empty receipt must decode");
    assert_eq!(clean.1, 1, "canonical empty receipt must roundtrip exactly");
    assert!(clean.2 > 0, "canonical receipt must emit a bounded stream");
    assert_eq!(clean.2, clean.3, "derived count must match encoded bytes");

    for mode in 1..=3 {
        let malformed = status
            .call(&mut store, mode)
            .unwrap_or_else(|error| panic!("malformed mode {mode} should run: {error:?}"));
        assert_eq!(malformed.0, 0, "malformed mode {mode} must reject");
        assert_eq!(
            malformed.1, 1,
            "malformed mode {mode} must reset to the canonical empty carrier",
        );
        assert_eq!(malformed.2, clean.2);
        assert_eq!(malformed.3, clean.3);
    }
}
