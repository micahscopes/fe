//! Checked execution of explicit selected nullary const methods.

use common::{file::File, ingot::IngotKind};
use hir::{
    analysis::{
        semantic::{
            BlockedInfo, EvalFailure, EvalOutcome, GenericSubst, SemConstId,
            eval_body_owner_const_with_args,
        },
        ty::{
            trait_def::{SelectedEvidenceError, TraitInstId, prove_selected_hir_impl_evidence},
            trait_resolution::TraitSolveCx,
            ty_check::BodyOwner,
            ty_def::TyFlags,
            visitor::collect_flags,
        },
    },
    hir_def::{HirIngot, IdentId, ImplTrait, ItemKind},
    span::LazySpan,
};

use crate::{DriverDataBase, checked_source::diagnostics};

#[derive(Debug)]
pub enum SelectedMethodError<'db> {
    Unsupported,
    Source(String),
    Evidence(SelectedEvidenceError),
    MissingMethod,
    Blocked(BlockedInfo<'db>),
    Execution(EvalFailure<'db>),
}

/// Evaluates one selected explicit method after ordinary source certification.
///
/// The file must be standalone with builtin-only dependencies, and the selected
/// impl must be a top-level item in it. Goals must be ground. The method must be safe,
/// nullary and const, without method-local generics, const predicates or effects.
/// Method-local trait bounds use concrete associated bindings from the selected
/// application, then must be ground and projection-free. They are proved
/// separately for both the trait and implementation declaration. Projections
/// belonging to other applications are unsupported.
/// Default bodies are excluded. Results remain tied to this database; this API
/// provides neither cross-stage transport nor an exception to coherence.
pub fn evaluate_selected_const_method<'db>(
    db: &'db DriverDataBase,
    file: File,
    selected: ImplTrait<'db>,
    goal: TraitInstId<'db>,
    method: IdentId<'db>,
) -> Result<SemConstId<'db>, SelectedMethodError<'db>> {
    let top_mod = db.top_mod(file);
    let ingot = top_mod.ingot(db);
    if ingot.kind(db) != IngotKind::StandAlone
        || ingot
            .resolved_external_ingots(db)
            .iter()
            .any(|(_, dependency)| !matches!(dependency.kind(db), IngotKind::Core | IngotKind::Std))
        || !top_mod
            .children_non_nested(db)
            .any(|item| item == ItemKind::ImplTrait(selected))
        || selected
            .span()
            .resolve(db)
            .is_none_or(|span| span.file != file)
        || collect_flags(db, goal).contains(TyFlags::HAS_PARAM)
    {
        return Err(SelectedMethodError::Unsupported);
    }
    if let Some(message) = diagnostics(db, file) {
        return Err(SelectedMethodError::Source(message));
    }
    let evidence = prove_selected_hir_impl_evidence(
        db,
        TraitSolveCx::new(db, top_mod.scope()),
        selected,
        goal,
    )
    .map_err(SelectedMethodError::Evidence)?;
    let (function, args) = evidence
        .nullary_const_method(method)
        .map_err(SelectedMethodError::Evidence)?
        .ok_or(SelectedMethodError::MissingMethod)?;
    match eval_body_owner_const_with_args(
        db,
        BodyOwner::Func(function),
        GenericSubst::complete(db, args),
        Vec::new(),
    ) {
        EvalOutcome::Ready(value) => Ok(value),
        EvalOutcome::Blocked(info) => Err(SelectedMethodError::Blocked(info)),
        EvalOutcome::Failed(failure) => Err(SelectedMethodError::Execution(failure)),
    }
}
