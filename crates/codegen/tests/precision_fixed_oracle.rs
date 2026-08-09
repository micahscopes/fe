//! Bit-identical oracle gate for the precision axis: `precision::fixed::{mul,
//! add, sub, sqr, escape}` (the general `Fixed<L>` integer fixed-point number,
//! `SHARED_LIMB_CORE_DESIGN.md`) compiled to wasm and compared, LIMB FOR LIMB,
//! against an INDEPENDENT num-bigint fixed-point reference (which knows nothing
//! of 13-bit limbs, the CIOS sliding window, or sign-magnitude plumbing), over
//! edge + directed-tie + wrap-adversarial + random operands at L=4 AND L=8.
//!
//! `Fixed<L>` reuses `field.fe`'s 13-bit limb machinery verbatim, so B = 8192
//! and a value is `(-1)^sign * mag * 2^-F` with `F = 13*(L-1)` fractional bits
//! (one integer limb at index L-1). The closed-form multiply spec is exact
//! round-to-nearest-ties-up:
//!   mul((sa,a),(sb,b)) = (sa xor sb, floor((|a|*|b| + B^(L-1)/2) / B^(L-1)))
//! wrapping mod B^L. The reference below computes exactly that with BigUint.
//!
//! Comparison rule (sign-magnitude): magnitudes always compared limb-for-limb;
//! signs compared only when the magnitude is nonzero (negative zero is
//! representable and harmless).

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use num_bigint::BigUint;
use std::path::Path;
use url::Url;

const LIMB_BITS: u32 = 13;
const B: u32 = 8192; // 2^13

fn base() -> BigUint {
    BigUint::from(B)
}
/// B^L (the wrap modulus, one past the top of the L-limb range).
fn modulus(l: usize) -> BigUint {
    base().pow(l as u32)
}
/// B^(L-1) = 2^F (the fixed-point scale: dividing the true product by this
/// realigns the radix point).
fn scale(l: usize) -> BigUint {
    base().pow((l - 1) as u32)
}

/// Decompose a magnitude (< B^L) into its L limbs, LSB first.
fn to_limbs(x: &BigUint, l: usize) -> Vec<u32> {
    let mask = BigUint::from(B - 1);
    (0..l)
        .map(|j| {
            let limb = (x >> (LIMB_BITS as usize * j)) & &mask;
            limb.to_u32_digits().first().copied().unwrap_or(0)
        })
        .collect()
}

// -------------------------------------------------------------------------
// The INDEPENDENT reference: sign-magnitude fixed-point over BigUint.
// -------------------------------------------------------------------------

#[derive(Clone)]
struct Fx {
    sign: u32,
    mag: BigUint,
}

/// mul/sqr: exact round-to-nearest-ties-up of `|a|*|b| / B^(L-1)`, wrap mod B^L.
fn ref_mul(a: &Fx, b: &Fx, l: usize) -> Fx {
    let x = &a.mag * &b.mag;
    let half = scale(l) >> 1u32; // B^(L-1)/2 = 2^(F-1)
    let rounded = (x + half) / scale(l);
    let mag = rounded % modulus(l);
    Fx {
        sign: a.sign ^ b.sign,
        mag,
    }
}

/// add: same sign -> add magnitudes (wrap mod B^L); opposite -> subtract the
/// smaller from the larger and keep the larger's sign (`a >= b` decided the
/// same way `SubBorrow`'s borrow-out does: borrow==0 iff a>=b).
fn ref_add(a: &Fx, b: &Fx, l: usize) -> Fx {
    if a.sign == b.sign {
        Fx {
            sign: a.sign,
            mag: (&a.mag + &b.mag) % modulus(l),
        }
    } else if a.mag >= b.mag {
        Fx {
            sign: a.sign,
            mag: &a.mag - &b.mag,
        }
    } else {
        Fx {
            sign: b.sign,
            mag: &b.mag - &a.mag,
        }
    }
}

fn ref_sub(a: &Fx, b: &Fx, l: usize) -> Fx {
    let nb = Fx {
        sign: 1 - b.sign,
        mag: b.mag.clone(),
    };
    ref_add(a, &nb, l)
}

/// Strict `|z|^2 > 4`: top (integer) limb > 4, or == 4 with any nonzero
/// fractional limb.
fn ref_escaped(mag2: &BigUint, l: usize) -> bool {
    let top = mag2 / scale(l); // limb L-1 (mag2 < B^L so this is < B)
    let frac = mag2 % scale(l);
    top > BigUint::from(4u32) || (top == BigUint::from(4u32) && frac != BigUint::from(0u32))
}

