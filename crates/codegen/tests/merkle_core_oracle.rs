//! Executable oracle for the field-generic Merkle proof shapes.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{layout_for, BackendKind, OptLevel};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;
use wasmtime::Val;

fn fixture_url() -> Url {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/merkle_core_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "generic Merkle fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("generic Merkle fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected generic Merkle diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("generic Merkle fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("generic Merkle Wasm should validate");
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

fn hash2(left: u32, right: u32) -> u32 {
    left * 17 + right * 31 + 7
}

fn binary_root(mut node: u32, depth: usize, mut index: u32, siblings: &[u32]) -> u32 {
    for &sibling in &siblings[..depth] {
        node = if index & 1 == 0 {
            hash2(node, sibling)
        } else {
            hash2(sibling, node)
        };
        index >>= 1;
    }
    node
}

fn full_root(mut leaves: Vec<u32>) -> u32 {
    while leaves.len() > 1 {
        leaves = leaves
            .chunks_exact(2)
            .map(|pair| hash2(pair[0], pair[1]))
            .collect();
    }
    leaves[0]
}

fn reference_multipath(leaves: &[u32], requests: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let mut indices = requests.to_vec();
    indices.sort_unstable();
    indices.dedup();
    assert!(!indices.is_empty());
    assert!(indices.iter().all(|&index| index < leaves.len() as u32));
    let leaf_indices = indices.clone();
    let mut nodes = leaves.to_vec();
    let mut siblings = Vec::new();
    while nodes.len() > 1 {
        let mut next_indices = Vec::new();
        let mut cursor = 0;
        while cursor < indices.len() {
            let index = indices[cursor];
            let paired =
                index & 1 == 0 && cursor + 1 < indices.len() && indices[cursor + 1] == index + 1;
            if paired {
                cursor += 2;
            } else {
                siblings.push(nodes[(index ^ 1) as usize]);
                cursor += 1;
            }
            next_indices.push(index / 2);
        }
        nodes = nodes
            .chunks_exact(2)
            .map(|pair| hash2(pair[0], pair[1]))
            .collect();
        indices = next_indices;
    }
    (leaf_indices, siblings)
}

#[test]
fn generic_merkle_shapes_execute_and_fail_closed() {
    let bytes = compile_wasm();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("Merkle module should load");
    assert!(
        module.imports().next().is_none(),
        "generic Merkle gate must remain zero-import",
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("generic Merkle module should instantiate");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("generic Merkle fixture should export memory");

    let binary_cases = [
        (7, 0, 0, [11, 13, 17]),
        (7, 1, 1, [11, 13, 17]),
        (7, 3, 5, [11, 13, 17]),
    ];
    for (leaf, depth, index, siblings) in binary_cases {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "binary3",
                &[
                    leaf,
                    depth as u32,
                    index,
                    siblings[0],
                    siblings[1],
                    siblings[2]
                ],
                2,
            ),
            vec![1, binary_root(leaf, depth, index, &siblings)],
        );
    }
    assert_eq!(
        call(&mut store, &instance, "binary3", &[7, 3, 8, 11, 13, 17], 2),
        vec![0, 0],
        "out-of-capacity path index must fail closed",
    );
    assert_eq!(
        call(&mut store, &instance, "binary3", &[7, 4, 0, 11, 13, 17], 2),
        vec![0, 0],
        "oversized depth must fail closed",
    );

    let leaves4 = [3, 5, 7, 11];
    assert_eq!(
        call(&mut store, &instance, "frontier4", &leaves4, 3),
        vec![1, 4, full_root(leaves4.to_vec())],
    );
    assert_eq!(
        call(&mut store, &instance, "incomplete_frontier3", &[3, 5, 7], 1),
        vec![0],
        "non-power-of-two frontier must not claim a root",
    );
    assert_eq!(
        call(&mut store, &instance, "overflowing_frontier4", &leaves4, 1),
        vec![0],
        "frontier capacity overflow must fail closed",
    );

    let leaves8 = [3, 5, 7, 11, 13, 17, 19, 23];
    let root8 = full_root(leaves8.to_vec());
    for index in 0..4 {
        let clean = call(&mut store, &instance, "pair8", &[index, 0], 6);
        assert_eq!(clean[0], 1);
        assert_eq!(clean[1], root8);
        assert_eq!(clean[2], 1);
        let mutated = call(&mut store, &instance, "pair8", &[index, 1], 6);
        assert_eq!(mutated[0], 1);
        assert_ne!(mutated[1], root8);
        assert_eq!(mutated[2], 0);
    }
    assert_eq!(
        call(&mut store, &instance, "pair8", &[4, 0], 6)[0],
        0,
        "pair local index must remain inside one half",
    );

    let leaves16 = vec![2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53];
    let root16 = full_root(leaves16);
    for index in 0..4 {
        let clean = call(&mut store, &instance, "quartet16", &[index, 0], 6);
        assert_eq!(clean[0], 1);
        assert_eq!(clean[1], root16);
        assert_eq!(clean[2], 1);
        let mutated = call(&mut store, &instance, "quartet16", &[index, 1], 6);
        assert_eq!(mutated[0], 1);
        assert_ne!(mutated[1], root16);
        assert_eq!(mutated[2], 0);
    }
    assert_eq!(
        call(&mut store, &instance, "quartet16", &[4, 0], 6)[0],
        0,
        "quartet local index must remain inside one quarter",
    );

    let multipath_leaves = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53];
    let multipath_root = full_root(multipath_leaves.to_vec());
    for requests in [
        [0, 1, 2, 3],
        [0, 4, 8, 12],
        [12, 0, 4, 8],
        [3, 3, 3, 3],
        [15, 0, 15, 0],
    ] {
        let (indices, siblings) = reference_multipath(&multipath_leaves, &requests);
        let actual = call(
            &mut store,
            &instance,
            "multipath16",
            &[requests[0], requests[1], requests[2], requests[3], 0],
            17,
        );
        assert_eq!(actual[0], 1, "valid multipath requests must open");
        assert_eq!(actual[1], indices.len() as u32);
        assert_eq!(actual[2], siblings.len() as u32);
        assert_eq!(actual[3], multipath_root);
        assert_eq!(actual[4], 1, "canonical multipath must verify");
        let mut expected_indices = vec![0; 4];
        expected_indices[..indices.len()].copy_from_slice(&indices);
        assert_eq!(&actual[5..9], expected_indices);
        let mut expected_siblings = vec![0; 8];
        expected_siblings[..siblings.len()].copy_from_slice(&siblings);
        assert_eq!(&actual[9..17], expected_siblings);
    }
    assert_eq!(
        call(&mut store, &instance, "multipath16", &[0, 4, 8, 16, 0], 17)[0],
        0,
        "out-of-domain multipath requests must fail closed",
    );
    for mutation in 1..=4 {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "multipath16",
                &[0, 4, 8, 12, mutation],
                17,
            )[4],
            0,
            "multipath mutation {mutation} must be rejected",
        );
    }
    for mutation in 5..=8 {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "multipath16",
                &[3, 3, 3, 3, mutation],
                17,
            )[4],
            0,
            "noncanonical multipath mutation {mutation} must be rejected",
        );
    }

    let encoded = call(&mut store, &instance, "multipath_receipt16_encoded", &[], 2);
    let pointer = encoded[0];
    let length = encoded[1];
    let (indices, siblings) = reference_multipath(&multipath_leaves, &[12, 0, 4, 8]);
    let mut expected = vec![1, 1, 1, 4, siblings.len() as u32, 1];
    expected.extend(
        indices
            .iter()
            .map(|&index| multipath_leaves[index as usize]),
    );
    expected.extend(indices.iter());
    assert_eq!(
        call(
            &mut store,
            &instance,
            "multipath_receipt16_decode_at",
            &[pointer, length],
            14,
        ),
        expected,
        "canonical role-branded receipt must roundtrip and authenticate",
    );

    assert_eq!(
        call(
            &mut store,
            &instance,
            "multipath_receipt16_decode_at",
            &[pointer, length - 4],
            14,
        )[0],
        0,
        "truncated receipt must fail canonical completion",
    );
    assert_eq!(
        call(
            &mut store,
            &instance,
            "multipath_receipt16_decode_at",
            &[pointer, length + 4],
            14,
        )[0],
        0,
        "trailing receipt data must fail canonical completion",
    );

    let first_value_word = 8 + siblings.len();
    let mut word = [0u8; 4];
    memory
        .read(&store, pointer as usize + first_value_word * 4, &mut word)
        .expect("receipt value must be readable");
    let original = u32::from_le_bytes(word);
    memory
        .write(
            &mut store,
            pointer as usize + first_value_word * 4,
            &(original + 1).to_le_bytes(),
        )
        .expect("receipt value mutation must be writable");
    let mutated = call(
        &mut store,
        &instance,
        "multipath_receipt16_decode_at",
        &[pointer, length],
        14,
    );
    assert_eq!(
        mutated[0], 1,
        "field mutation remains canonically decodable"
    );
    assert_eq!(
        mutated[5], 0,
        "authenticated value mutation must be rejected"
    );
}
