//! Bit-identical oracle gate for the precision axis P2 (linchpin slice):
//! `precision::field::mul` at the BN254 Fr instantiation
//! (`precision::bn254_fr::Bn254Fr`, `L=20`, `LIMB_BITS=13`) is compared,
//! LIMB FOR LIMB, against the existing hand/gen-scripted BN254 Fr field-mul
//! kernels (PRECISION_TYPES_RESEARCH.md P2 gate; ROLLCALL_GOAL.md "Precision
//! axis": "`Field<p>` proven == both BN254 fixtures limb-for-limb, retiring
//! gen_field_mul.py and unifying the 4-backend prover"). `Field<p>` is both
//! the prover's element type and the gate for the Conal Merkle slice.
//!
//! THREE witnesses, all compiled to wasm and run under wasmtime, over the
//! SAME operand set:
//!   1. `field_bn254fr_mul_limb{0..19}` -- the general form, `precision::
//!      field::mul::<20, Bn254Fr>`, wrapped per-limb (`precision_field_
//!      bn254fr_oracle_ingot`, this test's own fixture; see that file's doc
//!      for why per-limb wrappers instead of a `k`-indexed export).
//!   2. `field_mul_bn254_fr` -- the EXISTING fully-unrolled, `gen_field_mul.
//!      py`-generated kernel (`crates/codegen/tests/fixtures/spirv/
//!      field_mul_bn254_fr.fe`), reused verbatim via `include_str!` (not
//!      transcribed: this fixture is already independent and already
//!      oracle-proven elsewhere in this suite, so re-including it carries
//!      zero transcription-drift risk).
//!   3. `field_mul_bn254_fr_loop` -- the EXISTING rolled/loop-form twin
//!      (`field_mul_bn254_fr_loop.fe`), same reuse.
//!   4. An INDEPENDENT num-bigint Montgomery oracle (`a*b*R^-1 mod p`,
//!      structurally unrelated to CIOS), anchoring all three.
//!
//! Operand coverage: 0, 1, 2, p-1, p-2, (p-1)/2, a near-2^256 value (2^256-1
//! reduced mod p), the dense all-limbs-saturated value, and the Montgomery
//! anchors R/R^2, crossed pairwise, plus deterministic pseudo-random pairs.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use num_bigint::BigUint;
use std::path::Path;
use url::Url;

const LIMB_BITS: usize = 13;
const N: usize = 20;

const FIELD_MUL_UNROLLED_SRC: &str = include_str!("fixtures/spirv/field_mul_bn254_fr.fe");
const FIELD_MUL_LOOP_SRC: &str = include_str!("fixtures/spirv/field_mul_bn254_fr_loop.fe");

/// BN254 (alt_bn128) scalar field order Fr, parsed from decimal.
fn bn254_fr_prime() -> BigUint {
    BigUint::parse_bytes(
        b"21888242871839275222246405745257275088548364400416034343698204186575808495617",
        10,
    )
    .expect("BN254 Fr decimal should parse")
}

fn to_limbs(x: &BigUint, n: usize) -> Vec<u32> {
    let mask = BigUint::from(8191u32);
    (0..n)
        .map(|j| {
            let limb = (x >> (LIMB_BITS * j)) & &mask;
            limb.to_u32_digits().first().copied().unwrap_or(0)
        })
        .collect()
}

/// The INDEPENDENT bigint oracle: `a*b*R^-1 mod p` via num-bigint (which
/// knows nothing of 13-bit limbs or CIOS), decomposed into `n` limbs.
fn mont_oracle(a: &BigUint, b: &BigUint, p: &BigUint, n: usize) -> Vec<u32> {
    let r = BigUint::from(1u32) << (LIMB_BITS * n);
    let rinv = r.modpow(&(p - BigUint::from(2u32)), p);
    let mont = (((a * b) % p) * &rinv) % p;
    to_limbs(&mont, n)
}

/// A deterministic pseudo-random field element (xorshift64, no rand dep).
fn next_field(s: &mut u64, p: &BigUint) -> BigUint {
    let mut x = BigUint::from(0u32);
    for _ in 0..5 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        x = (x << 64) | BigUint::from(*s);
    }
    x % p
}

