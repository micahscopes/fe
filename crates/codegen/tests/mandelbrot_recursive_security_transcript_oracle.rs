//! Independent value and mutation gate for the production security transcript relation.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use num_bigint::BigUint;
use p3_baby_bear::{BabyBear, default_babybear_poseidon2_16};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_symmetric::Permutation;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const MODULUS: u32 = 2_013_265_921;
const WIDTH: usize = 16;
const PERMUTATION_PRODUCTS: u32 = 564;
const ASSERTIONS: u32 = 27;
const DIGEST_FIELDS: u32 = 8;
const SPONGE_RATE: u32 = 8;
const STATEMENT_WORDS: u32 =
    1 + 1 + (1 + DIGEST_FIELDS) + 1 + 1 + (1 + DIGEST_FIELDS) + (1 + DIGEST_FIELDS);
const PROFILE_WORDS: u32 = 44;
const PROFILE_FIELDS: u32 = (PROFILE_WORDS * 32).div_ceil(30);

const fn sponge_permutations(payload_fields: u32) -> u32 {
    (payload_fields + 2).div_ceil(SPONGE_RATE)
}

const BINDING_PERMUTATIONS: u32 = sponge_permutations(DIGEST_FIELDS * 2);
const TRANSCRIPT_PERMUTATIONS: u32 = sponge_permutations(STATEMENT_WORDS)
    + BINDING_PERMUTATIONS
    + BINDING_PERMUTATIONS
    + sponge_permutations(PROFILE_FIELDS)
    + BINDING_PERMUTATIONS;

fn compile_wasm() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_recursive_security_transcript_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "security transcript fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("security transcript fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected security transcript diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("security transcript fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("security transcript Wasm should validate");
    bytes
}

fn instance() -> (wasmtime::Store<()>, wasmtime::Instance) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, compile_wasm())
        .expect("security transcript module should load");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("security transcript module should instantiate");
    (store, instance)
}

fn call_u32s(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    arguments: &[u32],
    result_count: usize,
) -> Vec<u32> {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("missing `{name}` export"));
    let arguments = arguments
        .iter()
        .map(|value| Val::I32(*value as i32))
        .collect::<Vec<_>>();
    let mut results = vec![Val::I32(0); result_count];
    function
        .call(&mut *store, &arguments, &mut results)
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

fn digest(seed: u32) -> [u32; 8] {
    [
        seed + 1,
        seed + 3,
        seed + 5,
        seed + 7,
        seed + 11,
        seed + 13,
        seed + 17,
        seed + 19,
    ]
}

fn security_floor_log2(value: u64) -> u64 {
    assert_ne!(value, 0);
    let integer_part = 63 - value.leading_zeros() as u64;
    let mut result = integer_part << 16;
    let mut mantissa = if integer_part >= 31 {
        value >> (integer_part - 31)
    } else {
        value << (31 - integer_part)
    };
    for bit in 0..16 {
        mantissa = (mantissa * mantissa) >> 31;
        if mantissa >= 1u64 << 32 {
            mantissa >>= 1;
            result |= 1u64 << (15 - bit);
        }
    }
    result
}

fn security_ceil_log2(value: u64) -> u64 {
    let floor = security_floor_log2(value);
    if value.is_power_of_two() {
        floor
    } else {
        floor + 1
    }
}

fn push_u64(words: &mut Vec<u32>, value: u64) {
    words.push(value as u32);
    words.push((value >> 32) as u32);
}

fn reference_profile_words(air_counts: [u32; 4]) -> Vec<u32> {
    let target_bits = 100u32;
    let max_composed_proofs = 1_024u32;
    let extension_degree = 4u32;
    let trace_length = 4_096u32;
    let lde_length = 8_192u32;
    let composition_degree_bound = 4_095u32;
    let log_blowup = 1u32;
    let folding_arity = 2u32;
    let max_air_constraints = 8_192u32;
    let hash_collision_bits = 128u32;
    let query_pow_bits = 0u32;
    let commit_pow_bits = 0u32;
    let field_bits = security_floor_log2(MODULUS as u64) * extension_degree as u64;
    let numerator = field_bits + 94_548 + ((log_blowup as u64) << 16);
    let correction = security_ceil_log2(numerator) - security_floor_log2(field_bits);
    let bits_per_query = ((log_blowup as u64) << 16) - correction;
    let union_bits = security_ceil_log2(max_composed_proofs as u64);
    let local_target = ((target_bits as u64) << 16) + union_bits;
    let query_count = local_target.div_ceil(bits_per_query) as u32;
    assert_eq!(query_count, 114);
    let query_bits = bits_per_query * query_count as u64;
    let air_bits = field_bits - security_ceil_log2(max_air_constraints as u64);
    let commit_factor = (folding_arity as u64 - 1) * (lde_length as u64 + 1);
    let commit_bits = field_bits - security_ceil_log2(commit_factor);
    let local_attained = query_bits
        .min(air_bits)
        .min(commit_bits)
        .min((hash_collision_bits as u64) << 16);
    let global_attained = local_attained - union_bits;

    let mut words = vec![
        1,
        1,
        target_bits,
        max_composed_proofs,
        MODULUS,
        extension_degree,
        trace_length,
        lde_length,
        composition_degree_bound,
        log_blowup,
        folding_arity,
        max_air_constraints,
        hash_collision_bits,
        query_pow_bits,
        commit_pow_bits,
        query_count,
    ];
    for value in [
        field_bits,
        bits_per_query,
        local_target,
        query_bits,
        air_bits,
        commit_bits,
        local_attained,
        global_attained,
    ] {
        push_u64(&mut words, value);
    }
    words.extend([
        1,
        air_counts[0],
        air_counts[1],
        air_counts[2],
        air_counts[3],
        air_counts.into_iter().sum(),
        2,
        2,
        1,
        1,
        2,
        query_count,
    ]);
    assert_eq!(words.len(), 44);
    words
}

