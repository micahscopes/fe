//! Executable gate for the Fe-derived sparse BabyBear AIR constraint shape.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const AUDITED_CONSTRAINT_CAP: u32 = 8_192;

fn call_words(
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

fn quotient_degree_bound(trace_length: u32, expression_degree: u32, zerofier: u32) -> u32 {
    expression_degree
        .checked_mul(trace_length - 1)
        .expect("production degree arithmetic should remain bounded")
        .saturating_sub(zerofier)
}

fn fixture_url() -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_sparse_air_shape_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "sparse AIR shape fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("sparse AIR shape fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected sparse AIR shape diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("sparse AIR shape fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("sparse AIR shape Wasm should validate");
    bytes
}

#[test]
fn exact_composition_interpreter_derives_the_audited_air_shape() {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, compile_wasm())
        .expect("sparse AIR shape module should load");
    assert_eq!(module.imports().len(), 0, "fixture must remain zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("sparse AIR shape module should instantiate");
    let shape = instance
        .get_typed_func::<(), (i32, i32, i32, i32, i32)>(&mut store, "sparse_air_shape_l4")
        .expect("sparse AIR shape export")
        .call(&mut store, ())
        .expect("Fe shape interpreter should execute");
    let shape = [
        shape.0 as u32,
        shape.1 as u32,
        shape.2 as u32,
        shape.3 as u32,
        shape.4 as u32,
    ];

    assert!(shape[..4].iter().all(|count| *count > 0));
    assert_eq!(shape[4], shape[..4].iter().sum::<u32>());
    assert!(
        shape[4] <= AUDITED_CONSTRAINT_CAP,
        "the authored AIR exceeds the security policy's audited cap: {shape:?}",
    );

    let degrees = call_words(&mut store, &instance, "sparse_air_degree_shape_l4", &[], 9);
    assert_eq!(degrees[0], 1, "the degree interpretation must remain valid");
    assert_eq!(
        &degrees[1..6],
        &[13, 8, 1, 8, 13],
        "the first quadratic plan slice must retain its audited degree reduction",
    );
    let trace_length = 4_096;
    let reference_composition_degree = [
        quotient_degree_bound(trace_length, degrees[1], trace_length),
        quotient_degree_bound(trace_length, degrees[2], trace_length - 1),
        quotient_degree_bound(trace_length, degrees[3], 1),
        quotient_degree_bound(trace_length, degrees[4], 1),
    ]
    .into_iter()
    .max()
    .unwrap();
    assert_eq!(degrees[6], reference_composition_degree);
    assert_eq!(degrees[6], 49_139);
    assert_eq!(degrees[7], shape[4]);
    assert_eq!(degrees[7], 468);
    assert_eq!(
        degrees[8], 0,
        "the current degree-13 AIR must not be represented as fitting degree 4,096",
    );
}
