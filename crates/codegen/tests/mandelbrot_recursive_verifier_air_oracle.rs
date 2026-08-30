//! Independent execution gate for the first recursive child-verifier relation.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use p3_baby_bear::{BabyBear as P3BabyBear, default_babybear_poseidon2_16};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_symmetric::Permutation;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const PRODUCTS: u32 = 8;
const ASSERTIONS: u32 = 10;
const MERKLE_PRODUCTS: u32 = 573;
const MERKLE_ASSERTIONS: u32 = 9;
const PATH_DEPTH: u32 = 4;
const PATH_PRODUCTS: u32 = PATH_DEPTH * MERKLE_PRODUCTS;
const PATH_ASSERTIONS: u32 = PATH_DEPTH + 9;
const BASE_AIR_FIELDS: u32 = 260;
const INTERACTION_AIR_FIELDS: u32 = 152;
const PERMUTATION_PRODUCTS: u32 = 564;
const BASE_LEAF_PERMUTATIONS: u32 = 34;
const INTERACTION_LEAF_PERMUTATIONS: u32 = 21;
const BASE_LEAF_PRODUCTS: u32 = BASE_LEAF_PERMUTATIONS * PERMUTATION_PRODUCTS;
const INTERACTION_LEAF_PRODUCTS: u32 = INTERACTION_LEAF_PERMUTATIONS * PERMUTATION_PRODUCTS;
const LEAF_ASSERTIONS: u32 = 8;
const MULTI_WIDTH: usize = 16;
const MULTI_MAX_LEAVES: usize = 4;
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

fn multipath_summary(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    case: u32,
) -> [u32; 11] {
    let function = instance
        .get_func(&mut *store, "multipath_case_summary")
        .expect("missing `multipath_case_summary` export");
    let mut results = vec![Val::I32(0); 11];
    function
        .call(&mut *store, &[Val::I32(case as i32)], &mut results)
        .expect("`multipath_case_summary` should execute");
    std::array::from_fn(|index| match results[index] {
        Val::I32(value) => value as u32,
        ref other => panic!("unexpected summary lane {index}: {other:?}"),
    })
}

fn production_multipath_capacities(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
) -> [u32; 3] {
    let function = instance
        .get_func(&mut *store, "production_multipath_relation_capacities")
        .expect("missing `production_multipath_relation_capacities` export");
    let mut results = vec![Val::I32(0); 3];
    function
        .call(&mut *store, &[], &mut results)
        .expect("`production_multipath_relation_capacities` should execute");
    std::array::from_fn(|index| match results[index] {
        Val::I32(value) => value as u32,
        ref other => panic!("unexpected capacity lane {index}: {other:?}"),
    })
}

fn production_opening_capacities(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
) -> [u32; 3] {
    let function = instance
        .get_func(&mut *store, "production_opening_relation_capacities")
        .expect("missing `production_opening_relation_capacities` export");
    let mut results = vec![Val::I32(0); 3];
    function
        .call(&mut *store, &[], &mut results)
        .expect("`production_opening_relation_capacities` should execute");
    std::array::from_fn(|index| match results[index] {
        Val::I32(value) => value as u32,
        ref other => panic!("unexpected opening capacity lane {index}: {other:?}"),
    })
}

fn production_opening_arena_summary(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    seed: u32,
    mutation: u32,
) -> [u32; 14] {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("missing `{name}` export"));
    let mut results = vec![Val::I32(0); 14];
    function
        .call(
            &mut *store,
            &[Val::I32(seed as i32), Val::I32(mutation as i32)],
            &mut results,
        )
        .unwrap_or_else(|error| panic!("`{name}` should execute: {error:?}"));
    std::array::from_fn(|index| match results[index] {
        Val::I32(value) => value as u32,
        ref other => panic!("unexpected production opening lane {index}: {other:?}"),
    })
}

