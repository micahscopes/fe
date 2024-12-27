use common::{InputDb, InputIngot};

use hir::{HirDb, LowerHirDb, SpannedHirDb};
use hir_analysis::HirAnalysisDb;
use salsa::{ParallelDatabase, Snapshot};

use super::get_std::get_std_ingot;

#[salsa::jar(db = LanguageServerDb)]
pub struct Jar(crate::functionality::diagnostics::file_line_starts);

pub trait LanguageServerDb:
    salsa::DbWithJar<Jar> + HirAnalysisDb + HirDb + LowerHirDb + SpannedHirDb + InputDb
{
}

impl<DB> LanguageServerDb for DB where
    DB: Sized + salsa::DbWithJar<Jar> + HirAnalysisDb + HirDb + LowerHirDb + SpannedHirDb + InputDb
{
}

#[salsa::db(
    common::Jar,
    hir::Jar,
    hir::LowerJar,
    hir::SpannedJar,
    hir_analysis::Jar,
    Jar
)]
pub struct LanguageServerDatabase {
    storage: salsa::Storage<Self>,
    std_ingot: Option<InputIngot>,
}

impl Default for LanguageServerDatabase {
    fn default() -> Self {
        let mut db = LanguageServerDatabase {
            storage: salsa::Storage::default(),
            std_ingot: None,
        };

        db.std_ingot = Some(get_std_ingot(&mut db));
        db
    }
}

impl LanguageServerDatabase {
    pub(crate) fn std_ingot(&self) -> &InputIngot {
        self.std_ingot
            .as_ref()
            .expect("std ingot should have been created at startup")
    }
}

impl salsa::Database for LanguageServerDatabase {
    fn salsa_event(&self, _: salsa::Event) {}
}

impl ParallelDatabase for LanguageServerDatabase {
    fn snapshot(&self) -> Snapshot<Self> {
        Snapshot::new(LanguageServerDatabase {
            storage: self.storage.snapshot(),
            std_ingot: self.std_ingot.clone(),
        })
    }
}
