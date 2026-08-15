use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

const SOURCE: &str = r#"
pub fn complement_u32(value: u32) -> u32 { ~value }
pub fn complement_i32(value: i32) -> i32 { ~value }
pub fn complement_u8(value: u8) -> u8 { ~value }
"#;

fn compile() -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///wasm_unary_bitnot.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("bitwise-not probe should compile")
        .into_bytecode()
        .expect("Wasm bytecode");
    wasmparser::validate(&bytes).unwrap();
    bytes
}

#[test]
fn integer_bitwise_not_executes_with_logical_width() {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, compile()).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let complement_u32 = instance
        .get_typed_func::<i32, i32>(&mut store, "complement_u32")
        .unwrap();
    let complement_i32 = instance
        .get_typed_func::<i32, i32>(&mut store, "complement_i32")
        .unwrap();
    let complement_u8 = instance
        .get_typed_func::<i32, i32>(&mut store, "complement_u8")
        .unwrap();

    for value in [0i32, 1, 0x0f0f0f0f, -1] {
        assert_eq!(complement_u32.call(&mut store, value).unwrap(), !value);
        assert_eq!(complement_i32.call(&mut store, value).unwrap(), !value);
    }
    for value in [0i32, 1, 0x0f, 0x80, 0xff] {
        assert_eq!(
            complement_u8.call(&mut store, value).unwrap(),
            (!(value as u8)) as i32
        );
    }
}
