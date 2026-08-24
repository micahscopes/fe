//! Independent execution gate for Fe factor-tree structural analysis.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use std::sync::OnceLock;
use url::Url;
use wasmtime::Val;

fn fixture_url() -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/parallel_structure_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "parallel structure fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("parallel structure fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected parallel structure diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("parallel structure fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("parallel structure Wasm should validate");
    bytes
}

fn compiled_wasm() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    WASM.get_or_init(compile_wasm)
}

fn receipt(name: &str) -> [u32; 9] {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, compiled_wasm())
        .expect("parallel structure module should load");
    assert_eq!(module.imports().len(), 0, "fixture must remain zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("parallel structure module should instantiate");
    let function = instance
        .get_func(&mut store, name)
        .unwrap_or_else(|| panic!("missing `{name}` export"));
    let mut results = [Val::I32(0); 9];
    function
        .call(&mut store, &[], &mut results)
        .unwrap_or_else(|error| panic!("`{name}` should execute: {error:?}"));
    results.map(|value| match value {
        Val::I32(word) => word as u32,
        other => panic!("`{name}` returned non-u32 lane {other:?}"),
    })
}

fn expected(points: u32, factors: u32, tree_depth: u32, nodes: u32) -> [u32; 9] {
    let butterflies = points / 2 * factors;
    [
        points,
        factors,
        butterflies,
        butterflies,
        factors,
        tree_depth,
        nodes,
        2,
        points,
    ]
}

#[test]
fn named_and_irregular_factor_trees_derive_independent_structural_receipts() {
    assert_eq!(receipt("dit4_receipt"), expected(16, 4, 3, 3));
    assert_eq!(receipt("dif4_receipt"), expected(16, 4, 3, 3));
    assert_eq!(receipt("bush2_receipt"), expected(16, 4, 2, 3));
    assert_eq!(receipt("irregular16_receipt"), expected(16, 4, 3, 3));

    assert_ne!(
        receipt("dit4_receipt")[5],
        receipt("bush2_receipt")[5],
        "association must remain visible even when transform size and work agree",
    );
}
