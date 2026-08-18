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
                actual[1..], expected_root,
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
}
