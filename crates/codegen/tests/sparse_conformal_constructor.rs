use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

const SOURCE: &str = include_str!("fixtures/sparse_conformal_constructor.fe");

#[test]
fn direct_and_fco_derived_sparse_constructors_execute_identically() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///sparse_conformal_constructor.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected sparse constructor diagnostics:\n{diagnostics}"
    );

    let wasm = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("sparse conformal constructors should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytecode");
    wasmparser::validate(&wasm).expect("sparse constructor Wasm must validate");

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(
        module.imports().next().is_none(),
        "constructor spike should need no host imports"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let direct = instance
        .get_typed_func::<(), f32>(&mut store, "direct_sparse_conformal_constructor")
        .unwrap();
    let derived = instance
        .get_typed_func::<(), f32>(&mut store, "derived_sparse_conformal_constructor")
        .unwrap();

    let direct_value = direct.call(&mut store, ()).unwrap();
    let derived_value = derived.call(&mut store, ()).unwrap();
    assert_eq!(direct_value, 17.5);
    assert_eq!(derived_value, direct_value);
}
