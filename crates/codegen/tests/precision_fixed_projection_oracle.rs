//! Independent exactness gate for `precision::fixed::to_f32_bits<L>`.
//!
//! Fe performs the projection with a recursive view over 13-bit limbs. This
//! oracle instead treats the magnitude as one BigUint, divides it at the
//! binary32 precision boundary, and applies round-to-nearest, ties-to-even.
//! The two implementations share only the mathematical representation.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use num_bigint::{BigInt, BigUint, Sign};
use std::path::Path;
use url::Url;

const LIMB_BITS: usize = 13;
const LIMB_BASE: u32 = 8192;

fn compile_gate() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/precision_fixed_projection_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "projection gate ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("projection gate ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected projection gate diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("projection gate should compile to Wasm")
        .into_bytecode()
        .expect("Wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("projection gate Wasm should validate");
    bytes
}

fn call_u32(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    params: &[i32],
) -> u32 {
    use wasmtime::Val;
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("`{name}` export should exist"));
    let params = params.iter().copied().map(Val::I32).collect::<Vec<_>>();
    let mut result = [Val::I32(0)];
    function
        .call(&mut *store, &params, &mut result)
        .unwrap_or_else(|error| panic!("`{name}` should run: {error:?}"));
    match result[0] {
        Val::I32(value) => value as u32,
        ref other => panic!("`{name}` returned {other:?}, expected i32"),
    }
}

fn call_u32x4(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    params: &[i32],
) -> [u32; 4] {
    use wasmtime::Val;
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("`{name}` export should exist"));
    let params = params.iter().copied().map(Val::I32).collect::<Vec<_>>();
    let mut result = [Val::I32(0), Val::I32(0), Val::I32(0), Val::I32(0)];
    function
        .call(&mut *store, &params, &mut result)
        .unwrap_or_else(|error| panic!("`{name}` should run: {error:?}"));
    result.map(|value| match value {
        Val::I32(word) => word as u32,
        ref other => panic!("`{name}` returned {other:?}, expected four i32 words"),
    })
}

fn modulus(l: usize) -> BigUint {
    BigUint::from(1u32) << (LIMB_BITS * l)
}

fn limbs(magnitude: &BigUint, l: usize) -> Vec<i32> {
    let mask = BigUint::from(LIMB_BASE - 1);
    (0..l)
        .map(|index| {
            let limb = (magnitude >> (LIMB_BITS * index)) & &mask;
            limb.to_u32_digits().first().copied().unwrap_or(0) as i32
        })
        .collect()
}

/// Correctly rounded binary32 encoding of `sign * magnitude * 2^-F`, where
/// `F = 13 * (L - 1)`. All tested widths are in binary32's normal range.
fn reference_bits(sign: u32, magnitude: &BigUint, l: usize) -> u32 {
    if magnitude == &BigUint::from(0u32) {
        return 0;
    }
    let high = magnitude.bits() as i32 - 1;
    let mut exponent = high - (LIMB_BITS * (l - 1)) as i32 + 127;
    let mut retained = if high <= 23 {
        (magnitude << (23 - high) as usize)
            .to_u32_digits()
            .first()
            .copied()
            .unwrap_or(0)
    } else {
        let shift = (high - 23) as usize;
        let mut q = (magnitude >> shift)
            .to_u32_digits()
            .first()
            .copied()
            .unwrap_or(0);
        let remainder = magnitude - (BigUint::from(q) << shift);
        let half = BigUint::from(1u32) << (shift - 1);
        if remainder > half || (remainder == half && q & 1 == 1) {
            q += 1;
        }
        q
    };
    if retained == 1 << 24 {
        retained = 1 << 23;
        exponent += 1;
    }
    (sign << 31) | ((exponent as u32) << 23) | (retained & 0x7f_ffff)
}

/// Decode a normal binary32 word as a signed integer in Fixed's `2^-F`
/// units. Projection outputs are exact multiples of that unit for all widths
/// under test, so any right shift divides evenly.
fn scaled_integer_from_f32_bits(bits: u32, fractional_bits: usize) -> BigInt {
    if bits & 0x7fff_ffff == 0 {
        return BigInt::from(0u32);
    }
    let exponent = ((bits >> 23) & 0xff) as i32 - 127;
    let significand = BigInt::from((1u32 << 23) | (bits & 0x7f_ffff));
    let shift = exponent - 23 + fractional_bits as i32;
    let magnitude = if shift >= 0 {
        significand << shift as usize
    } else {
        significand >> (-shift) as usize
    };
    if bits >> 31 == 0 {
        magnitude
    } else {
        -magnitude
    }
}

fn reference_chunk_bits(sign: u32, magnitude: &BigUint, l: usize, offset: i32) -> u32 {
    if magnitude == &BigUint::from(0u32) {
        return 0;
    }
    let high = magnitude.bits() as i32 - 1;
    let chunk_high = high - offset;
    if chunk_high < 0 {
        return 0;
    }
    let chunk_low = chunk_high - 23;
    let mask = BigUint::from(0xff_ffffu32);
    let retained = if chunk_low < 0 {
        (magnitude << (-chunk_low) as usize) & &mask
    } else {
        (magnitude >> chunk_low as usize) & &mask
    }
    .to_u32_digits()
    .first()
    .copied()
    .unwrap_or(0);
    if retained == 0 {
        return 0;
    }
    let word_high = 31 - retained.leading_zeros() as i32;
    let fractional_bits = LIMB_BITS * (l - 1);
    let exponent = chunk_low + word_high - fractional_bits as i32 + 127;
    let significand = retained << (23 - word_high);
    (sign << 31) | ((exponent as u32) << 23) | (significand & 0x7f_ffff)
}

