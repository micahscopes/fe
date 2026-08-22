//! Independent exactness gate for Fe-derived width-16 BabyBear Poseidon2.
//!
//! Fe derives every round constant from the shared Hades Grain construction.
//! The separately maintained Plonky3 implementation supplies both the
//! canonical constants and the permutation semantics.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use num_bigint::BigUint;
use p3_baby_bear::{
    BABYBEAR_POSEIDON2_RC_16_EXTERNAL_FINAL, BABYBEAR_POSEIDON2_RC_16_EXTERNAL_INITIAL,
    BABYBEAR_POSEIDON2_RC_16_INTERNAL, BabyBear, default_babybear_poseidon2_16,
};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_symmetric::Permutation;
use std::path::Path;
use std::sync::OnceLock;
use url::Url;
use wasmtime::Val;

const WIDTH: usize = 16;
const ROUND_CONSTANT_COUNT: usize = 8 * WIDTH + 13;
const BABY_BEAR_MODULUS: u32 = 2_013_265_921;

fn fixture_url() -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/poseidon_baby_bear_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn initialized_db() -> DriverDataBase {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "BabyBear Poseidon2 oracle fixture initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("BabyBear Poseidon2 oracle fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected BabyBear Poseidon2 fixture diagnostics:\n{diagnostics}"
    );
    db
}

fn compile_wasm() -> Vec<u8> {
    let db = initialized_db();
    let ingot = db
        .workspace()
        .containing_ingot(&db, fixture_url())
        .expect("BabyBear Poseidon2 oracle fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("BabyBear Poseidon2 fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("BabyBear Poseidon2 Wasm should validate");
    bytes
}

fn compiled_wasm() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    WASM.get_or_init(compile_wasm)
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

fn reference_parameters() -> Vec<u32> {
    let mut parameters = Vec::with_capacity(ROUND_CONSTANT_COUNT);
    for round in BABYBEAR_POSEIDON2_RC_16_EXTERNAL_INITIAL {
        parameters.extend(round.map(|value| value.as_canonical_u32()));
    }
    parameters.extend(BABYBEAR_POSEIDON2_RC_16_INTERNAL.map(|value| value.as_canonical_u32()));
    for round in BABYBEAR_POSEIDON2_RC_16_EXTERNAL_FINAL {
        parameters.extend(round.map(|value| value.as_canonical_u32()));
    }
    assert_eq!(parameters.len(), ROUND_CONSTANT_COUNT);
    parameters
}

fn reference_permutation(input: [u32; WIDTH]) -> [u32; WIDTH] {
    let mut state = input.map(BabyBear::from_u32);
    default_babybear_poseidon2_16().permute_mut(&mut state);
    state.map(|value| value.as_canonical_u32())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReferencePacking {
    bit_length: u32,
    fields: Vec<u32>,
}

fn bigint_u32(value: BigUint) -> u32 {
    value.to_u32_digits().first().copied().unwrap_or(0)
}

fn reference_pack(values: &[u32], widths: &[u32], field_count: usize) -> Option<ReferencePacking> {
    assert_eq!(values.len(), widths.len());
    let mut packed = BigUint::from(0u32);
    let mut bit_length = 0u32;
    for (&value, &width) in values.iter().zip(widths) {
        if width > 32 || (width < 32 && value >> width != 0) {
            return None;
        }
        if bit_length + width > (field_count as u32) * 30 {
            return None;
        }
        packed |= BigUint::from(value) << bit_length as usize;
        bit_length += width;
    }
    let mask = (BigUint::from(1u32) << 30usize) - BigUint::from(1u32);
    let fields = (0..field_count)
        .map(|index| bigint_u32((&packed >> (index * 30)) & &mask))
        .collect();
    Some(ReferencePacking { bit_length, fields })
}

fn reference_sponge(message: &[u32]) -> [u32; 8] {
    let mut state = [0u32; WIDTH];
    for block in message.chunks(8) {
        state[..block.len()].copy_from_slice(block);
        state = reference_permutation(state);
    }
    state[..8].try_into().unwrap()
}

fn reference_packed_commitment(domain: u32, packed: &ReferencePacking) -> [u32; 8] {
    let mut message = vec![domain, packed.bit_length];
    message.extend_from_slice(&packed.fields);
    reference_sponge(&message)
}

fn packing_arguments(values: [u32; 4], widths: [u32; 4]) -> [u32; 8] {
    [
        values[0], widths[0], values[1], widths[1], values[2], widths[2], values[3], widths[3],
    ]
}

fn protocol_tag(label: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*label)
}

fn reference_field_commitment(tag: &[u8; 4], fields: &[u32]) -> [u32; 8] {
    let mut message = vec![u32::from_be_bytes(*tag), fields.len() as u32];
    message.extend_from_slice(fields);
    reference_sponge(&message)
}

#[test]
fn fe_derived_poseidon2_matches_plonky3_parameters_and_permutations() {
    let bytes = compiled_wasm();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("Poseidon2 module should load");
    assert!(
        module.imports().next().is_none(),
        "Poseidon2 gate must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("Poseidon2 module should instantiate");

    for (index, expected) in reference_parameters().into_iter().enumerate() {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "derived_parameter",
                &[index as u32],
                1,
            ),
            vec![expected],
            "round constant {index} differs from Plonky3",
        );
    }

    let mut sequential = [0u32; WIDTH];
    for (index, value) in sequential.iter_mut().enumerate() {
        *value = index as u32;
    }
    let inputs = [
        [0u32; WIDTH],
        sequential,
        [u32::MAX; WIDTH],
        [
            894_848_333,
            1_437_655_012,
            1_200_606_629,
            1_690_012_884,
            71_131_202,
            1_749_206_695,
            1_717_947_831,
            120_589_055,
            19_776_022,
            42_382_981,
            1_831_865_506,
            724_844_064,
            171_220_207,
            1_299_207_443,
            227_047_920,
            1_783_754_913,
        ],
        [
            1,
            17,
            257,
            65_537,
            123_456_789,
            2_013_265_920,
            2_013_265_921,
            2_013_265_922,
            0x8000_0000,
            0x7fff_ffff,
            0x1357_9bdf,
            0x2468_ace0,
            31,
            27,
            16,
            13,
        ],
    ];

    for input in inputs {
        let expected = reference_permutation(input);
        assert_eq!(
            call(&mut store, &instance, "poseidon2_permute16", &input, WIDTH,),
            expected,
            "Fe Poseidon2 differs from Plonky3 for {input:?}",
        );
    }
}

