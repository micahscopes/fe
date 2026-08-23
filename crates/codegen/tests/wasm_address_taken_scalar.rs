//! Exact Wasm regression for scalar Slots materialized by mutable borrows.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;

fn fixture_url() -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wasm_address_taken_scalar_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "address-taken scalar fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("address-taken scalar fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected address-taken scalar diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("address-taken scalar fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("address-taken scalar Wasm should validate");
    bytes
}

#[test]
fn address_taken_scalars_read_the_pointee_in_every_value_lane() {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, compile_wasm()).expect("module should load");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("module should instantiate without imports");

    let plain = instance
        .get_typed_func::<i64, i64>(&mut store, "address_taken_u64")
        .unwrap();
    let arithmetic = instance
        .get_typed_func::<i64, i64>(&mut store, "address_taken_u64_arithmetic")
        .unwrap();
    let call = instance
        .get_typed_func::<i64, i64>(&mut store, "address_taken_u64_call")
        .unwrap();
    let copy = instance
        .get_typed_func::<i64, i64>(&mut store, "address_taken_u64_copy")
        .unwrap();
    for value in [0_u64, 1, 1_u64 << 40, 0x0123_4567_89ab_cdef] {
        assert_eq!(plain.call(&mut store, value as i64).unwrap() as u64, value);
        assert_eq!(call.call(&mut store, value as i64).unwrap() as u64, value);
        assert_eq!(copy.call(&mut store, value as i64).unwrap() as u64, value);
        assert_eq!(
            arithmetic.call(&mut store, value as i64).unwrap() as u64,
            (value ^ 0x9e37_79b9_7f4a_7c15).wrapping_add(11),
        );
    }

    let branch = instance
        .get_typed_func::<i32, i32>(&mut store, "address_taken_bool_branch")
        .unwrap();
    assert_eq!(branch.call(&mut store, 0).unwrap(), 31);
    assert_eq!(branch.call(&mut store, 1).unwrap(), 29);

    let negate = instance
        .get_typed_func::<f32, f32>(&mut store, "address_taken_f32_negate")
        .unwrap();
    assert_eq!(negate.call(&mut store, 13.25).unwrap(), -13.25);
}
