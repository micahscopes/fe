//! Permanent regression witness for the cross-ingot `collect_trait_impls`
//! salsa cycle (SALSA_CYCLE_DECISION.md / SALSA_CYCLE_INVESTIGATION.md).
//!
//! The dependency ingot `assoc_proj_cycle_dep` carries a BARE associated-type
//! projection in an impl header (`impl<T: Tr + Copy> Tr for Wrap<T> { type Out
//! = Wrap<T::Out> }`), which forms the `{ingot_trait_env, collect_trait_impls}`
//! query SCC. The consumer ingot enters that ingot as a DEPENDENCY and forces
//! the projection to normalize at a concrete instantiation, so
//! `TraitEnv::collect(consumer)` calls `collect_trait_impls(dependency)`
//! directly and `collect_trait_impls` becomes the re-entered cycle head.
//!
//! Two witnesses, two shapes:
//!
//! - `cross_ingot_bare_projection_does_not_cycle_collect_trait_impls`
//!   (bare-PARAM receiver, `type Out = Wrap<T::Out>`): after the bound-priority
//!   fix (Leg 1 of the perf fix) this shape resolves through the receiver's
//!   bounds alone and NO LONGER enters the SCC. It stays green with OR without
//!   the `collect_trait_impls` handler; it is the witness that bound-priority
//!   keeps this common shape out of the cycle env-free.
//! - `cross_ingot_concrete_projection_cycles_collect_trait_impls`
//!   (concrete RECEIVER, `type Out = Base::Out`): assumptions cannot resolve a
//!   concrete-receiver projection and bound-priority does not fire, so the impl
//!   search still consults `ingot_trait_env` during collection and the SCC
//!   genuinely forms. This is the LIVE red-without-the-fix witness: remove the
//!   two `cycle_fn`/`cycle_initial` attribute lines on `collect_trait_impls`
//!   (`crates/hir/src/analysis/ty/trait_lower.rs`) and this test panics inside
//!   salsa naming `collect_trait_impls`, while the bare-param test above stays
//!   green.

use common::InputDb;
use driver::DriverDataBase;
use hir::hir_def::HirIngot;
use url::Url;

fn fixture_url(dir: &str) -> Url {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(dir);
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn assert_consumer_compiles_clean(dir: &str) {
    let url = fixture_url(dir);
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "cross-ingot assoc-projection cycle fixture ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("consumer ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected cross-ingot assoc-projection diagnostics:\n{diagnostics}"
    );
}

/// Bare-PARAM receiver: bound-priority keeps this shape out of the SCC entirely,
/// so it resolves env-free and stays green regardless of the cycle handler.
#[test]
fn cross_ingot_bare_projection_does_not_cycle_collect_trait_impls() {
    assert_consumer_compiles_clean("assoc_proj_cycle_consumer_ingot");
}

/// Concrete RECEIVER: the impl search is genuinely required, the SCC still forms,
/// and `collect_trait_impls` must fixpoint-iterate instead of panicking on an
/// unhandled cycle head. Live red-without-the-handler witness.
#[test]
fn cross_ingot_concrete_projection_cycles_collect_trait_impls() {
    assert_consumer_compiles_clean("assoc_proj_cycle_concrete_consumer_ingot");
}
