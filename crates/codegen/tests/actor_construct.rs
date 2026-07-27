//! Acceptance for the R-A1 `actor` construct on the DEC render program.
//!
//! `DecSurface` is declared as an `actor` in `demos/sketches/dec/src/lib.fe`.
//! Two claims are gated here:
//!
//!  1. The render entry and mode the CLI used to pass as
//!     `--entry dec_render --mode render` are DERIVED from the actor
//!     declaration, and explicit flags that contradict it are rejected.
//!  2. The `actor` desugar reproduces the flattened free kernel a hand-written
//!     `pub fn` would emit, byte for byte.
//!
//! Note on the DEC render bundle itself: it cannot be lowered to wasm or
//! SPIR-V today because `dec_render` uses `!=` (via `laplacian0`) and the
//! wasm/SPIR-V "R1" path does not yet lower `NotEq` (a pre-existing R2 gap that
//! blocks the flag path and the actor path identically). So claim 2's BYTE
//! comparison runs on a small buildable analog kernel, while claim 1 runs on
//! the real DEC actor; together they establish that the actor reproduces the
//! flag-built inputs with zero backend change.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WasmCompileOptions, WebBundleMode, actor_web_entry, compile_runtime_package_wasm_with_options,
    resolve_web_entry,
};
use hir::hir_def::{HirIngot, TopLevelMod};
use url::Url;

fn ingot_root(relative: &str) -> Url {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn ingot_top_mod<'db>(db: &'db DriverDataBase, url: &Url) -> TopLevelMod<'db> {
    let ingot = db
        .workspace()
        .containing_ingot(db, url.clone())
        .expect("ingot");
    ingot.root_mod(db)
}

fn build_entry_wasm(db: &DriverDataBase, top_mod: TopLevelMod<'_>, entry: &str) -> Vec<u8> {
    let package =
        mir::build_wasm_runtime_package_for_entries(db, top_mod, &[entry.to_string()]).unwrap();
    compile_runtime_package_wasm_with_options(
        db,
        &package,
        WasmCompileOptions::default().with_optimization(),
    )
    .unwrap()
    .bytes
}

/// Opens the DEC ingot and returns its clean top module (init + no diagnostics).
fn dec_top_mod<'db>(db: &'db DriverDataBase, url: &Url) -> TopLevelMod<'db> {
    let top_mod = ingot_top_mod(db, url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(db);
    assert!(diagnostics.is_empty(), "unexpected dec diagnostics:\n{diagnostics}");
    top_mod
}

#[test]
fn dec_actor_reproduces_the_flag_built_bundle() {
    // The DEC render program is declared as an `actor`; the render entry and
    // mode the flags used to supply are DERIVED from that declaration, so the
    // derivation path reproduces exactly the build inputs the flag path used.
    let mut db = DriverDataBase::default();
    let url = ingot_root("../../demos/sketches/dec");
    assert!(!driver::init_ingot(&mut db, &url), "dec ingot init diagnostics");
    let top_mod = dec_top_mod(&db, &url);

    // Zero-config: no flags supplied, entry + mode derived from the actor.
    let derived = resolve_web_entry(&db, top_mod, None, None).expect("derivation");
    assert_eq!(derived, ("dec_render".to_string(), WebBundleMode::Render));

    // The same fact, read directly off the declaration.
    assert_eq!(
        actor_web_entry(&db, top_mod).unwrap(),
        Some(("dec_render".to_string(), WebBundleMode::Render)),
    );

    // Explicit flags that MATCH the declaration are accepted and reconcile to
    // the identical build inputs the flag path built from.
    let reconciled = resolve_web_entry(
        &db,
        top_mod,
        Some("dec_render".to_string()),
        Some(WebBundleMode::Render),
    )
    .expect("matching flags");
    assert_eq!(reconciled, derived);
}

#[test]
fn flags_contradicting_the_actor_are_rejected() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("../../demos/sketches/dec");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = dec_top_mod(&db, &url);

    // An explicit entry that is not the actor's fragment behavior.
    let err = resolve_web_entry(
        &db,
        top_mod,
        Some("not_dec_render".to_string()),
        Some(WebBundleMode::Render),
    )
    .unwrap_err();
    let text = format!("{err}");
    assert!(text.contains("not_dec_render") && text.contains("dec_render"), "{text}");

    // An explicit mode that contradicts the derived render mode.
    let err = resolve_web_entry(
        &db,
        top_mod,
        Some("dec_render".to_string()),
        Some(WebBundleMode::Grid),
    )
    .unwrap_err();
    assert!(format!("{err}").contains("contradicts"), "{err}");
}

#[test]
fn actor_without_a_unique_fragment_behavior_is_rejected() {
    // Two role-marked behaviors in one actor: no unique render entry to pick.
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_two_fragment");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let err = actor_web_entry(&db, top_mod).unwrap_err();
    assert!(format!("{err}").contains("FragmentSurface"), "{err}");

    // An actor with the placement row but no fragment behavior at all.
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_no_fragment");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let err = actor_web_entry(&db, top_mod).unwrap_err();
    assert!(format!("{err}").contains("no `FragmentSurface`"), "{err}");
}

#[test]
fn desugar_reproduces_the_handwritten_kernel_byte_for_byte() {
    // The `actor`-desugared `paint` and the hand-written free `paint` are built
    // as the same-named wasm entry from two sibling ingots and must be byte
    // identical: the flattened parameters, the `self.<field>` rewrite, and the
    // dropped placement row together reproduce exactly what a hand-written free
    // kernel emits.
    let mut db = DriverDataBase::default();
    let actor_url = ingot_root("tests/fixtures/actor_repro_actor");
    let free_url = ingot_root("tests/fixtures/actor_repro_free");
    assert!(!driver::init_ingot(&mut db, &actor_url), "actor fixture diagnostics");
    assert!(!driver::init_ingot(&mut db, &free_url), "free fixture diagnostics");

    let actor_mod = ingot_top_mod(&db, &actor_url);
    let free_mod = ingot_top_mod(&db, &free_url);
    let actor_diags = db.run_on_top_mod(actor_mod).format_diags(&db);
    assert!(actor_diags.is_empty(), "actor fixture diagnostics:\n{actor_diags}");
    let free_diags = db.run_on_top_mod(free_mod).format_diags(&db);
    assert!(free_diags.is_empty(), "free fixture diagnostics:\n{free_diags}");

    let actor_wasm = build_entry_wasm(&db, actor_mod, "paint");
    let free_wasm = build_entry_wasm(&db, free_mod, "paint");
    assert_eq!(
        actor_wasm, free_wasm,
        "actor-desugared `paint` must reproduce the hand-written free kernel byte for byte"
    );
}
