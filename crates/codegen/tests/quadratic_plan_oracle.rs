//! Independent execution gate for Fe-authored quadratic arithmetic plans.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const BABY_BEAR_MODULUS: u64 = 2_013_265_921;

fn bb_add(left: u32, right: u32) -> u32 {
    ((left as u64 + right as u64) % BABY_BEAR_MODULUS) as u32
}

fn bb_mul(left: u32, right: u32) -> u32 {
    (left as u64 * right as u64 % BABY_BEAR_MODULUS) as u32
}

fn fixture_url() -> Url {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quadratic_plan_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "quadratic plan fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("quadratic plan fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected quadratic plan diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("quadratic plan fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("quadratic plan Wasm should validate");
    bytes
}

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

#[test]
fn shared_plan_matches_independent_field_math_and_rejects_every_node_mutation() {
    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, compile_wasm()).expect("quadratic plan module should load");
    assert_eq!(module.imports().len(), 0, "fixture must remain zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("quadratic plan module should instantiate");

    for [a, b, c] in [
        [3u32, 5, 7],
        [BABY_BEAR_MODULUS as u32 - 1, 19, 23],
        [0, 29, 31],
        [0x1234_5678, 0x3456_789a, 0x5678_9abc],
    ] {
        let a = (a as u64 % BABY_BEAR_MODULUS) as u32;
        let b = (b as u64 % BABY_BEAR_MODULUS) as u32;
        let c = (c as u64 % BABY_BEAR_MODULUS) as u32;
        let ab = bb_mul(a, b);
        let bc = bb_mul(b, c);
        let output = bb_mul(bb_add(ab, c), bc);
        let clean = call_words(
            &mut store,
            &instance,
            "quadratic_plan_audit",
            &[a, b, c, 0],
            8,
        );
        assert_eq!(clean, [1, 0, 0, 0, ab, bc, output, output]);

        for mutation in 1..=3 {
            let mutated = call_words(
                &mut store,
                &instance,
                "quadratic_plan_audit",
                &[a, b, c, mutation],
                8,
            );
            assert_eq!(mutated[0], 1, "mutation {mutation} executes the full plan");
            assert!(
                mutated[1..4].iter().any(|residual| *residual != 0),
                "committed node mutation {mutation} must violate a quadratic residual",
            );
        }

        for function in [
            "quadratic_plan_under_capacity",
            "quadratic_plan_over_capacity",
        ] {
            assert_eq!(
                call_words(&mut store, &instance, function, &[a, b, c], 1),
                [0],
                "{function} must fail closed",
            );
        }
    }

    for [a, b] in [
        [3u32, 5],
        [BABY_BEAR_MODULUS as u32 - 1, 19],
        [0, 29],
        [0x1234_5678, 0x3456_789a],
    ] {
        let a = (a as u64 % BABY_BEAR_MODULUS) as u32;
        let b = (b as u64 % BABY_BEAR_MODULUS) as u32;
        let c = bb_add(a, b);
        let ab = bb_mul(a, b);
        let bc = bb_mul(b, c);
        let output = bb_mul(bb_add(a, ab), bc);
        let clean = call_words(
            &mut store,
            &instance,
            "quadratic_relation_audit",
            &[a, b, c, output, 0],
            8,
        );
        assert_eq!(clean, [1, 0, 0, 0, 0, 0, output, output]);

        let streamed = call_words(
            &mut store,
            &instance,
            "quadratic_relation_stream_audit",
            &[a, b, c, output, 0],
            8,
        );
        assert_eq!(streamed, [1, 0, 0, 1, 0, 0, 0, 0]);

        let broken_product = call_words(
            &mut store,
            &instance,
            "quadratic_relation_stream_audit",
            &[a, b, c, output, 1],
            8,
        );
        assert_eq!(broken_product[0], 1);
        assert!(broken_product[1] > 0, "local product mutation must reject");
        assert_eq!(broken_product[3], 1);
        assert!(
            broken_product[5] > 0,
            "re-interpretation must reject the changed product",
        );

        let rewired = call_words(
            &mut store,
            &instance,
            "quadratic_relation_stream_audit",
            &[a, b, c, output, 2],
            8,
        );
        assert_eq!(&rewired[..4], &[1, 0, 0, 1]);
        assert!(
            rewired[4] > 0,
            "coherent local products must not conceal changed operand copies",
        );
        assert_eq!(rewired[5], 0, "coherent products remain locally valid");

        let changed_stored_assertion = call_words(
            &mut store,
            &instance,
            "quadratic_relation_stream_audit",
            &[a, b, c, output, 3],
            8,
        );
        assert_eq!(changed_stored_assertion[2], 1);
        assert!(
            changed_stored_assertion[6] > 0,
            "stored assertions must copy the live authored relation",
        );
        assert_eq!(changed_stored_assertion[7], 0);

        let changed_claim = call_words(
            &mut store,
            &instance,
            "quadratic_relation_stream_audit",
            &[a, b, c, output, 4],
            8,
        );
        assert_eq!(&changed_claim[..3], &[1, 0, 0]);
        assert!(
            changed_claim[6] > 0,
            "changed claim must break its stored copy"
        );
        assert!(
            changed_claim[7] > 0,
            "changed claim must violate the relation"
        );

        assert_eq!(
            call_words(
                &mut store,
                &instance,
                "quadratic_relation_stream_wrong_shape",
                &[a, b, c, output],
                1,
            ),
            [0],
            "an incomplete stream must fail closed",
        );

        for mutation in 1..=3 {
            let mutated = call_words(
                &mut store,
                &instance,
                "quadratic_relation_audit",
                &[a, b, c, output, mutation],
                8,
            );
            assert_eq!(mutated[0], 1, "mutation {mutation} executes the relation");
            assert!(
                mutated[1..4].iter().any(|residual| *residual != 0),
                "relation node mutation {mutation} must violate a product residual",
            );
        }

        let wrong_output = call_words(
            &mut store,
            &instance,
            "quadratic_relation_audit",
            &[a, b, c, bb_add(output, 1), 0],
            8,
        );
        assert_ne!(wrong_output[4], 0, "output assertion must reject mutation");

        let changed_c = bb_add(c, 1);
        let changed_bc = bb_mul(b, changed_c);
        let changed_output = bb_mul(bb_add(a, ab), changed_bc);
        let wrong_input = call_words(
            &mut store,
            &instance,
            "quadratic_relation_audit",
            &[a, b, changed_c, changed_output, 0],
            8,
        );
        assert_eq!(&wrong_input[1..5], &[0, 0, 0, 0]);
        assert_ne!(wrong_input[5], 0, "input assertion must reject mutation");

        for function in [
            "quadratic_relation_wrong_product_shape",
            "quadratic_relation_wrong_assertion_shape",
        ] {
            assert_eq!(
                call_words(&mut store, &instance, function, &[a, b, c, output], 1,),
                [0],
                "{function} must fail closed",
            );
        }
    }
}
