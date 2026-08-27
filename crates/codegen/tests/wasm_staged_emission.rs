//! Regression gate for releasing compiler state before Wasm backend emission.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    PreparedWasmEmission, WasmCompileOptions, compile_prepared_runtime_package_wasm,
    compile_runtime_package_wasm_with_options, prepare_runtime_package_wasm_with_options,
};
use hir::hir_def::HirIngot;
use url::Url;

const SOURCE: &str = r#"
pub fn calculate(_ value: u32) -> u32 {
    value * 3 + 7
}
"#;

fn with_package<T>(
    url: &str,
    f: impl for<'db> FnOnce(&'db DriverDataBase, mir::RuntimePackage<'db>) -> T,
) -> T {
    let mut db = DriverDataBase::default();
    let url = Url::parse(url).unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let ingot = db.workspace().containing_ingot(&db, url).unwrap();
    let package =
        mir::build_wasm_runtime_package_for_entry(&db, ingot.root_mod(&db), "calculate").unwrap();
    f(&db, package)
}

fn execute(bytes: &[u8]) -> i32 {
    wasmparser::validate(bytes).unwrap();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).unwrap();
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    instance
        .get_typed_func::<i32, i32>(&mut store, "calculate")
        .unwrap()
        .call(&mut store, 5)
        .unwrap()
}

#[test]
fn staged_wasm_emission_survives_database_drop_and_matches_direct_execution() {
    let direct = with_package("file:///staged_wasm_direct.fe", |db, package| {
        compile_runtime_package_wasm_with_options(db, &package, WasmCompileOptions::default())
            .unwrap()
            .bytes
    });
    let prepared: PreparedWasmEmission =
        with_package("file:///staged_wasm_prepared.fe", |db, package| {
            prepare_runtime_package_wasm_with_options(db, &package, WasmCompileOptions::default())
                .unwrap()
        });
    let staged = compile_prepared_runtime_package_wasm(prepared)
        .unwrap()
        .bytes;

    assert_eq!(execute(&direct), 22);
    assert_eq!(execute(&staged), 22);
    assert_eq!(direct, staged);
}
