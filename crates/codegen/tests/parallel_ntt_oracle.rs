//! Independent value gate for Fe factor-tree scalar NTT interpretations.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use std::sync::OnceLock;
use url::Url;
use wasmtime::Val;

const MODULUS: u32 = 2_013_265_921;
const TWO_ADICITY: u32 = 27;

fn fixture_url() -> Url {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parallel_ntt_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "parallel NTT fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("parallel NTT fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected parallel NTT diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("parallel NTT fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("parallel NTT Wasm should validate");
    bytes
}

fn compiled_wasm() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    WASM.get_or_init(compile_wasm)
}

fn call_words(name: &str, arguments: &[u32], result_count: usize) -> Vec<u32> {
    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, compiled_wasm()).expect("parallel NTT module should load");
    assert_eq!(module.imports().len(), 0, "fixture must remain zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("parallel NTT module should instantiate");
    let function = instance
        .get_func(&mut store, name)
        .unwrap_or_else(|| panic!("missing `{name}` export"));
    let params: Vec<Val> = arguments
        .iter()
        .map(|value| Val::I32(*value as i32))
        .collect();
    let mut results = vec![Val::I32(0); result_count];
    function
        .call(&mut store, &params, &mut results)
        .unwrap_or_else(|error| panic!("`{name}` should execute: {error:?}"));
    results
        .into_iter()
        .map(|value| match value {
            Val::I32(word) => word as u32,
            other => panic!("`{name}` returned non-u32 lane {other:?}"),
        })
        .collect()
}

fn pow_mod(mut base: u64, mut exponent: u32) -> u32 {
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

fn direct_ntt(values: &[u32], inverse: bool) -> Vec<u32> {
    assert!(values.len().is_power_of_two());
    let log_n = values.len().ilog2();
    let maximal_root = pow_mod(31, 15);
    let mut root = pow_mod(u64::from(maximal_root), 1 << (TWO_ADICITY - log_n));
    if inverse {
        root = pow_mod(u64::from(root), MODULUS - 2);
    }
    let modulus = u64::from(MODULUS);
    let mut output = vec![0u32; values.len()];
    for (index, slot) in output.iter_mut().enumerate() {
        let point = pow_mod(u64::from(root), index as u32);
        let mut power = 1u64;
        let mut sum = 0u64;
        for value in values {
            sum = (sum + u64::from(*value % MODULUS) * power) % modulus;
            power = power * u64::from(point) % modulus;
        }
        *slot = sum as u32;
    }
    if inverse {
        let scale = pow_mod(values.len() as u64, MODULUS - 2);
        for value in &mut output {
            *value = (u64::from(*value) * u64::from(scale) % modulus) as u32;
        }
    }
    output
}

fn direct_coset_lde(values: &[u32], output_len: usize, shift: u32) -> Vec<u32> {
    let coefficients = direct_ntt(values, true);
    let log_n = output_len.ilog2();
    let maximal_root = pow_mod(31, 15);
    let root = pow_mod(u64::from(maximal_root), 1 << (TWO_ADICITY - log_n));
    let modulus = u64::from(MODULUS);
    (0..output_len)
        .map(|index| {
            let point = u64::from(shift % MODULUS)
                * u64::from(pow_mod(u64::from(root), index as u32))
                % modulus;
            let mut power = 1u64;
            let mut sum = 0u64;
            for coefficient in &coefficients {
                sum = (sum + u64::from(*coefficient) * power) % modulus;
                power = power * point % modulus;
            }
            sum as u32
        })
        .collect()
}

fn vectors16() -> Vec<Vec<u32>> {
    let mut pseudo_random = Vec::with_capacity(16);
    let mut state = 0x3141_5926u32;
    for _ in 0..16 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        pseudo_random.push(state);
    }
    vec![
        (0..16).collect(),
        vec![
            0,
            1,
            MODULUS - 1,
            MODULUS,
            MODULUS + 1,
            u32::MAX,
            17,
            31,
            65_535,
            65_536,
            1_000_000,
            0x8000_0000,
            123_456_789,
            987_654_321,
            42,
            7,
        ],
        pseudo_random,
    ]
}

fn ext4_pattern_component(seed: u32, component: usize) -> Vec<u32> {
    (0..8u32)
        .map(|index| match component {
            0 => seed + index,
            1 => seed + 17 + index,
            2 => seed + 37 + index + index,
            _ => seed + 71 + index + index + index,
        })
        .collect()
}

#[test]
fn dit_dif_bush_and_irregular_plans_match_direct_polynomial_evaluation() {
    for values in vectors16() {
        let expected = direct_ntt(&values, false);
        for name in ["dit16", "dif16", "bush16", "irregular16"] {
            assert_eq!(
                call_words(name, &values, 16),
                expected,
                "{name} must preserve the independently evaluated transform",
            );
        }
        assert_eq!(
            call_words("bush16_roundtrip", &values, 16),
            values
                .iter()
                .map(|value| value % MODULUS)
                .collect::<Vec<_>>(),
        );
    }
}

#[test]
fn factor_tree_coset_lde_matches_direct_evaluation_and_fails_closed() {
    let vectors = [
        vec![0; 8],
        vec![1, 2, 3, 4, 5, 6, 7, 8],
        vec![
            MODULUS - 1,
            MODULUS - 2,
            17,
            123_456_789,
            998_244_353,
            1_000_000_007,
            u32::MAX,
            0x8000_0000,
        ],
    ];

    for values in &vectors {
        assert_eq!(call_words("dit8", values, 8), direct_ntt(values, false));
        assert_eq!(
            call_words("dit8_roundtrip", values, 8),
            values
                .iter()
                .map(|value| value % MODULUS)
                .collect::<Vec<_>>(),
        );
        for shift in [7, 123_456_789] {
            let mut arguments = values.clone();
            arguments.push(shift);
            let actual = call_words("lde8x16", &arguments, 17);
            assert_eq!(actual[0], 1);
            assert_eq!(&actual[1..], direct_coset_lde(values, 16, shift));
        }
    }

    for shift in [0, 1] {
        let mut invalid_arguments = vectors[1].clone();
        invalid_arguments.push(shift);
        assert_eq!(
            call_words("lde8x16", &invalid_arguments, 17),
            vec![0; 17],
            "zero or subgroup shift {shift} cannot select a coset",
        );
    }

    for seed in [0, 97] {
        for component in 0..4 {
            let values = ext4_pattern_component(seed, component);
            assert_eq!(
                call_words("ext4_ntt8_component", &[seed, component as u32], 8,),
                direct_ntt(&values, false),
            );
            for shift in [7, 123_456_789] {
                let actual = call_words(
                    "ext4_lde8x16_component",
                    &[seed, component as u32, shift],
                    17,
                );
                assert_eq!(actual[0], 1);
                assert_eq!(&actual[1..], direct_coset_lde(&values, 16, shift));
            }
        }
    }

    for shift in [0, 1] {
        assert_eq!(
            call_words("ext4_lde8x16_component", &[97, 2, shift], 17),
            vec![0; 17],
        );
    }
}
