use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;

const GATE_MANIFEST: &str = include_str!("fixtures/wasm_transitive_trait_gate/fe.toml");

#[test]
fn dependency_body_resolves_impl_without_redundant_root_dependency() {
    assert!(
        !GATE_MANIFEST.contains("precision"),
        "the root gate must not paper over transitive impl resolution"
    );
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm_transitive_trait_gate");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url), "gate diagnostics");
    let ingot = db.workspace().containing_ingot(&db, url).unwrap();
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let wasm = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("a dependency body must retain its own trait environment")
        .into_bytecode()
        .expect("Wasm bytecode");
    wasmparser::validate(&wasm).unwrap();

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).unwrap();
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let sum = instance
        .get_typed_func::<(u32, u32), u32>(&mut store, "transitive_sum")
        .unwrap();
    assert_eq!(sum.call(&mut store, (13, 29)).unwrap(), 42);
    let field_sum = instance
        .get_typed_func::<(u32, u32), u32>(&mut store, "transitive_field_sum_low_word")
        .unwrap();
    assert_eq!(field_sum.call(&mut store, (13, 29)).unwrap(), 42);
}
