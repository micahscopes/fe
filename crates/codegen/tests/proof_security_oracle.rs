//! Independent oracle for Fe-derived recursive proof security policy.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use p3_baby_bear::{BabyBear, default_babybear_poseidon2_16};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_symmetric::Permutation;
use std::path::Path;
use std::sync::OnceLock;
use url::Url;
use wasmtime::Val;

const MODULUS: u32 = 2_013_265_921;
const ONE_Q16: f64 = 65_536.0;
const POSEIDON_WIDTH: usize = 16;

fn fixture_url() -> Url {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/proof_security_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "proof security fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("proof security fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected proof security diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("proof security fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("proof security Wasm should validate");
    bytes
}

fn compiled_wasm() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    WASM.get_or_init(compile_wasm)
}

fn instance() -> (wasmtime::Store<()>, wasmtime::Instance) {
    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, compiled_wasm()).expect("proof security module should load");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("proof security module should instantiate");
    (store, instance)
}

fn call_words(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    result_count: usize,
) -> Vec<u32> {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("missing `{name}` export"));
    let mut results = vec![Val::I32(0); result_count];
    function
        .call(&mut *store, &[], &mut results)
        .unwrap_or_else(|error| panic!("`{name}` should execute: {error:?}"));
    results
        .into_iter()
        .map(|value| match value {
            Val::I32(word) => word as u32,
            other => panic!("`{name}` returned non-u32 lane {other:?}"),
        })
        .collect()
}

fn reference_permutation(input: [u32; POSEIDON_WIDTH]) -> [u32; POSEIDON_WIDTH] {
    let mut state = input.map(BabyBear::from_u32);
    default_babybear_poseidon2_16().permute_mut(&mut state);
    state.map(|value| value.as_canonical_u32())
}

fn reference_sponge(message: &[u32]) -> [u32; 8] {
    let mut state = [0u32; POSEIDON_WIDTH];
    for block in message.chunks(8) {
        state[..block.len()].copy_from_slice(block);
        state = reference_permutation(state);
    }
    state[..8].try_into().unwrap()
}

fn reference_query_index(seed: u32, query: u32) -> u32 {
    let mut message = vec![u32::from_be_bytes(*b"FQ02"), 9];
    message.extend((0..8).map(|offset| seed + offset));
    message.push(query);
    reference_sponge(&message)[0] & 4095
}

fn random_words_bits_per_query() -> f64 {
    let field_bits = 4.0 * f64::from(MODULUS).log2();
    let rho = 0.5;
    let eta = (core::f64::consts::LOG2_E + 1.0) * rho / field_bits;
    -(rho + eta).log2()
}

fn assert_conservative_q16(actual: u32, truth: f64, label: &str) {
    let actual = f64::from(actual) / ONE_Q16;
    assert!(
        actual <= truth + 1e-12,
        "{label} overstates {truth}: {actual}"
    );
    assert!(
        truth - actual < 5.0 / ONE_Q16,
        "{label} is unexpectedly loose: truth={truth}, actual={actual}",
    );
}

#[test]
fn logarithms_bracket_independent_f64_values() {
    let (mut store, instance) = instance();
    let function = instance
        .get_typed_func::<i64, (i64, i64)>(&mut store, "logarithm_bracket")
        .expect("logarithm bracket export");
    for value in [1_u64, 2, 3, 7, 8, 8193, u32::MAX as u64, 1_u64 << 63] {
        let (floor, ceil) = function
            .call(&mut store, value as i64)
            .expect("Fe logarithm should execute");
        let truth = (value as f64).log2() * ONE_Q16;
        assert!((floor as u64) as f64 <= truth + 1e-9);
        assert!((ceil as u64) as f64 + 1e-9 >= truth);
        assert!(truth - ((floor as u64) as f64) < 2.0);
        assert!(((ceil as u64) as f64) - truth < 2.0);
    }
}

#[test]
fn recursive_union_budget_changes_the_derived_query_plan_and_fails_closed() {
    let (mut store, instance) = instance();
    let leaf = call_words(&mut store, &instance, "leaf_policy100", 10);
    let recursive = call_words(&mut store, &instance, "recursive_policy100x1024", 10);
    let over_budget = call_words(&mut store, &instance, "recursive_policy100x2048", 10);

    let per_query = random_words_bits_per_query();
    assert_eq!(leaf[0], 1);
    assert_eq!(leaf[1], (100.0 / per_query).ceil() as u32);
    assert_eq!(leaf[1], 103);
    assert_conservative_q16(
        leaf[2],
        4.0 * f64::from(MODULUS).log2(),
        "extension field bits",
    );
    assert_conservative_q16(leaf[3], per_query, "FRI bits per query");

    assert_eq!(recursive[0], 1);
    assert_eq!(recursive[1], (110.0 / per_query).ceil() as u32);
    assert_eq!(recursive[1], 114);
    assert!(recursive[9] >= 100 * 65_536);
    assert_eq!(
        call_words(&mut store, &instance, "recursive_policy_degree_boundary", 2,),
        vec![4_095, 4_096],
        "the composition degree must fit strictly inside the claimed domain",
    );

    assert_eq!(
        call_words(&mut store, &instance, "recursive_security_query_plan", 6,),
        vec![1, 114, 1, 115, 114, 0],
        "the Fe-derived policy must instantiate the FCO query plan directly",
    );

    assert_eq!(over_budget[0], 0);
    assert_eq!(over_budget[1], (111.0 / per_query).ceil() as u32);
    assert!(over_budget[9] < 100 * 65_536);

    let roundtrip = instance
        .get_typed_func::<i64, i64>(&mut store, "canonical_u64_roundtrip")
        .expect("canonical u64 roundtrip export");
    for value in [1_u64, (1_u64 << 32) | 7, 0x89ab_cdef_0123_4567, u64::MAX] {
        assert_eq!(
            roundtrip
                .call(&mut store, value as i64)
                .expect("canonical u64 roundtrip should execute") as u64,
            value,
        );
    }

    let derived = instance
        .get_typed_func::<i32, i32>(&mut store, "derived_policy_mutation")
        .expect("derived policy mutation export");
    for (case, expected) in [(0, 1), (1, 0), (2, 0), (3, 0), (4, 0)] {
        assert_eq!(
            derived
                .call(&mut store, case)
                .expect("policy recomputation should execute"),
            expected,
            "derived policy mutation case {case}",
        );
    }

    let malformed = instance
        .get_typed_func::<i32, i32>(&mut store, "malformed_policy")
        .expect("malformed policy export");
    for case in 0..6 {
        assert_eq!(
            malformed
                .call(&mut store, case)
                .expect("policy should execute"),
            0,
            "malformed policy case {case} must fail closed",
        );
    }
}

