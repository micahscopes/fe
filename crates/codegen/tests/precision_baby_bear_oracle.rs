//! Independent exactness gate for the portable u32-native BabyBear field.
//!
//! Fe derives one-word Montgomery arithmetic from the BabyBear parameter
//! block. This test executes the Wasm artifact against ordinary u64 modular
//! arithmetic, then requires the same authored multiply to lower through the
//! Naga-validated SPIR-V and browser-WGSL path without a u64 capability.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const MODULUS: u32 = 2_013_265_921;
const TWO_ADICITY: u32 = 27;

fn fixture_url() -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/precision_baby_bear_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn initialized_db() -> DriverDataBase {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "BabyBear oracle fixture initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("BabyBear oracle fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected BabyBear fixture diagnostics:\n{diagnostics}"
    );
    db
}

fn compile_wasm() -> Vec<u8> {
    let db = initialized_db();
    let ingot = db
        .workspace()
        .containing_ingot(&db, fixture_url())
        .expect("BabyBear oracle fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("BabyBear fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("BabyBear Wasm should validate");
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

fn inverse_mod_2_32(odd: u32) -> u32 {
    let modulus = 1i128 << 32;
    let mut old_r = i128::from(odd);
    let mut r = modulus;
    let mut old_s = 1i128;
    let mut s = 0i128;
    while r != 0 {
        let quotient = old_r / r;
        (old_r, r) = (r, old_r - quotient * r);
        (old_s, s) = (s, old_s - quotient * s);
    }
    old_s.rem_euclid(modulus) as u32
}

#[test]
fn baby_bear_word_field_matches_independent_u64_oracle() {
    let bytes = compile_wasm();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("BabyBear module should load");
    assert!(
        module.imports().next().is_none(),
        "field gate must be zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("BabyBear module should instantiate");

    let parameters = call(&mut store, &instance, "baby_bear_parameters", &[], 3);
    let expected_inverse = inverse_mod_2_32(MODULUS).wrapping_neg();
    let radix = (1u128 << 32) % u128::from(MODULUS);
    let expected_r2 = (radix * radix % u128::from(MODULUS)) as u32;
    let expected_root = pow_mod(31, 15);
    assert_eq!(
        parameters,
        vec![expected_inverse, expected_r2, expected_root],
        "all Montgomery and subgroup constants must be Fe-derived",
    );
    let values = [
        0,
        1,
        2,
        3,
        MODULUS - 2,
        MODULUS - 1,
        MODULUS,
        MODULUS + 1,
        123_456_789,
        0x8000_0000,
        u32::MAX,
    ];
    for &value in &values {
        assert_eq!(
            call(&mut store, &instance, "baby_bear_roundtrip", &[value], 1),
            vec![value % MODULUS],
            "roundtrip mismatch for {value}",
        );
        assert_eq!(
            call(&mut store, &instance, "baby_bear_neg", &[value], 1),
            vec![if value % MODULUS == 0 {
                0
            } else {
                MODULUS - value % MODULUS
            }],
            "negation mismatch for {value}",
        );
    }

    for &left in &values {
        for &right in &values {
            let a = u64::from(left % MODULUS);
            let b = u64::from(right % MODULUS);
            let p = u64::from(MODULUS);
            assert_eq!(
                call(&mut store, &instance, "baby_bear_add", &[left, right], 1),
                vec![((a + b) % p) as u32],
                "addition mismatch for ({left}, {right})",
            );
            assert_eq!(
                call(&mut store, &instance, "baby_bear_sub", &[left, right], 1),
                vec![((a + p - b) % p) as u32],
                "subtraction mismatch for ({left}, {right})",
            );
            assert_eq!(
                call(&mut store, &instance, "baby_bear_mul", &[left, right], 1),
                vec![(a * b % p) as u32],
                "multiplication mismatch for ({left}, {right})",
            );
        }
    }

    for &(value, exponent) in &[
        (0, 0),
        (0, 17),
        (2, 0),
        (2, 1),
        (2, 31),
        (31, 15),
        (123_456_789, 1_000_003),
    ] {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "baby_bear_pow",
                &[value, exponent],
                1,
            ),
            vec![pow_mod(u64::from(value), exponent)],
            "power mismatch for ({value}, {exponent})",
        );
    }
    assert_eq!(
        call(&mut store, &instance, "baby_bear_inverse", &[0], 1),
        vec![0],
    );
    for &value in &values[1..] {
        let canonical = value % MODULUS;
        if canonical == 0 {
            continue;
        }
        let inverse = call(&mut store, &instance, "baby_bear_inverse", &[value], 1)[0];
        assert_eq!(
            u64::from(canonical) * u64::from(inverse) % u64::from(MODULUS),
            1,
            "inverse mismatch for {value}",
        );
    }

    for log_order in 0..=TWO_ADICITY {
        let root = call(
            &mut store,
            &instance,
            "baby_bear_two_adic_root",
            &[log_order],
            1,
        )[0];
        assert_eq!(
            pow_mod(u64::from(root), 1 << log_order),
            1,
            "root order does not divide 2^{log_order}",
        );
        if log_order > 0 {
            assert_eq!(
                pow_mod(u64::from(root), 1 << (log_order - 1)),
                MODULUS - 1,
                "root must have exact order 2^{log_order}",
            );
        }
    }
    assert_eq!(
        call(
            &mut store,
            &instance,
            "baby_bear_two_adic_root",
            &[TWO_ADICITY + 1],
            1,
        ),
        vec![0],
        "unsupported subgroup order must fail closed",
    );
}

#[test]
fn baby_bear_multiply_lowers_to_browser_u32_spirv() {
    let db = initialized_db();
    let ingot = db
        .workspace()
        .containing_ingot(&db, fixture_url())
        .expect("BabyBear oracle fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "baby_bear_mul")
        .expect("BabyBear multiply should build a runtime package");
    let artifact =
        fe_codegen::compile_runtime_package_spirv_with_workgroup(&db, &package, [1, 1, 1])
            .expect("u32-native BabyBear multiply should lower to SPIR-V");
    assert_eq!(artifact.words.first().copied(), Some(0x0723_0203));
    assert_eq!(
        artifact.layout.word,
        sonatina_codegen::isa::spirv::WordKind::U32,
    );
    let wgsl = artifact.wgsl.expect("Naga should emit BabyBear WGSL");
    assert!(!wgsl.contains("i64") && !wgsl.contains("u64"));
    assert!(
        !wgsl.contains("if "),
        "BabyBear multiply must remain branch-free in browser WGSL",
    );
    let module = naga::front::wgsl::parse_str(&wgsl).expect("BabyBear WGSL should parse");
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    );
    validator
        .validate(&module)
        .expect("BabyBear WGSL should validate in the browser profile");
}
