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
const MODULUS: u32 = 2_013_265_921;
const TWO_ADICITY: u32 = 27;
const SCALE: i64 = 4096;
const ESCAPE_MAGNITUDE: i64 = 1 << 26;

#[derive(Clone)]
struct SignedWord {
    sign: u32,
    magnitude: u32,
}

#[derive(Clone)]
struct AirRow {
    step: u32,
    zr: SignedWord,
    zi: SignedWord,
    rr: u32,
    ii: u32,
    magnitude: u32,
    q_re: SignedWord,
    r_re: u32,
    q_im: SignedWord,
    r_im: u32,
    terminal: u32,
}

#[derive(Clone)]
struct ProofRow {
    active: u32,
    terminal: u32,
    air: AirRow,
}

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

fn base_pow(mut base: u64, mut exponent: u32) -> u32 {
    let modulus = u64::from(MODULUS);
    base %= modulus;
    let mut result = 1u64;
    while exponent != 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus;
        }
        base = base * base % modulus;
        exponent >>= 1;
    }
    result as u32
}

fn base_add(left: u32, right: u32) -> u32 {
    ((u64::from(left) + u64::from(right)) % u64::from(MODULUS)) as u32
}

fn base_mul(left: u32, right: u32) -> u32 {
    (u64::from(left) * u64::from(right) % u64::from(MODULUS)) as u32
}

fn subgroup_root(log_order: u32) -> u32 {
    let maximal_root = base_pow(31, 15);
    base_pow(maximal_root.into(), 1 << (TWO_ADICITY - log_order))
}

fn signed(value: i64) -> SignedWord {
    SignedWord {
        sign: u32::from(value < 0),
        magnitude: value.unsigned_abs() as u32,
    }
}

fn air_row(step: u32, zr: i64, zi: i64) -> AirRow {
    let rr = zr * zr;
    let ii = zi * zi;
    let magnitude = rr + ii;
    let real_numerator = rr - ii;
    let imaginary_numerator = 2 * zr * zi;
    AirRow {
        step,
        zr: signed(zr),
        zi: signed(zi),
        rr: rr as u32,
        ii: ii as u32,
        magnitude: magnitude as u32,
        q_re: signed(real_numerator.div_euclid(SCALE)),
        r_re: real_numerator.rem_euclid(SCALE) as u32,
        q_im: signed(imaginary_numerator.div_euclid(SCALE)),
        r_im: imaginary_numerator.rem_euclid(SCALE) as u32,
        terminal: u32::from(magnitude >= ESCAPE_MAGNITUDE),
    }
}

fn trace4(c_re: i64, c_im: i64, bound: u32) -> Option<[ProofRow; 4]> {
    if !(-8192..4096).contains(&c_re) || !(-6144..6144).contains(&c_im) || bound > 1_048_576 {
        return None;
    }
    let mut zr = 0i64;
    let mut zi = 0i64;
    let mut rows = Vec::new();
    for step in 0..=bound {
        let air = air_row(step, zr, zi);
        let terminal = air.terminal;
        rows.push(ProofRow {
            active: 1,
            terminal,
            air: air.clone(),
        });
        if terminal == 1 {
            break;
        }
        let next_zr = (zr * zr - zi * zi).div_euclid(SCALE) + c_re;
        let next_zi = (2 * zr * zi).div_euclid(SCALE) + c_im;
        zr = next_zr;
        zi = next_zi;
    }
    if rows.last().is_none_or(|row| row.terminal == 0) || rows.len().next_power_of_two() != 4 {
        return None;
    }
    let terminal = rows.last().unwrap().air.clone();
    while rows.len() < 4 {
        rows.push(ProofRow {
            active: 0,
            terminal: 0,
            air: terminal.clone(),
        });
    }
    rows.try_into().ok()
}

fn commitment_words(row: &ProofRow) -> [u32; 17] {
    [
        row.air.step,
        row.air.zr.sign,
        row.air.zr.magnitude,
        row.air.zi.sign,
        row.air.zi.magnitude,
        row.air.rr,
        row.air.ii,
        row.air.magnitude,
        row.air.q_re.sign,
        row.air.q_re.magnitude,
        row.air.r_re,
        row.air.q_im.sign,
        row.air.q_im.magnitude,
        row.air.r_im,
        row.air.terminal,
        row.active,
        row.terminal,
    ]
}

fn append_range_witness(bits: &mut Vec<u32>, value: u32, width: usize) {
    let decomposition = (0..width).map(|bit| (value >> bit) & 1).collect::<Vec<_>>();
    bits.extend(decomposition.iter().copied());
    let mut seen = 0u32;
    bits.extend(decomposition.into_iter().map(|bit| {
        seen |= bit;
        seen
    }));
}