#[test]
fn compact_query_range_matches_typed_plan_and_independent_poseidon() {
    let (mut store, instance) = instance();
    let typed_equivalence = instance
        .get_typed_func::<i32, i32>(&mut store, "compact_checkpoint_query_plan_matches")
        .expect("compact checkpoint equivalence export");
    let security_samples = instance
        .get_typed_func::<i32, (i32, i32)>(&mut store, "recursive_security_query_samples")
        .expect("security query sample export");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("proof security fixture memory");

    for seed in [0_u32, 1, 47, 1_000_003] {
        assert_eq!(
            typed_equivalence
                .call(&mut store, seed as i32)
                .expect("compact checkpoint equivalence should execute"),
            1,
            "compact query interpretation diverged at seed {seed}",
        );
        let (pointer, length) = security_samples
            .call(&mut store, seed as i32)
            .expect("security query sampling should execute");
        assert_eq!(length, 114 * 4);
        let mut bytes = vec![0u8; length as usize];
        memory
            .read(&store, pointer as usize, &mut bytes)
            .expect("security query sample bytes must be readable");
        let actual = bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
            .collect::<Vec<_>>();
        let expected = (1..=114)
            .map(|query| reference_query_index(seed, query))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "indexed Poseidon samples at seed {seed}");
    }
}

#[test]
fn production_profiles_bind_executed_queries_and_air_shape() {
    let (mut store, instance) = instance();
    let checkpoint = call_words(
        &mut store,
        &instance,
        "production_checkpoint_profile_words",
        22,
    );
    let recursive = call_words(
        &mut store,
        &instance,
        "production_recursive_profile_words",
        22,
    );

    for (words, target, proofs, queries) in [(&checkpoint, 3, 1, 4), (&recursive, 100, 1_024, 114)]
    {
        assert_eq!(
            &words[..8],
            &[1, 1, target, proofs, 4_096, 8_192, 4_095, queries]
        );
        assert_eq!(words[8], 1, "the AIR shape interpreter must be valid");
        assert!(words[9..13].iter().all(|count| *count > 0));
        assert_eq!(words[13], words[9..13].iter().sum::<u32>());
        assert_eq!(words[13], 691);
        assert_eq!(&words[14..19], &[2, 2, 1, 1, 2]);
        assert_eq!(
            words[19], queries,
            "the committed profile must bind executed queries"
        );
        assert_eq!(
            &words[20..],
            &[44, 47],
            "canonical profile capacity changed"
        );
    }

    let projection_matches = instance
        .get_typed_func::<(), i32>(
            &mut store,
            "production_recursive_profile_projection_matches",
        )
        .expect("recursive profile projection parity export")
        .call(&mut store, ())
        .expect("recursive profile projection parity should execute");
    assert_eq!(
        projection_matches, 1,
        "u32 placement projection changed SP01"
    );

    let clean = instance
        .get_typed_func::<i32, (i32, i32, i32, i32, i32, i32, i32, i32, i32)>(
            &mut store,
            "production_checkpoint_profile_commitment",
        )
        .expect("production profile commitment export")
        .call(&mut store, 0)
        .expect("clean production profile commitment should execute");
    let clean = [
        clean.0, clean.1, clean.2, clean.3, clean.4, clean.5, clean.6, clean.7, clean.8,
    ]
    .map(|word| word as u32);
    assert_eq!(clean[0], 1);
    assert!(clean[1..].iter().any(|word| *word != 0));

    for mutation in 1..=6 {
        let function = instance
            .get_func(&mut store, "production_checkpoint_profile_commitment")
            .expect("production profile commitment export");
        let mut results = vec![Val::I32(0); 9];
        function
            .call(&mut store, &[Val::I32(mutation)], &mut results)
            .expect("mutated production profile commitment should execute");
        let words = results
            .into_iter()
            .map(|value| match value {
                Val::I32(word) => word as u32,
                other => panic!("profile mutation returned non-u32 lane {other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            words,
            vec![0; 9],
            "profile mutation {mutation} must fail closed"
        );
    }
}
