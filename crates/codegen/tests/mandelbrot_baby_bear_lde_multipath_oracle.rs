//! Independent exactness gate for sparse BabyBear AIR LDE openings.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{layout_for, BackendKind, OptLevel};
use hir::hir_def::HirIngot;
use p3_baby_bear::{default_babybear_poseidon2_16, BabyBear};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_symmetric::Permutation;
use std::collections::BTreeSet;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const WIDTH: usize = 16;
const LDE: u32 = 16;
const MAIN_FIELDS: u32 = 17;
const AUXILIARY_FIELDS: u32 = 411;

fn fixture_url() -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_baby_bear_lde_multipath_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "sparse BabyBear AIR LDE fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("sparse BabyBear AIR LDE fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected sparse BabyBear AIR LDE diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O2)
        .expect("sparse BabyBear AIR LDE fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("sparse BabyBear AIR LDE Wasm should validate");
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

fn reference_permutation(input: [u32; WIDTH]) -> [u32; WIDTH] {
    let mut state = input.map(BabyBear::from_u32);
    default_babybear_poseidon2_16().permute_mut(&mut state);
    state.map(|value| value.as_canonical_u32())
}

fn reference_sponge(message: &[u32]) -> [u32; 8] {
    let mut state = [0u32; WIDTH];
    for block in message.chunks(8) {
        state[..block.len()].copy_from_slice(block);
        state = reference_permutation(state);
    }
    state[..8].try_into().unwrap()
}

fn reference_field_commitment(tag: &[u8; 4], fields: &[u32]) -> [u32; 8] {
    let mut message = vec![u32::from_be_bytes(*tag), fields.len() as u32];
    message.extend_from_slice(fields);
    reference_sponge(&message)
}

fn digest_compress(left: [u32; 8], right: [u32; 8]) -> [u32; 8] {
    let mut state = [0u32; WIDTH];
    state[..8].copy_from_slice(&left);
    state[8..].copy_from_slice(&right);
    reference_permutation(state)[..8].try_into().unwrap()
}

fn digest_merkle_root(mut leaves: Vec<[u32; 8]>) -> [u32; 8] {
    assert!(!leaves.is_empty() && leaves.len().is_power_of_two());
    while leaves.len() > 1 {
        leaves = leaves
            .chunks_exact(2)
            .map(|pair| digest_compress(pair[0], pair[1]))
            .collect();
    }
    leaves[0]
}

fn digest_seed(base: u32) -> [u32; 8] {
    core::array::from_fn(|index| base + index as u32)
}

fn round_tag(prefix: &[u8; 2], round: u32) -> [u8; 4] {
    [
        prefix[0],
        prefix[1],
        b'0' + (round / 10) as u8,
        b'0' + (round % 10) as u8,
    ]
}

fn squeeze_challenge(tag: &[u8; 4], digest: [u32; 8]) -> [u32; 4] {
    let mut message = vec![u32::from_be_bytes(*tag), 8];
    message.extend(digest);
    reference_sponge(&message)[..4].try_into().unwrap()
}

fn query_requests(transcript: u32) -> Vec<u32> {
    (1..=4)
        .flat_map(|query| {
            let sampled =
                squeeze_challenge(&round_tag(b"FQ", query), digest_seed(transcript))[0] & 7;
            [
                sampled,
                (sampled + 4) % LDE,
                sampled + 8,
                (sampled + 12) % LDE,
            ]
        })
        .collect()
}

fn canonical_indices(requests: &[u32]) -> Vec<u32> {
    requests
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn multipath_sibling_count(width: u32, requests: &[u32]) -> usize {
    let mut indices = canonical_indices(requests);
    assert!(!indices.is_empty());
    assert!(indices.iter().all(|&index| index < width));
    let mut siblings = 0;
    let mut level_width = width;
    while level_width > 1 {
        let mut next = Vec::new();
        let mut cursor = 0;
        while cursor < indices.len() {
            let index = indices[cursor];
            let paired =
                index & 1 == 0 && cursor + 1 < indices.len() && indices[cursor + 1] == index + 1;
            if paired {
                cursor += 2;
            } else {
                siblings += 1;
                cursor += 1;
            }
            next.push(index / 2);
        }
        indices = next;
        level_width /= 2;
    }
    siblings
}

fn packed_indices(indices: &[u32], offset: usize) -> u32 {
    (0..8).fold(0, |packed, cursor| {
        let value = indices.get(offset + cursor).copied().unwrap_or(0);
        packed | ((value & 15) << (cursor * 4))
    })
}

fn main_row(seed: u32, evaluation: u32) -> Vec<u32> {
    (0..MAIN_FIELDS)
        .map(|column| (seed + evaluation * 1009 + column * 37) % 1_900_000_007)
        .collect()
}

fn auxiliary_row(seed: u32, evaluation: u32) -> Vec<u32> {
    (0..AUXILIARY_FIELDS)
        .map(|column| (seed + 700_001 + evaluation * 4001 + column * 53) % 1_900_000_007)
        .collect()
}

fn expected_status(seed: u32, transcript: u32) -> Vec<u32> {
    let requests = query_requests(transcript);
    let indices = canonical_indices(&requests);
    let sibling_count = multipath_sibling_count(LDE, &requests) as u32;
    let main_root = digest_merkle_root(
        (0..LDE)
            .map(|evaluation| reference_field_commitment(b"BL01", &main_row(seed, evaluation)))
            .collect(),
    );
    let auxiliary_root = digest_merkle_root(
        (0..LDE)
            .map(|evaluation| reference_field_commitment(b"BY01", &auxiliary_row(seed, evaluation)))
            .collect(),
    );
    let low = packed_indices(&indices, 0);
    let high = packed_indices(&indices, 8);
    let mut expected = vec![
        1,
        1,
        indices.len() as u32,
        sibling_count,
        1,
        1,
        indices.len() as u32,
        sibling_count,
        1,
        low,
        high,
        low,
        high,
    ];
    expected.extend(main_root);
    expected.extend(auxiliary_root);
    expected
}

#[test]
fn sparse_air_lde_openings_match_independent_roots_and_fail_closed() {
    let bytes = compile_wasm();
    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, bytes).expect("sparse BabyBear AIR LDE module should load");
    assert!(
        module.imports().next().is_none(),
        "sparse BabyBear AIR LDE gate must remain zero-import",
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("sparse BabyBear AIR LDE module should instantiate");

    for (seed, transcript) in [(97, 431), (0, 433)] {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "air_lde_multipath16_status",
                &[seed, transcript, 0],
                29,
            ),
            expected_status(seed, transcript),
            "typed query plan must drive exact sparse AIR LDE openings",
        );
    }

    assert_eq!(
        call(
            &mut store,
            &instance,
            "air_fri_receipt16_mutations",
            &[97, 431, (-3072i32) as u32, 1024, 7],
            8,
        ),
        vec![1, 0, 0, 0, 0, 0, 0, 0],
        "canonical sparse AIR and FRI receipt must accept once and bind every boundary",
    );

    for mutation in 1..=6 {
        let actual = call(
            &mut store,
            &instance,
            "air_lde_multipath16_status",
            &[97, 431, mutation],
            29,
        );
        if mutation <= 3 {
            assert_eq!(actual[4], 0, "main opening mutation {mutation} must fail");
            assert_eq!(
                actual[8], 1,
                "main mutation must not corrupt auxiliary verification"
            );
        } else {
            assert_eq!(
                actual[4], 1,
                "auxiliary mutation must not corrupt main verification"
            );
            assert_eq!(
                actual[8], 0,
                "auxiliary opening mutation {mutation} must fail"
            );
        }
    }
}
