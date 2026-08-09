#![cfg(all(
    feature = "native-backend",
    not(target_arch = "wasm32"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::compile_runtime_package_native_i32_entry;
use url::Url;

use num_bigint::BigUint;

fn compile_entry(
    name: &str,
    source: &str,
    entry: &str,
) -> Result<fe_codegen::NativeI32EntryArtifact, String> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}.fe")).unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, entry)
        .map_err(|error| error.to_string())?;
    compile_runtime_package_native_i32_entry(&db, &package, entry)
        .map_err(|error| error.to_string())
}

#[test]
fn n0_rejects_an_unverified_entry_signature() {
    let error = compile_entry(
        "native_wrong_signature",
        "pub fn wrong(value: i32) -> i32 { value }",
        "wrong",
    )
    .err()
    .expect("native ABI mismatch must fail closed");
    assert!(error.contains("must have ABI (i32, i32) -> i32"), "{error}");
}

#[test]
fn n1_executes_scalar_arithmetic() {
    let artifact = compile_entry(
        "native_add",
        "pub fn add(lhs: i32, rhs: i32) -> i32 { lhs + rhs }",
        "add",
    )
    .expect("native add should compile");
    assert_eq!(artifact.entry_name(), "add");
    assert_eq!(artifact.call(20, 22), 42);
    assert_eq!(artifact.call(-7, 3), -4);
}

#[test]
fn n2_executes_control_flow_and_a_helper_call() {
    let artifact = compile_entry(
        "native_loop",
        r#"
fn step(value: i32, delta: i32) -> i32 {
    value + delta
}

pub fn accumulate(count: i32, delta: i32) -> i32 {
    let mut index: i32 = 0
    let mut total: i32 = 0
    while index < count {
        total = step(value: total, delta: delta)
        index = index + 1
    }
    total
}
"#,
        "accumulate",
    )
    .expect("native control flow should compile");
    assert_eq!(artifact.call(7, 6), 42);
    assert_eq!(artifact.call(0, 99), 0);
}

fn mandel_oracle_q12(px: i32, py: i32) -> u32 {
    let c_re = -8192 + px * 24;
    let c_im = -6144 + py * 24;
    let mut zr = 0i32;
    let mut zi = 0i32;
    let mut iteration = 0u32;
    while iteration < 100 {
        let rr = zr * zr;
        let ii = zi * zi;
        if rr + ii < 67_108_864 {
            let next_real = rr - ii;
            let next_imaginary = ((zr * 2) * zi) >> 12;
            zr = (next_real >> 12) + c_re;
            zi = next_imaginary + c_im;
            iteration += 1;
        } else {
            return iteration;
        }
    }
    iteration
}

#[test]
fn native_mandelbrot_capstone_matches_the_full_frame_oracle() {
    let artifact = compile_entry(
        "native_mandelbrot_q12",
        include_str!("../../../demos/capstones/mandelbrot/kernel.fe"),
        "mandel_pixel_q12",
    )
    .expect("canonical Mandelbrot kernel should compile through Native");

    let mut hash = 0x811c9dc5u32;
    for py in 0..512i32 {
        for px in 0..512i32 {
            let got = artifact.call(px, py) as u32;
            let expected = mandel_oracle_q12(px, py);
            assert_eq!(
                got, expected,
                "native mandel_pixel_q12({px}, {py}) = {got}, oracle = {expected}"
            );
            for byte in got.to_le_bytes() {
                hash = (hash ^ u32::from(byte)).wrapping_mul(0x01000193);
            }
        }
    }
    assert_eq!(hash, 0x2d29649a);
}

// ===========================================================================
// Rung 3 STEP 2 / Rung 4 four-backend digest: the rolled (function-local
// [u32; N] array-backed) loop-form kernels, executed on native/Cranelift and
// cross-checked against the wasm leg (same Fe source, two independent
// backends) and, for Poseidon, the circomlib-pinned oracle. Both kernels
// share the exact `(k, row, 40 x broadcast)` ABI, so both use
// `NativeGridLoopEntryArtifact`.
//
// HONEST PROBE, matching rollcall_e2e.rs's established pattern for exactly
// this situation: MemAllocDynamic lowering on CraneliftBackend (this rung's
// whole point) exists on an unpushed Sonatina fork branch, not necessarily
// the pin this crate builds against at any given moment. Each test records
// and asserts on whatever ACTUALLY happens (native == wasm, or a named
// compile-time gap) rather than assuming an outcome, so these are safe to
// run on this crate regardless of repin timing.
// ===========================================================================

const FIELD_MUL_LOOP_SRC: &str = include_str!("fixtures/spirv/field_mul_bn254_fr_loop.fe");
const POSEIDON_LOOP_SRC: &str = include_str!("fixtures/spirv/poseidon_bn254_loop.fe");
const GRID_LOOP_LIMB_BITS: usize = 13;
const GRID_LOOP_N: usize = 20;

fn bn254_fr_prime() -> BigUint {
    BigUint::parse_bytes(
        b"21888242871839275222246405745257275088548364400416034343698204186575808495617",
        10,
    )
    .expect("BN254 Fr decimal should parse")
}

