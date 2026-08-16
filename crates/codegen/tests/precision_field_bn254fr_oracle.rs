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
/// The SPIR-V MSM slice's standalone 4-limb kernel for the 51-bit probe prime
/// (`p = 2^51 - 129`), independently generated, reused verbatim as a second
/// reference witness for the loop-form generality gate (see the L=4 test).
const FIELD_MUL_PROBE51_SRC: &str = include_str!("fixtures/spirv/field_mul_probe.fe");

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

fn from_limbs(words: &[u32]) -> BigUint {
    words
        .iter()
        .enumerate()
        .fold(BigUint::from(0u32), |value, (index, word)| {
            value | (BigUint::from(*word) << (LIMB_BITS * index))
        })
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

fn bn254_roundtrip_u32(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    value: u32,
) -> Vec<u32> {
    use wasmtime::Val;
    let function = instance
        .get_func(&mut *store, "field_bn254fr_roundtrip_u32")
        .expect("`field_bn254fr_roundtrip_u32` export should exist");
    let mut results = vec![Val::I32(0); N];
    function
        .call(&mut *store, &[Val::I32(value as i32)], &mut results)
        .unwrap_or_else(|error| panic!("BN254 u32 roundtrip should run: {error:?}"));
    results
        .into_iter()
        .map(|result| match result {
            Val::I32(word) => word as u32,
            other => panic!("BN254 roundtrip result must be i32, got {other:?}"),
        })
        .collect()
}

fn bn254_tuple_words(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    args: &[u32],
) -> Vec<u32> {
    use wasmtime::Val;
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("`{name}` export should exist"));
    let params = args
        .iter()
        .copied()
        .map(|word| Val::I32(word as i32))
        .collect::<Vec<_>>();
    let mut results = vec![Val::I32(0); N];
    function
        .call(&mut *store, &params, &mut results)
        .unwrap_or_else(|error| panic!("{name} should run: {error:?}"));
    results
        .into_iter()
        .map(|result| match result {
            Val::I32(word) => word as u32,
            other => panic!("{name} result must be i32, got {other:?}"),
        })
        .collect()
}

