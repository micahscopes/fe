use crate::analysis::HirAnalysisDb;
use crate::analysis::ty::trait_def::TraitInstId;
use crate::analysis::ty::trait_resolution::{
    GoalSatisfiability, Selection, TraitSolveCx, is_goal_satisfiable,
};
use crate::hir_def::scope_graph::ScopeId;

/// Context independence: whether a selection made at a position is also
/// made, and its prerequisites proved, with no caller premises at all. A callee
/// whose choices hold this way can shed its caller's context.
///
/// The question is what holds without the caller's context, so the solver
/// context behind it deliberately has no premises. It is never handed out:
/// callers can only ask the questions below.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WithoutCallerPremises<'db> {
    solve_cx: TraitSolveCx<'db>,
}

impl<'db> WithoutCallerPremises<'db> {
    /// The callee owner's scope with no premises.
    pub(crate) fn at(db: &'db dyn HirAnalysisDb, owner: ScopeId<'db>) -> Self {
        Self {
            solve_cx: TraitSolveCx::without_caller_premises(
                db,
                owner,
                super::context::context_authority(),
            ),
        }
    }

    /// Ordinary selection without caller premises picks `resolved` again.
    pub(crate) fn selects_method(
        self,
        db: &'db dyn HirAnalysisDb,
        resolved: crate::analysis::ty::trait_def::ResolvedImplInstance<'db>,
        method: crate::hir_def::IdentId<'db>,
    ) -> bool {
        matches!(
            crate::analysis::ty::trait_def::resolve_trait_method_instance(
                db,
                self.solve_cx,
                resolved.trait_inst(),
                method,
            ),
            Selection::Unique(found) if found.resolved() == resolved
        )
    }

    /// Prove `goal` without caller premises.
    pub(crate) fn prove(
        self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> GoalSatisfiability<'db> {
        is_goal_satisfiable(db, self.solve_cx, goal)
    }

    /// Re-establish a retained selection without caller premises.
    pub(crate) fn revalidate(
        self,
        db: &'db dyn HirAnalysisDb,
        selection: crate::analysis::semantic::SelectedApplication<'db>,
    ) -> Result<
        crate::analysis::ty::trait_def::SelectedImplEvidence<'db>,
        crate::analysis::ty::trait_def::SelectedEvidenceError,
    > {
        selection.revalidate(db, self.solve_cx, selection.goal())
    }

    /// Re-prove a recorded choice without caller premises.
    pub(crate) fn prove_choice(
        self,
        db: &'db dyn HirAnalysisDb,
        choice: crate::analysis::ty::ty_check::TraitChoiceId<'db>,
    ) -> Result<
        crate::analysis::ty::ty_check::TraitChoiceId<'db>,
        crate::analysis::ty::trait_def::SelectedEvidenceError,
    > {
        choice.in_proven_context(db, self.solve_cx)
    }
}