/// Compile a Fe source string to wasm bytecode through `BackendKind::Wasm`.
fn compile_source_to_wasm(source: &str, tag: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{tag}.fe")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .unwrap_or_else(|err| panic!("wasm compilation of `{tag}` failed: {err}"))
        .into_bytecode()
        .expect("wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("produced invalid wasm");
    bytes
}

/// Compile the `precision_field_bn254fr_oracle_ingot` fixture (the general
/// `Field<p>` form, wrapped per-limb) to wasm.
fn compile_field_gate_ingot_to_wasm() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/precision_field_bn254fr_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "precision Field<p>/BN254 Fr oracle gate ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("precision Field<p>/BN254 Fr oracle gate ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected gate-ingot diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("precision/BN254 Fr oracle gate ingot should compile to wasm")
        .into_bytecode()
        .expect("wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("gate ingot wasm should validate");
    bytes
}

fn instantiate(bytes: &[u8]) -> (wasmtime::Store<()>, wasmtime::Instance) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    (store, instance)
}

/// Execute a `(k, row, a0..a19, b0..b19) -> u32` kernel (the existing
/// unrolled/loop reference ABI) over all `n` limb indices for a single
/// `(a, b)`. Past wasmtime's typed-tuple arity, so the untyped `Func::call`
/// path is used.
fn reference_field_mul_limbs(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    fn_name: &str,
    a_limbs: &[u32],
    b_limbs: &[u32],
    n: usize,
) -> Vec<u32> {
    use wasmtime::Val;
    let f = instance
        .get_func(&mut *store, fn_name)
        .unwrap_or_else(|| panic!("`{fn_name}` export should exist"));
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let mut params: Vec<Val> = Vec::with_capacity(2 + 2 * n);
        params.push(Val::I32(k as i32));
        params.push(Val::I32(0));
        for &l in a_limbs {
            params.push(Val::I32(l as i32));
        }
        for &l in b_limbs {
            params.push(Val::I32(l as i32));
        }
        let mut results = [Val::I32(0)];
        f.call(&mut *store, &params, &mut results)
            .unwrap_or_else(|e| panic!("{fn_name}(k={k}) should run: {e:?}"));
        out.push(match results[0] {
            Val::I32(v) => v as u32,
            other => panic!("{fn_name} result must be i32, got {other:?}"),
        });
    }
    out
}

/// Execute the general `Field<p>` gate ingot's per-limb export wrappers
/// (`field_bn254fr_mul_limb{k}(a0..a19,b0..b19) -> u32`, no `k` argument --
/// see the fixture's own doc for why) over all `n` limb indices for a single
/// `(a, b)`.
fn field_mul_limbs(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    a_limbs: &[u32],
    b_limbs: &[u32],
    n: usize,
) -> Vec<u32> {
    use wasmtime::Val;
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let fn_name = format!("field_bn254fr_mul_limb{k}");
        let f = instance
            .get_func(&mut *store, &fn_name)
            .unwrap_or_else(|| panic!("`{fn_name}` export should exist"));
        let mut params: Vec<Val> = Vec::with_capacity(2 * n);
        for &l in a_limbs {
            params.push(Val::I32(l as i32));
        }
        for &l in b_limbs {
            params.push(Val::I32(l as i32));
        }
        let mut results = [Val::I32(0)];
        f.call(&mut *store, &params, &mut results)
            .unwrap_or_else(|e| panic!("{fn_name}(...) should run: {e:?}"));
        out.push(match results[0] {
            Val::I32(v) => v as u32,
            other => panic!("{fn_name} result must be i32, got {other:?}"),
        });
    }
    out
}

/// THE GATE: `Field<p>::mul` at BN254 Fr matches BOTH existing kernels AND
/// the independent num-bigint Montgomery oracle, limb for limb, over
/// representative + edge operand pairs.
#[test]
fn diag_compile_field_gate_ingot_only() {
    let t0 = std::time::Instant::now();
    let bytes = compile_field_gate_ingot_to_wasm();
    eprintln!(
        "diag: compiled field gate ingot to wasm in {:?}, {} bytes",
        t0.elapsed(),
        bytes.len()
    );
}

