//! Deep-center oracle gate (precision axis, mandelbrot increment 1): a GPU-free
//! proof that the per-pixel coordinate `demos/sketches/mandelbrot`'s `escape`
//! feeds the `Fixed<8>` orbit now carries FULL `Fixed<8>` (~1e-27) precision,
//! past the old ~1e-11 df64 wall.
//!
//! It compiles the REAL general construction `precision::fixed::from_qf_offset<8>`
//! (the exact call `escape` makes: a four-word f32 float-expansion center plus a
//! per-pixel `u*zoom` offset, stitched into one `Fixed<8>` value) to wasm, runs
//! it under wasmtime, and asserts, from the emitted limbs alone (no
//! reimplementation of the numerics):
//!
//!   1. STRICT RESOLUTION. Across a full 512-pixel row at `zoom = 1e-16`, every
//!      pixel's `c_x` is a DISTINCT, strictly increasing `Fixed<8>` value. The
//!      deep render resolves every pixel; it does not mush.
//!
//!   2. df64 COULD NOT. Dropping the two DEEP words (`w2 = w3 = 0`), i.e. a
//!      two-word df64 center, shifts `c_x` by hundreds of per-pixel steps: the
//!      deep bits (~1e-16, below the df64 two-f32 floor) are un-representable in
//!      a df64 center, so a df64 renderer of the same view() parameters lands on
//!      an entirely different region. `Fixed<8>` holds them exactly.
//!
//! The Fixed<8> value of `(sign, limb0..limb7)` is `(-1)^sign * mag * 2^-F`
//! with `F = 13*(8-1) = 91` and `mag = sum_k limb_k * 8192^k` (limb 0 least
//! significant). Comparisons below are exact (BigInt over the limbs); the
//! per-pixel-step scaling is the only floating quantity.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use num_bigint::{BigInt, BigUint};
use std::path::Path;
use url::Url;

const LIMB_BITS: u32 = 13;
const L: usize = 8;
const F: u32 = LIMB_BITS * (L as u32 - 1); // 91 fractional bits

// The mandelbrot demo's own view() values (kept in sync with
// `demos/sketches/mandelbrot/src/lib.fe`: the Seahorse-Valley deep center and
// the default deep zoom). Parsed to f32 by the identical IEEE round-to-nearest
// rule the Fe compiler uses, so these are the same f32 words the shader sees.
const W0X: f32 = -0.7436438798904419;
const W1X: f32 = -0.0000000071467170;
const W2X: f32 = -0.0000000000000003; // deep word (~ -3e-16), below the df64 floor
const W3X: f32 = -0.00000000000000000000002; // deeper word (~ -2e-23)
const ZOOM: f32 = 0.0000000000000001; // 1e-16, far past the old ~1e-11 df64 wall
const RES: f32 = 512.0;

fn compile_gate_ingot_to_wasm() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/precision_deep_center_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "deep-center oracle gate ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("deep-center oracle gate ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected gate-ingot diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("deep-center oracle gate ingot should compile to wasm")
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

/// Call an `(f32 x5) -> u32` export.
fn call_u32(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    args: [f32; 5],
) -> u32 {
    use wasmtime::Val;
    let f = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("`{name}` export should exist"));
    let vals: Vec<Val> = args.iter().map(|&a| Val::F32(a.to_bits())).collect();
    let mut results = [Val::I32(0)];
    f.call(&mut *store, &vals, &mut results)
        .unwrap_or_else(|e| panic!("{name}(...) should run: {e:?}"));
    match results[0] {
        Val::I32(v) => v as u32,
        other => panic!("{name} result must be i32/u32, got {other:?}"),
    }
}

/// The signed `Fixed<8>` value (as an exact integer in units of 2^-91, i.e. the
/// signed magnitude) that `from_qf_offset<8>(w0, w1, w2, w3, off)` produces.
fn deep_center_value(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    w0: f32,
    w1: f32,
    w2: f32,
    w3: f32,
    off: f32,
) -> BigInt {
    let args = [w0, w1, w2, w3, off];
    let sign = call_u32(store, instance, "deep_center_l8_sign", args);
    // Non-overlapping limbs (each < 2^13, shifted by 13*k): OR == ADD.
    let mut mag = BigUint::from(0u32);
    for k in 0..L {
        let limb = call_u32(store, instance, &format!("deep_center_l8_limb{k}"), args);
        mag |= BigUint::from(limb) << (LIMB_BITS as usize * k);
    }
    let mag = BigInt::from(mag);
    if sign == 1 { -mag } else { mag }
}