/// The escape reference: the identical integer orbit `z <- z^2 + c`, returning
/// the iteration count (== max_iter for points that never escape).
fn ref_escape(cx: &Fx, cy: &Fx, max_iter: i32, l: usize) -> i32 {
    let zero = Fx {
        sign: 0,
        mag: BigUint::from(0u32),
    };
    let mut zx = zero.clone();
    let mut zy = zero.clone();
    let mut count = 0i32;
    let mut done = false;
    let mut i = 0i32;
    while i < max_iter {
        if !done {
            let xx = ref_mul(&zx, &zx, l);
            let yy = ref_mul(&zy, &zy, l);
            let mag2 = ref_add(&xx, &yy, l).mag;
            if ref_escaped(&mag2, l) {
                done = true;
            } else {
                let re = ref_sub(&xx, &yy, l);
                let nzx = ref_add(&re, cx, l);
                let m = ref_mul(&zx, &zy, l);
                let d = ref_add(&m, &m, l);
                let nzy = ref_add(&d, cy, l);
                zx = nzx;
                zy = nzy;
                count += 1;
            }
        }
        i += 1;
    }
    count
}

// -------------------------------------------------------------------------
// wasm compile + call harness (mirrors precision_field_bn254fr_oracle.rs).
// -------------------------------------------------------------------------

fn compile_fixed_gate_ingot_to_wasm() -> (Vec<u8>, std::time::Duration) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/precision_fixed_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    let t0 = std::time::Instant::now();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "precision Fixed<L> oracle gate ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("precision Fixed<L> oracle gate ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected gate-ingot diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("precision Fixed<L> oracle gate ingot should compile to wasm")
        .into_bytecode()
        .expect("wasm output should be bytecode");
    let elapsed = t0.elapsed();
    wasmparser::validate(&bytes).expect("gate ingot wasm should validate");
    (bytes, elapsed)
}

fn instantiate(bytes: &[u8]) -> (wasmtime::Store<()>, wasmtime::Instance) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    (store, instance)
}

/// Call an export by name with i32 params, returning its i32 result (untyped
/// path: the L=8 arities exceed wasmtime's typed-tuple support).
fn call_i32(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    params: &[i32],
) -> i32 {
    use wasmtime::Val;
    let f = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("`{name}` export should exist"));
    let vals: Vec<Val> = params.iter().map(|&p| Val::I32(p)).collect();
    let mut results = [Val::I32(0)];
    f.call(&mut *store, &vals, &mut results)
        .unwrap_or_else(|e| panic!("{name}(...) should run: {e:?}"));
    match results[0] {
        Val::I32(v) => v,
        other => panic!("{name} result must be i32, got {other:?}"),
    }
}

/// Assemble the flattened binary-op params `[sa, a0..a_{L-1}, sb, b0..b_{L-1}]`.
fn bin_params(a: &Fx, b: &Fx, l: usize) -> Vec<i32> {
    let mut p = vec![a.sign as i32];
    p.extend(to_limbs(&a.mag, l).iter().map(|&x| x as i32));
    p.push(b.sign as i32);
    p.extend(to_limbs(&b.mag, l).iter().map(|&x| x as i32));
    p
}

/// Read `(sign, limbs)` back from the per-limb + sign exports for one op/L.
fn fe_result(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    op: &str,
    l: usize,
    params: &[i32],
) -> Fx {
    let sign = call_i32(store, instance, &format!("fixed_{op}_l{l}_sign"), params) as u32;
    let mut mag = BigUint::from(0u32);
    for k in 0..l {
        let limb =
            call_i32(store, instance, &format!("fixed_{op}_l{l}_limb{k}"), params) as u32;
        mag |= BigUint::from(limb) << (LIMB_BITS as usize * k);
    }
    Fx { sign, mag }
}

/// The sign-magnitude comparison rule: magnitudes always equal; signs equal
/// unless the magnitude is zero.
fn assert_fx_eq(got: &Fx, want: &Fx, ctx: &str) {
    assert_eq!(
        got.mag, want.mag,
        "{ctx}: magnitude mismatch (got {}, want {})",
        got.mag, want.mag
    );
    if want.mag != BigUint::from(0u32) {
        assert_eq!(got.sign, want.sign, "{ctx}: sign mismatch");
    }
}

