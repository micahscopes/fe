//! Independent exactness gate for BabyBear Mandelbrot schema commitments.
//!
//! Fe owns the nominal proof schemas and their field interpreter. This oracle
//! independently reconstructs the bit strings with bigint arithmetic and
//! applies Plonky3's canonical Poseidon2 permutation.

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

const ROW_WIDTHS: [u32; 17] = [21, 1, 15, 1, 15, 30, 30, 31, 1, 18, 12, 1, 19, 12, 1, 1, 1];
const PUBLIC_WIDTHS: [u32; 8] = [1, 14, 1, 13, 21, 21, 21, 22];
const WIDTH: usize = 16;
const MODULUS: u32 = 2_013_265_921;
const TWO_ADICITY: u32 = 27;
const EXT_NONRESIDUE: u32 = 11;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Ext4([u32; 4]);

impl Ext4 {
    fn from_base(value: u32) -> Self {
        Self([value % MODULUS, 0, 0, 0])
    }

    fn add(self, other: Self) -> Self {
        Self(core::array::from_fn(|index| {
            ((u64::from(self.0[index]) + u64::from(other.0[index])) % u64::from(MODULUS)) as u32
        }))
    }

    fn sub(self, other: Self) -> Self {
        Self(core::array::from_fn(|index| {
            ((u64::from(self.0[index]) + u64::from(MODULUS) - u64::from(other.0[index]))
                % u64::from(MODULUS)) as u32
        }))
    }

    /// Independent schoolbook convolution in F[X]/(X^4 - 11).
    fn mul(self, other: Self) -> Self {
        let modulus = u64::from(MODULUS);
        let mut output = [0u64; 4];
        for left in 0..4 {
            for right in 0..4 {
                let mut term = u64::from(self.0[left]) * u64::from(other.0[right]) % modulus;
                let mut degree = left + right;
                if degree >= 4 {
                    degree -= 4;
                    term = term * u64::from(EXT_NONRESIDUE) % modulus;
                }
                output[degree] = (output[degree] + term) % modulus;
            }
        }
        Self(output.map(|word| word as u32))
    }
}