fn reference_quad_bits(sign: u32, magnitude: &BigUint, l: usize) -> ([u32; 4], BigInt) {
    let words = [0, 24, 48, 72].map(|offset| reference_chunk_bits(sign, magnitude, l, offset));
    let mut remaining = BigInt::from_biguint(
        if sign == 0 { Sign::Plus } else { Sign::Minus },
        magnitude.clone(),
    );
    let fractional_bits = LIMB_BITS * (l - 1);
    for word in words {
        remaining -= scaled_integer_from_f32_bits(word, fractional_bits);
    }
    (words, remaining)
}

fn directed_cases(l: usize) -> Vec<(String, BigUint)> {
    let modulus = modulus(l);
    let scale = BigUint::from(1u32) << (LIMB_BITS * (l - 1));
    let mut cases = vec![
        ("zero".into(), BigUint::from(0u32)),
        ("ulp".into(), BigUint::from(1u32)),
        ("one".into(), scale.clone()),
        ("one-minus-ulp".into(), &scale - 1u32),
        ("one-plus-ulp".into(), &scale + 1u32),
        ("max".into(), &modulus - 1u32),
    ];
    let max_high = LIMB_BITS * l - 1;
    for high in [24usize, max_high.saturating_sub(1), max_high] {
        if high < 24 || high > max_high {
            continue;
        }
        let shift = high - 23;
        for q in [0x80_0000u32, 0x80_0001, 0xff_fffe, 0xff_ffff] {
            let base = BigUint::from(q) << shift;
            let half = BigUint::from(1u32) << (shift - 1);
            for (tag, value) in [
                ("below", &base + &half - 1u32),
                ("tie", &base + &half),
                ("above", &base + &half + 1u32),
            ] {
                if value < modulus {
                    cases.push((format!("h{high}-q{q:x}-{tag}"), value));
                }
            }
        }
    }
    cases
}

fn next_random(seed: &mut u64, l: usize) -> BigUint {
    let mut value = BigUint::from(0u32);
    for _ in 0..((LIMB_BITS * l).div_ceil(64)) {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        value = (value << 64) | BigUint::from(*seed);
    }
    value % modulus(l)
}

#[test]
fn fixed_to_f32_bits_matches_independent_biguint_rounding_oracle() {
    let wasm = compile_gate();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load gate");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("projection gate should instantiate");

    for l in [2usize, 4, 6, 8] {
        let mut cases = directed_cases(l);
        let mut seed = 0x4f52_4249_545f_4633u64 ^ l as u64;
        for index in 0..256 {
            cases.push((format!("random-{index}"), next_random(&mut seed, l)));
        }
        for (name, magnitude) in &cases {
            for sign in [0u32, 1] {
                let mut params = vec![sign as i32];
                params.extend(limbs(magnitude, l));
                let got = call_u32(
                    &mut store,
                    &instance,
                    &format!("fixed_to_f32_bits_l{l}"),
                    &params,
                );
                let want = reference_bits(sign, magnitude, l);
                assert_eq!(
                    got, want,
                    "L={l} {name} sign={sign}, magnitude={magnitude}: got {got:#010x}, want {want:#010x}"
                );
            }
        }
        eprintln!("  L={l}: {} exact projection words green", cases.len() * 2);
    }
}

#[test]
fn fixed_to_f32_quad_bits_matches_independent_chunk_oracle() {
    let wasm = compile_gate();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load gate");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("projection gate should instantiate");

    let l = 8usize;
    let mut cases = directed_cases(l);
    let scale = BigUint::from(1u32) << (LIMB_BITS * (l - 1));
    let orbit_bound = BigUint::from(8u32) * &scale;
    let mut seed = 0x5155_4144_5f46_3332u64;
    for index in 0..256 {
        cases.push((
            format!("orbit-random-{index}"),
            next_random(&mut seed, l) % &orbit_bound,
        ));
    }

    for (name, magnitude) in &cases {
        for sign in [0u32, 1] {
            let mut params = vec![sign as i32];
            params.extend(limbs(magnitude, l));
            let got = call_u32x4(&mut store, &instance, "fixed_to_f32_quad_bits_l8", &params);
            let (want, remaining) = reference_quad_bits(sign, magnitude, l);
            assert_eq!(
                got, want,
                "L=8 {name} sign={sign}, magnitude={magnitude}: got {got:08x?}, want {want:08x?}"
            );
            if magnitude < &orbit_bound {
                assert_eq!(
                    remaining,
                    BigInt::from(0u32),
                    "four binary32 words must exactly cover an in-range Fixed<8> orbit value"
                );
            }
        }
    }
    eprintln!(
        "  L=8: {} exact four-word chunk decompositions green",
        cases.len() * 2
    );
}