#[test]
fn bn254_field_power_inverse_and_two_adic_roots_match_bigint_oracle() {
    let p = bn254_fr_prime();
    let one = BigUint::from(1u32);
    let p_minus_two = &p - BigUint::from(2u32);
    let root_exponent = (&p - &one) >> 28usize;
    let maximal_root = BigUint::from(5u32).modpow(&root_exponent, &p);
    let wasm = compile_field_gate_ingot_to_wasm();
    let (mut store, instance) = instantiate(&wasm);

    for (base, exponent) in [
        (0u32, 0u32),
        (0, 19),
        (1, u32::MAX),
        (2, 31),
        (5, 65_537),
        (u32::MAX, u32::MAX),
    ] {
        let expected = BigUint::from(base).modpow(&BigUint::from(exponent), &p);
        assert_eq!(
            bn254_tuple_words(
                &mut store,
                &instance,
                "field_bn254fr_pow_u32",
                &[base, exponent],
            ),
            to_limbs(&expected, N),
            "Fe square-and-multiply must match bigint for {base}^{exponent}",
        );
    }

    for value in [0u32, 1, 2, 5, 65_537, u32::MAX] {
        let expected = if value == 0 {
            BigUint::from(0u32)
        } else {
            BigUint::from(value).modpow(&p_minus_two, &p)
        };
        let inverse =
            bn254_tuple_words(&mut store, &instance, "field_bn254fr_inverse_u32", &[value]);
        assert_eq!(
            inverse,
            to_limbs(&expected, N),
            "Fe Fermat inverse must match bigint for {value}",
        );
        if value != 0 {
            assert_eq!(
                (BigUint::from(value) * from_limbs(&inverse)) % &p,
                one,
                "Fe inverse must be multiplicative for {value}",
            );
        }
    }

    for log_order in [0u32, 1, 2, 4, 8, 16, 28] {
        let expected = maximal_root.modpow(&(BigUint::from(1u32) << (28u32 - log_order)), &p);
        assert_eq!(
            bn254_tuple_words(
                &mut store,
                &instance,
                "field_bn254fr_two_adic_root",
                &[log_order],
            ),
            to_limbs(&expected, N),
            "Fe-derived 2^{log_order} root must match bigint derivation",
        );

        let order = 1u32 << log_order;
        assert_eq!(
            bn254_tuple_words(
                &mut store,
                &instance,
                "field_bn254fr_two_adic_root_power",
                &[log_order, order],
            ),
            to_limbs(&one, N),
            "root^{order} must equal one",
        );
        if log_order > 0 {
            assert_eq!(
                bn254_tuple_words(
                    &mut store,
                    &instance,
                    "field_bn254fr_two_adic_root_power",
                    &[log_order, order >> 1],
                ),
                to_limbs(&(&p - &one), N),
                "root^(order/2) must equal minus one",
            );
        }
    }

    assert_eq!(
        bn254_tuple_words(&mut store, &instance, "field_bn254fr_two_adic_root", &[29],),
        vec![0; N],
        "unsupported two-adic orders must fail closed",
    );
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

    for value in [0, 1, 8191, 8192, 65_535, u32::MAX] {
        assert_eq!(
            bn254_roundtrip_u32(&mut field_store, &field_instance, value),
            to_limbs(&BigUint::from(value), n),
            "BN254 u32 -> Montgomery -> plain roundtrip for {value}"
        );
    }

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

// ---------------------------------------------------------------------------
// SECOND-MODULUS generality gate (design section 4): the SAME general
// `precision::field::mul`, at L=4 with a NON-BN254 modulus (the 51-bit prime
// p = 2^51 - 129), is bit-identical to the independent 4-limb kernel
// `field_mul_probe.fe` AND to a num-bigint Montgomery oracle. A form that only
// works at L=20/BN254 cannot pass this.
// ---------------------------------------------------------------------------

const N4: usize = 4;

/// The 51-bit probe prime `p = 2^51 - 129` (matches `spirv_e2e.rs`'s
/// `probe_prime()` and `field_mul_probe.fe`'s header).
fn probe51_prime() -> BigUint {
    BigUint::from(2_251_799_813_685_119u64)
}

/// Compile the `precision_field_probe51_oracle_ingot` fixture (the general
/// `Field<p>` form at `mul::<4, ProbeP51>`, wrapped per-limb) to wasm.
fn compile_probe51_gate_ingot_to_wasm() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/precision_field_probe51_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "precision Field<p>/ProbeP51 (L=4) oracle gate ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("precision Field<p>/ProbeP51 oracle gate ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected probe51 gate-ingot diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("precision/ProbeP51 oracle gate ingot should compile to wasm")
        .into_bytecode()
        .expect("wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("probe51 gate ingot wasm should validate");
    bytes
}

#[test]
fn modulus_branded_field_element_spirv_leg_is_honestly_reported() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/precision_field_probe51_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "precision FieldElement SPIR-V gate ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("precision FieldElement SPIR-V gate ingot");
    let top_mod = ingot.root_mod(&db);
    let package =
        mir::build_wasm_runtime_package_for_entry(&db, top_mod, "probe51_words_mul_limb0")
            .expect("modulus-branded FieldElement should build a runtime package");
    match fe_codegen::compile_runtime_package_spirv_with_workgroup(&db, &package, [1, 1, 1]) {
        Ok(artifact) => {
            assert_eq!(artifact.words.first().copied(), Some(0x0723_0203));
            assert_eq!(
                artifact.layout.word,
                sonatina_codegen::isa::spirv::WordKind::U32
            );
            let wgsl = artifact.wgsl.expect("naga should emit browser WGSL");
            let module = naga::front::wgsl::parse_str(&wgsl).expect("WGSL should reparse");
            let mut validator = naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::default(),
            );
            validator
                .validate(&module)
                .expect("FieldElement WGSL should validate in the browser profile");
        }
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("residual call to `mul_words`"),
                "unexpected FieldElement SPIR-V failure: {message}"
            );
            assert!(
                message.contains("linkage=Private") && !message.contains("ALWAYSINLINE"),
                "the diagnostic must identify an ordinary private Fe helper: {message}"
            );
            eprintln!(
                "FieldElement SPIR-V leg is not yet available: the call-free shader lowering \
                 retained the array-returning Fe helper `mul_words`. Wasm semantics remain \
                 independently executed; generated call-free GPU kernels remain canonical."
            );
        }
    }
}

