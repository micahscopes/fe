use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

#[test]
fn portable_assert_returns_on_success_and_traps_on_failure() {
    for opt in [OptLevel::O0, OptLevel::O2] {
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///wasm_assert_failure.fe").unwrap();
        db.workspace().touch(&mut db, url.clone(), Some(r#"
pub fn checked(_ value: u32) -> u32 {
    assert!(value > 0)
    value
}
pub fn checked_message(_ value: u32) -> u32 {
    assert!(value > 0, "positive value required")
    value
}
"#.to_owned()));
        let file = db.workspace().get(&db, &url).unwrap();
        let top_mod = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
        assert!(diagnostics.is_empty(), "{diagnostics}");
        let bytes = BackendKind::Wasm.create()
            .compile(&db, top_mod, layout_for(BackendKind::Wasm), opt)
            .expect("portable assertion must not allocate an EVM revert payload")
            .into_bytecode().expect("Wasm bytecode");
        wasmparser::validate(&bytes).unwrap();
        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, bytes).unwrap();
        assert!(module.imports().next().is_none());
        let mut store = wasmtime::Store::new(&engine, ());
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        for name in ["checked", "checked_message"] {
            let checked = instance.get_typed_func::<i32, i32>(&mut store, name).unwrap();
            assert_eq!(checked.call(&mut store, 7).unwrap(), 7);
            assert!(checked.call(&mut store, 0).is_err());
        }
    }
}
