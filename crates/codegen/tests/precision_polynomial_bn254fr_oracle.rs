//! Independent semantic gate for the generic Fe radix-2 transform.
//! Rust computes the evaluation definition directly in `num-bigint`, without
//! sharing the Fe butterfly schedule or twiddle recurrence.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use num_bigint::BigUint;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const LIMB_BITS: usize = 13;
const LIMBS: usize = 20;

fn prime() -> BigUint {
    BigUint::parse_bytes(
        b"21888242871839275222246405745257275088548364400416034343698204186575808495617",
        10,
    )
    .expect("BN254 Fr decimal should parse")
}

fn to_limbs(value: &BigUint) -> Vec<u32> {
    let mask = BigUint::from(8191u32);
    (0..LIMBS)
        .map(|index| {
            ((value >> (LIMB_BITS * index)) & &mask)
                .to_u32_digits()
                .first()
                .copied()
                .unwrap_or(0)
        })
        .collect()
}

fn compile_gate() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/precision_polynomial_bn254fr_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("gate ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected polynomial gate diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("generic radix-2 gate should compile")
        .into_bytecode()
        .expect("Wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("gate Wasm should validate");
    bytes
}

#[test]
fn invalid_radix2_domains_are_rejected_by_const_predicates() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/precision_polynomial_domain_reject_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("domain rejection gate ingot");
    let diagnostics = db.run_on_top_mod(ingot.root_mod(&db)).format_diags(&db);
    assert_eq!(
        diagnostics
            .matches("const predicate is not satisfied")
            .count(),
        2,
        "both the non-power-of-two and field-unsupported domains must fail at compile time:\n\
         {diagnostics}",
    );
    assert!(
        diagnostics.contains("radix2_ntt<3, 20, Bn254Fr>")
            && diagnostics.contains("radix2_ntt<8, 3, AdicityTwo>"),
        "diagnostics must identify both rejected transform instantiations:\n{diagnostics}",
    );
}

fn call_words(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    output: usize,
    values: &[u32],
) -> Vec<u32> {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("`{name}` export should exist"));
    let mut params = Vec::with_capacity(values.len() + 1);
    params.push(Val::I32(output as i32));
    params.extend(values.iter().map(|value| Val::I32(*value as i32)));
    let mut results = vec![Val::I32(0); LIMBS];
    function
        .call(&mut *store, &params, &mut results)
        .unwrap_or_else(|error| panic!("{name}[{output}] should run: {error:?}"));
    results
        .into_iter()
        .map(|result| match result {
            Val::I32(word) => word as u32,
            other => panic!("{name} result must be i32, got {other:?}"),
        })
        .collect()
}

fn direct_dft(values: &[u32], modulus: &BigUint) -> Vec<BigUint> {
    let n = values.len();
    let log_n = n.trailing_zeros();
    let root_exponent = (modulus - BigUint::from(1u32)) >> 28usize;
    let maximal_root = BigUint::from(5u32).modpow(&root_exponent, modulus);
    let root = maximal_root.modpow(&(BigUint::from(1u32) << (28u32 - log_n)), modulus);
    (0..n)
        .map(|output| {
            values
                .iter()
                .enumerate()
                .fold(BigUint::from(0u32), |sum, (coefficient, value)| {
                    let exponent = BigUint::from((output * coefficient) as u32);
                    (sum + BigUint::from(*value) * root.modpow(&exponent, modulus)) % modulus
                })
        })
        .collect()
}

fn subgroup_root(log_n: u32, modulus: &BigUint) -> BigUint {
    let root_exponent = (modulus - BigUint::from(1u32)) >> 28usize;
    BigUint::from(5u32)
        .modpow(&root_exponent, modulus)
        .modpow(&(BigUint::from(1u32) << (28u32 - log_n)), modulus)
}

fn direct_coset_lde(
    evaluations: &[u32],
    output_size: usize,
    shift: u32,
    modulus: &BigUint,
) -> Vec<BigUint> {
    let input_size = evaluations.len();
    let input_root = subgroup_root(input_size.trailing_zeros(), modulus);
    let inverse_root = input_root.modpow(&(modulus - BigUint::from(2u32)), modulus);
    let inverse_size = BigUint::from(input_size).modpow(&(modulus - BigUint::from(2u32)), modulus);
    let coefficients = (0..input_size)
        .map(|coefficient| {
            let sum =
                evaluations
                    .iter()
                    .enumerate()
                    .fold(BigUint::from(0u32), |sum, (sample, value)| {
                        let exponent = BigUint::from((sample * coefficient) as u32);
                        (sum + BigUint::from(*value) * inverse_root.modpow(&exponent, modulus))
                            % modulus
                    });
            (sum * &inverse_size) % modulus
        })
        .collect::<Vec<_>>();
    let output_root = subgroup_root(output_size.trailing_zeros(), modulus);
    (0..output_size)
        .map(|output| {
            let point = (BigUint::from(shift)
                * output_root.modpow(&BigUint::from(output as u32), modulus))
                % modulus;
            coefficients
                .iter()
                .rev()
                .fold(BigUint::from(0u32), |value, coefficient| {
                    (value * &point + coefficient) % modulus
                })
        })
        .collect()
}

