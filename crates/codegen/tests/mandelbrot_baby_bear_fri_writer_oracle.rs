//! Exactness gate for caller-owned FRI placement.
//!
//! The established value interpreter is independently checked against the
//! Rust/Plonky3 oracle in `mandelbrot_baby_bear_encoding_oracle`. This focused
//! gate requires the caller-owned writer to match that interpreter at every
//! folded lane, committed layer root, terminal value, and transcript lane.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const MODULUS: u32 = 2_013_265_921;

fn fixture_url() -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_baby_bear_fri_writer_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "FRI writer fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("FRI writer fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected FRI writer diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O2)
        .expect("FRI writer fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("FRI writer Wasm should validate");
    bytes
}

fn call(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    arguments: &[u32],
    result_count: usize,
) -> Vec<u32> {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("missing `{name}` export"));
    let params: Vec<Val> = arguments
        .iter()
        .map(|value| Val::I32(*value as i32))
        .collect();
    let mut results = vec![Val::I32(0); result_count];
    function
        .call(&mut *store, &params, &mut results)
        .unwrap_or_else(|error| panic!("`{name}` should execute: {error:?}"));
    results
        .into_iter()
        .map(|value| match value {
            Val::I32(word) => word as u32,
            other => panic!("`{name}` returned non-u32 lane {other:?}"),
        })
        .collect()
}

#[test]
fn caller_owned_fri_writer_matches_the_independently_checked_value_interpreter() {
    let bytes = compile_wasm();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("FRI writer module should load");
    assert!(
        module.imports().next().is_none(),
        "FRI writer gate must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("FRI writer module should instantiate");

    for arguments in [
        [0, 1, 2, 3, 4, 7],
        [97, 17, 23, 41, 73, 7],
        [97, 17, 23, 41, 73, 123_456_789],
        [97, 17, 23, 41, 73, 0],
        [97, 17, 23, 41, 73, MODULUS],
    ] {
        assert_eq!(
            call(&mut store, &instance, "fri_fold8x4_writer", &arguments, 17),
            call(&mut store, &instance, "fri_fold8x4_value", &arguments, 17),
            "caller-owned fold differs for {arguments:?}",
        );
    }

    for (seed, transcript, shift) in [
        (97, 439, 7),
        (0, 401, 123_456_789),
        (97, 439, 0),
        (97, 439, MODULUS),
    ] {
        for component in 0..8 {
            let arguments = [seed, transcript, shift, component];
            assert_eq!(
                call(&mut store, &instance, "fri_chain16_writer", &arguments, 7),
                call(&mut store, &instance, "fri_chain16_value", &arguments, 7),
                "caller-owned FRI chain differs for {arguments:?}",
            );
        }
    }
}