#[test]
fn field_mul_matches_both_bn254_fr_kernels_and_bigint_oracle() {
    let p = bn254_fr_prime();
    let n = N;
    let one = BigUint::from(1u32);
    let two = BigUint::from(2u32);

    let mut edges: Vec<(String, BigUint)> = vec![
        ("0".into(), BigUint::from(0u32)),
        ("1".into(), one.clone()),
        ("2".into(), two.clone()),
        ("p-1".into(), &p - &one),
        ("p-2".into(), &p - &two),
        ("(p-1)/2".into(), (&p - &one) / &two),
    ];
    let mut dense = BigUint::from(0u32);
    for j in 0..n {
        dense |= BigUint::from(8191u32) << (LIMB_BITS * j);
    }
    edges.push(("dense".into(), &dense % &p));
    let r = BigUint::from(1u32) << (LIMB_BITS * n);
    edges.push(("R".into(), &r % &p));
    edges.push(("R^2".into(), (&r * &r) % &p));
    let near_2_256 = (BigUint::from(1u32) << 256u32) - &one;
    edges.push(("near_2^256".into(), &near_2_256 % &p));

    let mut products: Vec<(String, BigUint, BigUint)> = Vec::new();
    for (na, a) in &edges {
        for (nb, b) in &edges {
            products.push((format!("{na} x {nb}"), a.clone(), b.clone()));
        }
    }
    let mut seed: u64 = 0xC0FF_EE15_5EED_1234;
    for idx in 0..64 {
        let a = next_field(&mut seed, &p);
        let b = next_field(&mut seed, &p);
        products.push((format!("rand{idx}"), a, b));
    }

    let cases: Vec<(String, Vec<u32>, Vec<u32>, Vec<u32>)> = products
        .iter()
        .map(|(name, a, b)| {
            (
                name.clone(),
                to_limbs(a, n),
                to_limbs(b, n),
                mont_oracle(a, b, &p, n),
            )
        })
        .collect();

    let unrolled_wasm = compile_source_to_wasm(FIELD_MUL_UNROLLED_SRC, "field_mul_bn254_fr");
    let loop_wasm = compile_source_to_wasm(FIELD_MUL_LOOP_SRC, "field_mul_bn254_fr_loop");
    let field_wasm = compile_field_gate_ingot_to_wasm();

    let (mut unrolled_store, unrolled_instance) = instantiate(&unrolled_wasm);
    let (mut loop_store, loop_instance) = instantiate(&loop_wasm);
    let (mut field_store, field_instance) = instantiate(&field_wasm);

    for (name, al, bl, oracle) in &cases {
        let got_unrolled = reference_field_mul_limbs(
            &mut unrolled_store,
            &unrolled_instance,
            "field_mul_bn254_fr",
            al,
            bl,
            n,
        );
        let got_loop = reference_field_mul_limbs(
            &mut loop_store,
            &loop_instance,
            "field_mul_bn254_fr_loop",
            al,
            bl,
            n,
        );
        let got_field = field_mul_limbs(&mut field_store, &field_instance, al, bl, n);

        assert_eq!(
            &got_unrolled, oracle,
            "sanity: unrolled field_mul_bn254_fr({name}) must equal the num-bigint oracle"
        );
        assert_eq!(
            &got_loop, oracle,
            "sanity: loop-form field_mul_bn254_fr_loop({name}) must equal the num-bigint oracle"
        );
        assert_eq!(
            &got_field, oracle,
            "Field<p>::mul::<20, Bn254Fr>({name}) must equal the num-bigint Montgomery oracle \
             a*b*R^-1 mod p"
        );
        assert_eq!(
            &got_field, &got_unrolled,
            "Field<p>::mul::<20, Bn254Fr>({name}) must be BIT-IDENTICAL, limb for limb, to the \
             existing unrolled field_mul_bn254_fr kernel"
        );
        assert_eq!(
            &got_field, &got_loop,
            "Field<p>::mul::<20, Bn254Fr>({name}) must be BIT-IDENTICAL, limb for limb, to the \
             existing loop-form field_mul_bn254_fr_loop kernel"
        );
    }

    eprintln!(
        "  Field<p>::mul::<20, Bn254Fr> == field_mul_bn254_fr == field_mul_bn254_fr_loop == \
         num-bigint Montgomery oracle, limb-for-limb, over {} operand products (incl p-1 x p-1, \
         dense-limb, R, R^2, near-2^256).",
        cases.len()
    );
}