fn fixture_url() -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_baby_bear_encoding_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "BabyBear Mandelbrot encoding fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("BabyBear Mandelbrot encoding fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected BabyBear Mandelbrot encoding diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("BabyBear Mandelbrot encoding fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("BabyBear Mandelbrot encoding Wasm should validate");
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

fn bigint_u32(value: BigUint) -> u32 {
    value.to_u32_digits().first().copied().unwrap_or(0)
}

fn reference_pack(values: &[u32], widths: &[u32], fields: usize) -> Option<Vec<u32>> {
    assert_eq!(values.len(), widths.len());
    let mut packed = BigUint::from(0u32);
    let mut offset = 0u32;
    for (&value, &width) in values.iter().zip(widths) {
        if width > 32 || (width < 32 && value >> width != 0) {
            return None;
        }
        if offset + width > fields as u32 * 30 {
            return None;
        }
        packed |= BigUint::from(value) << offset as usize;
        offset += width;
    }
    let mask = (BigUint::from(1u32) << 30usize) - BigUint::from(1u32);
    let mut result = vec![offset];
    result.extend((0..fields).map(|index| bigint_u32((&packed >> (index * 30)) & &mask)));
    Some(result)
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

fn commitment(tag: &[u8; 4], encoding: &[u32]) -> [u32; 8] {
    let mut message = vec![u32::from_be_bytes(*tag), encoding[0]];
    message.extend_from_slice(&encoding[1..]);
    reference_sponge(&message)
}

fn reference_field_commitment(tag: &[u8; 4], fields: &[u32]) -> [u32; 8] {
    let mut message = vec![u32::from_be_bytes(*tag), fields.len() as u32];
    message.extend_from_slice(fields);
    reference_sponge(&message)
}

fn digest_seed(base: u32) -> [u32; 8] {
    core::array::from_fn(|index| base + index as u32)
}

fn digest_compress(left: [u32; 8], right: [u32; 8]) -> [u32; 8] {
    let mut state = [0u32; WIDTH];
    state[..8].copy_from_slice(&left);
    state[8..].copy_from_slice(&right);
    reference_permutation(state)[..8].try_into().unwrap()
}

fn merkle_root4(leaves: [u32; 4]) -> [u32; 8] {
    digest_compress(
        digest_compress(digest_seed(leaves[0]), digest_seed(leaves[1])),
        digest_compress(digest_seed(leaves[2]), digest_seed(leaves[3])),
    )
}

fn bind_digest(tag: &[u8; 4], left: [u32; 8], right: [u32; 8]) -> [u32; 8] {
    let mut message = vec![u32::from_be_bytes(*tag), 16];
    message.extend(left);
    message.extend(right);
    reference_sponge(&message)
}

fn squeeze_challenge(tag: &[u8; 4], digest: [u32; 8]) -> [u32; 4] {
    let mut message = vec![u32::from_be_bytes(*tag), 8];
    message.extend(digest);
    reference_sponge(&message)[..4].try_into().unwrap()
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

fn fri_pattern<const N: usize>(seed: u32) -> [Ext4; N] {
    core::array::from_fn(|index| {
        let i = index as u32;
        Ext4([
            (seed + i) % MODULUS,
            (seed + 17 + i) % MODULUS,
            (seed + 37 + i + i) % MODULUS,
            (seed + 71 + i + i + i) % MODULUS,
        ])
    })
}

fn reference_fri_fold(values: &[Ext4], beta: Ext4, shift: u32) -> Option<Vec<Ext4>> {
    assert!(values.len() > 1 && values.len().is_power_of_two());
    let shift = shift % MODULUS;
    if shift == 0 {
        return None;
    }
    let inverse_two = base_pow(2, MODULUS - 2);
    let maximal_root = base_pow(31, 15);
    let root = base_pow(
        u64::from(maximal_root),
        1 << (TWO_ADICITY - values.len().ilog2()),
    );
    let mut point = shift;
    Some(
        (0..values.len() / 2)
            .map(|index| {
                let positive = values[index];
                let negative = values[index + values.len() / 2];
                let even = positive.add(negative).mul(Ext4::from_base(inverse_two));
                let point_inverse = base_pow(u64::from(point), MODULUS - 2);
                let odd = positive
                    .sub(negative)
                    .mul(Ext4::from_base(inverse_two))
                    .mul(Ext4::from_base(point_inverse));
                point = (u64::from(point) * u64::from(root) % u64::from(MODULUS)) as u32;
                even.add(beta.mul(odd))
            })
            .collect(),
    )
}

fn flattened_fri_fold(seed: u32, beta: Ext4, shift: u32) -> Vec<u32> {
    let values = fri_pattern::<8>(seed);
    let Some(folded) = reference_fri_fold(&values, beta, shift) else {
        return vec![0; 17];
    };
    let mut words = vec![1];
    words.extend(folded.into_iter().flat_map(|value| value.0));
    words
}

fn round_tag(prefix: &[u8; 2], round: u32) -> [u8; 4] {
    [
        prefix[0],
        prefix[1],
        b'0' + (round / 10) as u8,
        b'0' + (round % 10) as u8,
    ]
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

struct ReferenceFriChain16 {
    roots: [[u32; 8]; 4],
    final_evaluation: Ext4,
    final_transcript: [u32; 8],
}

fn reference_fri_chain16(
    seed: u32,
    transcript_seed: u32,
    shift: u32,
) -> Option<ReferenceFriChain16> {
    reference_fri_chain16_from_digest(seed, digest_seed(transcript_seed), shift)
}

fn reference_fri_chain16_from_digest(
    seed: u32,
    starting_transcript: [u32; 8],
    shift: u32,
) -> Option<ReferenceFriChain16> {
    let mut evaluations = fri_pattern::<16>(seed).to_vec();
    let mut transcript = starting_transcript;
    let mut shift = shift % MODULUS;
    if shift == 0 {
        return None;
    }
    let mut roots = [[0u32; 8]; 4];

    for round in 1..=4 {
        let challenge = Ext4(squeeze_challenge(&round_tag(b"FC", round), transcript));
        evaluations = reference_fri_fold(&evaluations, challenge, shift)?;
        let row_tag = round_tag(b"FR", round);
        let leaves = evaluations
            .iter()
            .map(|value| reference_field_commitment(&row_tag, &value.0))
            .collect();
        roots[(round - 1) as usize] = digest_merkle_root(leaves);
        transcript = bind_digest(
            &round_tag(b"FT", round),
            transcript,
            roots[(round - 1) as usize],
        );
        shift = (u64::from(shift) * u64::from(shift) % u64::from(MODULUS)) as u32;
    }

    Some(ReferenceFriChain16 {
        roots,
        final_evaluation: evaluations[0],
        final_transcript: transcript,
    })
}

fn expected_fri_query16_index(seed: u32, air_transcript_seed: u32, shift: u32) -> Option<u32> {
    let composition_leaves = fri_pattern::<16>(seed)
        .iter()
        .map(|value| reference_field_commitment(b"BC02", &value.0))
        .collect();
    let composition_root = digest_merkle_root(composition_leaves);
    let composition_transcript =
        bind_digest(b"BC03", digest_seed(air_transcript_seed), composition_root);
    let chain = reference_fri_chain16_from_digest(seed, composition_transcript, shift)?;
    Some(squeeze_challenge(b"FQ01", chain.final_transcript)[0] & 7)
}

fn expected_fri_chain16_component(
    seed: u32,
    transcript_seed: u32,
    shift: u32,
    component: usize,
) -> Vec<u32> {
    let Some(chain) = reference_fri_chain16(seed, transcript_seed, shift) else {
        return vec![0; 7];
    };
    vec![
        1,
        chain.roots[0][component],
        chain.roots[1][component],
        chain.roots[2][component],
        chain.roots[3][component],
        if component < 4 {
            chain.final_evaluation.0[component]
        } else {
            0
        },
        chain.final_transcript[component],
    ]
}

fn append_range_witness(bits: &mut Vec<u32>, value: u32, width: u32) {
    let mut seen = 0u32;
    for bit in 0..width {
        bits.push((value >> bit) & 1);
    }
    for bit in 0..width {
        seen |= (value >> bit) & 1;
        bits.push(seen);
    }
}

fn auxiliary_bits(row: &[u32; 17]) -> Vec<u32> {
    let mut bits = Vec::with_capacity(411);
    append_range_witness(&mut bits, row[0], 21);
    append_range_witness(&mut bits, row[2], 15);
    append_range_witness(&mut bits, row[4], 15);
    append_range_witness(&mut bits, row[5], 30);
    append_range_witness(&mut bits, row[6], 30);
    append_range_witness(&mut bits, row[7], 31);
    append_range_witness(&mut bits, row[9], 18);
    append_range_witness(&mut bits, row[10], 12);
    append_range_witness(&mut bits, row[12], 19);
    append_range_witness(&mut bits, row[13], 12);
    let mut seen = 0u32;
    for bit in 26..31 {
        seen |= (row[7] >> bit) & 1;
        bits.push(seen);
    }
    assert_eq!(bits.len(), 411);
    bits
}

fn expected_row(row: &[u32; 17]) -> Option<Vec<u32>> {
    let packed = reference_pack(row, &ROW_WIDTHS, 7)?;
    let mut result = vec![1];
    result.extend_from_slice(&packed);
    result.extend(commitment(b"BR01", &packed));
    Some(result)
}

fn expected_auxiliary(row: &[u32; 17]) -> Option<Vec<u32>> {
    reference_pack(row, &ROW_WIDTHS, 7)?;
    let bits = auxiliary_bits(row);
    let packed = reference_pack(&bits, &vec![1; bits.len()], 14)?;
    let mut result = vec![1];
    result.extend_from_slice(&packed);
    result.extend(commitment(b"BA01", &packed));
    Some(result)
}

#[test]
fn production_mandelbrot_schemas_match_bigint_and_plonky3() {
    let bytes = compile_wasm();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("encoding module should load");
    assert!(
        module.imports().next().is_none(),
        "BabyBear Mandelbrot encoding must remain zero-import",
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("encoding module should instantiate");

    let rows = [
        [0; 17],
        [
            (1 << 21) - 1,
            1,
            (1 << 15) - 1,
            1,
            (1 << 15) - 1,
            (1 << 30) - 1,
            (1 << 30) - 1,
            (1 << 31) - 1,
            1,
            (1 << 18) - 1,
            (1 << 12) - 1,
            1,
            (1 << 19) - 1,
            (1 << 12) - 1,
            1,
            1,
            1,
        ],
        [
            37,
            1,
            2345,
            0,
            6789,
            98_765_432,
            123_456_789,
            222_222_221,
            1,
            19_733,
            2047,
            0,
            123_456,
            3071,
            1,
            1,
            0,
        ],
    ];

    for row in rows {
        assert_eq!(
            call(&mut store, &instance, "encoded_row", &row, 17),
            expected_row(&row).unwrap(),
            "main row encoding differs for {row:?}",
        );
        assert_eq!(
            call(&mut store, &instance, "encoded_auxiliary", &row, 24),
            expected_auxiliary(&row).unwrap(),
            "auxiliary row encoding differs for {row:?}",
        );
    }

    let baseline = rows[2];
    let baseline_main = expected_row(&baseline).unwrap();
    for (word, width) in ROW_WIDTHS.into_iter().enumerate() {
        for bit in 0..width {
            let mut mutated = baseline;
            mutated[word] ^= 1u32 << bit;
            let expected = expected_row(&mutated).unwrap();
            assert_ne!(
                expected, baseline_main,
                "row bit {word}:{bit} was not injective"
            );
            assert_eq!(
                call(&mut store, &instance, "encoded_row", &mutated, 17),
                expected,
                "row bit {word}:{bit} moved in the Fe encoding",
            );
        }
    }

    for (word, width) in [
        (0, 21),
        (2, 15),
        (4, 15),
        (5, 30),
        (6, 30),
        (7, 31),
        (9, 18),
        (10, 12),
        (12, 19),
        (13, 12),
    ] {
        let baseline_auxiliary = expected_auxiliary(&baseline).unwrap();
        for bit in 0..width {
            let mut mutated = baseline;
            mutated[word] ^= 1u32 << bit;
            let expected = expected_auxiliary(&mutated).unwrap();
            assert_ne!(
                expected, baseline_auxiliary,
                "auxiliary bit {word}:{bit} was lost"
            );
            assert_eq!(
                call(&mut store, &instance, "encoded_auxiliary", &mutated, 24),
                expected,
                "auxiliary bit {word}:{bit} moved in the Fe encoding",
            );
        }
    }

    let mut invalid_row = baseline;
    invalid_row[0] = 1 << 21;
    assert_eq!(
        call(&mut store, &instance, "encoded_row", &invalid_row, 17),
        vec![0; 17],
        "out-of-range main row must fail closed",
    );
    assert_eq!(
        call(&mut store, &instance, "encoded_auxiliary", &invalid_row, 24),
        vec![0; 24],
        "out-of-range auxiliary source must fail closed",
    );

    let public_cases = [
        [0; 8],
        [
            1,
            (1 << 14) - 1,
            1,
            (1 << 13) - 1,
            (1 << 21) - 1,
            (1 << 21) - 1,
            (1 << 21) - 1,
            (1 << 22) - 1,
        ],
        [1, 4802, 0, 1211, 4096, 37, 38, 64],
    ];
    for public in public_cases {
        let packed = reference_pack(&public, &PUBLIC_WIDTHS, 4).unwrap();
        let mut expected = vec![1];
        expected.extend_from_slice(&packed);
        expected.extend(commitment(b"BP01", &packed));
        assert_eq!(
            call(&mut store, &instance, "encoded_public", &public, 14),
            expected,
            "public encoding differs for {public:?}",
        );
    }

    let mut invalid_public = public_cases[2];
    invalid_public[1] = 1 << 14;
    assert_eq!(
        call(&mut store, &instance, "encoded_public", &invalid_public, 14),
        vec![0; 14],
        "out-of-range public claim must fail closed",
    );

    let leaves = [17, 23, 41, 1_900_000_007];
    let expected_root = merkle_root4(leaves);
    assert_eq!(
        call(
            &mut store,
            &instance,
            "trace_root4",
            &[leaves[0], leaves[1], leaves[2], leaves[3], 0],
            9,
        ),
        {
            let mut words = vec![1];
            words.extend(expected_root);
            words
        },
        "typed trace tree differs from the Plonky3 Merkle oracle",
    );
    assert_eq!(
        call(
            &mut store,
            &instance,
            "auxiliary_root4",
            &[leaves[0], leaves[1], leaves[2], leaves[3], 0],
            9,
        ),
        {
            let mut words = vec![1];
            words.extend(expected_root);
            words
        },
        "typed auxiliary tree differs from the Plonky3 Merkle oracle",
    );
    for invalid_mask in [1, 2, 4, 8] {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "trace_root4",
                &[leaves[0], leaves[1], leaves[2], leaves[3], invalid_mask],
                9,
            ),
            vec![0; 9],
            "invalid trace leaf {invalid_mask:#x} must fail closed",
        );
        assert_eq!(
            call(
                &mut store,
                &instance,
                "auxiliary_root4",
                &[leaves[0], leaves[1], leaves[2], leaves[3], invalid_mask],
                9,
            ),
            vec![0; 9],
            "invalid auxiliary leaf {invalid_mask:#x} must fail closed",
        );
    }

    for leaf_index in 0..leaves.len() {
        for bit in 0..30 {
            let mut mutated = leaves;
            mutated[leaf_index] ^= 1 << bit;
            let actual = call(
                &mut store,
                &instance,
                "trace_root4",
                &[mutated[0], mutated[1], mutated[2], mutated[3], 0],
                9,
            );
            let mut expected = vec![1];
            expected.extend(merkle_root4(mutated));
            assert_eq!(actual, expected);
            assert_ne!(
                actual[1..],
                expected_root,
                "trace leaf mutation {leaf_index}:{bit} did not change the root",
            );
        }
    }

    let public_seed = 193;
    let trace_seed = 223;
    let auxiliary_seed = 257;
    let statement = bind_digest(b"BS01", digest_seed(public_seed), digest_seed(trace_seed));
    let transcript = bind_digest(b"BT01", statement, digest_seed(auxiliary_seed));
    let mut expected_bound = vec![1];
    expected_bound.extend(statement);
    expected_bound.push(1);
    expected_bound.extend(transcript);
    assert_eq!(
        call(
            &mut store,
            &instance,
            "bind_roots",
            &[public_seed, trace_seed, auxiliary_seed, 0],
            18,
        ),
        expected_bound,
        "typed statement and auxiliary transcript differ from Plonky3",
    );
    for invalid_mask in [1, 2, 7] {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "bind_roots",
                &[public_seed, trace_seed, auxiliary_seed, invalid_mask],
                18,
            ),
            vec![0; 18],
            "invalid typed root mask {invalid_mask:#x} must fail closed",
        );
    }
    assert_eq!(
        call(
            &mut store,
            &instance,
            "bind_roots",
            &[public_seed, trace_seed, auxiliary_seed, 4],
            18,
        ),
        {
            let mut words = vec![1];
            words.extend(statement);
            words.extend([0; 9]);
            words
        },
        "an invalid auxiliary root must preserve the prior statement but reject the transcript",
    );

    let transcript_seed = 311;
    let expected_challenge = squeeze_challenge(b"BC01", digest_seed(transcript_seed));
    let mut expected_words = vec![1];
    expected_words.extend_from_slice(&expected_challenge);
    assert_eq!(
        call(
            &mut store,
            &instance,
            "composition_challenge4",
            &[transcript_seed, 1],
            5,
        ),
        expected_words,
        "quartic composition challenge differs from Plonky3",
    );
    assert_eq!(
        call(
            &mut store,
            &instance,
            "composition_challenge4",
            &[transcript_seed, 0],
            5,
        ),
        vec![0; 5],
        "invalid transcript must not produce an extension challenge",
    );

    for seed in [0, 257, 65_537] {
        let main_leaves = (0..16u32)
            .map(|index| digest_seed(seed + index * 16))
            .collect();
        let auxiliary_leaves = (0..16u32)
            .map(|index| digest_seed(seed + 512 + index * 16))
            .collect();
        let main_root = digest_merkle_root(main_leaves);
        let auxiliary_root = digest_merkle_root(auxiliary_leaves);
        for index in 0..4 {
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "air_lde_quartet16",
                    &[seed, index, 0],
                    6,
                ),
                vec![1, 1, main_root[0], auxiliary_root[0], 1, 1],
                "typed AIR quartet opening differs from the independent tree at seed {seed} index {index}",
            );
            for mutation in 1..=6 {
                let actual = call(
                    &mut store,
                    &instance,
                    "air_lde_quartet16",
                    &[seed, index, mutation],
                    6,
                );
                let expected_main = u32::from(mutation == 3);
                let expected_auxiliary = u32::from(mutation != 3);
                assert_eq!(
                    actual[4], expected_main,
                    "main AIR quartet mutation {mutation} had the wrong result",
                );
                assert_eq!(
                    actual[5], expected_auxiliary,
                    "auxiliary AIR quartet mutation {mutation} had the wrong result",
                );
            }
        }
        assert_eq!(
            call(&mut store, &instance, "air_lde_quartet16", &[seed, 4, 0], 6,),
            vec![0, 0, main_root[0], auxiliary_root[0], 0, 0],
            "out-of-quarter AIR opening indices must fail closed",
        );
    }

    let main_lde = [
        0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610, 987, 1597,
    ];
    let baseline_main_lde = reference_field_commitment(b"BL01", &main_lde);
    assert_eq!(
        call(&mut store, &instance, "main_lde17", &main_lde, 8),
        baseline_main_lde,
        "production main LDE row commitment differs from Plonky3",
    );
    for index in 0..main_lde.len() {
        let mut mutated = main_lde;
        mutated[index] += 1;
        let actual = call(&mut store, &instance, "main_lde17", &mutated, 8);
        assert_eq!(actual, reference_field_commitment(b"BL01", &mutated));
        assert_ne!(
            actual, baseline_main_lde,
            "main LDE field {index} was not bound"
        );
    }

    for seed in [0, 1, u32::MAX, 0x1357_9bdf] {
        let auxiliary: Vec<u32> = (0..411u32)
            .map(|index| ((seed >> (index & 31)) ^ index ^ (index >> 3)) & 0x3fff_ffff)
            .collect();
        assert_eq!(
            call(&mut store, &instance, "auxiliary_lde411", &[seed], 8),
            reference_field_commitment(b"BY01", &auxiliary),
            "production 411-field auxiliary LDE row differs for seed {seed:#x}",
        );
    }

    let composition = [17, 23, 41, 73];
    let baseline_composition = reference_field_commitment(b"BC02", &composition);
    assert_eq!(
        call(&mut store, &instance, "composition_row4", &composition, 8),
        baseline_composition,
        "production quartic composition row differs from Plonky3",
    );
    for index in 0..composition.len() {
        let mut mutated = composition;
        mutated[index] += 1;
        let actual = call(&mut store, &instance, "composition_row4", &mutated, 8);
        assert_eq!(actual, reference_field_commitment(b"BC02", &mutated));
        assert_ne!(
            actual, baseline_composition,
            "composition coefficient {index} was not bound",
        );
    }

    let proof_seed = 401;
    let main_seed = 409;
    let auxiliary_seed = 419;
    let main_transcript = bind_digest(b"BL02", digest_seed(proof_seed), digest_seed(main_seed));
    let air_transcript = bind_digest(b"BY02", main_transcript, digest_seed(auxiliary_seed));
    assert_eq!(
        call(
            &mut store,
            &instance,
            "air_transcript",
            &[proof_seed, main_seed, auxiliary_seed],
            8,
        ),
        air_transcript,
        "main and auxiliary LDE transcript order differs from Plonky3",
    );

    let air_seed = 431;
    let composition_seed = 433;
    assert_eq!(
        call(
            &mut store,
            &instance,
            "composition_transcript",
            &[air_seed, composition_seed],
            8,
        ),
        bind_digest(
            b"BC03",
            digest_seed(air_seed),
            digest_seed(composition_seed)
        ),
        "composition transcript binding differs from Plonky3",
    );

    let fri_transcript_seed = 439;
    assert_eq!(
        call(
            &mut store,
            &instance,
            "fri_challenge1",
            &[fri_transcript_seed],
            4,
        ),
        squeeze_challenge(b"FC01", digest_seed(fri_transcript_seed)),
        "FRI round-one challenge differs from Plonky3",
    );
    assert_eq!(
        call(
            &mut store,
            &instance,
            "fri_challenge2",
            &[fri_transcript_seed],
            4,
        ),
        squeeze_challenge(b"FC02", digest_seed(fri_transcript_seed)),
        "FRI round-two challenge differs from Plonky3",
    );

    let fri_row = [443, 449, 457, 461];
    assert_eq!(
        call(&mut store, &instance, "fri_row1", &fri_row, 8),
        reference_field_commitment(b"FR01", &fri_row),
        "FRI quartic row commitment differs from Plonky3",
    );

    for seed in [0, 97] {
        for beta in [Ext4([1, 2, 3, 4]), Ext4([17, 23, 41, 73])] {
            for shift in [7, 123_456_789] {
                let mut arguments = vec![seed];
                arguments.extend(beta.0);
                arguments.push(shift);
                assert_eq!(
                    call(&mut store, &instance, "fri_fold8x4", &arguments, 17),
                    flattened_fri_fold(seed, beta, shift),
                    "quartic FRI fold differs for seed {seed}, beta {beta:?}, shift {shift}",
                );
            }
        }
    }

    for invalid_shift in [0, MODULUS] {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "fri_fold8x4",
                &[97, 17, 23, 41, 73, invalid_shift],
                17,
            ),
            vec![0; 17],
            "zero FRI coset shift must fail closed",
        );
    }

    let baseline_fold = call(
        &mut store,
        &instance,
        "fri_fold8x4",
        &[97, 17, 23, 41, 73, 7],
        17,
    );
    for mutation in [
        [98, 17, 23, 41, 73, 7],
        [97, 16, 23, 41, 73, 7],
        [97, 17, 23, 41, 72, 7],
        [97, 17, 23, 41, 73, 8],
    ] {
        assert_ne!(
            call(&mut store, &instance, "fri_fold8x4", &mutation, 17),
            baseline_fold,
            "FRI seed, challenge, and shift mutations must change the fold",
        );
    }

    for (seed, transcript_seed, shift) in [(97, 439, 7), (0, 401, 123_456_789)] {
        for component in 0..8 {
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "fri_chain16_component",
                    &[seed, transcript_seed, shift, component as u32],
                    7,
                ),
                expected_fri_chain16_component(seed, transcript_seed, shift, component),
                "authenticated FRI chain differs at component {component}",
            );
        }
    }

    for invalid_shift in [0, MODULUS] {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "fri_chain16_component",
                &[97, 439, invalid_shift, 0],
                7,
            ),
            vec![0; 7],
            "complete FRI chain must reject a zero coset shift",
        );
    }

    let baseline_chain = call(
        &mut store,
        &instance,
        "fri_chain16_component",
        &[97, 439, 7, 0],
        7,
    );
    for mutation in [[98, 439, 7, 0], [97, 438, 7, 0], [97, 439, 8, 0]] {
        assert_ne!(
            call(&mut store, &instance, "fri_chain16_component", &mutation, 7,),
            baseline_chain,
            "FRI codeword, transcript, and shift mutations must change the chain",
        );
    }

    for transcript_seed in [0, 439, 1_900_000_007] {
        let sample = squeeze_challenge(b"FQ01", digest_seed(transcript_seed))[0];
        assert_eq!(
            call(&mut store, &instance, "fri_query1", &[transcript_seed], 3,),
            vec![1, sample, sample & 7],
            "typed FRI query sampling differs from Plonky3",
        );
    }

    for (seed, air_transcript_seed, shift) in [(97, 431, 7), (0, 433, 123_456_789)] {
        let query_index = expected_fri_query16_index(seed, air_transcript_seed, shift).unwrap();
        assert_eq!(
            call(
                &mut store,
                &instance,
                "fri_query16_status",
                &[seed, air_transcript_seed, shift],
                3,
            ),
            vec![1, 1, query_index],
            "authenticated FRI opening must verify at the transcript-derived index",
        );
        assert_eq!(
            call(
                &mut store,
                &instance,
                "fri_query16_mutation",
                &[seed, air_transcript_seed, shift, 0],
                1,
            ),
            vec![1],
            "unmodified authenticated FRI opening must verify",
        );
        for mutation in 1..=9 {
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "fri_query16_mutation",
                    &[seed, air_transcript_seed, shift, mutation],
                    1,
                ),
                vec![0],
                "authenticated FRI opening mutation {mutation} must be rejected",
            );
        }
    }

    for invalid_shift in [0, MODULUS] {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "fri_query16_status",
                &[97, 431, invalid_shift],
                3,
            ),
            vec![0, 0, 0],
            "query opening must reject a zero coset shift",
        );
    }
}