fn reference_pack_32(words: &[u32], field_count: usize) -> Vec<u32> {
    let mut packed = BigUint::from(0u32);
    for (index, value) in words.iter().enumerate() {
        packed |= BigUint::from(*value) << (index * 32);
    }
    let mask = (BigUint::from(1u32) << 30usize) - BigUint::from(1u32);
    (0..field_count)
        .map(|index| {
            ((&packed >> (index * 30)) & &mask)
                .to_u32_digits()
                .first()
                .copied()
                .unwrap_or(0)
        })
        .collect()
}

fn reference_transcript(seed: u32, air_counts: [u32; 4]) -> [u32; 8] {
    let start_iteration = seed & 255;
    let mut statement_fields = vec![1, 1, 1];
    statement_fields.extend(digest(seed + 200));
    statement_fields.extend([start_iteration, start_iteration + 1, 1]);
    statement_fields.extend(digest(seed + 300));
    statement_fields.push(1);
    statement_fields.extend(digest(seed + 400));
    assert_eq!(statement_fields.len(), 31);
    let statement = reference_field_commitment(b"AS01", &statement_fields);

    let base_root = digest(seed + 100);
    let interaction_root = digest(seed + 500);
    let mut roots = base_root.to_vec();
    roots.extend(interaction_root);
    let roots = reference_field_commitment(b"AT01", &roots);
    let mut air = statement.to_vec();
    air.extend(roots);
    let air = reference_field_commitment(b"AT02", &air);

    let profile_words = reference_profile_words(air_counts);
    let profile_fields = reference_pack_32(&profile_words, 47);
    let mut profile_message = vec![u32::from_be_bytes(*b"SP01"), 44 * 32];
    profile_message.extend(profile_fields);
    let profile = reference_sponge(&profile_message);

    let mut security = air.to_vec();
    security.extend(profile);
    reference_field_commitment(b"SP02", &security)
}

#[test]
fn production_security_transcript_is_one_exact_mutation_gated_relation() {
    let (mut store, instance) = instance();
    let capacities = call_u32s(
        &mut store,
        &instance,
        "production_security_transcript_relation_capacities",
        &[],
        3,
    );
    assert_eq!(
        capacities,
        [
            TRANSCRIPT_PERMUTATIONS,
            TRANSCRIPT_PERMUTATIONS * PERMUTATION_PRODUCTS,
            ASSERTIONS,
        ],
    );
    assert_eq!(capacities[1], capacities[0] * PERMUTATION_PRODUCTS);
    assert_eq!(capacities[2], ASSERTIONS);

    let counts = call_u32s(
        &mut store,
        &instance,
        "production_security_transcript_air_counts",
        &[],
        4,
    );
    let counts: [u32; 4] = counts.try_into().unwrap();
    assert_eq!(counts, [312, 352, 16, 11]);

    for seed in [0u32, 47, 1_000_003] {
        let clean = call_u32s(
            &mut store,
            &instance,
            "production_security_transcript_relation_audit",
            &[seed, 0],
            14,
        );
        assert_eq!(&clean[..6], &[1, 1, 0, 0, capacities[1], capacities[2]]);
        assert_eq!(&clean[6..], &reference_transcript(seed, counts));

        let changed_statement = call_u32s(
            &mut store,
            &instance,
            "production_security_transcript_relation_audit",
            &[seed, 3],
            14,
        );
        assert_eq!(&changed_statement[..4], &[1, 1, 0, 0]);
        assert_ne!(&changed_statement[6..], &clean[6..]);
    }

    for mutation in [1u32, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16] {
        let rejected = call_u32s(
            &mut store,
            &instance,
            "production_security_transcript_relation_audit",
            &[47, mutation],
            14,
        );
        assert_eq!(rejected[0], 0, "semantic mutation {mutation} accepted");
        assert_eq!(
            rejected[1], 1,
            "semantic mutation {mutation} changed relation shape"
        );
        assert!(
            rejected[3] > 0,
            "semantic mutation {mutation} left no assertion residual"
        );
    }

    for mutation in 100u32..=103 {
        let rejected = call_u32s(
            &mut store,
            &instance,
            "production_security_transcript_relation_audit",
            &[47, mutation],
            14,
        );
        assert_eq!(rejected[0], 1);
        assert_eq!(rejected[1], 1);
        assert!(
            rejected[2] + rejected[3] > 0,
            "stored relation mutation {mutation} escaped replay",
        );
    }
}