fn auxiliary_bits(row: &ProofRow) -> Vec<u32> {
    let mut bits = Vec::with_capacity(AUXILIARY_FIELDS as usize);
    append_range_witness(&mut bits, row.air.step, 21);
    append_range_witness(&mut bits, row.air.zr.magnitude, 15);
    append_range_witness(&mut bits, row.air.zi.magnitude, 15);
    append_range_witness(&mut bits, row.air.rr, 30);
    append_range_witness(&mut bits, row.air.ii, 30);
    append_range_witness(&mut bits, row.air.magnitude, 31);
    append_range_witness(&mut bits, row.air.q_re.magnitude, 18);
    append_range_witness(&mut bits, row.air.r_re, 12);
    append_range_witness(&mut bits, row.air.q_im.magnitude, 19);
    append_range_witness(&mut bits, row.air.r_im, 12);
    let mut seen = 0u32;
    bits.extend((26..31).map(|bit| {
        seen |= (row.air.magnitude >> bit) & 1;
        seen
    }));
    assert_eq!(bits.len(), AUXILIARY_FIELDS as usize);
    bits
}

/// Direct inverse DFT and polynomial evaluation. This deliberately does not
/// replay Fe's radix-2 butterfly implementation.
fn baby_bear_lde_column(values: [u32; 4], shift: u32) -> [u32; 16] {
    let root4 = subgroup_root(2);
    let inverse_root4 = base_pow(root4.into(), MODULUS - 2);
    let inverse_four = base_pow(4, MODULUS - 2);
    let coefficients: [u32; 4] = core::array::from_fn(|coefficient| {
        let sum = values.iter().enumerate().fold(0, |sum, (sample, value)| {
            let twiddle = base_pow(inverse_root4.into(), (sample * coefficient) as u32);
            base_add(sum, base_mul(*value, twiddle))
        });
        base_mul(sum, inverse_four)
    });
    let root16 = subgroup_root(4);
    core::array::from_fn(|evaluation| {
        let point = base_mul(shift, base_pow(root16.into(), evaluation as u32));
        coefficients.iter().rev().fold(0, |value, coefficient| {
            base_add(base_mul(value, point), *coefficient)
        })
    })
}

fn expected_canonical_lde_roots(c_re: i64, c_im: i64, bound: u32, shift: u32) -> Vec<u32> {
    let rows = trace4(c_re, c_im, bound).expect("canonical four-row escape trace");
    let words: [[u32; 17]; 4] = core::array::from_fn(|row| commitment_words(&rows[row]));
    let auxiliary: [Vec<u32>; 4] = core::array::from_fn(|row| auxiliary_bits(&rows[row]));
    let mut main_rows = vec![vec![0; MAIN_FIELDS as usize]; LDE as usize];
    for column in 0..MAIN_FIELDS as usize {
        let extended = baby_bear_lde_column(core::array::from_fn(|row| words[row][column]), shift);
        for evaluation in 0..LDE as usize {
            main_rows[evaluation][column] = extended[evaluation];
        }
    }
    let mut auxiliary_rows = vec![vec![0; AUXILIARY_FIELDS as usize]; LDE as usize];
    for column in 0..AUXILIARY_FIELDS as usize {
        let extended =
            baby_bear_lde_column(core::array::from_fn(|row| auxiliary[row][column]), shift);
        for evaluation in 0..LDE as usize {
            auxiliary_rows[evaluation][column] = extended[evaluation];
        }
    }
    let main_root = digest_merkle_root(
        main_rows
            .iter()
            .map(|row| reference_field_commitment(b"BL01", row))
            .collect(),
    );
    let auxiliary_root = digest_merkle_root(
        auxiliary_rows
            .iter()
            .map(|row| reference_field_commitment(b"BY01", row))
            .collect(),
    );
    let mut expected = vec![1];
    expected.extend(main_root);
    expected.extend(auxiliary_root);
    expected
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

    assert_eq!(
        call(
            &mut store,
            &instance,
            "canonical_air_lde16_roots",
            &[3072, 0, 16, 7],
            17,
        ),
        expected_canonical_lde_roots(3072, 0, 16, 7),
        "production BabyBear LDE must match an independent direct DFT oracle",
    );
    assert_eq!(
        call(
            &mut store,
            &instance,
            "canonical_air_lde16_roots",
            &[0, 0, 16, 7],
            17,
        ),
        vec![0; 17],
        "a non-escaping claim must not produce a canonical LDE",
    );
    assert_eq!(
        call(
            &mut store,
            &instance,
            "canonical_air_lde16_roots",
            &[3072, 0, 16, 0],
            17,
        ),
        vec![0; 17],
        "the zero coset shift must fail closed",
    );

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
