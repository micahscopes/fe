use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WasmCompileOptions, compile_runtime_package_wasm_with_options};
use url::Url;

fn source_module(db: &mut DriverDataBase) -> Url {
    let url = Url::parse("file:///wasm_canonical_arena.fe").unwrap();
    db.workspace().touch(
        db,
        url.clone(),
        Some("pub fn update(value: u32) -> u32 { value + 1 }\n".to_owned()),
    );
    url
}

#[test]
fn canonical_arena_emission_is_explicit_and_typed() {
    let mut db = DriverDataBase::default();
    let url = source_module(&mut db);
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "update").unwrap();
    let ordinary =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .unwrap();
    let canonical = compile_runtime_package_wasm_with_options(
        &db,
        &package,
        WasmCompileOptions::default().with_canonical_arena(),
    )
    .unwrap();

    let engine = wasmtime::Engine::default();
    let ordinary_module = wasmtime::Module::new(&engine, &ordinary.bytes).unwrap();
    let canonical_module = wasmtime::Module::new(&engine, &canonical.bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let ordinary_instance = wasmtime::Instance::new(&mut store, &ordinary_module, &[]).unwrap();
    let ordinary_memory = ordinary_instance.get_memory(&mut store, "memory").unwrap();
    assert!(!ordinary_memory.ty(&store).is_64());
    assert!(
        ordinary_instance
            .get_func(&mut store, "fe_cabi_alloc")
            .is_none()
    );
    assert!(
        ordinary_instance
            .get_func(&mut store, "fe_cabi_reset")
            .is_none()
    );

    let canonical_instance = wasmtime::Instance::new(&mut store, &canonical_module, &[]).unwrap();
    let canonical_memory = canonical_instance
        .get_memory(&mut store, "memory")
        .unwrap();
    assert!(!canonical_memory.ty(&store).is_64());
    canonical_instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .unwrap();
    canonical_instance
        .get_typed_func::<(), ()>(&mut store, "fe_cabi_reset")
        .unwrap();
}
