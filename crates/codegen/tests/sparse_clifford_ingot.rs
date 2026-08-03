use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WasmCompileOptions, compile_runtime_package_wasm_with_options};
use hir::hir_def::HirIngot;
use url::Url;

#[test]
fn real_ingot_dependency_materializes_shared_sparse_plan_and_executes_wasm() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sparse_clifford_consumer_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("consumer ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected sparse-Clifford ingot diagnostics:\n{diagnostics}"
    );

    let package =
        mir::build_wasm_runtime_package_for_entry(&db, top_mod, "sparse_clifford_ingot_probe")
            .expect("cross-ingot sparse plan should build a runtime package");
    let wasm =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .expect("cross-ingot sparse plan should compile to Wasm")
            .bytes;
    wasmparser::validate(&wasm).expect("cross-ingot Wasm validates");

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let probe = instance
        .get_typed_func::<(), i32>(&mut store, "sparse_clifford_ingot_probe")
        .unwrap();
    assert_eq!(probe.call(&mut store, ()).unwrap(), 3140);
}
