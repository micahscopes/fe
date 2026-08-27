//! Independent execution gate for the first recursive child-verifier relation.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{layout_for, BackendKind, OptLevel};
use hir::hir_def::HirIngot;
use p3_baby_bear::{default_babybear_poseidon2_16, BabyBear as P3BabyBear};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_symmetric::Permutation;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const PRODUCTS: u32 = 8;
const ASSERTIONS: u32 = 9;
const MERKLE_PRODUCTS: u32 = 573;
const MERKLE_ASSERTIONS: u32 = 9;
const PATH_DEPTH: u32 = 4;
const PATH_PRODUCTS: u32 = PATH_DEPTH * MERKLE_PRODUCTS;
const PATH_ASSERTIONS: u32 = PATH_DEPTH + 9;
const BABY_BEAR_MODULUS: u32 = 2_013_265_921;

const PATH_LEAF: [u32; 8] = [0, 1, 2, 3, 5, 8, 13, 21];
const PATH_SIBLINGS: [[u32; 8]; 4] = [
    [34, 55, 89, 144, 233, 377, 610, 987],
    [1597, 2584, 4181, 6765, 10946, 17711, 28657, 46368],
    [7, 11, 19, 31, 47, 71, 107, 163],
    [257, 389, 587, 887, 1327, 1999, 3001, 4513],
];

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
    name: &str,
    values: &[u32],
) -> [u32; 6] {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("missing `{name}` export"));
    let params: Vec<Val> = values.iter().map(|value| Val::I32(*value as i32)).collect();
    let mut results = vec![Val::I32(0); 6];
    function
        .call(&mut *store, &params, &mut results)
        .unwrap_or_else(|error| panic!("`{name}` should execute: {error:?}"));
    std::array::from_fn(|index| match results[index] {
        Val::I32(value) => value as u32,
        ref other => panic!("unexpected result lane {index}: {other:?}"),
    })
}

fn header_audit(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    values: [u32; 7],
) -> [u32; 6] {
    audit(store, instance, "receipt_header_relation_audit", &values)
}

fn reference_compress(left: [u32; 8], right: [u32; 8]) -> [u32; 8] {
    let mut state = [P3BabyBear::ZERO; 16];
    for (destination, value) in state.iter_mut().zip(left.into_iter().chain(right)) {
        *destination = P3BabyBear::from_u32(value);
    }
    default_babybear_poseidon2_16().permute_mut(&mut state);
    std::array::from_fn(|index| state[index].as_canonical_u32())
}

fn merkle_arguments(
    child: [u32; 8],
    sibling: [u32; 8],
    direction: u32,
    parent: [u32; 8],
    mutation: u32,
) -> Vec<u32> {
    let mut values = Vec::with_capacity(26);
    values.extend(child);
    values.extend(sibling);
    values.push(direction);
    values.extend(parent);
    values.push(mutation);
    values
}

fn reference_path(index: u32) -> [u32; 8] {
    let mut node = PATH_LEAF;
    for (level, sibling) in PATH_SIBLINGS.into_iter().enumerate() {
        node = if ((index >> level) & 1) == 0 {
            reference_compress(node, sibling)
        } else {
            reference_compress(sibling, node)
        };
    }
    node
}

fn path_arguments(
    path_index: u32,
    direction_source: u32,
    root: [u32; 8],
    mutation: u32,
) -> Vec<u32> {
    let mut values = Vec::with_capacity(11);
    values.extend([path_index, direction_source]);
    values.extend(root);
    values.push(mutation);
    values
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
        header_audit(&mut store, &instance, valid),
        [1, 1, 0, 0, PRODUCTS, ASSERTIONS],
    );

    for input in 0..6 {
        let mut invalid = valid;
        invalid[input] = 0;
        let result = header_audit(&mut store, &instance, invalid);
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
        let result = header_audit(&mut store, &instance, mutated);
        assert_eq!(result[0], 1, "the semantic inputs remain valid");
        assert_eq!(result[1], 1, "product mutation preserves relation shape");
        assert!(result[2] > 0, "product mutation {mutation} must reject");
        assert_eq!(&result[4..], &[PRODUCTS, ASSERTIONS]);
    }
}

