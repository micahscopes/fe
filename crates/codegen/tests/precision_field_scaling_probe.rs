//! THROWAWAY measurement harness (not part of the permanent suite, safe to
//! delete): times compilation of the general `precision::field::mul<L, M:
//! Modulus<L>>` at varying limb counts `L`, via the throwaway
//! `precision_scaling_probe_l{L}` fixtures (crates/codegen/tests/fixtures/),
//! to settle whether the L=20 `mul` compile-time blowup (measured: >5min,
//! compute-bound, not the salsa fixpoint) is super-linear in L (untracked
//! normalization recompute) or linear-but-large (raw monomorphization cost).
//!
//! Mirrors `precision_field_bn254fr_oracle.rs`'s own
//! `compile_field_gate_ingot_to_wasm` / `diag_compile_field_gate_ingot_only`
//! exactly (same DriverDataBase/init_ingot/BackendKind::Wasm/O0 path), just
//! parameterized by fixture directory name and timed individually so each L
//! can be run (and bounded with an external `timeout`) as its own test
//! binary invocation.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;

/// Compile the named `precision_scaling_probe_l{L}` fixture ingot to wasm,
/// returning (elapsed, wasm byte length). Panics loudly (with diagnostics) on
/// any compile failure, same posture as the oracle gate's own helper.
fn compile_probe_to_wasm(fixture_dir_name: &str) -> (std::time::Duration, usize) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture_dir_name);
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    let t0 = std::time::Instant::now();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "probe ingot `{fixture_dir_name}` initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .unwrap_or_else(|| panic!("probe ingot `{fixture_dir_name}` should resolve"));
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected probe-ingot `{fixture_dir_name}` diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .unwrap_or_else(|err| panic!("probe `{fixture_dir_name}` wasm compile failed: {err}"))
        .into_bytecode()
        .expect("wasm output should be bytecode");
    let elapsed = t0.elapsed();
    wasmparser::validate(&bytes).expect("probe wasm should validate");
    (elapsed, bytes.len())
}

fn run_probe(fixture_dir_name: &str) {
    let (elapsed, n_bytes) = compile_probe_to_wasm(fixture_dir_name);
    eprintln!(
        "SCALING_PROBE {fixture_dir_name}: compiled to wasm in {:?} ({:.3}s), {n_bytes} bytes",
        elapsed,
        elapsed.as_secs_f64()
    );
}

// One #[test] per L so each can be invoked (and externally `timeout`-bounded)
// independently: `cargo test --release -p fe-codegen --test
// precision_field_scaling_probe probe_l8 -- --nocapture`.

#[test]
fn probe_l2() {
    run_probe("precision_scaling_probe_l2");
}

#[test]
fn probe_l4() {
    run_probe("precision_scaling_probe_l4");
}

#[test]
fn probe_l8() {
    run_probe("precision_scaling_probe_l8");
}

#[test]
fn probe_l12() {
    run_probe("precision_scaling_probe_l12");
}

#[test]
fn probe_l16() {
    run_probe("precision_scaling_probe_l16");
}

#[test]
fn probe_l20() {
    run_probe("precision_scaling_probe_l20");
}