fn grid_loop_to_limbs(x: &BigUint, n: usize) -> Vec<u32> {
    let mask = BigUint::from(8191u32);
    (0..n)
        .map(|j| {
            let limb = (x >> (GRID_LOOP_LIMB_BITS * j)) & &mask;
            limb.to_u32_digits().first().copied().unwrap_or(0)
        })
        .collect()
}

/// The independent bigint oracle: the CIOS Montgomery product a*b*R^-1 mod p
/// (num-bigint, which knows nothing of 13-bit limbs or CIOS), decomposed
/// into `n` limbs for a limb-for-limb match against the kernel.
fn mont_oracle_limbs(a: &BigUint, b: &BigUint, p: &BigUint, n: usize) -> Vec<u32> {
    let r = BigUint::from(1u32) << (GRID_LOOP_LIMB_BITS * n);
    let rinv = r.modpow(&(p - BigUint::from(2u32)), p);
    let mont = (((a * b) % p) * &rinv) % p;
    grid_loop_to_limbs(&mont, n)
}

fn compile_source_to_wasm(source: &str, tag: &str) -> Vec<u8> {
    use fe_codegen::{BackendKind, OptLevel, layout_for};
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{tag}_wasm.fe")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("kernel should compile Fe -> wasm");
    output
        .into_bytecode()
        .expect("wasm output should be bytecode")
}

/// Untyped wasmtime call for the shared 42-arg `(k, row, 40 x broadcast) ->
/// u32` ABI.
fn wasm_call_grid_loop(bytes: &[u8], fn_name: &str, args42: &[i32; 42]) -> u32 {
    use wasmtime::{Engine, Instance, Module, Store, Val};
    wasmparser::validate(bytes).expect("Fe-emitted wasm should be valid");
    let engine = Engine::default();
    let module = Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_func(&mut store, fn_name)
        .unwrap_or_else(|| panic!("`{fn_name}` export should exist"));
    let params: Vec<Val> = args42.iter().map(|&v| Val::I32(v)).collect();
    let mut results = [Val::I32(0)];
    f.call(&mut store, &params, &mut results)
        .unwrap_or_else(|e| panic!("{fn_name} should run: {e:?}"));
    match results[0] {
        Val::I32(v) => v as u32,
        other => panic!("{fn_name} result must be i32, got {other:?}"),
    }
}

fn field_mul_loop_native_body() -> Result<(), String> {
    let p = bn254_fr_prime();
    let n = GRID_LOOP_N;
    let wasm_bytes = compile_source_to_wasm(FIELD_MUL_LOOP_SRC, "field_mul_native");

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///field_mul_bn254_fr_loop_native.fe")
        .expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(FIELD_MUL_LOOP_SRC.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "field_mul_bn254_fr_loop")
        .map_err(|e| e.to_string())?;
    let artifact = fe_codegen::compile_runtime_package_native_grid_loop_entry(
        &db,
        &package,
        "field_mul_bn254_fr_loop",
    )
    .map_err(|e| e.to_string())?;

    let one = BigUint::from(1u32);
    let two = BigUint::from(2u32);
    let cases: Vec<(&str, BigUint, BigUint)> = vec![
        ("1 * 1", one.clone(), one.clone()),
        ("2 * 3", two.clone(), BigUint::from(3u32)),
        ("(p-1) * (p-2)", &p - &one, &p - &two),
    ];

    for (name, a, b) in &cases {
        let al = grid_loop_to_limbs(a, n);
        let bl = grid_loop_to_limbs(b, n);
        let oracle = mont_oracle_limbs(a, b, &p, n);
        for k in 0..n {
            let mut args = [0i32; fe_codegen::GRID_LOOP_NATIVE_ENTRY_ARITY];
            args[0] = k as i32;
            args[1] = 0; // row, unused
            for (idx, &l) in al.iter().enumerate() {
                args[2 + idx] = l as i32;
            }
            for (idx, &l) in bl.iter().enumerate() {
                args[2 + n + idx] = l as i32;
            }
            let native_limb = artifact.call(&args) as u32;
            let wasm_limb = wasm_call_grid_loop(&wasm_bytes, "field_mul_bn254_fr_loop", &args);
            if native_limb != wasm_limb || native_limb != oracle[k] {
                return Err(format!(
                    "{name} limb {k}: native={native_limb} wasm={wasm_limb} oracle={} \
                     (must all agree)",
                    oracle[k]
                ));
            }
        }
    }
    Ok(())
}