/// Execute the ProbeP51 gate ingot's per-limb wrappers
/// (`probe51_mul_limb{k}(a0..a3,b0..b3) -> u32`) over all `N4` limb indices.
fn probe51_field_mul_limbs(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    a_limbs: &[u32],
    b_limbs: &[u32],
    prefix: &str,
) -> Vec<u32> {
    use wasmtime::Val;
    let mut out = Vec::with_capacity(N4);
    for k in 0..N4 {
        let fn_name = format!("{prefix}_limb{k}");
        let f = instance
            .get_func(&mut *store, &fn_name)
            .unwrap_or_else(|| panic!("`{fn_name}` export should exist"));
        let mut params: Vec<Val> = Vec::with_capacity(2 * N4);
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

fn probe51_roundtrip_u32(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    value: u32,
) -> Vec<u32> {
    let mut out = Vec::with_capacity(N4);
    for k in 0..N4 {
        let fn_name = format!("probe51_roundtrip_u32_limb{k}");
        let function = instance
            .get_typed_func::<i32, i32>(&mut *store, &fn_name)
            .unwrap_or_else(|_| panic!("`{fn_name}` export should exist"));
        out.push(
            function
                .call(&mut *store, value as i32)
                .unwrap_or_else(|error| panic!("{fn_name}({value}) should run: {error:?}"))
                as u32,
        );
    }
    out
}

fn probe51_tuple_words(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    args: &[i32],
) -> Vec<u32> {
    use wasmtime::Val;
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("`{name}` export should exist"));
    let params = args.iter().copied().map(Val::I32).collect::<Vec<_>>();
    let mut results = vec![Val::I32(0); N4];
    function
        .call(&mut *store, &params, &mut results)
        .unwrap_or_else(|error| panic!("{name} should run: {error:?}"));
    results
        .into_iter()
        .map(|result| match result {
            Val::I32(word) => word as u32,
            other => panic!("{name} result must be i32, got {other:?}"),
        })
        .collect()
}

#[test]
fn field_mul_l4_second_modulus_matches_probe_kernel_and_bigint_oracle() {
    let p = probe51_prime();
    let n = N4;
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
    let near_2_52 = (BigUint::from(1u32) << 52u32) - &one;
    edges.push(("near_2^52".into(), &near_2_52 % &p));

    let mut products: Vec<(String, BigUint, BigUint)> = Vec::new();
    for (na, a) in &edges {
        for (nb, b) in &edges {
            products.push((format!("{na} x {nb}"), a.clone(), b.clone()));
        }
    }
    let mut seed: u64 = 0x5150_B1FF_1234_ABCD;
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

    let probe_wasm = compile_source_to_wasm(FIELD_MUL_PROBE51_SRC, "field_mul_probe");
    let field_wasm = compile_probe51_gate_ingot_to_wasm();

    let (mut probe_store, probe_instance) = instantiate(&probe_wasm);
    let (mut field_store, field_instance) = instantiate(&field_wasm);

    for (name, al, bl, oracle) in &cases {
        let got_probe = reference_field_mul_limbs(
            &mut probe_store,
            &probe_instance,
            "field_mul_probe",
            al,
            bl,
            n,
        );
        let got_field =
            probe51_field_mul_limbs(&mut field_store, &field_instance, al, bl, "probe51_mul");
        let got_words = probe51_field_mul_limbs(
            &mut field_store,
            &field_instance,
            al,
            bl,
            "probe51_words_mul",
        );
        let got_sum = probe51_field_mul_limbs(
            &mut field_store,
            &field_instance,
            al,
            bl,
            "probe51_words_add",
        );
        let a_value = from_limbs(al);
        let b_value = from_limbs(bl);
        let sum_oracle = to_limbs(&((&a_value + &b_value) % &p), n);
        let difference_oracle = to_limbs(&((&a_value + &p - &b_value) % &p), n);
        let negation_oracle = if a_value == BigUint::from(0u32) {
            vec![0; n]
        } else {
            to_limbs(&(&p - &a_value), n)
        };
        let squared = from_limbs(&mont_oracle(&a_value, &a_value, &p, n));
        let fourth = from_limbs(&mont_oracle(&squared, &squared, &p, n));
        let pow5_oracle = mont_oracle(&fourth, &a_value, &p, n);
        let binary_args = al
            .iter()
            .chain(bl)
            .map(|word| *word as i32)
            .collect::<Vec<_>>();
        let unary_args = al.iter().map(|word| *word as i32).collect::<Vec<_>>();
        let got_difference = probe51_tuple_words(
            &mut field_store,
            &field_instance,
            "probe51_words_sub",
            &binary_args,
        );
        let got_negation = probe51_tuple_words(
            &mut field_store,
            &field_instance,
            "probe51_words_neg",
            &unary_args,
        );
        let got_pow5 = probe51_tuple_words(
            &mut field_store,
            &field_instance,
            "probe51_words_pow5",
            &unary_args,
        );

        assert_eq!(
            &got_probe, oracle,
            "sanity: standalone field_mul_probe({name}) must equal the num-bigint oracle"
        );
        assert_eq!(
            &got_field, oracle,
            "Field<p>::mul::<4, ProbeP51>({name}) must equal the num-bigint Montgomery oracle \
             a*b*R^-1 mod p (p = 2^51 - 129)"
        );
        assert_eq!(
            &got_field, &got_probe,
            "Field<p>::mul::<4, ProbeP51>({name}) must be BIT-IDENTICAL, limb for limb, to the \
             independent 4-limb field_mul_probe kernel"
        );
        assert_eq!(
            &got_words, oracle,
            "FieldWords::mul_words::<4, ProbeP51>({name}) must equal the num-bigint Montgomery \
             oracle a*b*R^-1 mod p"
        );
        assert_eq!(
            &got_words, &got_field,
            "array-native and structural Field<p> boundaries must agree limb for limb for {name}"
        );
        assert_eq!(
            got_sum, sum_oracle,
            "modulus-branded FieldElement addition must equal (a+b) mod p for {name}"
        );
        assert_eq!(
            got_difference, difference_oracle,
            "modulus-branded FieldElement subtraction must equal (a-b) mod p for {name}"
        );
        assert_eq!(
            got_negation, negation_oracle,
            "modulus-branded FieldElement negation must equal -a mod p for {name}"
        );
        assert_eq!(
            got_pow5, pow5_oracle,
            "modulus-branded FieldElement pow5 must retain Montgomery form for {name}"
        );
    }

    for value in [0, 1, 8191, 8192, 65_535, u32::MAX] {
        assert_eq!(
            probe51_roundtrip_u32(&mut field_store, &field_instance, value),
            to_limbs(&BigUint::from(value), n),
            "u32 -> Montgomery -> plain roundtrip for {value}"
        );
    }

    for value in [i32::MIN, -65_535, -1, 0, 1, 65_535, i32::MAX] {
        let expected = if value < 0 {
            &p - BigUint::from(value.unsigned_abs())
        } else {
            BigUint::from(value as u32)
        };
        assert_eq!(
            probe51_tuple_words(
                &mut field_store,
                &field_instance,
                "probe51_signed_roundtrip",
                &[value],
            ),
            to_limbs(&expected, n),
            "signed i32 -> field -> plain roundtrip for {value}"
        );
    }

    eprintln!(
        "  Field<p>::mul and array-native mul_words at ProbeP51 (p = 2^51 - 129, NON-BN254) \
         == field_mul_probe == num-bigint Montgomery oracle, limb-for-limb, over {} operand \
         products: the general loop form is not a re-blessed BN254 kernel.",
        cases.len()
    );
}
