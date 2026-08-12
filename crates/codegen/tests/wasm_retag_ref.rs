//! Wasm parity gate for representation-preserving Fe reference retags.
//!
//! A borrowed nested record can acquire a more specific capability/view while
//! crossing an ordinary helper boundary. MIR records that proof as
//! `RetagRef`; after recursive scalar-value reification it must execute as an
//! identity, not force exemplary Fe code to flatten its records manually.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

fn compile_to_wasm(source: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///wasm_retag_ref.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("representation-preserving retag should lower")
        .into_bytecode()
        .expect("Wasm bytecode")
}

#[test]
fn nested_record_view_retag_executes_as_value_identity() {
    let wasm = compile_to_wasm(
        r#"
pub struct Inner { pub x: f32, pub y: f32 }
impl Copy for Inner {}
pub struct Outer { pub inner: Inner, pub bias: f32 }
impl Copy for Outer {}

fn read(_ value: Outer) -> f32 {
    let inner: Inner = value.inner
    inner.x * 3.0 + inner.y - value.bias
}

pub fn through_helper(_ value: Outer) -> f32 { read(value) }
"#,
    );
    wasmparser::validate(&wasm).unwrap();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let run = instance
        .get_typed_func::<(f32, f32, f32), f32>(&mut store, "through_helper")
        .unwrap();
    assert_eq!(run.call(&mut store, (2.0, 0.5, 1.25)).unwrap(), 5.25);
}
