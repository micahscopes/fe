//! Independent execution gate for the fixed-size recursive merge relation.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const MAX_RECURSIVE_CLAIM_BOUND: u32 = 1 << 30;
const EXPECTED_PRODUCTS: u32 = 11 * 30 + 3 * 31;
const EXPECTED_ASSERTIONS: u32 = EXPECTED_PRODUCTS + 9 + 3 * 32 + 12 + 5 * 8 + 3;

fn fixture_url() -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_recursive_merge_air_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "recursive merge AIR fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("recursive merge AIR fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected recursive merge AIR diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("recursive merge AIR fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("recursive merge AIR Wasm should validate");
    bytes
}

fn audit(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    values: [u32; 7],
) -> [u32; 7] {
    let function = instance
        .get_func(&mut *store, "recursive_merge_air_audit")
        .expect("recursive merge audit export");
    let params: Vec<Val> = values
        .into_iter()
        .map(|value| Val::I32(value as i32))
        .collect();
    let mut results = vec![Val::I32(0); 7];
    function
        .call(&mut *store, &params, &mut results)
        .expect("recursive merge audit should execute");
    std::array::from_fn(|index| match results[index] {
        Val::I32(value) => value as u32,
        ref other => panic!("unexpected result lane {index}: {other:?}"),
    })
}

fn stream_audit(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    values: [u32; 7],
) -> [u32; 8] {
    let function = instance
        .get_func(&mut *store, "recursive_merge_stream_audit")
        .expect("recursive merge stream audit export");
    let params: Vec<Val> = values
        .into_iter()
        .map(|value| Val::I32(value as i32))
        .collect();
    let mut results = vec![Val::I32(0); 8];
    function
        .call(&mut *store, &params, &mut results)
        .expect("recursive merge stream audit should execute");
    std::array::from_fn(|index| match results[index] {
        Val::I32(value) => value as u32,
        ref other => panic!("unexpected stream result lane {index}: {other:?}"),
    })
}

#[test]
fn recursive_merge_relation_matches_integer_semantics_and_rejects_mutations() {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, compile_wasm())
        .expect("recursive merge AIR module should load");
    assert_eq!(module.imports().len(), 0, "fixture must remain zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("recursive merge AIR module should instantiate");

    let valid_cases = [
        [0, 1, 2, 1, 1, 7, 0],
        [7, 19, 42, 5, 9, 31, 0],
        [
            MAX_RECURSIVE_CLAIM_BOUND - 3,
            MAX_RECURSIVE_CLAIM_BOUND - 1,
            MAX_RECURSIVE_CLAIM_BOUND,
            17,
            23,
            101,
            0,
        ],
        [0, 1, 2, MAX_RECURSIVE_CLAIM_BOUND - 1, 1, 211, 0],
    ];
    for values in valid_cases {
        assert_eq!(
            audit(&mut store, &instance, values),
            [1, 1, 1, 0, 0, EXPECTED_PRODUCTS, EXPECTED_ASSERTIONS,],
            "valid ordered merge {values:?}",
        );
        assert_eq!(
            stream_audit(&mut store, &instance, values),
            [1, 0, 0, 1, 0, 0, 0, 0],
            "valid streamed merge {values:?}",
        );

        for mutation in 1..=12 {
            let mut mutated = values;
            mutated[6] = mutation;
            let result = audit(&mut store, &instance, mutated);
            assert_eq!(
                result[0],
                if mutation <= 8 { 0 } else { 1 },
                "direct relation mutation {mutation}",
            );
            assert_eq!(result[1], 0, "constraint relation mutation {mutation}");
            assert_eq!(result[2], 1, "mutation preserves fixed relation shape");
            assert!(
                result[3] > 0 || result[4] > 0,
                "mutation {mutation} must produce a nonzero residual",
            );
        }

        for mutation in 1..=13 {
            let mut mutated = values;
            mutated[6] = mutation;
            let result = stream_audit(&mut store, &instance, mutated);
            assert_eq!(result[0], 1, "mutation preserves stream shape");
            assert_eq!(result[3], 1, "mutation preserves replay shape");
            assert!(
                result[1..8].iter().any(|value| *value > 0),
                "streamed mutation {mutation} must reject",
            );
            if mutation == 13 {
                assert_eq!(&result[1..3], &[0, 0]);
                assert!(
                    result[4] > 0,
                    "coherent products must not conceal changed operand copies",
                );
                assert_eq!(result[5], 0, "coherent products remain locally valid");
            }
        }
    }

    for invalid in [
        [0, 1, 2, 0, 1, 13, 0],
        [0, 1, 2, MAX_RECURSIVE_CLAIM_BOUND, 1, 17, 0],
        [3, 3, 4, 1, 1, 19, 0],
        [3, 5, 5, 1, 1, 23, 0],
        [0, 1, MAX_RECURSIVE_CLAIM_BOUND + 1, 1, 1, 29, 0],
    ] {
        let result = audit(&mut store, &instance, invalid);
        assert_eq!(result[0], 0, "invalid integer relation {invalid:?}");
        assert_eq!(result[1], 0, "invalid constrained relation {invalid:?}");
        assert_eq!(result[2], 1, "invalid values retain fixed relation shape");
        assert!(result[3] > 0 || result[4] > 0);
        let streamed = stream_audit(&mut store, &instance, invalid);
        assert_eq!(streamed[0], 1, "invalid values retain stream shape");
        assert_eq!(streamed[3], 1, "invalid values retain replay shape");
        assert!(streamed[1..8].iter().any(|value| *value > 0));
    }

    let modular_wrap_attack = audit(
        &mut store,
        &instance,
        [
            MAX_RECURSIVE_CLAIM_BOUND - 1,
            134_217_726,
            134_217_727,
            1,
            1,
            37,
            12,
        ],
    );
    assert_eq!(
        modular_wrap_attack[0], 0,
        "wrapped order is not an integer order"
    );
    assert_eq!(modular_wrap_attack[1], 0, "final carry rejects field wrap");
    assert_eq!(
        modular_wrap_attack[2], 1,
        "attack retains the exact plan shape"
    );
    assert!(modular_wrap_attack[4] > 0, "wrap must violate an assertion");
    let streamed_wrap = stream_audit(
        &mut store,
        &instance,
        [
            MAX_RECURSIVE_CLAIM_BOUND - 1,
            134_217_726,
            134_217_727,
            1,
            1,
            37,
            12,
        ],
    );
    assert_eq!(streamed_wrap[0], 1);
    assert_eq!(streamed_wrap[3], 1);
    assert!(streamed_wrap[7] > 0, "stream replay rejects field wrap");
}