fn call_lde_words(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    output: usize,
    shift: u32,
    evaluations: &[u32],
) -> (bool, Vec<u32>) {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("`{name}` export should exist"));
    let mut params = Vec::with_capacity(evaluations.len() + 2);
    params.push(Val::I32(output as i32));
    params.push(Val::I32(shift as i32));
    params.extend(evaluations.iter().map(|value| Val::I32(*value as i32)));
    let mut results = vec![Val::I32(0); LIMBS + 1];
    function
        .call(&mut *store, &params, &mut results)
        .unwrap_or_else(|error| panic!("{name}[{output}] should run: {error:?}"));
    let valid = match results[0] {
        Val::I32(value) => value != 0,
        ref other => panic!("{name} validity must be i32, got {other:?}"),
    };
    let words = results
        .into_iter()
        .skip(1)
        .map(|result| match result {
            Val::I32(word) => word as u32,
            other => panic!("{name} result must be i32, got {other:?}"),
        })
        .collect();
    (valid, words)
}

#[test]
fn generic_radix2_ntt_and_intt_match_direct_bigint_dft() {
    let wasm = compile_gate();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).expect("Wasm module should load");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("zero-import gate should instantiate");
    let modulus = prime();

    let cases = [
        ("4", vec![0, 1, 2, u32::MAX]),
        ("8", vec![5, 15, 39, 77, 129, 195, 275, 369]),
        (
            "16",
            vec![
                1, 3, 9, 27, 81, 243, 729, 2187, 6561, 19_683, 59_049, 177_147, 531_441, 1_594_323,
                4_782_969, 14_348_907,
            ],
        ),
    ];

    for (size, values) in cases {
        let expected = direct_dft(&values, &modulus);
        let ntt = format!("ntt{size}_words");
        let roundtrip = format!("roundtrip{size}_words");
        for output in 0..values.len() {
            assert_eq!(
                call_words(&mut store, &instance, &ntt, output, &values),
                to_limbs(&expected[output]),
                "generic Fe NTT-{size} output {output} must equal the direct bigint DFT",
            );
            assert_eq!(
                call_words(&mut store, &instance, &roundtrip, output, &values),
                to_limbs(&BigUint::from(values[output])),
                "generic Fe INTT-{size}(NTT-{size}) output {output} must recover its coefficient",
            );
        }
    }

    assert_eq!(
        call_words(&mut store, &instance, "ntt4_words", 4, &[1, 2, 3, 4]),
        vec![0; LIMBS],
        "the test boundary must fail closed on an invalid output index",
    );
}

#[test]
fn generic_coset_lde_matches_direct_interpolation_and_evaluation() {
    let wasm = compile_gate();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).expect("Wasm module should load");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("zero-import gate should instantiate");
    let modulus = prime();
    let shift = 5u32;

    for (name, evaluations) in [
        ("coset_lde4_to16_words", vec![2, 3, 5, 7]),
        (
            "coset_lde8_to16_words",
            vec![5, 15, 39, 77, 129, 195, 275, 369],
        ),
    ] {
        let expected = direct_coset_lde(&evaluations, 16, shift, &modulus);
        for output in 0..16 {
            let (valid, words) =
                call_lde_words(&mut store, &instance, name, output, shift, &evaluations);
            assert!(valid, "shift 5 must define a disjoint evaluation coset");
            assert_eq!(
                words,
                to_limbs(&expected[output]),
                "generic Fe coset LDE output {output} must equal direct bigint interpolation and evaluation",
            );
        }

        for invalid_shift in [0, 1] {
            let (valid, words) =
                call_lde_words(&mut store, &instance, name, 0, invalid_shift, &evaluations);
            assert!(!valid, "shift {invalid_shift} must be rejected");
            assert_eq!(words, vec![0; LIMBS], "invalid cosets must fail closed");
        }
    }
}
