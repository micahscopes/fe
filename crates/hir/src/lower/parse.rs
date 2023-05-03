use common::{
    diagnostics::{AnalysisPass, CompleteDiagnostic, GlobalErrorCode, Severity, Span, SpanKind},
    InputFile,
};
use parser::GreenNode;

use crate::{diagnostics::DiagnosticVoucher, hir_def::TopLevelMod, HirDb, SpannedHirDb};

#[salsa::tracked]
pub(crate) fn parse_file_impl(db: &dyn HirDb, top_mod: TopLevelMod) -> GreenNode {
    let file = top_mod.file(db);
    let text = file.text(db.upcast());
    let (node, parse_errors) = parser::parse_source_file(text);

    for error in parse_errors {
        ParseDiagnosticAccumulator::push(db, ParseDiagnostic { file, error });
    }
    node
}

#[doc(hidden)]
#[salsa::accumulator]
pub struct ParseDiagnosticAccumulator(ParseDiagnostic);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParseDiagnostic {
    file: InputFile,
    error: parser::ParseError,
}

// `ParseError` has span information, but this is not a problem because the
// parsing procedure itself depends on the file content, and thus span
// information.
impl DiagnosticVoucher for ParseDiagnostic {
    fn error_code(&self) -> GlobalErrorCode {
        GlobalErrorCode::new(AnalysisPass::Parse, 0)
    }

    fn to_complete(self, _db: &dyn SpannedHirDb) -> CompleteDiagnostic {
        let error_code = self.error_code();
        let span = Span::new(self.file, self.error.range, SpanKind::Original);
        CompleteDiagnostic::new(Severity::Error, self.error.msg, span, vec![], error_code)
    }
}