#[test]
fn bounded_bits_are_injective_and_commit_with_length_and_domain() {
    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, compiled_wasm()).expect("packing oracle module should load");
    assert!(
        module.imports().next().is_none(),
        "packing and commitment gate must remain zero-import",
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("packing oracle module should instantiate");

    let directed = [
        ([0, 0, 0, 0], [0, 0, 0, 0]),
        ([u32::MAX; 4], [32; 4]),
        ([0x1fff_ffff, 3, 0x3fff_ffff, 0x7fff_ffff], [29, 2, 30, 31]),
        ([1, 0, 1, 0], [1, 29, 1, 29]),
        ([0x3fff_ffff, 1, 0x3fff_ffff, 1], [30, 1, 30, 1]),
    ];

    for (values, widths) in directed {
        let expected = reference_pack(&values, &widths, 5).expect("directed input is valid");
        let mut expected_words = vec![1, expected.bit_length];
        expected_words.extend_from_slice(&expected.fields);
        let arguments = packing_arguments(values, widths);
        assert_eq!(
            call(&mut store, &instance, "packed4", &arguments, 7),
            expected_words,
            "directed packing differs for values={values:?}, widths={widths:?}",
        );
        assert_eq!(
            call(&mut store, &instance, "packed4_commitment", &arguments, 9,),
            {
                let mut words = vec![1];
                words.extend(reference_packed_commitment(
                    protocol_tag(b"BP01"),
                    &expected,
                ));
                words
            },
            "typed commitment differs for values={values:?}, widths={widths:?}",
        );

        let mut reconstructed = BigUint::from(0u32);
        for (index, field) in expected.fields.iter().enumerate() {
            reconstructed |= BigUint::from(*field) << (30 * index);
        }
        let mut offset = 0usize;
        for (&value, &width) in values.iter().zip(&widths) {
            let mask = if width == 0 {
                BigUint::from(0u32)
            } else {
                (BigUint::from(1u32) << width as usize) - BigUint::from(1u32)
            };
            assert_eq!(
                bigint_u32((&reconstructed >> offset) & mask),
                value,
                "round trip changed source word at bit offset {offset}",
            );
            offset += width as usize;
        }
    }

    let mut state = 0x6d2b_79f5u32;
    for _ in 0..128 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let widths = [
            state % 33,
            (state >> 7) % 33,
            (state >> 14) % 33,
            (state >> 21) % 33,
        ];
        let mut values = [0u32; 4];
        for index in 0..4 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            values[index] = if widths[index] == 32 {
                state
            } else if widths[index] == 0 {
                0
            } else {
                state & ((1u32 << widths[index]) - 1)
            };
        }
        let expected = reference_pack(&values, &widths, 5).expect("random input fits five lanes");
        let mut expected_words = vec![1, expected.bit_length];
        expected_words.extend_from_slice(&expected.fields);
        assert_eq!(
            call(
                &mut store,
                &instance,
                "packed4",
                &packing_arguments(values, widths),
                7,
            ),
            expected_words,
            "random packing differs for values={values:?}, widths={widths:?}",
        );
    }

    let baseline_values = [0x89ab_cdef, 0x6543_210f, 0x2aaa_aaaa, 0x0123_4567];
    let baseline_widths = [32, 31, 30, 29];
    let baseline = call(
        &mut store,
        &instance,
        "packed4",
        &packing_arguments(baseline_values, baseline_widths),
        7,
    );
    for word in 0..4 {
        for bit in 0..baseline_widths[word] {
            let mut mutated = baseline_values;
            mutated[word] ^= 1u32 << bit;
            let got = call(
                &mut store,
                &instance,
                "packed4",
                &packing_arguments(mutated, baseline_widths),
                7,
            );
            assert_ne!(got, baseline, "source bit mutation {word}:{bit} was lost");
            let expected = reference_pack(&mutated, &baseline_widths, 5).unwrap();
            let mut expected_words = vec![1, expected.bit_length];
            expected_words.extend_from_slice(&expected.fields);
            assert_eq!(
                got, expected_words,
                "source bit mutation {word}:{bit} moved"
            );
        }
    }

    for (values, widths) in [([2, 0, 0, 0], [1, 0, 0, 0]), ([1, 0, 0, 0], [33, 0, 0, 0])] {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "packed4",
                &packing_arguments(values, widths),
                7,
            ),
            vec![0; 7],
            "invalid bounded input must fail closed",
        );
    }
    assert_eq!(
        call(
            &mut store,
            &instance,
            "packed4_overflow_valid",
            &packing_arguments([u32::MAX; 4], [32; 4]),
            1,
        ),
        vec![0],
        "128 bits must not fit in four 30-bit payload lanes",
    );
    assert_eq!(
        call(
            &mut store,
            &instance,
            "packed4_invalid_domain",
            &packing_arguments([1, 2, 3, 4], [1, 2, 2, 3]),
            1,
        ),
        vec![0],
        "a noncanonical domain field must fail closed",
    );

    let short = reference_pack(&[1, 0, 0, 0], &[1, 0, 0, 0], 5).unwrap();
    let long = reference_pack(&[1, 0, 0, 0], &[1, 1, 0, 0], 5).unwrap();
    assert_eq!(
        short.fields, long.fields,
        "fixture must isolate length binding"
    );
    let short_digest = call(
        &mut store,
        &instance,
        "packed4_commitment",
        &packing_arguments([1, 0, 0, 0], [1, 0, 0, 0]),
        9,
    );
    let long_digest = call(
        &mut store,
        &instance,
        "packed4_commitment",
        &packing_arguments([1, 0, 0, 0], [1, 1, 0, 0]),
        9,
    );
    assert_ne!(
        short_digest, long_digest,
        "bit length must affect the digest"
    );

    for seed in [0, 1, u32::MAX, 0x1357_9bdf] {
        let values: Vec<u32> = (0..411)
            .map(|index| ((seed >> (index & 31)) ^ index ^ (index >> 3)) & 1)
            .collect();
        let widths = vec![1u32; 411];
        let expected = reference_pack(&values, &widths, 14).unwrap();
        let mut expected_words = vec![1, 411];
        expected_words.extend_from_slice(&expected.fields);
        assert_eq!(
            call(&mut store, &instance, "packed411", &[seed], 16),
            expected_words,
            "411-bit maximum single-permutation payload differs for seed {seed:#x}",
        );
        assert_eq!(
            call(&mut store, &instance, "packed411_commitment", &[seed], 9,),
            {
                let mut words = vec![1];
                words.extend(reference_packed_commitment(
                    protocol_tag(b"BP01"),
                    &expected,
                ));
                words
            },
            "411-bit typed commitment differs for seed {seed:#x}",
        );
    }

    let fields17 = [
        0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610, 987, 1597,
    ];
    assert_eq!(
        call(&mut store, &instance, "fields17", &fields17, 8),
        reference_field_commitment(b"BV01", &fields17),
        "17-field sponge differs from the independent Plonky3 model",
    );
    assert_eq!(
        call(&mut store, &instance, "fields17_staged", &fields17, 9,),
        {
            let mut expected = vec![1];
            expected.extend(reference_field_commitment(b"BV01", &fields17));
            expected
        },
        "checkpointed field sponge differs from the independent Plonky3 model",
    );
    for index in 0..fields17.len() {
        let mut mutated = fields17;
        mutated[index] += 1;
        let actual = call(&mut store, &instance, "fields17", &mutated, 8);
        assert_eq!(actual, reference_field_commitment(b"BV01", &mutated));
        assert_ne!(
            actual,
            reference_field_commitment(b"BV01", &fields17),
            "field-vector position {index} was not bound",
        );
        let staged = call(&mut store, &instance, "fields17_staged", &mutated, 9);
        let mut staged_expected = vec![1];
        staged_expected.extend(reference_field_commitment(b"BV01", &mutated));
        assert_eq!(staged, staged_expected);
    }

    let extensions = [1, 2, 3, 4, 101, 103, 107, 109];
    assert_eq!(
        call(&mut store, &instance, "extensions2", &extensions, 8),
        reference_field_commitment(b"BV01", &extensions),
        "quartic flattening order differs from the independent Plonky3 model",
    );
    for index in 0..extensions.len() {
        let mut mutated = extensions;
        mutated[index] += 1;
        let actual = call(&mut store, &instance, "extensions2", &mutated, 8);
        assert_eq!(actual, reference_field_commitment(b"BV01", &mutated));
        assert_ne!(
            actual,
            reference_field_commitment(b"BV01", &extensions),
            "extension coefficient {index} was not bound",
        );
    }

    for arguments in [
        [0, 0, 0, 0, 0, 0, 0],
        [1, 7, 11, 13, 17, 19, 23],
        [1, 1_000_000_000, 8191, 4096, 31, 1, 0],
    ] {
        let fields = [
            (arguments[0] == 1) as u32,
            arguments[1],
            arguments[2],
            arguments[3],
            arguments[4],
            arguments[5],
            arguments[6],
        ];
        let mut expected = vec![1];
        expected.extend(reference_field_commitment(b"BV01", &fields));
        assert_eq!(
            call(
                &mut store,
                &instance,
                "canonical7_commitment",
                &arguments,
                9,
            ),
            expected,
            "canonical record commitment must equal the direct field sponge",
        );
    }

    assert_eq!(
        call(
            &mut store,
            &instance,
            "canonical7_commitment",
            &[1, BABY_BEAR_MODULUS, 1, 2, 3, 4, 5],
            9,
        ),
        vec![0; 9],
        "a canonical word outside BabyBear must fail closed",
    );
}