fn fx(sign: u32, mag: BigUint) -> Fx {
    Fx { sign, mag }
}

/// The named unsigned magnitudes for one L (edge + directed-tie + wrap).
fn base_mags(l: usize) -> Vec<(String, BigUint)> {
    let s = scale(l); // B^(L-1) = value 1.0
    let m = modulus(l);
    let mut v: Vec<(String, BigUint)> = vec![
        ("0".into(), BigUint::from(0u32)),
        ("ulp".into(), BigUint::from(1u32)),
        ("half".into(), &s >> 1u32),
        ("one".into(), s.clone()),
        ("two".into(), &s * 2u32),
        ("four".into(), &s * 4u32),
        ("six".into(), &s * 6u32),
        ("1.5".into(), &s + (&s >> 1u32)),
        ("max".into(), &m - 1u32), // all limbs saturated
    ];
    // Dense pattern that is not a round multiple of the scale (stresses every
    // limb of the sliding window).
    let mut dense = BigUint::from(0u32);
    for j in 0..l {
        dense |= BigUint::from(5000u32 + 111 * j as u32) << (LIMB_BITS as usize * j);
    }
    v.push(("dense".into(), dense % &m));
    // Directed rounding-tie operands: guard limb (limb L-2 of the product) at
    // exactly B/2 - 1, B/2, B/2 + 1 when crossed with `ulp` (X = 1 * this, so
    // limb L-2 of X is this value's limb L-2). Places the round-half-up
    // boundary precisely.
    let bh = BigUint::from(B / 2); // B/2
    let scale_lm2 = base().pow((l - 2) as u32); // B^(L-2)
    for (tag, k) in [("tie-", 1i64), ("tie0", 0), ("tie+", -1)] {
        // value = (B/2 + delta) * B^(L-2) so limb L-2 is exactly B/2 + delta.
        let mut val = (&bh) * &scale_lm2;
        if k > 0 {
            val += &scale_lm2 * BigUint::from(k as u32);
        } else if k < 0 {
            val -= &scale_lm2 * BigUint::from((-k) as u32);
        }
        v.push((format!("{tag}"), val % &m));
    }
    v
}

/// A deterministic pseudo-random magnitude (< B^L), xorshift64.
fn next_mag(s: &mut u64, l: usize) -> BigUint {
    let mut x = BigUint::from(0u32);
    for _ in 0..((l + 3) / 4) {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        x = (x << 64) | BigUint::from(*s);
    }
    x % modulus(l)
}

fn run_binops_for_l(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    l: usize,
) {
    let bases = base_mags(l);
    // Full magnitude cross, all four sign combos.
    let signs = [(0u32, 0u32), (0, 1), (1, 0), (1, 1)];
    let mut n_cases = 0usize;
    for (na, ma) in &bases {
        for (nb, mb) in &bases {
            for (sa, sb) in signs {
                let a = fx(sa, ma.clone());
                let b = fx(sb, mb.clone());
                for (op, want) in [
                    ("mul", ref_mul(&a, &b, l)),
                    ("add", ref_add(&a, &b, l)),
                    ("sub", ref_sub(&a, &b, l)),
                ] {
                    let params = bin_params(&a, &b, l);
                    let got = fe_result(store, instance, op, l, &params);
                    assert_fx_eq(
                        &got,
                        &want,
                        &format!("L{l} {op}({na}[s{sa}] , {nb}[s{sb}])"),
                    );
                    n_cases += 1;
                }
                // sqr uses a single operand (a); check against ref_mul(a,a) and
                // that fixed_sqr == the exact reference.
                let sqr_params: Vec<i32> = {
                    let mut p = vec![a.sign as i32];
                    p.extend(to_limbs(&a.mag, l).iter().map(|&x| x as i32));
                    p
                };
                let got_sqr = fe_result(store, instance, "sqr", l, &sqr_params);
                let want_sqr = ref_mul(&a, &a, l);
                assert_fx_eq(&got_sqr, &want_sqr, &format!("L{l} sqr({na}[s{sa}])"));
                // Internal gate: sqr(a) == mul(a,a) bit-for-bit (locks a future
                // dedicated SqrRows against the mul path).
                let aa_params = bin_params(&a, &a, l);
                let got_mul_aa = fe_result(store, instance, "mul", l, &aa_params);
                assert_fx_eq(
                    &got_sqr,
                    &got_mul_aa,
                    &format!("L{l} internal-gate sqr({na})==mul({na},{na})"),
                );
                n_cases += 2;
            }
        }
    }
    // Random pairs with random signs.
    let mut seed: u64 = 0xC0FF_EE15_5EED_1234 ^ (l as u64);
    for idx in 0..64 {
        let ma = next_mag(&mut seed, l);
        let sa = (seed & 1) as u32;
        let mb = next_mag(&mut seed, l);
        let sb = ((seed >> 1) & 1) as u32;
        let a = fx(sa, ma);
        let b = fx(sb, mb);
        for (op, want) in [
            ("mul", ref_mul(&a, &b, l)),
            ("add", ref_add(&a, &b, l)),
            ("sub", ref_sub(&a, &b, l)),
        ] {
            let params = bin_params(&a, &b, l);
            let got = fe_result(store, instance, op, l, &params);
            assert_fx_eq(&got, &want, &format!("L{l} {op} rand{idx}"));
            n_cases += 1;
        }
    }
    eprintln!("  L{l}: {n_cases} bit-identical mul/add/sub/sqr checks green.");
}