fn header_audit(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    values: [u32; 8],
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

fn production_sibling(seed: u32, level: u32) -> [u32; 8] {
    let stride = level * 97;
    [1, 3, 5, 7, 11, 13, 17, 19].map(|offset| seed + stride + offset)
}

fn reference_production_base_opening_root(seed: u32) -> [u32; 8] {
    let mut node = reference_field_commitment(b"LD01", &base_leaf_fields(seed, 0));
    for level in 0..13 {
        node = reference_compress(node, production_sibling(seed + 1000, level));
    }
    node
}

fn reference_production_interaction_opening_root(seed: u32) -> [u32; 8] {
    let base_root = production_sibling(seed + 2000, 13);
    let fields = interaction_leaf_fields(seed, 0, base_root);
    let mut node = reference_field_commitment(b"LD02", &fields);
    for level in 0..13 {
        node = reference_compress(node, production_sibling(seed + 3000, level));
    }
    node
}

fn reference_field_commitment(tag: &[u8; 4], fields: &[u32]) -> [u32; 8] {
    let mut message = vec![u32::from_be_bytes(*tag), fields.len() as u32];
    message.extend_from_slice(fields);
    let mut state = [P3BabyBear::ZERO; 16];
    for block in message.chunks(8) {
        for (destination, value) in state.iter_mut().zip(block) {
            *destination = P3BabyBear::from_u32(*value);
        }
        default_babybear_poseidon2_16().permute_mut(&mut state);
    }
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

fn base_leaf_fields(seed: u32, index: u32) -> Vec<u32> {
    let mut fields = vec![4, 4096, 8192, index];
    fields.extend((0..BASE_AIR_FIELDS).map(|lane| seed + lane * 17 + 1));
    fields
}

fn interaction_leaf_fields(seed: u32, index: u32, base_root: [u32; 8]) -> Vec<u32> {
    let mut fields = vec![4, 4096, 8192, index];
    fields.extend(base_root);
    fields.extend((0..INTERACTION_AIR_FIELDS).map(|lane| seed + lane * 29 + 3));
    fields
}

fn leaf_arguments(
    seed: u32,
    index: u32,
    base_root: Option<[u32; 8]>,
    commitment: [u32; 8],
    mutation: u32,
) -> Vec<u32> {
    let mut values = vec![seed, index];
    if let Some(root) = base_root {
        values.extend(root);
    }
    values.extend(commitment);
    values.push(mutation);
    values
}

fn deterministic_multipath_leaf(index: u32) -> [u32; 8] {
    [
        101 + index * 17,
        211 + index * 19,
        307 + index * 23,
        401 + index * 29,
        503 + index * 31,
        601 + index * 37,
        701 + index * 41,
        809 + index * 43,
    ]
}

fn multipath_requests(case: u32) -> [u32; MULTI_MAX_LEAVES] {
    match case {
        0 => [1, 2, 7, 12],
        1 => [0, 1, 2, 3],
        2 => [3, 3, 10, 10],
        _ => [0, 5, 6, 15],
    }
}

fn reference_multipath_root() -> [u32; 8] {
    let mut nodes: Vec<[u32; 8]> = (0..MULTI_WIDTH as u32)
        .map(deterministic_multipath_leaf)
        .collect();
    while nodes.len() > 1 {
        nodes = nodes
            .chunks_exact(2)
            .map(|pair| reference_compress(pair[0], pair[1]))
            .collect();
    }
    nodes[0]
}

fn reference_multipath_shape(case: u32) -> (u32, u32, u32) {
    let mut indices = multipath_requests(case).to_vec();
    indices.sort_unstable();
    indices.dedup();
    let leaf_count = indices.len() as u32;
    let mut sibling_count = 0u32;
    let mut hashes = 0u32;
    let mut width = MULTI_WIDTH;
    while width > 1 {
        let mut next = Vec::with_capacity(indices.len());
        let mut cursor = 0;
        while cursor < indices.len() {
            let index = indices[cursor];
            let paired =
                index & 1 == 0 && cursor + 1 < indices.len() && indices[cursor + 1] == index + 1;
            if paired {
                cursor += 2;
            } else {
                sibling_count += 1;
                cursor += 1;
            }
            next.push(index / 2);
            hashes += 1;
        }
        indices = next;
        width /= 2;
    }
    (leaf_count, sibling_count, hashes)
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

    let valid = [1, 1, 1, 7, 1, 1, 1, 0];
    assert_eq!(
        header_audit(&mut store, &instance, valid),
        [1, 1, 0, 0, PRODUCTS, ASSERTIONS],
    );

    for input in 0..7 {
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
        mutated[7] = mutation;
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

#[test]
fn deduplicated_multipath_relation_reuses_canonical_topology_and_bounded_rows() {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, compile_wasm())
        .expect("recursive verifier AIR module should load");
    assert_eq!(module.imports().len(), 0, "fixture must remain zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("recursive verifier AIR module should instantiate");

    assert_eq!(
        production_multipath_capacities(&mut store, &instance),
        [5_928, 3_396_744, 5_937],
        "production capacities must derive from 456 leaves and depth 13",
    );

    let expected_root = reference_multipath_root();
    for case in 0..4 {
        let (leaf_count, sibling_count, hashes) = reference_multipath_shape(case);
        let summary = multipath_summary(&mut store, &instance, case);
        assert_eq!(&summary[..8], &expected_root, "case {case} root");
        assert_eq!(summary[8], leaf_count, "case {case} leaf count");
        assert_eq!(summary[9], sibling_count, "case {case} sibling count");
        assert_eq!(summary[10], hashes, "case {case} hash task count");

        let clean = audit(
            &mut store,
            &instance,
            "deduplicated_merkle_multipath_relation_audit",
            &[case, 0],
        );
        assert_eq!(
            clean,
            [
                1,
                1,
                0,
                0,
                hashes * MERKLE_PRODUCTS,
                hashes + MERKLE_ASSERTIONS,
            ],
            "case {case} clean relation",
        );

        for mutation in 1..=7 {
            let result = audit(
                &mut store,
                &instance,
                "deduplicated_merkle_multipath_relation_audit",
                &[case, mutation],
            );
            assert_eq!(result[0], 0, "case {case} input mutation {mutation}");
            assert_eq!(result[1], 1, "case {case} bounded replay shape");
            assert!(
                result[2] > 0 || result[3] > 0,
                "case {case} input mutation {mutation} must leave a residual",
            );
        }

        for hash in 0..hashes {
            for product in [hash * MERKLE_PRODUCTS, (hash + 1) * MERKLE_PRODUCTS - 1] {
                let result = audit(
                    &mut store,
                    &instance,
                    "deduplicated_merkle_multipath_relation_audit",
                    &[case, 100 + product],
                );
                assert_eq!(result[0], 1, "case {case} semantic inputs");
                assert_eq!(result[1], 1, "case {case} bounded replay shape");
                assert!(
                    result[2] > 0,
                    "case {case} product mutation {product} must reject",
                );
            }
        }
    }
}

#[test]
fn production_opening_arenas_preserve_roles_shape_and_canonical_roots() {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, compile_wasm())
        .expect("recursive verifier AIR module should load");
    assert_eq!(module.imports().len(), 0, "fixture must remain zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("recursive verifier AIR module should instantiate");

    assert_eq!(
        production_opening_capacities(&mut store, &instance),
        [12_141_000, 8_797_608, 5_938],
        "leaf plans and depth-13 multipaths must derive production capacities",
    );

    let base_seed = 401;
    let base_root = reference_production_base_opening_root(base_seed);
    let base = production_opening_arena_summary(
        &mut store,
        &instance,
        "production_base_opening_arena_summary",
        base_seed,
        0,
    );
    assert_eq!(&base[..6], &[1, 1, 13, 13, 1, 13]);
    assert_eq!(&base[6..], &base_root, "production base root");
    for mutation in 1..=12 {
        let result = production_opening_arena_summary(
            &mut store,
            &instance,
            "production_base_opening_arena_summary",
            base_seed,
            mutation,
        );
        assert_eq!(
            result[0], 0,
            "production base mutation {mutation} must reject",
        );
    }

    let interaction_seed = 607;
    let interaction_root = reference_production_interaction_opening_root(interaction_seed);
    let interaction = production_opening_arena_summary(
        &mut store,
        &instance,
        "production_interaction_opening_arena_summary",
        interaction_seed,
        0,
    );
    assert_eq!(&interaction[..6], &[1, 1, 13, 13, 1, 13]);
    assert_eq!(
        &interaction[6..],
        &interaction_root,
        "production interaction root",
    );
    for mutation in 1..=14 {
        let result = production_opening_arena_summary(
            &mut store,
            &instance,
            "production_interaction_opening_arena_summary",
            interaction_seed,
            mutation,
        );
        assert_eq!(
            result[0], 0,
            "production interaction mutation {mutation} must reject",
        );
    }
}

#[test]
fn production_lde_leaf_relations_bind_derived_codecs_and_every_sponge_block() {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, compile_wasm())
        .expect("recursive verifier AIR module should load");
    assert_eq!(module.imports().len(), 0, "fixture must remain zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("recursive verifier AIR module should instantiate");

    let base_seed = 101;
    let base_index = 37;
    let base_commitment: [u32; 8] =
        reference_field_commitment(b"LD01", &base_leaf_fields(base_seed, base_index));
    assert_eq!(
        audit(
            &mut store,
            &instance,
            "production_base_lde_leaf_relation_audit",
            &leaf_arguments(base_seed, base_index, None, base_commitment, 0),
        ),
        [1, 1, 0, 0, BASE_LEAF_PRODUCTS, LEAF_ASSERTIONS],
    );

    let interaction_seed = 211;
    let interaction_index = 93;
    let base_root = reference_compress(PATH_LEAF, PATH_SIBLINGS[0]);
    let interaction_commitment: [u32; 8] = reference_field_commitment(
        b"LD02",
        &interaction_leaf_fields(interaction_seed, interaction_index, base_root),
    );
    assert_eq!(
        audit(
            &mut store,
            &instance,
            "production_interaction_lde_leaf_relation_audit",
            &leaf_arguments(
                interaction_seed,
                interaction_index,
                Some(base_root),
                interaction_commitment,
                0,
            ),
        ),
        [1, 1, 0, 0, INTERACTION_LEAF_PRODUCTS, LEAF_ASSERTIONS],
    );

    for lane in 0..8 {
        let mut wrong_base = base_commitment;
        wrong_base[lane] = (wrong_base[lane] + 1) % BABY_BEAR_MODULUS;
        let result = audit(
            &mut store,
            &instance,
            "production_base_lde_leaf_relation_audit",
            &leaf_arguments(base_seed, base_index, None, wrong_base, 0),
        );
        assert_eq!(result[0], 0, "wrong base commitment lane {lane}");
        assert_eq!(result[1], 1);
        assert!(result[3] > 0);

        let mut wrong_interaction = interaction_commitment;
        wrong_interaction[lane] = (wrong_interaction[lane] + 1) % BABY_BEAR_MODULUS;
        let result = audit(
            &mut store,
            &instance,
            "production_interaction_lde_leaf_relation_audit",
            &leaf_arguments(
                interaction_seed,
                interaction_index,
                Some(base_root),
                wrong_interaction,
                0,
            ),
        );
        assert_eq!(result[0], 0, "wrong interaction commitment lane {lane}");
        assert_eq!(result[1], 1);
        assert!(result[3] > 0);
    }

    for (name, values) in [
        (
            "base index",
            leaf_arguments(base_seed, base_index + 1, None, base_commitment, 0),
        ),
        (
            "interaction index",
            leaf_arguments(
                interaction_seed,
                interaction_index + 1,
                Some(base_root),
                interaction_commitment,
                0,
            ),
        ),
    ] {
        let export = if name == "base index" {
            "production_base_lde_leaf_relation_audit"
        } else {
            "production_interaction_lde_leaf_relation_audit"
        };
        let result = audit(&mut store, &instance, export, &values);
        assert_eq!(result[0], 0, "changed {name} must reject");
        assert_eq!(result[1], 1);
        assert!(result[3] > 0);
    }

    for permutation in 0..BASE_LEAF_PERMUTATIONS {
        for offset in [1, PERMUTATION_PRODUCTS] {
            let mutation = permutation * PERMUTATION_PRODUCTS + offset;
            let result = audit(
                &mut store,
                &instance,
                "production_base_lde_leaf_relation_audit",
                &leaf_arguments(base_seed, base_index, None, base_commitment, mutation),
            );
            assert_eq!(result[0], 1);
            assert_eq!(result[1], 1);
            assert!(result[2] > 0, "base product mutation {mutation}");
        }
    }

    for permutation in 0..INTERACTION_LEAF_PERMUTATIONS {
        for offset in [1, PERMUTATION_PRODUCTS] {
            let mutation = permutation * PERMUTATION_PRODUCTS + offset;
            let result = audit(
                &mut store,
                &instance,
                "production_interaction_lde_leaf_relation_audit",
                &leaf_arguments(
                    interaction_seed,
                    interaction_index,
                    Some(base_root),
                    interaction_commitment,
                    mutation,
                ),
            );
            assert_eq!(result[0], 1);
            assert_eq!(result[1], 1);
            assert!(result[2] > 0, "interaction product mutation {mutation}");
        }
    }
}