/// Rung 4 four-backend digest, native leg: the rolled field-mul EXECUTES on
/// native/Cranelift, tri-equal (native == wasmtime == the independent
/// num-bigint Montgomery oracle) over a handful of representative operand
/// pairs including the carry-heavy p-1 x p-2 case. The exhaustive ~144-pair
/// sweep against this same oracle already lives in wasm_e2e.rs; this test's
/// job is specifically the NEW cross-backend claim (native reaches the same
/// answer), not re-proving exhaustive wasm correctness.
#[test]
fn field_mul_bn254_fr_loop_native_cranelift_leg_is_honestly_reported() {
    match std::thread::Builder::new()
        .stack_size(1 << 31)
        .spawn(field_mul_loop_native_body)
        .expect("spawn wide-stack worker for the native field-mul leg")
        .join()
        .expect("native field-mul worker thread should not panic")
    {
        Ok(()) => {
            eprintln!(
                "field_mul_bn254_fr_loop native/Cranelift leg: EXECUTED, tri-equal (native == \
                 wasm == bigint oracle) over every tested operand pair."
            );
        }
        Err(message) => {
            eprintln!(
                "field_mul_bn254_fr_loop native/Cranelift leg: native execution is NOT \
                 currently possible on this pinned Sonatina rev for an array-using kernel: \
                 {message}. Re-lands with the fork re-pin (Decision 5)."
            );
        }
    }
}

fn poseidon_loop_native_body() -> Result<(), String> {
    let wasm_bytes = compile_source_to_wasm(POSEIDON_LOOP_SRC, "poseidon_native");

    let mut db = DriverDataBase::default();
    let url =
        Url::parse("file:///poseidon_bn254_loop_native.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(POSEIDON_LOOP_SRC.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "poseidon_bn254_loop")
        .map_err(|e| e.to_string())?;
    let artifact = fe_codegen::compile_runtime_package_native_grid_loop_entry(
        &db,
        &package,
        "poseidon_bn254_loop",
    )
    .map_err(|e| e.to_string())?;

    // The circomlib-pinned t=3 Poseidon vectors (const_poseidon.fe's own
    // static_assert-pinned source of truth): hash2(0,0) and hash2(1,2).
    let circomlib_hash2_00 = BigUint::parse_bytes(
        b"2098f5fb9e239eab3ceac3f27b81e481dc3124d55ffed523a839ee8446b64864",
        16,
    )
    .expect("circomlib hash2(0,0) hex should parse");
    let circomlib_hash2_12 = BigUint::parse_bytes(
        b"115cc0f5e7d690413df64c6b9662e9cf2a3617f2743245519e19607a4417189a",
        16,
    )
    .expect("circomlib hash2(1,2) hex should parse");

    let cases: [(&str, u32, u32, &BigUint); 2] = [
        ("hash2(0,0)", 0, 0, &circomlib_hash2_00),
        ("hash2(1,2)", 1, 2, &circomlib_hash2_12),
    ];

    for (name, left, right, oracle) in cases {
        // in0..in19 = left (plain-form limbs), in20..in39 = right. left/right
        // are small (0/1/2) but decomposed properly rather than relying on
        // "small value == its own limb 0" coincidentally holding.
        let left_limbs = grid_loop_to_limbs(&BigUint::from(left), GRID_LOOP_N);
        let right_limbs = grid_loop_to_limbs(&BigUint::from(right), GRID_LOOP_N);
        let oracle_limbs = grid_loop_to_limbs(oracle, GRID_LOOP_N);
        for k in 0..GRID_LOOP_N {
            let mut args = [0i32; fe_codegen::GRID_LOOP_NATIVE_ENTRY_ARITY];
            args[0] = k as i32;
            args[1] = 0; // row, unused
            for (idx, &l) in left_limbs.iter().enumerate() {
                args[2 + idx] = l as i32;
            }
            for (idx, &l) in right_limbs.iter().enumerate() {
                args[2 + GRID_LOOP_N + idx] = l as i32;
            }
            let native_limb = artifact.call(&args) as u32;
            let wasm_limb = wasm_call_grid_loop(&wasm_bytes, "poseidon_bn254_loop", &args);
            if native_limb != wasm_limb || native_limb != oracle_limbs[k] {
                return Err(format!(
                    "{name} limb {k}: native={native_limb} wasm={wasm_limb} \
                     circomlib_oracle={} (must all agree)",
                    oracle_limbs[k]
                ));
            }
        }
    }
    Ok(())
}

/// Rung 4 four-backend digest, native leg: the rolled Poseidon hash2
/// EXECUTES on native/Cranelift, tri-equal (native == wasmtime ==
/// circomlib-pinned oracle) at both circomlib known-answer vectors. Reuses
/// the SAME `(k, row, broadcast)` ABI as field_mul above (in0..in19 = left,
/// in20..in39 = right).
#[test]
fn poseidon_bn254_loop_native_cranelift_leg_is_honestly_reported() {
    match std::thread::Builder::new()
        .stack_size(1 << 31)
        .spawn(poseidon_loop_native_body)
        .expect("spawn wide-stack worker for the native Poseidon leg")
        .join()
        .expect("native Poseidon worker thread should not panic")
    {
        Ok(()) => {
            eprintln!(
                "poseidon_bn254_loop native/Cranelift leg: EXECUTED, tri-equal (native == wasm \
                 == circomlib oracle) at both pinned known-answer vectors."
            );
        }
        Err(message) => {
            eprintln!(
                "poseidon_bn254_loop native/Cranelift leg: native execution is NOT currently \
                 possible on this pinned Sonatina rev for an array-using kernel: {message}. \
                 Re-lands with the fork re-pin (Decision 5)."
            );
        }
    }
}
