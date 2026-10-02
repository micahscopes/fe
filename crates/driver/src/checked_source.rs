//! Shared ordinary source certification for explicit compiler API consumers.

use crate::DriverDataBase;
use common::{
    diagnostics::{CompleteDiagnostic, Severity},
    file::File,
};

pub(crate) fn complete_diagnostics(
    db: &DriverDataBase,
    file: File,
) -> Option<Vec<CompleteDiagnostic>> {
    let ingot = db.top_mod(file).ingot(db);
    let hir = db.run_on_ingot(ingot);
    if hir.has_errors(db) {
        return Some(hir.complete_diags(db));
    }
    let semantic = db.mir_diagnostics_for_ingot(ingot);
    semantic
        .iter()
        .any(|diag| diag.severity == Severity::Error)
        .then_some(semantic)
}