fn run_escape_for_l(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    l: usize,
) {
    let s = scale(l);
    // (name, cx, cy): the antenna tip c=(-2,0) must NOT escape (strict |z|^2>4);
    // an escaping point; an interior point; a boundary-adjacent center.
    let neg = |k: u32, sign: u32| -> Fx { fx(sign, &s * k) };
    let frac = |num: u32, den: u32, sign: u32| -> Fx { fx(sign, (&s * num) / den) };
    let centers: Vec<(String, Fx, Fx)> = vec![
        ("antenna(-2,0)".into(), neg(2, 1), neg(0, 0)),
        ("escape(1,1)".into(), neg(1, 0), neg(1, 0)),
        ("interior(-0.5,0)".into(), frac(1, 2, 1), neg(0, 0)),
        // boundary-adjacent: c ~ (-0.75, 0.1), the seahorse-valley neck.
        ("boundary(-0.75,0.1)".into(), frac(3, 4, 1), frac(1, 10, 0)),
        ("mini(0.28,0.53)".into(), frac(28, 100, 0), frac(53, 100, 0)),
    ];
    let mut n = 0usize;
    for max_iter in [32i32, 256] {
        for (name, cx, cy) in &centers {
            let mut params = vec![cx.sign as i32];
            params.extend(to_limbs(&cx.mag, l).iter().map(|&x| x as i32));
            params.push(cy.sign as i32);
            params.extend(to_limbs(&cy.mag, l).iter().map(|&x| x as i32));
            params.push(max_iter);
            let got = call_i32(store, instance, &format!("fixed_escape_l{l}"), &params);
            let want = ref_escape(cx, cy, max_iter, l);
            assert_eq!(
                got, want,
                "L{l} escape {name} max_iter={max_iter}: wasm count {got} != reference {want}"
            );
            n += 1;
        }
    }
    // The antenna point specifically must run the full budget (never escapes).
    assert_eq!(
        ref_escape(&neg(2, 1), &neg(0, 0), 256, l),
        256,
        "L{l}: antenna c=(-2,0) must never escape (count == max_iter)"
    );
    eprintln!("  L{l}: {n} escape iteration-count equalities green (incl antenna non-escape).");
}

#[test]
fn fixed_mul_add_sub_sqr_and_escape_match_bigint_reference() {
    let (wasm, elapsed) = compile_fixed_gate_ingot_to_wasm();
    eprintln!(
        "Fixed<L> gate ingot (L=2, 4, 6, 8) compiled to wasm in {:?} ({:.3}s), {} bytes.",
        elapsed,
        elapsed.as_secs_f64(),
        wasm.len()
    );
    let (mut store, instance) = instantiate(&wasm);
    // L=2 and L=6 are the tiers the adaptive mandelbrot escape kernel newly
    // selects among (alongside the original L=4 and L=8); every tier the demo
    // can land on is oracle-proven bit-identical here.
    for l in [2usize, 4, 6, 8] {
        run_binops_for_l(&mut store, &instance, l);
        run_escape_for_l(&mut store, &instance, l);
    }
    eprintln!(
        "Fixed<L>::{{mul,add,sub,sqr,escape}} == independent num-bigint fixed-point reference, \
         limb-for-limb, at L=2, 4, 6 and 8 (edges, directed rounding ties, wrap, signs, 64 randoms, \
         escape iteration counts incl the antenna non-escape)."
    );
}
