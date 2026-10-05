//! Declaration generation before a command analyses its target.
use std::ops::Deref;

use driver::{DriverDataBase, elaboration::ElaborationError};
use url::Url;

/// The database a command analyses: the authored one, or a copy that also holds
/// the declarations generated for the target.
pub(crate) enum PreparedDb<'a> {
    Authored(&'a DriverDataBase),
    Generated(Box<DriverDataBase>),
}

impl Deref for PreparedDb<'_> {
    type Target = DriverDataBase;

    fn deref(&self) -> &DriverDataBase {
        match self {
            Self::Authored(db) => db,
            Self::Generated(db) => db,
        }
    }
}

/// Run generation for `target` and its dependencies.
pub(crate) fn prepare<'a>(
    db: &'a DriverDataBase,
    target: &Url,
) -> Result<PreparedDb<'a>, ElaborationError> {
    Ok(match driver::elaboration::prepare_target(db, target)? {
        Some(generated) => PreparedDb::Generated(Box::new(generated)),
        None => PreparedDb::Authored(db),
    })
}

/// Run generation for `target`, printing a failure to standard error.
pub(crate) fn prepare_or_report<'a>(
    db: &'a DriverDataBase,
    target: &Url,
    header: Option<&Url>,
) -> Option<PreparedDb<'a>> {
    match prepare(db, target) {
        Ok(prepared) => Some(prepared),
        Err(error) => {
            report(db, &error, header);
            None
        }
    }
}

/// Print a generation error the way other diagnostics are printed: the same
/// colour handling and, when `header` is given, an `errors in <url>` line.
pub(crate) fn report(db: &DriverDataBase, error: &ElaborationError, header: Option<&Url>) {
    if let Some(url) = header {
        eprintln!("errors in {url}");
        eprintln!();
    }
    match error.diagnostic(db) {
        Some(diagnostic) => db.emit_complete_diagnostics(&[diagnostic]),
        None => eprintln!("{}", error.render(db)),
    }
}
