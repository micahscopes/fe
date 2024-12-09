use std::ops::Range;

use camino::Utf8Path;
use codespan_reporting as cs;
use common::{
    diagnostics::{LabelStyle, Severity},
    InputDb, InputFile,
};
use cs::{diagnostic as cs_diag, files as cs_files};
use hir::{diagnostics::DiagnosticVoucher, SpannedHirDb};

use crate::DriverDb;

pub trait ToCsDiag {
    fn to_cs(&self, db: &dyn SpannedInputDb) -> cs_diag::Diagnostic<InputFile>;
}

pub trait SpannedInputDb: SpannedHirDb + InputDb {}
impl<T> SpannedInputDb for T where T: SpannedHirDb + InputDb {}

impl<T> ToCsDiag for T
where
    T: for<'db> DiagnosticVoucher<'db>,
{
    fn to_cs(&self, db: &dyn SpannedInputDb) -> cs_diag::Diagnostic<InputFile> {
        let complete = self.to_complete(db.as_spanned_hir_db());

        let severity = convert_severity(complete.severity);
        let code = Some(complete.error_code.to_string());
        let message = complete.message;

        let labels = complete
            .sub_diagnostics
            .into_iter()
            .filter_map(|sub_diag| {
                let span = sub_diag.span?;
                match sub_diag.style {
                    LabelStyle::Primary => {
                        cs_diag::Label::new(cs_diag::LabelStyle::Primary, span.file, span.range)
                    }
                    LabelStyle::Secondary => {
                        cs_diag::Label::new(cs_diag::LabelStyle::Secondary, span.file, span.range)
                    }
                }
                .with_message(sub_diag.message)
                .into()
            })
            .collect();

        cs_diag::Diagnostic {
            severity,
            code,
            message,
            labels,
            notes: vec![],
        }
    }
}

fn convert_severity(severity: Severity) -> cs_diag::Severity {
    match severity {
        Severity::Error => cs_diag::Severity::Error,
        Severity::Warning => cs_diag::Severity::Warning,
        Severity::Note => cs_diag::Severity::Note,
    }
}

#[salsa::tracked(return_ref)]
pub fn file_line_starts(db: &dyn DriverDb, file: InputFile) -> Vec<usize> {
    cs::files::line_starts(file.text(db.as_input_db())).collect()
}

pub struct CsDbWrapper<'a>(pub &'a dyn DriverDb);

impl<'db> cs_files::Files<'db> for CsDbWrapper<'db> {
    type FileId = InputFile;
    type Name = &'db Utf8Path;
    type Source = &'db str;

    fn name(&'db self, file_id: Self::FileId) -> Result<Self::Name, cs_files::Error> {
        Ok(file_id.path(self.0.as_input_db()).as_path())
    }

    fn source(&'db self, file_id: Self::FileId) -> Result<Self::Source, cs_files::Error> {
        Ok(file_id.text(self.0.as_input_db()))
    }

    fn line_index(
        &'db self,
        file_id: Self::FileId,
        byte_index: usize,
    ) -> Result<usize, cs_files::Error> {
        let starts = file_line_starts(self.0, file_id);
        Ok(starts
            .binary_search(&byte_index)
            .unwrap_or_else(|next_line| next_line - 1))
    }

    fn line_range(
        &'db self,
        file_id: Self::FileId,
        line_index: usize,
    ) -> Result<Range<usize>, cs_files::Error> {
        let line_starts = file_line_starts(self.0, file_id);

        let start = *line_starts
            .get(line_index)
            .ok_or(cs_files::Error::LineTooLarge {
                given: line_index,
                max: line_starts.len() - 1,
            })?;

        let end = if line_index == line_starts.len() - 1 {
            file_id.text(self.0.as_input_db()).len()
        } else {
            *line_starts
                .get(line_index + 1)
                .ok_or(cs_files::Error::LineTooLarge {
                    given: line_index,
                    max: line_starts.len() - 1,
                })?
        };

        Ok(Range { start, end })
    }
}