#[test]
fn ordered_merkle_node_relation_matches_plonky3_and_rejects_mutations() {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, compile_wasm())
        .expect("recursive verifier AIR module should load");
    assert_eq!(module.imports().len(), 0, "fixture must remain zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("recursive verifier AIR module should instantiate");

    let child = [0, 1, 2, 3, 5, 8, 13, 21];
    let sibling = [34, 55, 89, 144, 233, 377, 610, 987];
    for direction in 0..=1 {
        let parent = if direction == 0 {
            reference_compress(child, sibling)
        } else {
            reference_compress(sibling, child)
        };
        let valid = merkle_arguments(child, sibling, direction, parent, 0);
        assert_eq!(
            audit(&mut store, &instance, "merkle_node_relation_audit", &valid),
            [1, 1, 0, 0, MERKLE_PRODUCTS, MERKLE_ASSERTIONS],
        );

        for lane in 0..8 {
            let mut wrong_parent = parent;
            wrong_parent[lane] = (wrong_parent[lane] + 1) % BABY_BEAR_MODULUS;
            let result = audit(
                &mut store,
                &instance,
                "merkle_node_relation_audit",
                &merkle_arguments(child, sibling, direction, wrong_parent, 0),
            );
            assert_eq!(result[0], 0, "wrong parent lane {lane} must reject");
            assert_eq!(result[1], 1, "wrong parent preserves relation shape");
            assert!(
                result[3] > 0,
                "wrong parent lane {lane} must leave a residual"
            );
        }

        for mutation in 1..=MERKLE_PRODUCTS {
            let result = audit(
                &mut store,
                &instance,
                "merkle_node_relation_audit",
                &merkle_arguments(child, sibling, direction, parent, mutation),
            );
            assert_eq!(result[0], 1, "semantic inputs remain valid");
            assert_eq!(result[1], 1, "product mutation preserves relation shape");
            assert!(result[2] > 0, "product mutation {mutation} must reject");
        }
    }

    let parent = reference_compress(child, sibling);
    let invalid_direction = audit(
        &mut store,
        &instance,
        "merkle_node_relation_audit",
        &merkle_arguments(child, sibling, 2, parent, 0),
    );
    assert_eq!(invalid_direction[0], 0);
    assert_eq!(invalid_direction[1], 1);
    assert!(invalid_direction[2] > 0 || invalid_direction[3] > 0);
}

#[test]
fn binary_merkle_path_relation_binds_index_and_chained_root() {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, compile_wasm())
        .expect("recursive verifier AIR module should load");
    assert_eq!(module.imports().len(), 0, "fixture must remain zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("recursive verifier AIR module should instantiate");

    for index in 0..(1 << PATH_DEPTH) {
        let root = reference_path(index);
        let result = audit(
            &mut store,
            &instance,
            "binary_merkle_path_relation_audit",
            &path_arguments(index, index, root, 0),
        );
        assert_eq!(
            result,
            [1, 1, 0, 0, PATH_PRODUCTS, PATH_ASSERTIONS],
            "path index {index}",
        );
    }

    let root = reference_path(11);
    for lane in 0..8 {
        let mut wrong_root = root;
        wrong_root[lane] = (wrong_root[lane] + 1) % BABY_BEAR_MODULUS;
        let result = audit(
            &mut store,
            &instance,
            "binary_merkle_path_relation_audit",
            &path_arguments(11, 11, wrong_root, 0),
        );
        assert_eq!(result[0], 0, "wrong path root lane {lane} must reject");
        assert_eq!(result[1], 1);
        assert!(result[3] > 0);
    }

    let mismatched_index = audit(
        &mut store,
        &instance,
        "binary_merkle_path_relation_audit",
        &path_arguments(3, 5, reference_path(5), 0),
    );
    assert_eq!(mismatched_index[0], 0);
    assert_eq!(mismatched_index[1], 1);
    assert!(mismatched_index[3] > 0);

    for level in 0..PATH_DEPTH {
        for offset in [1, 2, 10, MERKLE_PRODUCTS] {
            let mutation = level * MERKLE_PRODUCTS + offset;
            let result = audit(
                &mut store,
                &instance,
                "binary_merkle_path_relation_audit",
                &path_arguments(11, 11, root, mutation),
            );
            assert_eq!(result[0], 1);
            assert_eq!(result[1], 1);
            assert!(
                result[2] > 0,
                "path product mutation {mutation} must reject"
            );
        }
    }
}
