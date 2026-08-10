//! Bit-identical refactor-equivalence gate for the precision axis P1
//! (PRECISION_TYPES_RESEARCH.md P1 gate (i); ROLLCALL_GOAL.md "Precision
//! axis"; PRECISION_P0_PROBES.md verdict).
//!
//! Was staged at `demos/sketches/precision/oracle.rs` while another agent
//! held the shared cargo/cranelift build lock (mirrored
//! `demos/sketches/qcga_pencil/acceptance.rs`'s own staging discipline);
//! moved here once the lock was free and `cargo check -p fe-codegen --tests`
//! passed with it in place. `fe check` on both the `precision` ingot and the
//! `precision_expansion2_df64_oracle_ingot` fixture it drives was verified
//! with the prebuilt `fe` binary before this file existed; see the task
//! report for the exact commands.
//!
//! What this proves: `precision::Expansion<2>`'s `add`/`sub`/`add_f32`/
//! `mul`/`sqr` are BIT-FOR-BIT IDENTICAL to the hand-written df64 in
//! `demos/sketches/mandelbrot/src/lib.fe`, compiled to wasm and run under
//! wasmtime, over representative + edge float vectors. Per
//! PRECISION_TYPES_RESEARCH.md P1 gate (i), this is a TRANSIENT
//! refactor-equivalence net (acceptance-criteria doctrine: a safety net for
//! the migration step, never the enduring contract) -- once it is green, and
//! ONLY once it is green, `demos/sketches/mandelbrot/src/lib.fe` may be
//! rebased onto `precision::Expansion<2>` and its own hand df64 deleted
//! (PRECISION_TYPES_RESEARCH.md P1 gate (iv); ROLLCALL_GOAL.md "Precision
//! axis": "`Expansion<2>` proven bit-identical to the hand df64, then df64
//! DELETED"). The enduring contract (error-bound/property tests against a
//! high-precision oracle) is a later slice, not this gate.
//!
//! Both sides run through the SAME fixture ingot,
//! `crates/codegen/tests/fixtures/precision_expansion2_df64_oracle_ingot/`:
//! `df64_*` is a frozen, byte-for-byte transcription of mandelbrot's hand
//! df64 (made `pub` so this gate can call it; if mandelbrot's df64 ever
//! changes this drifts and the gate goes stale by construction -- an
//! intentional, cheap property, since this fixture is deleted the same day
//! df64 is deleted from mandelbrot); `expansion2_*` are thin wrappers over
//! the real general form, `precision::{add, sub, add_f32, mul, sqr}` at the
//! explicit `N = 2` instantiation. Every op is exported TWICE, once as a
//! `(f32, f32)` tuple return (the readable, directly-diffable-against-
//! mandelbrot core) and once split into two scalar-f32-return exports
//! (`*_hi`/`*_lo`): this test calls the scalar-split exports, matching this
//! codebase's own established wasmtime f32-multi-result convention
//! (`crates/codegen/tests/pga2d_meet_join_plan.rs`'s `wedge_c0`/`wedge_c1`/
//! `wedge_c2`), which sidesteps needing to confirm `(f32, f32)`
//! multi-value tuple returns work through `get_typed_func` (an f32-typed
//! multi-value precedent was not found in-tree; `(i32, i32)`/`(i64, i64)`
//! multi-value returns ARE proven, e.g. `wasm_e2e.rs`'s `mvt1_runtime_probe`
//! and `join2`, but nothing pins the f32 case, so this gate does not lean on
//! it).

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;

fn compile_oracle_gate_ingot_to_wasm() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/precision_expansion2_df64_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "precision Expansion<2>/df64 oracle gate ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("precision Expansion<2>/df64 oracle gate ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected gate-ingot diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("precision/df64 oracle gate ingot should compile to wasm")
        .into_bytecode()
        .expect("wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("gate ingot wasm should validate");
    bytes
}