/// The demo's own pixel -> `u` map, in f32 (matches `escape`:
/// `u = (px + 0.5) / res * 2 - 1`).
fn pixel_offset_x(px: i32, zoom: f32) -> f32 {
    let u = (px as f32 + 0.5) / RES * 2.0 - 1.0;
    u * zoom
}

#[test]
fn deep_center_holds_fixed8_precision_past_df64_wall() {
    let wasm = compile_gate_ingot_to_wasm();
    let (mut store, instance) = instantiate(&wasm);

    // -- (1) strict per-pixel resolution across a full deep row -------------
    // Every pixel's c_x is a DISTINCT, strictly increasing Fixed<8> value; the
    // deep render resolves the whole row rather than collapsing pixels.
    let zero = BigInt::from(0);
    let mut prev: Option<BigInt> = None;
    let mut min_gap: Option<BigInt> = None;
    let width = RES as i32;
    for px in 0..width {
        let off = pixel_offset_x(px, ZOOM);
        let v = deep_center_value(&mut store, &instance, W0X, W1X, W2X, W3X, off);
        if let Some(p) = &prev {
            let gap = &v - p;
            assert!(
                gap > zero,
                "deep row not strictly increasing at px={px}: c_x collapsed \
                 (gap {gap}); the deep render would mush here"
            );
            min_gap = Some(match min_gap.take() {
                Some(m) if m < gap => m,
                _ => gap,
            });
        }
        prev = Some(v);
    }
    let min_gap = min_gap.expect("row has >= 2 pixels");
    assert!(
        min_gap > zero,
        "every adjacent deep pixel pair must resolve to a DISTINCT Fixed<8> c_x"
    );

    // -- (2) df64 could not reach this center -------------------------------
    // Drop the two deep words (w2 = w3 = 0): a two-word df64 center. Its c_x at
    // the SAME center pixel differs from the full four-word center by the deep
    // residual, which we measure in per-pixel steps.
    let center_px = width / 2;
    let off = pixel_offset_x(center_px, ZOOM);
    let full = deep_center_value(&mut store, &instance, W0X, W1X, W2X, W3X, off);
    let df64_only = deep_center_value(&mut store, &instance, W0X, W1X, 0.0, 0.0, off);

    let shift_ulps = (&full - &df64_only).magnitude().clone(); // |diff| in units of 2^-91
    assert!(
        shift_ulps != BigUint::from(0u32),
        "the deep words must actually move the center (they are below the df64 \
         floor, so a df64 center silently drops them)"
    );

    // per-pixel step = 2*zoom/res, in the same 2^-91 ULP units.
    let step = 2.0f64 * ZOOM as f64 / RES as f64;
    let ulp = 2.0f64.powi(-(F as i32)); // 2^-91
    let step_ulps = step / ulp; // ~9.66e8 ULPs per pixel at zoom 1e-16
    // shift_ulps as f64 (it is ~7e11, exactly representable within f64 range).
    let shift_ulps_f = shift_ulps.to_string().parse::<f64>().unwrap();
    let shift_pixels = shift_ulps_f / step_ulps;

    assert!(
        shift_pixels > 100.0,
        "dropping the deep words (a df64 center) shifts the view by only \
         {shift_pixels:.1} px; expected many hundreds (the deep bits are \
         un-representable in df64, so a df64 render lands on a different region)"
    );

    eprintln!(
        "deep-center oracle: full 512-px row strictly increasing (min adjacent \
         Fixed<8> gap = {min_gap} ULPs of 2^-91); dropping the deep words \
         (df64 center) shifts the view {shift_pixels:.0} px at zoom {ZOOM:e} \
         -- df64 could not reach this location, Fixed<8> holds it."
    );
}
