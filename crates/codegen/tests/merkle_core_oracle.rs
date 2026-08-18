//! Executable oracle for the field-generic Merkle proof shapes.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;
use wasmtime::Val;

fn fixture_url() -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/merkle_core_oracle_ingot");
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
                &[leaf, depth as u32, index, siblings[0], siblings[1], siblings[2]],
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
}