/// Representative + edge `(a_hi, a_lo, b_hi, b_lo)` df64 vectors: typical
/// mandelbrot-range magnitudes, near-cancellation in the `hi` parts (the
/// case `df_add`'s leading `two_sum` exists for), opposite signs, the
/// escape-radius boundary, deep-zoom-floor-scale magnitudes, zero, equal
/// operands (so `sqr` and `mul(a, a)` are both exercised), and an exact
/// cancellation to zero.
const DF64_PAIRS: [(f32, f32, f32, f32); 12] = [
    (0.5, 0.0, 0.25, 0.0),
    (-0.75, 1.192092896e-8, 0.75, -1.192092896e-8),
    (1.5, 3.5e-8, -1.5, -2.1e-8),
    (0.0001, 1e-12, 0.0002, 2e-12),
    (-2.0, 0.0, 2.0, 0.0),
    (3.14159274, 1.19209289e-7, 2.71828175, 4.20530706e-8),
    (0.0, 0.0, 0.0, 0.0),
    (1.0, 0.0, 1.0, 0.0),
    (-0.123456789, 5e-9, 0.987654321, -3e-9),
    (100000.0, 0.001, -99999.999, 0.0001),
    (1.0, f32::EPSILON, 1.0, f32::EPSILON),
    (-1.0, 0.0, 1.0, 0.0),
];

/// Plain-f32 addends for `add_f32` (df64 + f32), covering zero, a
/// deep-zoom-scale offset, and both signs at O(1) magnitude.
const SCALAR_ADDENDS: [f32; 5] = [0.0, 1e-9, -0.001, 3.5, -2.5];

