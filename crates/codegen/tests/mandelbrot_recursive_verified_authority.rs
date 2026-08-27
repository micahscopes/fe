//! Nominal authority gate for proof-backed recursive Mandelbrot intervals.

use common::InputDb;
use driver::DriverDataBase;
use hir::hir_def::HirIngot;
use std::path::{Path, PathBuf};
use url::Url;

fn rejected_fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_recursive_verified_forge_rejected_ingot")
        .canonicalize()
        .unwrap()
}

#[test]
fn public_recursive_carriers_cannot_forge_verified_authority() {
    let url = Url::from_directory_path(rejected_fixture_path()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "recursive authority rejection fixture should initialize",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("recursive authority rejection fixture ingot");
    let diagnostics = db.run_on_top_mod(ingot.root_mod(&db)).format_diags(&db);
    assert!(
        diagnostics.contains("`committed` is not visible"),
        "verified recursive authority must remain unforgeable:\n{diagnostics}",
    );
    assert!(
        diagnostics.contains("type mismatch")
            && diagnostics.contains("8192, 456, 5928")
            && diagnostics.contains("8192, 16, 208"),
        "protocol-shape receipts must not satisfy the security leaf boundary:\n{diagnostics}",
    );
}
