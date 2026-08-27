//! Independent execution gate for the first recursive child-verifier relation.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{layout_for, BackendKind, OptLevel};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const PRODUCTS: u32 = 8;
const ASSERTIONS: u32 = 9;

fn compile_wasm() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_recursive_verifier_air_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "recursive verifier AIR fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("recursive verifier AIR fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected recursive verifier AIR diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("recursive verifier AIR fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("recursive verifier AIR Wasm should validate");
    bytes
}

fn audit(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    values: [u32; 7],
) -> [u32; 6] {
    let function = instance
        .get_func(&mut *store, "receipt_header_relation_audit")
        .expect("receipt-header relation audit export");
    let params: Vec<Val> = values
        .into_iter()
        .map(|value| Val::I32(value as i32))
        .collect();
    let mut results = vec![Val::I32(0); 6];
    function
        .call(&mut *store, &params, &mut results)
        .expect("receipt-header relation audit should execute");
    std::array::from_fn(|index| match results[index] {
        Val::I32(value) => value as u32,
        ref other => panic!("unexpected result lane {index}: {other:?}"),
    })
}

#[test]
fn receipt_header_relation_rejects_false_inputs_and_mutated_products() {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, compile_wasm())
        .expect("recursive verifier AIR module should load");
    assert_eq!(module.imports().len(), 0, "fixture must remain zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("recursive verifier AIR module should instantiate");

    let valid = [1, 1, 1, 7, 1, 1, 0];
    assert_eq!(
        audit(&mut store, &instance, valid),
        [1, 1, 0, 0, PRODUCTS, ASSERTIONS],
    );

    for input in 0..6 {
        let mut invalid = valid;
        invalid[input] = 0;
        let result = audit(&mut store, &instance, invalid);
        assert_eq!(result[0], 0, "input mutation {input} must reject");
        assert_eq!(result[1], 1, "input mutation preserves relation shape");
        assert!(
            result[2] > 0 || result[3] > 0,
            "input mutation {input} must leave a nonzero residual",
        );
        assert_eq!(&result[4..], &[PRODUCTS, ASSERTIONS]);
    }

    for mutation in 1..=PRODUCTS {
        let mut mutated = valid;
        mutated[6] = mutation;
        let result = audit(&mut store, &instance, mutated);
        assert_eq!(result[0], 1, "the semantic inputs remain valid");
        assert_eq!(result[1], 1, "product mutation preserves relation shape");
        assert!(result[2] > 0, "product mutation {mutation} must reject");
        assert_eq!(&result[4..], &[PRODUCTS, ASSERTIONS]);
    }
}