#[test]
fn expansion2_matches_hand_df64_bit_for_bit() {
    let wasm = compile_oracle_gate_ingot_to_wasm();

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(
        module.imports().next().is_none(),
        "the gate wasm must be self-contained (zero imports)"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();

    let func4 = |name: &str, store: &mut wasmtime::Store<()>| {
        instance
            .get_typed_func::<(f32, f32, f32, f32), f32>(&mut *store, name)
            .unwrap_or_else(|e| panic!("export `{name}` missing/mis-typed: {e}"))
    };
    let func3 = |name: &str, store: &mut wasmtime::Store<()>| {
        instance
            .get_typed_func::<(f32, f32, f32), f32>(&mut *store, name)
            .unwrap_or_else(|e| panic!("export `{name}` missing/mis-typed: {e}"))
    };
    let func2 = |name: &str, store: &mut wasmtime::Store<()>| {
        instance
            .get_typed_func::<(f32, f32), f32>(&mut *store, name)
            .unwrap_or_else(|e| panic!("export `{name}` missing/mis-typed: {e}"))
    };

    // add / sub / mul: (a_hi, a_lo, b_hi, b_lo) -> (hi, lo)
    let df64_add_hi = func4("df64_add_hi", &mut store);
    let df64_add_lo = func4("df64_add_lo", &mut store);
    let expansion2_add_hi = func4("expansion2_add_hi", &mut store);
    let expansion2_add_lo = func4("expansion2_add_lo", &mut store);

    let df64_sub_hi = func4("df64_sub_hi", &mut store);
    let df64_sub_lo = func4("df64_sub_lo", &mut store);
    let expansion2_sub_hi = func4("expansion2_sub_hi", &mut store);
    let expansion2_sub_lo = func4("expansion2_sub_lo", &mut store);

    let df64_mul_hi = func4("df64_mul_hi", &mut store);
    let df64_mul_lo = func4("df64_mul_lo", &mut store);
    let expansion2_mul_hi = func4("expansion2_mul_hi", &mut store);
    let expansion2_mul_lo = func4("expansion2_mul_lo", &mut store);

    // sqr: (a_hi, a_lo) -> (hi, lo)
    let df64_sqr_hi = func2("df64_sqr_hi", &mut store);
    let df64_sqr_lo = func2("df64_sqr_lo", &mut store);
    let expansion2_sqr_hi = func2("expansion2_sqr_hi", &mut store);
    let expansion2_sqr_lo = func2("expansion2_sqr_lo", &mut store);

    // add_f32: (a_hi, a_lo, b) -> (hi, lo)
    let df64_add_f32_hi = func3("df64_add_f32_hi", &mut store);
    let df64_add_f32_lo = func3("df64_add_f32_lo", &mut store);
    let expansion2_add_f32_hi = func3("expansion2_add_f32_hi", &mut store);
    let expansion2_add_f32_lo = func3("expansion2_add_f32_lo", &mut store);

    for &(a_hi, a_lo, b_hi, b_lo) in DF64_PAIRS.iter() {
        macro_rules! check4 {
            ($op:literal, $want_hi:expr, $want_lo:expr, $got_hi:expr, $got_lo:expr) => {
                let want_hi = $want_hi.call(&mut store, (a_hi, a_lo, b_hi, b_lo)).unwrap();
                let want_lo = $want_lo.call(&mut store, (a_hi, a_lo, b_hi, b_lo)).unwrap();
                let got_hi = $got_hi.call(&mut store, (a_hi, a_lo, b_hi, b_lo)).unwrap();
                let got_lo = $got_lo.call(&mut store, (a_hi, a_lo, b_hi, b_lo)).unwrap();
                assert_eq!(
                    (got_hi.to_bits(), got_lo.to_bits()),
                    (want_hi.to_bits(), want_lo.to_bits()),
                    "{} mismatch for a=({a_hi},{a_lo}) b=({b_hi},{b_lo}): \
                     got=({got_hi},{got_lo}) want=({want_hi},{want_lo})",
                    $op,
                );
            };
        }
        check4!(
            "add",
            df64_add_hi,
            df64_add_lo,
            expansion2_add_hi,
            expansion2_add_lo
        );
        check4!(
            "sub",
            df64_sub_hi,
            df64_sub_lo,
            expansion2_sub_hi,
            expansion2_sub_lo
        );
        check4!(
            "mul",
            df64_mul_hi,
            df64_mul_lo,
            expansion2_mul_hi,
            expansion2_mul_lo
        );

        let want_hi = df64_sqr_hi.call(&mut store, (a_hi, a_lo)).unwrap();
        let want_lo = df64_sqr_lo.call(&mut store, (a_hi, a_lo)).unwrap();
        let got_hi = expansion2_sqr_hi.call(&mut store, (a_hi, a_lo)).unwrap();
        let got_lo = expansion2_sqr_lo.call(&mut store, (a_hi, a_lo)).unwrap();
        assert_eq!(
            (got_hi.to_bits(), got_lo.to_bits()),
            (want_hi.to_bits(), want_lo.to_bits()),
            "sqr mismatch for a=({a_hi},{a_lo}): got=({got_hi},{got_lo}) want=({want_hi},{want_lo})",
        );

        for &b in SCALAR_ADDENDS.iter() {
            let want_hi = df64_add_f32_hi.call(&mut store, (a_hi, a_lo, b)).unwrap();
            let want_lo = df64_add_f32_lo.call(&mut store, (a_hi, a_lo, b)).unwrap();
            let got_hi = expansion2_add_f32_hi
                .call(&mut store, (a_hi, a_lo, b))
                .unwrap();
            let got_lo = expansion2_add_f32_lo
                .call(&mut store, (a_hi, a_lo, b))
                .unwrap();
            assert_eq!(
                (got_hi.to_bits(), got_lo.to_bits()),
                (want_hi.to_bits(), want_lo.to_bits()),
                "add_f32 mismatch for a=({a_hi},{a_lo}) b={b}: \
                 got=({got_hi},{got_lo}) want=({want_hi},{want_lo})",
            );
        }
    }
}
