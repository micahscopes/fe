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
) -> Option<PreparedDb<'a>> {
    match prepare(db, target) {
        Ok(prepared) => Some(prepared),
        Err(error) => {
            eprintln!("{}", error.render(db));
            None
        }
    }
}
