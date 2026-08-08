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
//! With the `cycle_fn`/`cycle_initial` handler on `collect_trait_impls` this
//! compiles clean. Remove those two attribute lines and this test panics
//! inside salsa naming `collect_trait_impls`: that is the red-without-the-fix
//! seal on the entry-topology analysis, not just a smoke test.

use common::InputDb;
use driver::DriverDataBase;
use hir::hir_def::HirIngot;
use url::Url;

fn consumer_url() -> Url {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/assoc_proj_cycle_consumer_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

/// Cross-ingot entry through the bare-projection dependency must fixpoint-iterate
/// the `collect_trait_impls` SCC instead of panicking on an unhandled cycle head.
#[test]
fn cross_ingot_bare_projection_does_not_cycle_collect_trait_impls() {
    let url = consumer_url();
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
