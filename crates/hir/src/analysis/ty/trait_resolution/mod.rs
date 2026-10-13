use super::{
    binder::Binder,
    canonical::{Canonical, Canonicalized, Solution},
    const_expr::ConstExpr,
    const_ty::ConstTyData,
    fold::{TyFoldable, ty_contains},
    normalize::normalize_from_assumptions,
    trait_def::{ImplementorId, TraitInstId},
    ty_def::{TyData, TyFlags, TyId},
    visitor::{TyVisitable, TyVisitor},
};
use crate::analysis::{
    HirAnalysisDb,
    semantic::{ConstRepr, SemConstId, SemConstValue, sem_const_ty},
    ty::{
        trait_resolution::{
            constraint::ty_constraints,
            table_solver::{TargetAnswer, TargetSolutionMatch, solve},
        },
        unify::UnificationTable,
    },
};
use crate::{
    Ingot,
    hir_def::{CallableDef, HirIngot, Trait, scope_graph::ScopeId},
};
use common::indexmap::IndexSet;
use constraint::collect_constraints;
use rustc_hash::FxHashSet;
use salsa::Update;
use std::cell::Cell;

pub(crate) mod constraint;
mod induction;
mod table_solver;

pub(crate) use table_solver::TargetSearchResult;
pub use table_solver::TargetSolutionStatus;

pub(crate) const TRAIT_SOLVER_ROOT_ANSWER_LIMIT: usize = 2;

/// The provisions in force at a position, as proof search reads them: each
/// visible goal with the implementation a named choice selects, or `None` when
/// opened evidence (or a transported witness) answers it. Search reads nothing
/// else of the position, so an ordinary position shares the empty key.
#[salsa::interned]
#[derive(Debug)]
pub struct ProofProvisions<'db> {
    #[return_ref]
    pub entries: Vec<(TraitInstId<'db>, Option<ImplementorId<'db>>)>,
}

impl<'db> ProofProvisions<'db> {
    pub fn none(db: &'db dyn HirAnalysisDb) -> Self {
        Self::new(db, Vec::new())
    }

    /// These provisions, with `inner`'s answering their own goals first, as an
    /// inner frame shadows an outer one.
    pub(crate) fn within(self, db: &'db dyn HirAnalysisDb, inner: Self) -> Self {
        if inner.entries(db).is_empty() {
            return self;
        }
        let same = |a: TraitInstId<'db>, b: TraitInstId<'db>| {
            a.def(db) == b.def(db) && a.args(db) == b.args(db)
        };
        let entries = self
            .entries(db)
            .iter()
            .copied()
            .filter(|(goal, _)| !inner.entries(db).iter().any(|(own, _)| same(*goal, *own)))
            .chain(inner.entries(db).iter().copied())
            .collect::<Vec<_>>();
        Self::new(db, entries)
    }
}

impl<'db> TyVisitable<'db> for ProofProvisions<'db> {
    fn visit_with<V: TyVisitor<'db> + ?Sized>(&self, visitor: &mut V) {
        for (goal, candidate) in self.entries(visitor.db()) {
            goal.visit_with(visitor);
            if let Some(candidate) = candidate {
                candidate.visit_with(visitor);
            }
        }
    }
}

impl<'db> TyFoldable<'db> for ProofProvisions<'db> {
    fn super_fold_with<F: super::fold::TyFolder<'db>>(
        self,
        db: &'db dyn HirAnalysisDb,
        folder: &mut F,
    ) -> Self {
        let entries = self
            .entries(db)
            .iter()
            .map(|&(goal, candidate)| {
                (
                    goal.fold_with(db, folder),
                    candidate.map(|candidate| candidate.fold_with(db, folder)),
                )
            })
            .collect::<Vec<_>>();
        Self::new(db, entries)
    }
}

// RD74 permits pure, fueled type normalization in proof search. Impl const
// conditions belong to the checker after the proof returns. Keep this guard
// across indirect helper calls and cached obligation queries.
std::thread_local! {
    static PROOF_SEARCH_DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub(super) struct ProofSearchGuard;

pub(super) fn enter_proof_search() -> ProofSearchGuard {
    PROOF_SEARCH_DEPTH.with(|depth| depth.set(depth.get() + 1));
    ProofSearchGuard
}

impl Drop for ProofSearchGuard {
    fn drop(&mut self) {
        PROOF_SEARCH_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}

pub(crate) fn assert_outside_proof_search() {
    assert!(
        PROOF_SEARCH_DEPTH.with(|depth| depth.get() == 0),
        "impl const condition discharged during trait proof search"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Update)]
pub struct TraitSolverQuery<'db> {
    pub goal: TraitInstId<'db>,
    pub assumptions: PredicateListId<'db>,
    /// Select an implementation at this goal; obligations still use assumptions.
    pub require_impl: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalGoalQuery<'db> {
    raw: TraitSolverQuery<'db>,
    canonical: Canonical<TraitSolverQuery<'db>>,
    original: Canonicalized<'db, TraitSolverQuery<'db>>,
}

impl<'db> CanonicalGoalQuery<'db> {
    pub fn new(
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
        assumptions: PredicateListId<'db>,
    ) -> Self {
        Self::from_query(
            db,
            TraitSolverQuery {
                goal,
                assumptions: assumptions.extend_all_bounds(db),
                require_impl: false,
            },
        )
    }

    pub fn from_query(db: &'db dyn HirAnalysisDb, raw: TraitSolverQuery<'db>) -> Self {
        let original = Canonicalized::new(db, raw);
        Self {
            raw,
            canonical: original.canonical(),
            original,
        }
    }

    pub fn goal(&self) -> TraitInstId<'db> {
        self.raw.goal
    }

    pub fn assumptions(&self) -> PredicateListId<'db> {
        self.raw.assumptions
    }

    pub fn canonical(&self) -> Canonical<TraitSolverQuery<'db>> {
        self.canonical
    }

    pub fn extract_solution<S, U>(
        &self,
        table: &mut crate::analysis::ty::unify::UnificationTableBase<'db, S>,
        solution: Solution<U>,
    ) -> U
    where
        S: crate::analysis::ty::unify::UnificationStore<'db>,
        U: TyFoldable<'db> + Update,
    {
        self.original.extract_solution(table, solution)
    }

    pub fn extract_subgoal<S>(
        &self,
        table: &mut crate::analysis::ty::unify::UnificationTableBase<'db, S>,
        solution: Solution<TraitInstId<'db>>,
    ) -> TraitInstId<'db>
    where
        S: crate::analysis::ty::unify::UnificationStore<'db>,
    {
        self.extract_solution(table, solution)
    }
}

/// Returns `true` if the given two implementors conflict.
///
/// Different named selections may overlap unless the trait is `#[fixed]`.
/// Within one selection, retain the ordinary coherence check:
/// - instantiate both implementors with fresh vars and unify them;
/// - then check that the merged constraints are satisfiable.
pub(crate) fn does_impl_trait_conflict<'db>(
    db: &'db dyn HirAnalysisDb,
    a: ImplementorId<'db>,
    b: ImplementorId<'db>,
) -> bool {
    let a_class = a.selection_class(db);
    let b_class = b.selection_class(db);
    let trait_ = a.trait_def(db);
    let fixed = crate::hir_def::ItemKind::Trait(trait_)
        .attrs(db)
        .is_some_and(|attrs| attrs.has_attr(db, "fixed"));
    if !fixed && a_class != b_class {
        return false;
    }
    let mut table = UnificationTable::new(db);
    let a = table.instantiate_with_fresh_vars(a);
    let b = table.instantiate_with_fresh_vars(b);

    if table.unify(a, b).is_err() {
        return false;
    }

    let a_constraints = a.constraints(db);
    let b_constraints = b.constraints(db);

    if a_constraints.is_empty(db) && b_constraints.is_empty(db) {
        return true;
    }

    // Check if all constraints from both implementations would be satisfiable
    // when the types are unified.
    let merged_constraints = a_constraints.merge(db, b_constraints);
    let solve_cx = TraitSolveCx::coherence(db, trait_);

    for &constraint in merged_constraints.list(db) {
        let constraint = constraint.fold_with(db, &mut table);

        match is_goal_satisfiable(db, solve_cx, constraint) {
            GoalSatisfiability::UnSat(_) | GoalSatisfiability::ContainsInvalid => {
                return false;
            }
            _ => {
                // Constraint is satisfiable or needs more information, continue checking.
            }
        }
    }

    true
}

/// A trait method's substituted bounds and the implementing method's bounds,
/// both normalized for comparison, with the context that asks whether the
/// former entail the latter.
pub(crate) struct SignatureComparison<'db> {
    pub(crate) context: TraitSolveCx<'db>,
    pub(crate) impl_bounds: PredicateListId<'db>,
    pub(crate) trait_bounds: PredicateListId<'db>,
}

#[derive(Debug, Clone)]
pub enum Selection<T> {
    Unique(T),
    Ambiguous(IndexSet<T>),
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Update)]
pub struct TraitSolveCx<'db> {
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
    provisions: ProofProvisions<'db>,
}

impl<'db> TraitSolveCx<'db> {
    /// The sole solver construction from a position's premise projection.
    pub(crate) fn at_position(
        db: &'db dyn HirAnalysisDb,
        premises: crate::analysis::semantic::SolverPremises<'db>,
        provisions: ProofProvisions<'db>,
        _authority: crate::core::semantic::ContextAuthority,
    ) -> Self {
        Self::new(db, premises.scope())
            .with_assumptions(premises.assumptions())
            .with_provisions(provisions)
    }

    fn new(db: &'db dyn HirAnalysisDb, scope: ScopeId<'db>) -> Self {
        Self {
            scope,
            assumptions: PredicateListId::empty_list(db),
            provisions: ProofProvisions::none(db),
        }
    }

    /// The semantic independence view asks what holds without caller premises.
    /// Its owner keeps this prover private and exposes only that question.
    pub(crate) fn without_caller_premises(
        db: &'db dyn HirAnalysisDb,
        scope: ScopeId<'db>,
        _authority: crate::core::semantic::ContextAuthority,
    ) -> Self {
        Self::new(db, scope)
    }

    /// Every proof made with this context sees the same provisions: a named
    /// choice restricts its goal to the chosen impl, whose prerequisites are
    /// still proved; opened evidence answers its own goal.
    fn with_provisions(self, provisions: ProofProvisions<'db>) -> Self {
        Self { provisions, ..self }
    }

    pub(crate) fn provisions(self) -> ProofProvisions<'db> {
        self.provisions
    }

    fn with_assumptions(self, assumptions: PredicateListId<'db>) -> Self {
        Self {
            assumptions,
            ..self
        }
    }

    /// Retain an accepted choice's source provisions unless shadowed at its
    /// destination. The destination's scope and premises stay fixed.
    pub(crate) fn transported_destination(
        self,
        db: &'db dyn HirAnalysisDb,
        origin: Self,
        _authority: crate::core::semantic::ContextAuthority,
    ) -> Self {
        self.with_provisions(origin.provisions.within(db, self.provisions))
    }

    /// Substitute a captured context without changing its source position.
    pub(crate) fn fold_at_captured_origin<F: crate::analysis::ty::fold::TyFolder<'db>>(
        self,
        db: &'db dyn HirAnalysisDb,
        folder: &mut F,
        _authority: crate::core::semantic::ContextAuthority,
    ) -> Self {
        use crate::analysis::ty::fold::TyFoldable;
        self.with_assumptions(self.assumptions.fold_with(db, folder))
            .with_provisions(self.provisions.fold_with(db, folder))
    }

    /// Coherence: whether two implementations of a trait can overlap. This is
    /// a question about the implementations themselves, not about any program
    /// position, so it deliberately has no premises. Only the overlap check
    /// below asks it.
    fn coherence(db: &'db dyn HirAnalysisDb, trait_: Trait<'db>) -> Self {
        Self::new(db, trait_.scope())
    }

    /// Signature comparison: whether a trait method's declared bounds, after
    /// substituting the implementation's arguments, entail the implementing
    /// method's bounds. The premises are that substituted signature, which no
    /// program position states, so they are not read from `context_at`. They
    /// are built here from the two methods and the substitution, so this
    /// context can only answer that question.
    pub(crate) fn signature_comparison(
        db: &'db dyn HirAnalysisDb,
        impl_method: CallableDef<'db>,
        trait_method: CallableDef<'db>,
        trait_inst: TraitInstId<'db>,
        param_subst: &crate::analysis::ty::ty_lower::CompleteSubst<'db>,
    ) -> SignatureComparison<'db> {
        use crate::analysis::ty::method_cmp::{
            instantiate_trait_method_template, normalize_predicates_for_comparison,
        };
        use constraint::collect_func_def_constraints;
        let impl_bounds =
            collect_func_def_constraints(db, impl_method, false).instantiate_identity();
        let trait_bounds = instantiate_trait_method_template(
            db,
            collect_func_def_constraints(db, trait_method, false),
            param_subst,
        );
        let compare_assumptions = impl_bounds.merge(db, trait_bounds).extend_all_bounds(db);
        let impl_bounds = normalize_predicates_for_comparison(
            db,
            impl_bounds,
            trait_method.scope(),
            compare_assumptions,
            trait_inst,
            false,
        );
        let trait_bounds = normalize_predicates_for_comparison(
            db,
            trait_bounds,
            trait_method.scope(),
            compare_assumptions,
            trait_inst,
            true,
        );
        SignatureComparison {
            context: Self::new(db, trait_method.scope()).with_assumptions(trait_bounds),
            impl_bounds,
            trait_bounds,
        }
    }

    /// The canonical proof query for `goal` under this context's premises.
    pub(crate) fn goal_query(
        self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> CanonicalGoalQuery<'db> {
        CanonicalGoalQuery::new(db, goal, self.assumptions)
    }

    pub fn assumptions(self) -> PredicateListId<'db> {
        self.assumptions
    }

    pub(crate) fn scope(self) -> ScopeId<'db> {
        self.scope
    }

    /// Retain the proof and its conditions until the caller chooses its view.
    pub(crate) fn select_impl_proof(
        self,
        db: &'db dyn HirAnalysisDb,
        inst: TraitInstId<'db>,
    ) -> GoalSatisfiability<'db> {
        let scope = self.normalization_scope_for_trait_inst(db, inst);
        let inst = normalize_trait_inst_preserving_validity(db, inst, scope, self.assumptions);
        // An assumption proves a bound; it is not a second implementation.
        // Keep inference goals on the ordinary proof query: an assumption may
        // select a different substitution from the implementations in scope.
        if inst.args(db).iter().any(|ty| ty.has_var(db))
            || inst
                .assoc_type_bindings(db)
                .values()
                .any(|ty| ty.has_var(db))
        {
            is_goal_satisfiable(db, self, inst)
        } else {
            let query = self.implementation_goal_query(db, inst);
            match is_goal_query_satisfiable(db, self, &query) {
                // A bound with no provable implementation can still be supplied
                // by the caller. Incomplete searches cannot establish this.
                GoalSatisfiability::UnSat(_) => is_goal_satisfiable(db, self, inst),
                result => result,
            }
        }
    }

    /// Specialize a known root effect provider from an implementation, rather
    /// than from the effect bound whose layout holes are being established.
    /// Prerequisites still use this position's premises and provisions.
    pub(crate) fn provider_implementation_proof(
        self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> GoalSatisfiability<'db> {
        let query = self.implementation_goal_query(db, goal);
        is_goal_query_satisfiable(db, self, &query)
    }

    fn implementation_goal_query(
        self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> CanonicalGoalQuery<'db> {
        CanonicalGoalQuery::from_query(
            db,
            TraitSolverQuery {
                goal,
                assumptions: self.assumptions.extend_all_bounds(db),
                require_impl: true,
            },
        )
    }

    pub(crate) fn search_ingots_for_trait_inst(
        self,
        db: &'db dyn HirAnalysisDb,
        inst: TraitInstId<'db>,
    ) -> (Ingot<'db>, Option<Ingot<'db>>) {
        Self::search_ingots_for_trait_inst_with_origin(db, self.scope.ingot(db), inst)
    }

    pub(crate) fn search_ingots_for_trait_inst_with_origin(
        db: &'db dyn HirAnalysisDb,
        origin_ingot: Ingot<'db>,
        inst: TraitInstId<'db>,
    ) -> (Ingot<'db>, Option<Ingot<'db>>) {
        let trait_ingot = inst.def(db).ingot(db);
        let self_ty = inst.self_ty(db);
        let self_ingot = self_ty.ingot(db).or_else(|| {
            // For projection `Self` types that still don't yield an ingot (e.g. all-trait-param
            // args), fall back to other trait arguments as a best-effort proxy.
            match self_ty.data(db) {
                TyData::AssocTy(_) | TyData::QualifiedTy(_) => {
                    inst.args(db).iter().skip(1).find_map(|ty| ty.ingot(db))
                }
                _ => None,
            }
        });

        let primary = self_ingot.unwrap_or(origin_ingot);
        if primary == trait_ingot {
            (primary, None)
        } else {
            (primary, Some(trait_ingot))
        }
    }

    pub(crate) fn normalization_scope_for_trait_inst(
        self,
        db: &'db dyn HirAnalysisDb,
        inst: TraitInstId<'db>,
    ) -> ScopeId<'db> {
        Self::normalization_scope_for_trait_inst_with_origin(db, self.scope.ingot(db), inst)
    }

    /// A selected method's input layouts are compared under the caller's
    /// premises and the trait application already established by selection.
    /// Retain this projection on the method rather than rebuilding it for
    /// each checked input; this premise-only view introduces no provisions.
    pub(crate) fn selected_method_layout_premises(
        self,
        db: &'db dyn HirAnalysisDb,
        inst: TraitInstId<'db>,
    ) -> crate::analysis::semantic::SolverPremises<'db> {
        let mut predicates: IndexSet<_> = self.assumptions.list(db).iter().copied().collect();
        predicates.insert(inst);
        unpositioned_premises(
            self.normalization_scope_for_trait_inst(db, inst),
            PredicateListId::new(db, predicates.into_iter().collect::<Vec<_>>()),
        )
    }

    pub(crate) fn normalization_scope_for_trait_inst_with_origin(
        db: &'db dyn HirAnalysisDb,
        origin_ingot: Ingot<'db>,
        inst: TraitInstId<'db>,
    ) -> ScopeId<'db> {
        let norm_ingot = inst
            .self_ty(db)
            .ingot(db)
            .or_else(|| inst.args(db).iter().find_map(|ty| ty.ingot(db)))
            .unwrap_or(origin_ingot);
        norm_ingot.root_mod(db).scope()
    }

    // Type normalization uses the ingot context. The caller scope is retained
    // separately for const conditions and the solver's cache identity.
    pub(crate) fn origin_scope(self, db: &'db dyn HirAnalysisDb) -> ScopeId<'db> {
        self.scope.ingot(db).root_mod(db).scope()
    }
}

pub(crate) fn normalize_trait_inst_preserving_validity<'db>(
    db: &'db dyn HirAnalysisDb,
    inst: TraitInstId<'db>,
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
) -> TraitInstId<'db> {
    let normalized = inst.normalize(db, scope, assumptions);
    let original_has_invalid = inst.args(db).iter().copied().any(|ty| ty.has_invalid(db))
        || inst
            .assoc_type_bindings(db)
            .values()
            .copied()
            .any(|ty| ty.has_invalid(db));
    let normalized_has_invalid = normalized
        .args(db)
        .iter()
        .copied()
        .any(|ty| ty.has_invalid(db))
        || normalized
            .assoc_type_bindings(db)
            .values()
            .copied()
            .any(|ty| ty.has_invalid(db));
    if !original_has_invalid && normalized_has_invalid {
        inst
    } else {
        normalized
    }
}

#[salsa::tracked(
    return_ref,
    cycle_fn=is_query_satisfiable_cycle_recover,
    cycle_initial=is_query_satisfiable_cycle_initial
)]
fn is_query_satisfiable<'db>(
    db: &'db dyn HirAnalysisDb,
    use_scope: ScopeId<'db>,
    query: Canonical<TraitSolverQuery<'db>>,
    target: Option<TargetAnswer<'db>>,
    provisions: ProofProvisions<'db>,
) -> GoalSatisfiability<'db> {
    if query.flags(db).contains(TyFlags::HAS_INVALID) {
        return GoalSatisfiability::ContainsInvalid;
    };

    solve(db, use_scope, query, target, provisions)
}

fn is_query_satisfiable_cycle_initial<'db>(
    _db: &'db dyn HirAnalysisDb,
    _use_scope: ScopeId<'db>,
    _query: Canonical<TraitSolverQuery<'db>>,
    _target: Option<TargetAnswer<'db>>,
    _provisions: ProofProvisions<'db>,
) -> GoalSatisfiability<'db> {
    // A cycle can arise while collecting an impl whose constraints contain an associated-type
    // projection: resolving the projection needs the trait environment that is currently being
    // assembled for the outer goal. Treat the incomplete pass as ambiguous so callers keep the
    // candidate alive; the next fixpoint iteration can decide it once impl collection converges.
    GoalSatisfiability::NeedsConfirmation {
        solutions: IndexSet::default(),
        completion: TraitSolveCompletion::Cycle,
    }
}

fn is_query_satisfiable_cycle_recover<'db>(
    _db: &'db dyn HirAnalysisDb,
    _value: &GoalSatisfiability<'db>,
    _count: u32,
    _use_scope: ScopeId<'db>,
    _query: Canonical<TraitSolverQuery<'db>>,
    _target: Option<TargetAnswer<'db>>,
    _provisions: ProofProvisions<'db>,
) -> salsa::CycleRecoveryAction<GoalSatisfiability<'db>> {
    salsa::CycleRecoveryAction::Iterate
}

fn query_has_solution<'db>(
    db: &'db dyn HirAnalysisDb,
    use_scope: ScopeId<'db>,
    query: Canonical<TraitSolverQuery<'db>>,
    target: (Canonical<TraitInstId<'db>>, TargetSolutionMatch),
    selected_candidate: Option<ImplementorId<'db>>,
    provisions: ProofProvisions<'db>,
) -> TargetSearchResult<'db> {
    let target = TargetAnswer::new(query, target.0, target.1, selected_candidate);
    match is_query_satisfiable(db, use_scope, query, Some(target), provisions) {
        GoalSatisfiability::NeedsConfirmation {
            solutions,
            completion: TraitSolveCompletion::TargetFound,
        } => TargetSearchResult {
            status: TargetSolutionStatus::Found,
            impl_conditions: Some(
                solutions
                    .first()
                    .expect("target result retains its proof answer")
                    .value
                    .impl_conditions,
            ),
        },
        GoalSatisfiability::NormalizationFault => {
            TargetSearchResult::without_conditions(TargetSolutionStatus::NormalizationFault)
        }
        GoalSatisfiability::NeedsConfirmation {
            completion: TraitSolveCompletion::Saturated,
            ..
        } => TargetSearchResult::without_conditions(TargetSolutionStatus::NotFound),
        GoalSatisfiability::NeedsConfirmation { .. } => {
            TargetSearchResult::without_conditions(TargetSolutionStatus::Incomplete)
        }
        // Saturation without the requested witness means the target is absent,
        // even if an ordinary answer for another instantiation was found.
        _ => TargetSearchResult::without_conditions(TargetSolutionStatus::NotFound),
    }
}

pub(crate) fn restricted_goal_query_result<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    candidate: ImplementorId<'db>,
    goal: TraitInstId<'db>,
) -> TargetSearchResult<'db> {
    let scope = solve_cx.normalization_scope_for_trait_inst(db, goal);
    let goal = normalize_trait_inst_preserving_validity(db, goal, scope, solve_cx.assumptions());
    let query = CanonicalGoalQuery::new(db, goal, solve_cx.assumptions());
    query_has_solution(
        db,
        solve_cx.origin_scope(db),
        query.canonical(),
        (Canonical::new(db, goal), TargetSolutionMatch::Equal),
        Some(candidate),
        solve_cx.provisions,
    )
}

pub(crate) fn goal_query_has_solution<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    query: &CanonicalGoalQuery<'db>,
    target: Canonical<TraitInstId<'db>>,
) -> bool {
    matches!(
        query_has_solution(
            db,
            solve_cx.origin_scope(db),
            query.canonical(),
            (target, TargetSolutionMatch::Equal),
            None,
            solve_cx.provisions,
        )
        .status,
        TargetSolutionStatus::Found
    )
}

/// Whether no answer for `query` differs from `target`, under the provisions
/// of `solve_cx`.
pub(crate) fn goal_query_has_no_distinct_solution<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    query: &CanonicalGoalQuery<'db>,
    target: Canonical<TraitInstId<'db>>,
) -> bool {
    matches!(
        query_has_solution(
            db,
            solve_cx.origin_scope(db),
            query.canonical(),
            (target, TargetSolutionMatch::NotEqual),
            None,
            solve_cx.provisions,
        )
        .status,
        TargetSolutionStatus::NotFound
    )
}

pub fn is_goal_query_satisfiable<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    query: &CanonicalGoalQuery<'db>,
) -> GoalSatisfiability<'db> {
    is_query_satisfiable(
        db,
        solve_cx.origin_scope(db),
        query.canonical(),
        None,
        solve_cx.provisions,
    )
    .clone()
}

/// Read-only proof questions. A consumer receives answers, never the prover
/// behind this view, and cannot replace its scope, premises or provisions.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProofQueries<'db>(TraitSolveCx<'db>);

impl<'db> ProofQueries<'db> {
    pub(crate) fn from_premises(
        db: &'db dyn HirAnalysisDb,
        premises: crate::analysis::semantic::SolverPremises<'db>,
    ) -> Self {
        Self(TraitSolveCx::new(db, premises.scope()).with_assumptions(premises.assumptions()))
    }
    pub(crate) fn from_context(context: TraitSolveCx<'db>) -> Self {
        Self(context)
    }
    pub(crate) fn assumptions(self) -> PredicateListId<'db> {
        self.0.assumptions()
    }
    pub(crate) fn goal_query(
        self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> CanonicalGoalQuery<'db> {
        self.0.goal_query(db, goal)
    }
    pub(crate) fn query_result(
        self,
        db: &'db dyn HirAnalysisDb,
        query: &CanonicalGoalQuery<'db>,
    ) -> GoalSatisfiability<'db> {
        assert_eq!(
            query.raw.assumptions,
            self.assumptions().extend_all_bounds(db),
            "proof query must retain its view's premises"
        );
        is_goal_query_satisfiable(db, self.0, query)
    }
    pub(crate) fn query_has_solution(
        self,
        db: &'db dyn HirAnalysisDb,
        query: &CanonicalGoalQuery<'db>,
        target: Canonical<TraitInstId<'db>>,
    ) -> bool {
        assert_eq!(
            query.raw.assumptions,
            self.assumptions().extend_all_bounds(db),
            "proof query must retain its view's premises"
        );
        goal_query_has_solution(db, self.0, query, target)
    }
    pub(crate) fn goal_result(
        self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> GoalSatisfiability<'db> {
        is_goal_satisfiable(db, self.0, goal)
    }
    pub(crate) fn selected_implementation_proof(
        self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> GoalSatisfiability<'db> {
        self.0.select_impl_proof(db, goal)
    }
    pub(crate) fn resolved_instance(
        self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> Selection<super::trait_def::ResolvedImplInstance<'db>> {
        super::trait_def::resolve_trait_impl_instance(db, self.0, goal)
    }
    pub(crate) fn search_ingots_for_trait_inst(
        self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> (Ingot<'db>, Option<Ingot<'db>>) {
        self.0.search_ingots_for_trait_inst(db, goal)
    }
    pub(crate) fn possible_implementations(
        self,
        db: &'db dyn HirAnalysisDb,
        ingot: Ingot<'db>,
        inst: Canonical<TraitInstId<'db>>,
    ) -> Vec<super::trait_def::ImplementorId<'db>> {
        super::trait_def::impls_for_ty_with_constraint_mode(
            db,
            ingot,
            super::trait_def::ImplConstraintQuery::TraitInstance(inst),
            self.0,
            true,
        )
    }
    pub(crate) fn satisfied_implementations(
        self,
        db: &'db dyn HirAnalysisDb,
        ingot: Ingot<'db>,
        ty: Canonical<TyId<'db>>,
    ) -> Vec<super::trait_def::ImplementorId<'db>> {
        super::trait_def::impls_for_ty_with_satisfied_constraints(db, ingot, ty, self.0)
    }
    pub(crate) fn application_requirements(
        self,
        db: &'db dyn HirAnalysisDb,
        ty: TyId<'db>,
        context: Option<crate::analysis::semantic::ImplEnv<'db>>,
    ) -> WellFormedness<'db> {
        check_type_application_requirements(db, self.0, ty, context)
    }
    pub(crate) fn copy_membership(
        self,
        db: &'db dyn HirAnalysisDb,
        inst: TraitInstId<'db>,
    ) -> bool {
        super::copy_goal_has_possible_impl(db, self.0, inst)
            && matches!(
                is_goal_satisfiable(db, self.0, inst),
                GoalSatisfiability::Satisfied(_)
            )
    }
}

/// A premise-only structural proof (RD108); it never chooses runtime dispatch
/// or imports named provisions, and exposes no context construction or edit.
pub fn is_goal_satisfiable_under_premises<'db>(
    db: &'db dyn HirAnalysisDb,
    premises: crate::analysis::semantic::SolverPremises<'db>,
    goal: TraitInstId<'db>,
) -> GoalSatisfiability<'db> {
    let context = TraitSolveCx::new(db, premises.scope()).with_assumptions(premises.assumptions());
    is_goal_satisfiable(db, context, goal)
}

pub fn is_goal_satisfiable<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    goal: TraitInstId<'db>,
) -> GoalSatisfiability<'db> {
    let query = CanonicalGoalQuery::new(db, goal, solve_cx.assumptions());
    is_goal_query_satisfiable(db, solve_cx, &query)
}

/// Checks if the given type is well-formed, i.e., the arguments of the given
/// type applications satisfies the constraints under the given assumptions.
#[salsa::tracked]
pub(crate) fn check_ty_wf<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    ty: TyId<'db>,
) -> WellFormedness<'db> {
    check_ty_wf_with_answers(db, solve_cx, ty, PredicateListId::empty_list(db))
}

/// Formation at a checked constructor reads its accepted root-bound answers.
/// These are decisions, not assumptions: concrete goals deliberately cannot
/// use assumptions in ordinary proof search. Nested formation is still checked.
#[salsa::tracked]
pub(crate) fn check_ty_wf_with_answers<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    root: TyId<'db>,
    answers: PredicateListId<'db>,
) -> WellFormedness<'db> {
    check_ty_formation(db, solve_cx, root, answers, None)
}

/// Checked lexical choices are available throughout the written type, unlike
/// answers recorded only for a constructor's root application. They remain
/// accepted decisions rather than ordinary proof-search assumptions.
#[salsa::tracked]
pub(crate) fn check_ty_wf_with_lexical_context<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    root: TyId<'db>,
    root_answers: PredicateListId<'db>,
    context: crate::analysis::semantic::ImplEnv<'db>,
) -> WellFormedness<'db> {
    check_ty_formation(db, solve_cx, root, root_answers, Some(context))
}

fn check_ty_formation<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    root: TyId<'db>,
    root_answers: PredicateListId<'db>,
    lexical_context: Option<crate::analysis::semantic::ImplEnv<'db>>,
) -> WellFormedness<'db> {
    let ty = root;
    let mut formation = WellFormedness::WellFormed;
    // Check arguments before their enclosing application without nesting a
    // tracked Salsa query for every node in a deep finite type. The base's
    // constraints are not checked as a partial application: that would
    // instantiate its binder with missing arguments.
    let mut pending = vec![(ty, false)];
    let mut seen = FxHashSet::default();
    while let Some((ty, ready)) = pending.pop() {
        let (base, args) = ty.decompose_ty_app(db);
        if !ready {
            if !seen.insert(ty) {
                continue;
            }
            pending.push((ty, true));
            pending.extend(args.iter().rev().map(|&arg| (arg, false)));
            continue;
        }
        let family_wf = check_type_application_requirements(db, solve_cx, ty, lexical_context);
        if !family_wf.is_wf() {
            return family_wf;
        }
        match base.data(db) {
            // The body is checked under its declaration's binder, not the caller's.
            TyData::TypeFamily { .. } => {}
            TyData::AssocTy(assoc) => {
                let wf = check_projected_trait_use_wf(db, solve_cx, assoc.trait_.as_predicate(db));
                if !wf.is_wf() {
                    return wf;
                }
                formation = formation.merge(db, wf);
            }
            TyData::ConstraintTerm(goal) => {
                for &arg in goal
                    .args(db)
                    .iter()
                    .chain(goal.assoc_type_bindings(db).values())
                {
                    let wf = check_ty_wf(db, solve_cx, arg);
                    if !wf.is_wf() {
                        return wf;
                    }
                    formation = formation.merge(db, wf);
                }
            }
            TyData::QualifiedTy(inst) => {
                let wf = check_projected_trait_use_wf(db, solve_cx, *inst);
                if !wf.is_wf() {
                    return wf;
                }
                formation = formation.merge(db, wf);
            }
            TyData::ConstTy(const_ty) => {
                let wf = check_const_ty_wf(db, solve_cx, *const_ty);
                if !wf.is_wf() {
                    return wf;
                }
                formation = formation.merge(db, wf);
            }
            TyData::TyApp(..)
            | TyData::TyVar(_)
            | TyData::TyParam(_)
            | TyData::TyBase(_)
            | TyData::Never
            | TyData::Invalid(_) => {}
        }

        let constraints = ty_constraints(db, ty);
        if constraints.is_empty(db) {
            continue;
        }
        let assumptions = solve_cx.assumptions();

        // Normalize constraints to resolve associated types.
        let normalized_constraints = {
            let scope = solve_cx.origin_scope(db);
            let normalized_list: Vec<_> = constraints
                .list(db)
                .iter()
                .map(|&goal| goal.normalize(db, scope, assumptions))
                .collect();
            PredicateListId::new(db, normalized_list)
        };

        for &goal in normalized_constraints.list(db) {
            if ty == root && root_answers.list(db).contains(&goal) {
                continue;
            }
            if let Some(selection) =
                lexical_context.and_then(|context| context.selection_for(db, goal))
            {
                // Slot matching is not proof of associated equalities. Read the
                // checked application and validate the complete requested goal;
                // an incompatible selection cannot fall back to ordinary lookup.
                if selection.revalidate(db, solve_cx, goal).is_err() {
                    return WellFormedness::IllFormed {
                        goal,
                        subgoal: None,
                    };
                }
                continue;
            }
            // Structural proof retains conditions for the authored use-site
            // reader; proof search itself never evaluates those conditions.
            let bound = unsatisfied_goal(db, solve_cx, goal);
            if !bound.is_wf() {
                return bound;
            }
            formation = formation.merge(db, bound);
        }
    }

    formation
}

fn check_const_ty_wf<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    const_ty: super::const_ty::ConstTyId<'db>,
) -> WellFormedness<'db> {
    let mut formation = WellFormedness::WellFormed;
    let wf = check_ty_wf(db, solve_cx, const_ty.ty(db));
    if !wf.is_wf() {
        return wf;
    }
    formation = formation.merge(db, wf);

    match const_ty.data(db) {
        ConstTyData::Computation { description, .. } => {
            let evaluated = const_ty.evaluate(db, Some(description.ty()));
            if evaluated != const_ty {
                return formation
                    .merge(db, check_ty_wf(db, solve_cx, TyId::const_ty(db, evaluated)));
            }
            if let ConstRepr::Term(term) = description.repr() {
                return formation.merge(db, check_ty_wf(db, solve_cx, TyId::const_ty(db, *term)));
            }
        }
        ConstTyData::Value(value) => {
            return formation.merge(db, check_sem_const_wf(db, solve_cx, value.value()));
        }
        ConstTyData::Description(value) => {
            return formation.merge(db, check_sem_const_wf(db, solve_cx, *value));
        }
        ConstTyData::Abstract(expr, _) => {
            let wf = check_const_expr_wf(db, solve_cx, *expr);
            if !wf.is_wf() {
                return wf;
            }
            formation = formation.merge(db, wf);
        }
        ConstTyData::TyVar(..)
        | ConstTyData::TyParam(..)
        | ConstTyData::Hole(..)
        | ConstTyData::Invalid(..)
        | ConstTyData::UnEvaluated { .. } => {}
    }

    formation
}

fn check_sem_const_wf<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    value: SemConstId<'db>,
) -> WellFormedness<'db> {
    let mut formation = WellFormedness::WellFormed;
    match value.value(db) {
        SemConstValue::Structural { ty, .. } => check_ty_wf(db, solve_cx, *ty),
        SemConstValue::Description(term) => check_ty_wf(db, solve_cx, TyId::const_ty(db, *term)),
        SemConstValue::Tuple { elems, .. } | SemConstValue::Array { elems, .. } => {
            for child in elems.iter().copied() {
                let wf = check_ty_wf(db, solve_cx, sem_const_ty(db, child));
                if !wf.is_wf() {
                    return wf;
                }
                formation = formation.merge(db, wf);
                let wf = check_sem_const_wf(db, solve_cx, child);
                if !wf.is_wf() {
                    return wf;
                }
                formation = formation.merge(db, wf);
            }
            formation
        }
        SemConstValue::Struct { fields, .. } | SemConstValue::Enum { fields, .. } => {
            for child in fields.iter().copied() {
                let wf = check_ty_wf(db, solve_cx, sem_const_ty(db, child));
                if !wf.is_wf() {
                    return wf;
                }
                formation = formation.merge(db, wf);
                let wf = check_sem_const_wf(db, solve_cx, child);
                if !wf.is_wf() {
                    return wf;
                }
                formation = formation.merge(db, wf);
            }
            formation
        }
        SemConstValue::Unit | SemConstValue::Scalar { .. } => formation,
    }
}

fn check_const_expr_wf<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    expr: super::const_expr::ConstExprId<'db>,
) -> WellFormedness<'db> {
    let mut formation = WellFormedness::WellFormed;
    struct TyCollector<'db> {
        db: &'db dyn HirAnalysisDb,
        tys: Vec<TyId<'db>>,
    }

    impl<'db> TyVisitor<'db> for TyCollector<'db> {
        fn db(&self) -> &'db dyn HirAnalysisDb {
            self.db
        }

        fn visit_ty(&mut self, ty: TyId<'db>) {
            self.tys.push(ty);
        }
    }

    match expr.data(db) {
        ConstExpr::Invocation(invocation) => {
            let mut collector = TyCollector {
                db,
                tys: Vec::new(),
            };
            invocation.key.visit_with(&mut collector);
            invocation.args.visit_with(&mut collector);
            for ty in collector.tys {
                let wf = check_ty_wf(db, solve_cx, ty);
                if !wf.is_wf() {
                    return wf;
                }
                formation = formation.merge(db, wf);
            }
        }
        ConstExpr::ArithBinOp { lhs, rhs, .. }
        | ConstExpr::ArrayRepeat {
            value: lhs,
            len: rhs,
        }
        | ConstExpr::ArrayIndex {
            array: lhs,
            index: rhs,
        } => {
            for ty in [*lhs, *rhs] {
                let wf = check_ty_wf(db, solve_cx, ty);
                if !wf.is_wf() {
                    return wf;
                }
                formation = formation.merge(db, wf);
            }
        }
        ConstExpr::UnOp { expr, .. } | ConstExpr::Field { value: expr, .. } => {
            let wf = check_ty_wf(db, solve_cx, *expr);
            if !wf.is_wf() {
                return wf;
            }
            formation = formation.merge(db, wf);
        }
        ConstExpr::Cast { expr, to } => {
            for ty in [*expr, *to] {
                let wf = check_ty_wf(db, solve_cx, ty);
                if !wf.is_wf() {
                    return wf;
                }
                formation = formation.merge(db, wf);
            }
        }
        ConstExpr::TraitConst(assoc) => {
            let wf = check_projected_trait_use_wf(db, solve_cx, assoc.inst());
            if !wf.is_wf() {
                return wf;
            }
            formation = formation.merge(db, wf);
        }
        ConstExpr::InherentConst(use_) => {
            let wf = check_ty_wf(db, solve_cx, use_.receiver_ty());
            if !wf.is_wf() {
                return wf;
            }
            formation = formation.merge(db, wf);
        }
    }

    formation
}

/// Check formation before normalization can erase a computed type or family. This
/// is also used by source-path diagnostics, which see intermediate projections.
/// Whether a bound promised for every argument holds: a family's declared
/// and parameter bounds, or a computed type's requirements. Only a completed
/// proof establishes it. An ambiguous or unfinished proof does not show that
/// the bound holds for all arguments, so it counts as a failure.
pub(crate) fn bound_holds_for_every_argument<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    goal: TraitInstId<'db>,
) -> bool {
    matches!(
        is_goal_satisfiable(db, solve_cx, goal),
        GoalSatisfiability::Satisfied(_)
    )
}

pub(crate) fn check_type_application_requirements<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    ty: TyId<'db>,
    lexical_context: Option<crate::analysis::semantic::ImplEnv<'db>>,
) -> WellFormedness<'db> {
    if let Some(name) = ty.lacks_args_for_parameter_bounds(db) {
        return WellFormedness::FamilyNeedsAllArgs { name };
    }
    let requirements = if matches!(
        ty.decompose_ty_app(db).0.data(db),
        TyData::TyBase(super::ty_def::TyBase::TypeFn(_))
    ) {
        Some(ty_constraints(db, ty))
    } else {
        ty.family_parameter_bounds(db)
    };
    if let Some(requirements) = requirements {
        for &goal in requirements.list(db) {
            let holds = match lexical_context.and_then(|context| context.selection_for(db, goal)) {
                Some(selection) => selection.revalidate(db, solve_cx, goal).is_ok(),
                None => bound_holds_for_every_argument(db, solve_cx, goal),
            };
            if !holds {
                return WellFormedness::IllFormed {
                    goal,
                    subgoal: None,
                };
            }
        }
    }
    WellFormedness::WellFormed
}

fn check_projected_trait_use_wf<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    inst: TraitInstId<'db>,
) -> WellFormedness<'db> {
    let mut formation = WellFormedness::WellFormed;
    for &arg in inst.args(db) {
        let wf = check_ty_wf(db, solve_cx, arg);
        if !wf.is_wf() {
            return wf;
        }
        formation = formation.merge(db, wf);
    }
    for &ty in inst.assoc_type_bindings(db).values() {
        let wf = check_ty_wf(db, solve_cx, ty);
        if !wf.is_wf() {
            return wf;
        }
        formation = formation.merge(db, wf);
    }

    formation.merge(db, unsatisfied_goal(db, solve_cx, inst))
}

fn unsatisfied_goal<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    goal: TraitInstId<'db>,
) -> WellFormedness<'db> {
    let query = CanonicalGoalQuery::new(db, goal, solve_cx.assumptions());
    let mut table = UnificationTable::new(db);
    match is_goal_query_satisfiable(db, solve_cx, &query) {
        GoalSatisfiability::Satisfied(solution) => {
            let conditions = query.extract_solution(&mut table, solution).impl_conditions;
            if conditions.instances(db).is_empty() && conditions.trait_goals(db).is_empty() {
                WellFormedness::WellFormed
            } else {
                WellFormedness::WithConditions(conditions)
            }
        }
        GoalSatisfiability::UnSat(subgoal) => {
            ill_formed_unless_by_induction(db, solve_cx, goal, &query, subgoal)
                .unwrap_or(WellFormedness::WellFormed)
        }
        _ => WellFormedness::WellFormed,
    }
}

/// A goal the solver could not satisfy makes a type ill formed, unless the
/// goal holds for every subject of a computed type by induction.
fn ill_formed_unless_by_induction<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    goal: TraitInstId<'db>,
    query: &CanonicalGoalQuery<'db>,
    subgoal: Option<Solution<TraitInstId<'db>>>,
) -> Option<WellFormedness<'db>> {
    if induction::discharge(db, solve_cx, goal) {
        return None;
    }
    let mut table = UnificationTable::new(db);
    let subgoal = subgoal.map(|subgoal| query.extract_subgoal(&mut table, subgoal));
    Some(WellFormedness::IllFormed { goal, subgoal })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Update)]
pub(crate) enum WellFormedness<'db> {
    WellFormed,
    WithConditions(ImplConditionsId<'db>),
    /// A family whose parameters have bounds, used without all of its
    /// arguments.
    FamilyNeedsAllArgs {
        name: crate::hir_def::IdentId<'db>,
    },
    IllFormed {
        goal: TraitInstId<'db>,
        subgoal: Option<TraitInstId<'db>>,
    },
}

impl<'db> WellFormedness<'db> {
    pub(crate) fn conditions(self) -> Option<ImplConditionsId<'db>> {
        match self {
            Self::WithConditions(conditions) => Some(conditions),
            _ => None,
        }
    }

    fn merge(self, db: &'db dyn HirAnalysisDb, other: Self) -> Self {
        if !self.is_wf() {
            return self;
        }
        if !other.is_wf() {
            return other;
        }
        match (self.conditions(), other.conditions()) {
            (None, _) => other,
            (_, None) => self,
            (Some(lhs), Some(rhs)) => {
                let mut instances = lhs.instances(db).clone();
                let mut goals = lhs.trait_goals(db).clone();
                for &instance in rhs.instances(db) {
                    if !instances.contains(&instance) {
                        instances.push(instance);
                    }
                }
                for &goal in rhs.trait_goals(db) {
                    if !goals.contains(&goal) {
                        goals.push(goal);
                    }
                }
                Self::WithConditions(ImplConditionsId::new(db, instances, goals))
            }
        }
    }

    pub(crate) fn is_wf(self) -> bool {
        matches!(
            self,
            WellFormedness::WellFormed | WellFormedness::WithConditions(_)
        )
    }

    pub(crate) fn without_subgoal(self) -> Self {
        match self {
            Self::IllFormed { goal, .. } => Self::IllFormed {
                goal,
                subgoal: None,
            },
            other => other,
        }
    }

    pub(crate) fn into_diag(
        self,
        span: crate::span::DynLazySpan<'db>,
    ) -> Option<super::diagnostics::TyDiagCollection<'db>> {
        use super::diagnostics::{TraitConstraintDiag, TyLowerDiag};
        match self {
            Self::WellFormed | Self::WithConditions(_) => None,
            Self::IllFormed { goal, subgoal } => Some(
                TraitConstraintDiag::TraitBoundNotSat {
                    span,
                    primary_goal: goal,
                    unsat_subgoal: subgoal,
                    required_by: None,
                    capability_hint: None,
                }
                .into(),
            ),
            Self::FamilyNeedsAllArgs { name } => {
                Some(TyLowerDiag::AssocTypeNeedsAllArgs { span, name }.into())
            }
        }
    }
}

/// Checks if the given trait instance are well-formed, i.e., the arguments of
/// the trait satisfies all constraints under the given assumptions.
#[salsa::tracked]
pub(crate) fn check_trait_inst_wf<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    trait_inst: TraitInstId<'db>,
) -> WellFormedness<'db> {
    check_trait_inst_wf_inner(db, solve_cx, trait_inst, false)
}

/// A recursive associated output bound promises that the output implements
/// this trait. Its `Self`-dependent parameter constraints are checked when an
/// impl supplies a concrete output. Independent constraints remain obligations
/// of the declaration.
pub(crate) fn check_trait_inst_wf_with_deferred_self_constraints<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    trait_inst: TraitInstId<'db>,
    defer_self_constraints: bool,
) -> WellFormedness<'db> {
    check_trait_inst_wf_inner(db, solve_cx, trait_inst, defer_self_constraints)
}

fn check_trait_inst_wf_inner<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    trait_inst: TraitInstId<'db>,
    defer_self_constraints: bool,
) -> WellFormedness<'db> {
    let mut formation = WellFormedness::WellFormed;
    for &arg in trait_inst.args(db) {
        let wf = check_ty_wf(db, solve_cx, arg);
        if !wf.is_wf() {
            return wf;
        }
        formation = formation.merge(db, wf);
    }
    for &ty in trait_inst.assoc_type_bindings(db).values() {
        let wf = check_ty_wf(db, solve_cx, ty);
        if !wf.is_wf() {
            return wf;
        }
        formation = formation.merge(db, wf);
    }

    let declared = collect_constraints(db, trait_inst.def(db).into());
    let deferred = defer_self_constraints.then(|| {
        let self_param = trait_inst.def(db).self_param(db);
        declared
            .skip_binder()
            .list(db)
            .iter()
            .map(|goal| {
                goal.args(db)
                    .iter()
                    .chain(goal.assoc_type_bindings(db).values())
                    .any(|&arg| ty_contains(db, arg, self_param))
            })
            .collect::<Vec<_>>()
    });
    let constraints = declared.instantiate(db, trait_inst.args(db));
    let assumptions = solve_cx.assumptions();

    // Normalize constraints after instantiation to resolve associated types
    let normalized_constraints = {
        let scope = solve_cx.normalization_scope_for_trait_inst(db, trait_inst);
        let normalized_list: Vec<_> = constraints
            .list(db)
            .iter()
            .enumerate()
            .filter_map(|(idx, &goal)| {
                (!deferred.as_ref().is_some_and(|flags| flags[idx])).then_some(goal)
            })
            .map(|goal| goal.normalize(db, scope, assumptions))
            .collect();
        PredicateListId::new(db, normalized_list)
    };

    for &goal in normalized_constraints.list(db) {
        let query = CanonicalGoalQuery::new(db, goal, assumptions);
        if let GoalSatisfiability::UnSat(subgoal) = is_goal_query_satisfiable(db, solve_cx, &query)
            && let Some(ill_formed) =
                ill_formed_unless_by_induction(db, solve_cx, goal, &query, subgoal)
        {
            return ill_formed;
        }
    }

    formation
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Update)]
pub struct TraitGoalSolution<'db> {
    pub(crate) inst: TraitInstId<'db>,
    pub(crate) implementor: ImplementorId<'db>,
    /// Const conditions left for the use-site checker after structural proof.
    pub(crate) impl_conditions: ImplConditionsId<'db>,
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ImplConditionsId<'db> {
    #[return_ref]
    pub instances: Vec<ImplementorId<'db>>,
    #[return_ref]
    pub trait_goals: Vec<TraitInstId<'db>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Update)]
pub enum TraitSolveCompletion {
    /// The complete least-fixpoint answer set was computed.
    Saturated,
    /// The configured number of root proof identities was reached.
    RootAnswerLimit { limit: usize },
    /// The engine's work-item budget was exhausted.
    StepLimit { limit: usize },
    /// The engine's canonical-table budget was exhausted.
    TableLimit { limit: usize },
    /// The engine's pending-work budget was exhausted.
    PendingWorkLimit { limit: usize },
    /// Fe's bounded-type-growth guard stopped the search.
    MaximumTypeDepth,
    /// A requested witness was found without enumerating the answer set.
    TargetFound,
    /// Salsa is iterating a recursive query to a fixpoint.
    Cycle,
}

impl TraitSolveCompletion {
    pub fn is_saturated(self) -> bool {
        matches!(self, Self::Saturated)
    }

    pub fn hit_root_answer_limit(self) -> bool {
        matches!(self, Self::RootAnswerLimit { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Update)]
pub enum GoalSatisfiability<'db> {
    /// Goal is satisfied with the unique solution.
    Satisfied(Solution<TraitGoalSolution<'db>>),
    /// The goal has multiple complete answers or resolution stopped before
    /// satisfiability and uniqueness could be decided.
    NeedsConfirmation {
        /// Complete or partial answers proved before resolution stopped.
        solutions: IndexSet<Solution<TraitGoalSolution<'db>>>,
        /// Whether the answer set is complete, or why resolution stopped.
        completion: TraitSolveCompletion,
    },

    /// Goal contains invalid.
    ContainsInvalid,
    /// A ground type or const needed for matching failed to normalize.
    NormalizationFault,
    /// The goal is not satisfied.
    /// It contains an unsatisfied subgoal if we can know the exact subgoal
    /// that makes the proof step stuck.
    UnSat(Option<Solution<TraitInstId<'db>>>),
}

#[salsa::interned]
#[derive(Debug)]
pub struct PredicateListId<'db> {
    #[return_ref]
    pub list: Vec<TraitInstId<'db>>,
}

impl<'db> PredicateListId<'db> {
    pub fn pretty_print(&self, db: &'db dyn HirAnalysisDb) -> String {
        format!(
            "{{{}}}",
            self.list(db)
                .iter()
                .map(|pred| pred.pretty_print(db, true))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    pub(crate) fn merge(self, db: &'db dyn HirAnalysisDb, other: Self) -> Self {
        let mut predicates: IndexSet<_> = self.list(db).iter().copied().collect();
        predicates.extend(other.list(db).iter().copied());
        PredicateListId::new(db, predicates.into_iter().collect::<Vec<_>>())
    }

    pub fn empty_list(db: &'db dyn HirAnalysisDb) -> Self {
        Self::new(db, Vec::new())
    }

    pub fn is_empty(self, db: &'db dyn HirAnalysisDb) -> bool {
        self.list(db).is_empty()
    }

    /// Transitively extends the predicate list with all implied bounds:
    /// - Super trait bounds
    /// - Associated type bounds from trait definitions
    pub fn extend_all_bounds(self, db: &'db dyn HirAnalysisDb) -> Self {
        extend_all_bounds_query(db, self)
    }

    fn extend_all_bounds_uncached(self, db: &'db dyn HirAnalysisDb) -> Self {
        let mut all_predicates: IndexSet<TraitInstId<'db>> =
            self.list(db).iter().copied().collect();

        let mut worklist: Vec<TraitInstId<'db>> = self.list(db).to_vec();

        while let Some(pred) = worklist.pop() {
            let hir_trait = pred.def(db);
            let evidence = PredicateListId::new(db, vec![pred]);
            // 1. Collect super traits
            for super_trait in hir_trait.super_traits(db) {
                let inst = normalize_from_assumptions(
                    db,
                    super_trait.instantiate(db, pred.args(db)),
                    unpositioned_premises(hir_trait.scope(), evidence),
                );
                if predicate_has_recursive_assoc_projection(db, inst) {
                    continue;
                }

                if all_predicates.insert(inst) {
                    // New predicate added, add to worklist for further processing
                    worklist.push(inst);
                }
            }

            // 2. Collect associated type bounds
            let formal_trait =
                TraitInstId::new_simple(db, hir_trait, hir_trait.params(db).to_vec());
            for trait_type in hir_trait.assoc_types(db) {
                // Family bounds quantify over a separate binder. They are
                // conditional rules for applied types, not predicates on the
                // bare constructor, and cannot enter this flat closure.
                if !trait_type.generic_params(db).data(db).is_empty() {
                    continue;
                }
                // Get the associated type name
                let Some(assoc_ty_name) = trait_type.name(db) else {
                    continue;
                };

                // Keep the entire bound in declaration coordinates until every
                // formal, including non-Self trait parameters, is instantiated.
                let assoc_ty = TyId::assoc_ty(db, formal_trait.trait_ref(db), assoc_ty_name);

                for bound in assoc_ty.assoc_type_bounds(db, trait_type) {
                    let trait_inst = normalize_from_assumptions(
                        db,
                        Binder::bind(hir_trait.into(), bound).instantiate(db, pred.args(db)),
                        unpositioned_premises(hir_trait.scope(), evidence),
                    );
                    if predicate_has_recursive_assoc_projection(db, trait_inst) {
                        continue;
                    }
                    if all_predicates.insert(trait_inst) {
                        worklist.push(trait_inst);
                    }
                }
            }
        }

        Self::new(db, all_predicates.into_iter().collect::<Vec<_>>())
    }
}

// Normalizing implied bounds can resolve traits under assumptions that lead
// back here. Iterate from the unextended list; extension only adds bounds.
#[salsa::tracked(
    cycle_initial = extend_all_bounds_cycle_initial,
    cycle_fn = extend_all_bounds_cycle_recover
)]
fn extend_all_bounds_query<'db>(
    db: &'db dyn HirAnalysisDb,
    predicates: PredicateListId<'db>,
) -> PredicateListId<'db> {
    predicates.extend_all_bounds_uncached(db)
}

fn extend_all_bounds_cycle_initial<'db>(
    _db: &'db dyn HirAnalysisDb,
    predicates: PredicateListId<'db>,
) -> PredicateListId<'db> {
    predicates
}

fn extend_all_bounds_cycle_recover<'db>(
    _db: &'db dyn HirAnalysisDb,
    _value: &PredicateListId<'db>,
    _count: u32,
    _predicates: PredicateListId<'db>,
) -> salsa::CycleRecoveryAction<PredicateListId<'db>> {
    salsa::CycleRecoveryAction::Iterate
}

/// Premises given to elaboration and premise normalization: only those
/// stated, never a position's context.
fn unpositioned_premises<'db>(
    scope: ScopeId<'db>,
    premises: PredicateListId<'db>,
) -> crate::analysis::semantic::SolverPremises<'db> {
    crate::analysis::semantic::SolverPremises::from_unpositioned_pair(scope, premises)
}

/// Whether a premise mentions an associated-type projection, the only thing
/// premise normalization can rewrite.
pub(crate) fn premise_has_projection<'db>(
    db: &'db dyn HirAnalysisDb,
    premise: TraitInstId<'db>,
) -> bool {
    premise
        .args(db)
        .iter()
        .chain(premise.assoc_type_bindings(db).values())
        .any(|ty| ty.has_projection(db))
}

/// A premise stated the way a goal is when the two are compared. Computed
/// once per premise and list, and only when the premise as stated does not
/// match. The normalized form is only for matching: it is not elaborated and
/// does not enter any premise list, so an implementation that wraps its
/// argument cannot make a list grow.
#[salsa::tracked(
    cycle_initial = normalized_premise_cycle_initial,
    cycle_fn = normalized_premise_cycle_recover
)]
pub(crate) fn normalized_premise<'db>(
    db: &'db dyn HirAnalysisDb,
    premise: TraitInstId<'db>,
    premises: PredicateListId<'db>,
) -> TraitInstId<'db> {
    normalize_premise(db, premise, premises)
}

fn normalized_premise_cycle_initial<'db>(
    _db: &'db dyn HirAnalysisDb,
    premise: TraitInstId<'db>,
    _premises: PredicateListId<'db>,
) -> TraitInstId<'db> {
    premise
}

fn normalized_premise_cycle_recover<'db>(
    _db: &'db dyn HirAnalysisDb,
    _value: &TraitInstId<'db>,
    _count: u32,
    _premise: TraitInstId<'db>,
    _premises: PredicateListId<'db>,
) -> salsa::CycleRecoveryAction<TraitInstId<'db>> {
    salsa::CycleRecoveryAction::Iterate
}

/// One premise normalized as a goal would be, without solving under the
/// list it belongs to: first by the associated equalities the list itself
/// establishes, then by implementations under no premises. A projection an
/// implementation fixes needs none, and solving under the list would re-enter
/// it. An implementation that needs a premise to apply is therefore not used.
fn normalize_premise<'db>(
    db: &'db dyn HirAnalysisDb,
    premise: TraitInstId<'db>,
    premises: PredicateListId<'db>,
) -> TraitInstId<'db> {
    let scope = TraitSolveCx::normalization_scope_for_trait_inst_with_origin(
        db,
        premise.def(db).ingot(db),
        premise,
    );
    let by_premises =
        normalize_from_assumptions(db, premise, unpositioned_premises(scope, premises));
    let normalized = by_premises.normalize(db, scope, PredicateListId::empty_list(db));
    let has_invalid = |inst: TraitInstId<'db>| {
        inst.args(db).iter().any(|ty| ty.has_invalid(db))
            || inst
                .assoc_type_bindings(db)
                .values()
                .any(|ty| ty.has_invalid(db))
    };
    // A normalization fault leaves the premise as far as it got.
    if !has_invalid(normalized) || has_invalid(premise) {
        normalized
    } else if !has_invalid(by_premises) {
        by_premises
    } else {
        premise
    }
}

fn predicate_has_recursive_assoc_projection<'db>(
    db: &'db dyn HirAnalysisDb,
    pred: TraitInstId<'db>,
) -> bool {
    pred.args(db)
        .iter()
        .any(|&arg| ty_has_recursive_assoc_projection(db, arg))
}

fn ty_has_recursive_assoc_projection<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> bool {
    fn impl_<'db>(
        db: &'db dyn HirAnalysisDb,
        ty: TyId<'db>,
        visited_tys: &mut FxHashSet<TyId<'db>>,
        seen_assoc_keys: &mut FxHashSet<(crate::hir_def::Trait<'db>, crate::hir_def::IdentId<'db>)>,
    ) -> bool {
        if !visited_tys.insert(ty) {
            return false;
        }

        let has_cycle = match ty.data(db) {
            TyData::ConstTy(const_ty) => impl_(db, const_ty.ty(db), visited_tys, seen_assoc_keys),
            TyData::AssocTy(assoc_ty) => {
                let key = (assoc_ty.trait_.def(db), assoc_ty.name);
                if !seen_assoc_keys.insert(key) {
                    true
                } else {
                    let has_cycle = assoc_ty
                        .trait_
                        .args(db)
                        .iter()
                        .copied()
                        .any(|arg| impl_(db, arg, visited_tys, seen_assoc_keys));
                    seen_assoc_keys.remove(&key);
                    has_cycle
                }
            }
            TyData::QualifiedTy(trait_inst) => {
                let args_have_cycle = trait_inst
                    .args(db)
                    .iter()
                    .copied()
                    .any(|arg| impl_(db, arg, visited_tys, seen_assoc_keys));
                let assoc_bindings_have_cycle = trait_inst
                    .assoc_type_bindings(db)
                    .values()
                    .copied()
                    .any(|ty| impl_(db, ty, visited_tys, seen_assoc_keys));
                args_have_cycle || assoc_bindings_have_cycle
            }
            TyData::TyApp(lhs, rhs) => {
                impl_(db, *lhs, visited_tys, seen_assoc_keys)
                    || impl_(db, *rhs, visited_tys, seen_assoc_keys)
            }
            _ => false,
        };

        visited_tys.remove(&ty);
        has_cycle
    }

    impl_(db, ty, &mut FxHashSet::default(), &mut FxHashSet::default())
}

#[cfg(test)]
mod tests {
    use common::indexmap::{IndexMap, IndexSet};
    use dir_test::{Fixture, dir_test};

    use super::{
        CanonicalGoalQuery, GoalSatisfiability, Selection, TargetSolutionStatus, TraitInstId,
        TraitSolveCompletion, TraitSolveCx, goal_query_has_solution, is_goal_query_satisfiable,
        is_goal_satisfiable, restricted_goal_query_result,
    };
    use crate::{
        analysis::ty::{
            adt_def::AdtRef,
            canonical::Canonical,
            trait_def::{
                ImplementorId, ImplementorOrigin, SelectedEvidenceError, impls_for_trait_in_ingots,
                prove_selected_impl_evidence, resolve_trait_impl_instance,
            },
            trait_resolution::{PredicateListId, constraint::collect_func_def_constraints},
            ty_def::{Kind, TyId, TyVarSort},
            ty_lower::collect_generic_params,
            unify::UnificationTable,
        },
        hir_def::{Func, IdentId, TopLevelMod, Trait},
        test_db::{HirAnalysisTestDb, find_func},
    };

    #[test]
    fn captured_context_folds_premises_without_moving_its_origin() {
        use crate::analysis::ty::fold::{TyFoldable, TyFolder};

        struct Replace<'db> {
            from: TyId<'db>,
            to: TyId<'db>,
        }
        impl<'db> TyFolder<'db> for Replace<'db> {
            fn fold_ty(
                &mut self,
                _db: &'db dyn crate::analysis::HirAnalysisDb,
                ty: TyId<'db>,
            ) -> TyId<'db> {
                if ty == self.from { self.to } else { ty }
            }
            fn fold_scope(
                &mut self,
                _scope: crate::hir_def::scope_graph::ScopeId<'db>,
            ) -> crate::hir_def::scope_graph::ScopeId<'db> {
                panic!("captured proof origin must not move during substitution")
            }
        }

        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "captured_context_fold.fe".into(),
            "trait Foo {}\nstruct First {}\nstruct Second {}\nimpl Foo for First {}\n",
        );
        let (top_mod, _) = db.top_mod(file);
        let first = named_struct_ty(&db, top_mod, "First");
        let second = named_struct_ty(&db, top_mod, "Second");
        let goal = TraitInstId::new(
            &db,
            named_trait(&db, top_mod, "Foo"),
            vec![first],
            IndexMap::new(),
        );
        let context = TraitSolveCx::new(&db, top_mod.scope())
            .with_assumptions(PredicateListId::new(&db, vec![goal]));
        let candidate = crate::analysis::ty::trait_def::impls_for_trait_def(
            &db,
            top_mod.scope().ingot(&db),
            goal.def(&db),
        )[0];
        let proof = prove_selected_impl_evidence(&db, context, candidate, goal)
            .expect("selected implementation supplies the retained context");
        let application = crate::analysis::semantic::SelectedApplication::from_evidence(&proof);
        let folded = application.fold_with(
            &db,
            &mut Replace {
                from: first,
                to: second,
            },
        );
        assert_eq!(folded.scope(), context.scope());
        assert_eq!(folded.goal().args(&db), &[second]);
        assert_eq!(folded.assumptions().list(&db)[0].args(&db), &[second]);
    }

    fn named_trait<'db>(
        db: &'db HirAnalysisTestDb,
        top_mod: TopLevelMod<'db>,
        name: &str,
    ) -> Trait<'db> {
        top_mod
            .all_traits(db)
            .iter()
            .copied()
            .find(|trait_| {
                trait_
                    .name(db)
                    .to_opt()
                    .is_some_and(|ident| ident.data(db) == name)
            })
            .unwrap_or_else(|| panic!("missing `{name}` trait"))
    }

    fn named_struct_ty<'db>(
        db: &'db HirAnalysisTestDb,
        top_mod: TopLevelMod<'db>,
        name: &str,
    ) -> TyId<'db> {
        let struct_ = top_mod
            .all_structs(db)
            .iter()
            .copied()
            .find(|struct_| {
                struct_
                    .name(db)
                    .to_opt()
                    .is_some_and(|ident| ident.data(db) == name)
            })
            .unwrap_or_else(|| panic!("missing `{name}` struct"));
        TyId::adt(db, AdtRef::from(struct_).as_adt(db))
    }

    fn nested_ty<'db>(
        db: &'db HirAnalysisTestDb,
        constructor: TyId<'db>,
        mut inner: TyId<'db>,
        depth: usize,
    ) -> TyId<'db> {
        for _ in 0..depth {
            inner = TyId::app(db, constructor, inner);
        }
        inner
    }

    fn candidates_for_goal<'db>(
        db: &'db HirAnalysisTestDb,
        top_mod: TopLevelMod<'db>,
        goal: TraitInstId<'db>,
    ) -> Vec<ImplementorId<'db>> {
        let solve_cx = TraitSolveCx::new(db, top_mod.scope());
        let (primary, secondary) = solve_cx.search_ingots_for_trait_inst(db, goal);
        impls_for_trait_in_ingots(db, primary, secondary, Canonical::new(db, goal), None).to_vec()
    }

    #[test]
    fn predicate_merge_is_stable_and_idempotent() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("predicate_merge.fe".into(), "trait A {}\ntrait B {}");
        let (top_mod, _) = db.top_mod(file);
        let a = TraitInstId::new(
            &db,
            named_trait(&db, top_mod, "A"),
            vec![TyId::bool(&db)],
            IndexMap::new(),
        );
        let b = TraitInstId::new(
            &db,
            named_trait(&db, top_mod, "B"),
            vec![TyId::bool(&db)],
            IndexMap::new(),
        );
        let left = PredicateListId::new(&db, vec![a, b, a]);
        let right = PredicateListId::new(&db, vec![b, a]);
        let merged = left.merge(&db, right);
        assert_eq!(merged.list(&db), &[a, b]);
        assert_eq!(merged.merge(&db, left), merged);
    }

    #[test]
    fn solver_query_includes_assumptions() {
        fn query_for<'db>(
            db: &'db HirAnalysisTestDb,
            func: Func<'db>,
            needs_a: Trait<'db>,
        ) -> (CanonicalGoalQuery<'db>, TraitSolveCx<'db>) {
            let ty_param = collect_generic_params(db, func.into()).explicit_params(db)[0];
            let assumptions =
                collect_func_def_constraints(db, func.into(), true).instantiate_identity();
            let goal =
                TraitInstId::new(db, needs_a, vec![TyId::unit(db), ty_param], IndexMap::new());
            let query = CanonicalGoalQuery::new(db, goal, assumptions);
            let solve_cx = TraitSolveCx::new(db, func.scope()).with_assumptions(assumptions);
            (query, solve_cx)
        }

        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "trait_solver_query_includes_assumptions.fe".into(),
            r#"
trait A {}
trait NeedsA<T> {}

impl<T: A> NeedsA<T> for () {}

fn with_a<T: A>() -> bool {
    true
}

fn without_a<T>() -> bool {
    true
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);

        let needs_a = top_mod
            .all_traits(&db)
            .iter()
            .copied()
            .find(|trait_| {
                trait_
                    .name(&db)
                    .to_opt()
                    .is_some_and(|name| name.data(&db) == "NeedsA")
            })
            .unwrap();
        let with_a = top_mod
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|func| {
                func.name(&db)
                    .to_opt()
                    .is_some_and(|name| name.data(&db) == "with_a")
            })
            .unwrap();
        let without_a = top_mod
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|func| {
                func.name(&db)
                    .to_opt()
                    .is_some_and(|name| name.data(&db) == "without_a")
            })
            .unwrap();

        let (with_query, with_cx) = query_for(&db, with_a, needs_a);
        let (without_query, without_cx) = query_for(&db, without_a, needs_a);

        assert_eq!(
            with_query.goal().pretty_print(&db, true),
            without_query.goal().pretty_print(&db, true)
        );
        assert_ne!(with_query.canonical(), without_query.canonical());
        assert!(matches!(
            is_goal_query_satisfiable(&db, with_cx, &with_query),
            GoalSatisfiability::Satisfied(_)
        ));
        assert!(matches!(
            is_goal_query_satisfiable(&db, without_cx, &without_query),
            GoalSatisfiability::UnSat(_)
        ));
    }

    #[test]
    fn tablesolve_classifies_cycles_and_distinct_implementors() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "tablesolve_classifies_cycles_and_distinct_implementors.fe".into(),
            r#"
trait Foo {}

struct Seed {}
struct SeedPeer {}
impl Foo for Seed {}
impl Foo for Seed where SeedPeer: Foo {}
impl Foo for SeedPeer where Seed: Foo {}

struct Dead {}
struct DeadPeer {}
impl Foo for Dead where DeadPeer: Foo {}
impl Foo for DeadPeer where Dead: Foo {}

struct Ambiguous {}
impl Foo for Ambiguous {}
impl Foo for Ambiguous {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let foo = named_trait(&db, top_mod, "Foo");
        let solve = |name| {
            let self_ty = named_struct_ty(&db, top_mod, name);
            let goal = TraitInstId::new(&db, foo, vec![self_ty], IndexMap::new());
            is_goal_satisfiable(&db, TraitSolveCx::new(&db, top_mod.scope()), goal)
        };

        assert!(matches!(
            solve("SeedPeer"),
            GoalSatisfiability::Satisfied(_)
        ));
        assert!(matches!(solve("Dead"), GoalSatisfiability::UnSat(Some(_))));
        assert!(matches!(
            solve("Ambiguous"),
            GoalSatisfiability::NeedsConfirmation {
                solutions,
                completion: TraitSolveCompletion::Saturated,
            } if solutions.len() == 2
        ));
    }

    #[test]
    fn tablesolve_bounds_growing_seedless_cycles() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "tablesolve_bounds_growing_seedless_cycles.fe".into(),
            r#"
trait Foo {}
struct Dead {}
struct Wrap<T> {}
impl<T> Foo for T where Wrap<T>: Foo {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let foo = named_trait(&db, top_mod, "Foo");
        let dead = named_struct_ty(&db, top_mod, "Dead");
        let wrap = named_struct_ty(&db, top_mod, "Wrap");
        let deep_root = nested_ty(&db, wrap, dead, 300);

        for self_ty in [dead, deep_root] {
            let goal = TraitInstId::new(&db, foo, vec![self_ty], IndexMap::new());
            assert!(matches!(
                is_goal_satisfiable(&db, TraitSolveCx::new(&db, top_mod.scope()), goal),
                GoalSatisfiability::NeedsConfirmation {
                    solutions,
                    completion: TraitSolveCompletion::MaximumTypeDepth,
                } if solutions.is_empty()
            ));
        }
    }

    #[test]
    fn tablesolve_allows_initially_deep_finite_queries() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "tablesolve_allows_initially_deep_finite_queries.fe".into(),
            r#"
trait Foo {}
trait Noise<T> {}

struct Leaf {}
struct Subject {}
struct Wrap<T> {}

impl<T> Foo for Wrap<T> {}
impl Foo for Subject {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let foo = named_trait(&db, top_mod, "Foo");
        let noise = named_trait(&db, top_mod, "Noise");
        let leaf = named_struct_ty(&db, top_mod, "Leaf");
        let subject = named_struct_ty(&db, top_mod, "Subject");
        let wrap = named_struct_ty(&db, top_mod, "Wrap");
        let deep = nested_ty(&db, wrap, leaf, 300);

        let deep_goal = TraitInstId::new(&db, foo, vec![deep], IndexMap::new());
        assert!(matches!(
            is_goal_satisfiable(&db, TraitSolveCx::new(&db, top_mod.scope()), deep_goal,),
            GoalSatisfiability::Satisfied(_)
        ));

        let unrelated_deep_assumption =
            TraitInstId::new(&db, noise, vec![subject, deep], IndexMap::new());
        let assumptions = PredicateListId::new(&db, vec![unrelated_deep_assumption]);
        let shallow_goal = TraitInstId::new(&db, foo, vec![subject], IndexMap::new());
        assert!(matches!(
            is_goal_satisfiable(
                &db,
                TraitSolveCx::new(&db, top_mod.scope()).with_assumptions(assumptions),
                shallow_goal,
            ),
            GoalSatisfiability::Satisfied(_)
        ));
    }

    #[test]
    fn tablesolve_preserves_answers_when_growth_limit_stops_search() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "tablesolve_preserves_answers_when_growth_limit_stops_search.fe".into(),
            r#"
trait Seed {}
trait Grow {}

struct Wrap<T> {}

impl<T> Grow for T where T: Seed {}
impl<T> Grow for T where Wrap<T>: Grow {}

fn probe<T: Seed>() {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let grow = named_trait(&db, top_mod, "Grow");
        let probe = top_mod
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|func| {
                func.name(&db)
                    .to_opt()
                    .is_some_and(|name| name.data(&db) == "probe")
            })
            .expect("missing `probe` function");
        let ty_param = collect_generic_params(&db, probe.into()).explicit_params(&db)[0];
        let assumptions =
            collect_func_def_constraints(&db, probe.into(), true).instantiate_identity();
        let goal = TraitInstId::new(&db, grow, vec![ty_param], IndexMap::new());

        assert!(matches!(
            is_goal_satisfiable(
                &db,
                TraitSolveCx::new(&db, top_mod.scope()).with_assumptions(assumptions),
                goal,
            ),
            GoalSatisfiability::NeedsConfirmation {
                solutions,
                completion: TraitSolveCompletion::MaximumTypeDepth,
            } if solutions.len() == 1
        ));
    }

    #[test]
    fn tablesolve_propagates_bindings_between_impl_constraints() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "tablesolve_propagates_bindings_between_impl_constraints.fe".into(),
            r#"
trait Goal<T> {}
trait Choose<T> {}
trait Accept<T> {}

struct Subject {}
struct A {}
struct B {}

impl Choose<A> for Subject {}
impl Accept<A> for Subject {}
impl Accept<B> for Subject {}
impl<SelfT, T> Goal<T> for SelfT where SelfT: Accept<T>, SelfT: Choose<T> {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let goal_trait = named_trait(&db, top_mod, "Goal");
        let subject = named_struct_ty(&db, top_mod, "Subject");
        let a = named_struct_ty(&db, top_mod, "A");
        let assumptions = PredicateListId::empty_list(&db);
        let solve_cx = TraitSolveCx::new(&db, top_mod.scope()).with_assumptions(assumptions);
        let mut table = UnificationTable::new(&db);
        let selected = table.new_var(TyVarSort::General, &Kind::Star);
        let goal = TraitInstId::new(&db, goal_trait, vec![subject, selected], IndexMap::new());
        let query = CanonicalGoalQuery::new(&db, goal, assumptions);
        let GoalSatisfiability::Satisfied(solution) =
            is_goal_query_satisfiable(&db, solve_cx, &query)
        else {
            panic!("the second constraint must observe the first constraint's binding");
        };
        let actual = query.extract_solution(&mut table, solution).inst;
        let expected = TraitInstId::new(&db, goal_trait, vec![subject, a], IndexMap::new());

        assert_eq!(Canonical::new(&db, actual), Canonical::new(&db, expected));
    }

    #[test]
    fn target_search_saturated_ambiguity_does_not_imply_a_distinct_instance() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "target_search_saturated_ambiguity.fe".into(),
            "trait Foo {}\nstruct First {}\nimpl Foo for First {}\nimpl Foo for First {}\n",
        );
        let (top_mod, _) = db.top_mod(file);
        let foo = named_trait(&db, top_mod, "Foo");
        let first = named_struct_ty(&db, top_mod, "First");
        let assumptions = PredicateListId::empty_list(&db);
        let context = TraitSolveCx::new(&db, top_mod.scope()).with_assumptions(assumptions);
        let goal = TraitInstId::new(&db, foo, vec![first], IndexMap::new());
        let query = CanonicalGoalQuery::new(&db, goal, assumptions);
        // As in the cutoff control, duplicate implementations test the solver
        // independently of declaration admission. Both prove the same instance.
        assert!(super::goal_query_has_no_distinct_solution(
            &db,
            context,
            &query,
            Canonical::new(&db, goal),
        ));
    }

    #[test]
    fn target_search_finds_an_answer_beyond_the_ambiguity_cutoff() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "target_search_finds_an_answer_beyond_the_ambiguity_cutoff.fe".into(),
            r#"
trait Foo {}
struct First {}
struct Second {}
struct Third {}
struct Wrap<T> {}
impl Foo for First {}
impl Foo for First {}
impl<T> Foo for T where Wrap<T>: Foo {}
impl Foo for Second {}
impl Foo for Third {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let foo = named_trait(&db, top_mod, "Foo");
        let assumptions = PredicateListId::empty_list(&db);
        let solve_cx = TraitSolveCx::new(&db, top_mod.scope()).with_assumptions(assumptions);
        let mut table = UnificationTable::new(&db);
        let self_ty = table.new_var(TyVarSort::General, &Kind::Star);
        let goal = TraitInstId::new(&db, foo, vec![self_ty], IndexMap::new());
        let query = CanonicalGoalQuery::new(&db, goal, assumptions);
        let GoalSatisfiability::NeedsConfirmation {
            solutions,
            completion: TraitSolveCompletion::RootAnswerLimit { limit: 2 },
        } = is_goal_query_satisfiable(&db, solve_cx, &query)
        else {
            panic!("the unconstrained goal must reach the configured answer cutoff");
        };
        assert_eq!(solutions.len(), 2);

        let returned: IndexSet<_> = solutions
            .iter()
            .map(|solution| Canonical::new(&db, solution.value.inst))
            .collect();
        assert_eq!(
            returned.len(),
            1,
            "the cutoff can contain two implementors of the same instance"
        );
        let target = ["First", "Second", "Third"]
            .into_iter()
            .map(|name| {
                let self_ty = named_struct_ty(&db, top_mod, name);
                Canonical::new(
                    &db,
                    TraitInstId::new(&db, foo, vec![self_ty], IndexMap::new()),
                )
            })
            .find(|candidate| !returned.contains(candidate))
            .expect("the two-answer cutoff must omit one implementation");

        assert!(goal_query_has_solution(&db, solve_cx, &query, target));
    }
    #[test]
    fn implementation_selection_uses_assumptions_for_obligations() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "implementation_selection.fe".into(),
            r#"
trait Bound {}
trait Picks { type Output }
struct Wrap<T> {}
impl<T: Bound> Picks for Wrap<T> { type Output = T }
fn supported<T: Bound>() {}
fn unsupported<T>() {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);
        let picks = named_trait(&db, top_mod, "Picks");
        let wrap = named_struct_ty(&db, top_mod, "Wrap");
        for (name, implementation) in [("supported", true), ("unsupported", false)] {
            let func = find_func(&db, top_mod, name);
            let parameter = collect_generic_params(&db, func.into()).explicit_params(&db)[0];
            let self_ty = TyId::app(&db, wrap, parameter);
            let goal = TraitInstId::new(&db, picks, vec![self_ty], IndexMap::new());
            let constraints =
                collect_func_def_constraints(&db, func.into(), true).instantiate_identity();
            let assumptions = PredicateListId::new(
                &db,
                constraints
                    .list(&db)
                    .iter()
                    .copied()
                    .chain([goal])
                    .collect::<Vec<_>>(),
            );
            let solve = TraitSolveCx::new(&db, func.scope()).with_assumptions(assumptions);
            let Selection::Unique(resolved) = resolve_trait_impl_instance(&db, solve, goal) else {
                panic!("{name}: expected unique evidence");
            };
            assert_eq!(
                !matches!(
                    resolved.selected().origin(&db),
                    ImplementorOrigin::Assumption
                ),
                implementation
            );
            if implementation {
                assert_eq!(
                    resolved.instantiated_assoc_ty(&db, IdentId::new(&db, "Output")),
                    Some(parameter)
                );
            }
        }
    }

    #[test]
    fn implementation_selection_preserves_competing_impls_with_an_assumption() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "competing_implementation_selection.fe".into(),
            r#"
trait Picks {}
struct Wrap<T> {}
impl<T> Picks for Wrap<T> {}
impl<T> Picks for Wrap<T> {}
fn probe<T>() {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let func = find_func(&db, top_mod, "probe");
        let parameter = collect_generic_params(&db, func.into()).explicit_params(&db)[0];
        let self_ty = TyId::app(&db, named_struct_ty(&db, top_mod, "Wrap"), parameter);
        let goal = TraitInstId::new(
            &db,
            named_trait(&db, top_mod, "Picks"),
            vec![self_ty],
            IndexMap::new(),
        );
        let solve = TraitSolveCx::new(&db, func.scope())
            .with_assumptions(PredicateListId::new(&db, vec![goal]));
        assert!(
            matches!(solve.select_impl_proof(&db, goal), GoalSatisfiability::NeedsConfirmation { solutions, .. } if solutions.len() == 2)
        );
    }

    #[test]
    fn implementation_selection_does_not_discard_inference_answers_from_assumptions() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "inferred_implementation_selection.fe".into(),
            r#"
trait Picks {}
struct Concrete {}
impl Picks for Concrete {}
fn probe<T: Picks>() {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);
        let func = find_func(&db, top_mod, "probe");
        let assumptions =
            collect_func_def_constraints(&db, func.into(), true).instantiate_identity();
        let mut table = UnificationTable::new(&db);
        let self_ty = table.new_var(TyVarSort::General, &Kind::Star);
        let goal = TraitInstId::new(
            &db,
            named_trait(&db, top_mod, "Picks"),
            vec![self_ty],
            IndexMap::new(),
        );
        let solve = TraitSolveCx::new(&db, func.scope()).with_assumptions(assumptions);
        assert!(matches!(
            solve.select_impl_proof(&db, goal),
            GoalSatisfiability::NeedsConfirmation { .. }
        ));
    }

    #[test]
    fn selected_candidate_proof_checks_its_own_premises() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "selected_candidate_proof_checks_its_own_premises.fe".into(),
            r#"
trait Marker {}
trait Goal {}

struct Missing {}
struct Subject {}

impl Goal for Subject where Missing: Marker {}
impl Goal for Subject {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let goal_trait = named_trait(&db, top_mod, "Goal");
        let subject = named_struct_ty(&db, top_mod, "Subject");
        let goal = TraitInstId::new(&db, goal_trait, vec![subject], IndexMap::new());
        let solve_cx = TraitSolveCx::new(&db, top_mod.scope());
        let candidates = candidates_for_goal(&db, top_mod, goal);

        assert_eq!(
            candidates.len(),
            2,
            "both competing impls must be collected"
        );
        let rejected = candidates
            .iter()
            .copied()
            .find(|candidate| !candidate.constraints(&db).is_empty(&db))
            .expect("missing constrained candidate");
        let accepted = candidates
            .iter()
            .copied()
            .find(|candidate| candidate.constraints(&db).is_empty(&db))
            .expect("missing unconditional candidate");

        assert!(matches!(
            is_goal_satisfiable(&db, solve_cx, goal),
            GoalSatisfiability::Satisfied(solution)
                if solution.value.implementor == accepted
        ));
        assert_eq!(
            restricted_goal_query_result(&db, solve_cx, rejected, goal).status,
            TargetSolutionStatus::NotFound
        );
        assert!(matches!(
            prove_selected_impl_evidence(&db, solve_cx, rejected, goal),
            Err(SelectedEvidenceError::NotProven)
        ));
        assert!(prove_selected_impl_evidence(&db, solve_cx, accepted, goal).is_ok());
        assert_eq!(
            restricted_goal_query_result(&db, solve_cx, accepted, goal).status,
            TargetSolutionStatus::Found
        );
    }

    #[test]
    fn selected_candidate_proof_checks_trait_arguments_and_associated_bindings() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "selected_candidate_proof_checks_trait_arguments_and_associated_bindings.fe".into(),
            r#"
trait Select<T> {}
trait Produce {
    type Output
}

struct Subject {}
struct A {}
struct B {}

impl Select<A> for Subject {}
impl Produce for Subject {
    type Output = A
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);
        let subject = named_struct_ty(&db, top_mod, "Subject");
        let a = named_struct_ty(&db, top_mod, "A");
        let b = named_struct_ty(&db, top_mod, "B");
        let select = named_trait(&db, top_mod, "Select");
        let produce = named_trait(&db, top_mod, "Produce");
        let solve_cx = TraitSolveCx::new(&db, top_mod.scope());

        let select_a = TraitInstId::new(&db, select, vec![subject, a], IndexMap::new());
        let select_b = TraitInstId::new(&db, select, vec![subject, b], IndexMap::new());
        let select_candidate = candidates_for_goal(&db, top_mod, select_a)
            .into_iter()
            .next()
            .expect("missing Select<A> candidate");
        assert_eq!(
            restricted_goal_query_result(&db, solve_cx, select_candidate, select_a).status,
            TargetSolutionStatus::Found
        );
        assert_eq!(
            restricted_goal_query_result(&db, solve_cx, select_candidate, select_b).status,
            TargetSolutionStatus::NotFound
        );

        let output = IdentId::new(&db, "Output".to_string());
        let produce_a = TraitInstId::new(
            &db,
            produce,
            vec![subject],
            [(output, a)].into_iter().collect::<IndexMap<_, _>>(),
        );
        let produce_b = TraitInstId::new(
            &db,
            produce,
            vec![subject],
            [(output, b)].into_iter().collect::<IndexMap<_, _>>(),
        );
        let produce_candidate = candidates_for_goal(&db, top_mod, produce_a)
            .into_iter()
            .next()
            .expect("missing Produce candidate");
        assert_eq!(
            restricted_goal_query_result(&db, solve_cx, produce_candidate, produce_a).status,
            TargetSolutionStatus::Found
        );
        assert_eq!(
            restricted_goal_query_result(&db, solve_cx, produce_candidate, produce_b).status,
            TargetSolutionStatus::NotFound
        );
    }

    // These overlapping declarations are solver probes, not newly admitted
    // source programs. Selected proof does not waive ordinary coherence.
    #[test]
    fn selected_evidence_keeps_associated_type_identity_across_edits() {
        use super::Selection;
        use crate::analysis::ty::trait_def::resolve_trait_impl_instance;
        use salsa::Setter;

        fn observe(
            db: &HirAnalysisTestDb,
            file: common::file::File,
            reverse: bool,
            left: &str,
        ) -> Vec<usize> {
            let (top_mod, _) = db.top_mod(file);
            let produce = named_trait(db, top_mod, "Produce");
            let subject = named_struct_ty(db, top_mod, "Subject");
            let output = IdentId::new(db, "Output".to_string());
            let goal = TraitInstId::new(db, produce, vec![subject], IndexMap::new());
            let cx = TraitSolveCx::new(db, top_mod.scope());
            let mut candidates = candidates_for_goal(db, top_mod, goal);
            assert_eq!(candidates.len(), 2);
            let first_impl = top_mod.all_impl_traits(db)[0];
            let first_candidate = candidates
                .iter()
                .find(|candidate| candidate.hir_impl_trait(db) == first_impl)
                .unwrap();
            assert_eq!(
                first_candidate.assoc_ty(db, output),
                Some(named_struct_ty(db, top_mod, left))
            );
            if reverse {
                candidates.reverse();
            }
            assert!(matches!(
                resolve_trait_impl_instance(db, cx, goal),
                Selection::Ambiguous(_)
            ));
            let mut observations = Vec::new();
            for name in ["A", "B"] {
                let ty = named_struct_ty(db, top_mod, name);
                let bound = TraitInstId::new(
                    db,
                    produce,
                    vec![subject],
                    [(output, ty)].into_iter().collect::<IndexMap<_, _>>(),
                );
                let Selection::Unique(resolved) = resolve_trait_impl_instance(db, cx, bound) else {
                    panic!("the explicit associated binding must identify one instance");
                };
                assert_eq!(resolved.instantiated_assoc_ty(db, output), Some(ty));
                for &candidate in &candidates {
                    let matches = candidate.assoc_ty(db, output) == Some(ty);
                    let evidence = prove_selected_impl_evidence(db, cx, candidate, goal).unwrap();
                    assert_eq!(evidence.selected(), candidate);
                    assert_eq!(evidence.goal(), goal);
                    let selected_ty = evidence.associated_type(output).unwrap().unwrap();
                    assert_eq!(selected_ty == ty, matches);
                    let mut consumer = UnificationTable::new(db);
                    assert_eq!(consumer.unify(selected_ty, ty).is_ok(), matches);
                    match prove_selected_impl_evidence(db, cx, candidate, bound) {
                        Ok(_) => assert!(matches),
                        Err(error) => {
                            assert!(!matches);
                            assert_eq!(error, SelectedEvidenceError::NotProven);
                        }
                    }
                    assert_eq!(
                        restricted_goal_query_result(db, cx, candidate, goal).status,
                        TargetSolutionStatus::Found
                    );
                    assert_eq!(
                        restricted_goal_query_result(db, cx, candidate, bound).status,
                        if matches {
                            TargetSolutionStatus::Found
                        } else {
                            TargetSolutionStatus::NotFound
                        }
                    );
                    assert_eq!(resolved.selected() == candidate, matches);
                }
                observations.push(
                    top_mod
                        .all_impl_traits(db)
                        .iter()
                        .position(|impl_| *impl_ == resolved.selected().hir_impl_trait(db))
                        .unwrap(),
                );
            }
            assert!(matches!(
                resolve_trait_impl_instance(db, cx, goal),
                Selection::Ambiguous(_)
            ));
            observations
        }
        for reverse in [false, true] {
            let mut db = HirAnalysisTestDb::default();
            let file = db.new_stand_alone("selected_identity_edits.fe".into(), "");
            for (left, right) in [("A", "B"), ("B", "A"), ("A", "B")] {
                let source = format!(
                    "trait Produce {{ type Output }}\nstruct Subject {{}}\nstruct A {{}}\nstruct B {{}}\nimpl Produce for Subject {{ type Output = {left} }}\nimpl Produce for Subject {{ type Output = {right} }}"
                );
                file.set_text(&mut db).to(source.clone());
                let warm = observe(&db, file, reverse, left);
                let mut fresh = HirAnalysisTestDb::default();
                let fresh_file =
                    fresh.new_stand_alone("selected_identity_edits.fe".into(), &source);
                assert_eq!(warm, observe(&fresh, fresh_file, !reverse, left));
            }
        }
    }

    #[test]
    fn proof_provisions_ignore_unread_position_fields() {
        use crate::analysis::semantic::{ImplEnv, SelectedApplication};
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "proof_provisions_projection.fe".into(),
            "trait Foo {} trait Marker {} struct S {} impl Foo for S {} fn position() {}",
        );
        let (top_mod, _) = db.top_mod(file);
        let subject = named_struct_ty(&db, top_mod, "S");
        let goal = TraitInstId::new(
            &db,
            named_trait(&db, top_mod, "Foo"),
            vec![subject],
            IndexMap::new(),
        );
        let assumptions = PredicateListId::empty_list(&db);
        let candidate = candidates_for_goal(&db, top_mod, goal)[0];
        let evidence = prove_selected_impl_evidence(
            &db,
            TraitSolveCx::new(&db, top_mod.scope()),
            candidate,
            goal,
        )
        .unwrap();
        let selection = SelectedApplication::from_evidence(&evidence);
        let marker = TraitInstId::new(
            &db,
            named_trait(&db, top_mod, "Marker"),
            vec![subject],
            IndexMap::new(),
        );
        let a = ImplEnv::test_fixture(
            &db,
            crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                top_mod.scope(),
                assumptions,
            ),
            Vec::new(),
            vec![selection],
            Vec::new(),
        );
        // Scope and premises differ; the search reads neither.
        let b = ImplEnv::test_fixture(
            &db,
            crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                top_mod.all_funcs(&db)[0].scope(),
                PredicateListId::new(&db, vec![marker]),
            ),
            Vec::new(),
            vec![selection],
            Vec::new(),
        );
        assert_ne!(a, b);
        assert_eq!(a.proof_provisions(&db), b.proof_provisions(&db));
        // Nor does it read the selected application's captured origin: only
        // its goal and candidate restrict the subgoal.
        let other_evidence = prove_selected_impl_evidence(
            &db,
            TraitSolveCx::new(&db, top_mod.all_funcs(&db)[0].scope())
                .with_assumptions(PredicateListId::new(&db, vec![marker])),
            candidate,
            goal,
        )
        .unwrap();
        let other_selection = SelectedApplication::from_evidence(&other_evidence);
        assert_ne!(selection, other_selection);
        let c = b.within_named_frame(
            &db,
            crate::semantic::Frame::from_named_applications(vec![other_selection]),
        );
        assert_eq!(a.proof_provisions(&db), c.proof_provisions(&db));
        // Opened evidence is read: it answers its own goal.
        let opened = ImplEnv::test_fixture(
            &db,
            crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                top_mod.scope(),
                assumptions,
            ),
            vec![marker],
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(
            opened.proof_provisions(&db).entries(&db),
            &vec![(marker, None)]
        );
        // Ordinary positions, whatever their scope and premises, share the
        // empty projection, and so the same proof query keys as a context
        // built without any position.
        let ordinary = ImplEnv::test_fixture(
            &db,
            crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                top_mod.scope(),
                assumptions,
            ),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let other_ordinary = ImplEnv::test_fixture(
            &db,
            crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                top_mod.all_funcs(&db)[0].scope(),
                PredicateListId::new(&db, vec![marker]),
            ),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let empty = TraitSolveCx::new(&db, top_mod.scope()).provisions();
        assert_eq!(ordinary.proof_provisions(&db), empty);
        assert_eq!(other_ordinary.proof_provisions(&db), empty);
        assert_eq!(ordinary.solver_context(&db).provisions(), empty);
        let query = CanonicalGoalQuery::new(&db, goal, assumptions);
        assert_eq!(
            super::goal_query_has_no_distinct_solution(
                &db,
                TraitSolveCx::new(&db, top_mod.scope()),
                &query,
                Canonical::new(&db, goal),
            ),
            super::goal_query_has_no_distinct_solution(
                &db,
                ordinary.solver_context(&db),
                &query,
                Canonical::new(&db, goal),
            )
        );
    }

    #[test]
    fn selected_evidence_requires_its_assumptions_in_the_destination() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "selected_evidence_scope.fe".into(),
            r#"
trait Marker {}
trait Unused {}
trait Produce { type Output }
struct Wrapper<T> {}
impl<T: Marker> Produce for Wrapper<T> { type Output = T }
fn context<T: Marker>() where T: Unused {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);
        let func = top_mod.all_funcs(&db)[0];
        let param = collect_generic_params(&db, func.into()).explicit_params(&db)[0];
        let assumptions =
            collect_func_def_constraints(&db, func.into(), true).instantiate_identity();
        let output = IdentId::new(&db, "Output".to_string());
        let wrapper = TyId::app(&db, named_struct_ty(&db, top_mod, "Wrapper"), param);
        let goal = TraitInstId::new(
            &db,
            named_trait(&db, top_mod, "Produce"),
            vec![wrapper],
            [(output, param)].into_iter().collect::<IndexMap<_, _>>(),
        );
        let candidate = candidates_for_goal(&db, top_mod, goal)[0];
        let evidence = prove_selected_impl_evidence(
            &db,
            TraitSolveCx::new(&db, func.scope()).with_assumptions(assumptions),
            candidate,
            goal,
        )
        .unwrap();
        assert_eq!(evidence.associated_type(output).unwrap(), Some(param));
        let required_only = PredicateListId::new(
            &db,
            vec![TraitInstId::new(
                &db,
                named_trait(&db, top_mod, "Marker"),
                vec![param],
                IndexMap::new(),
            )],
        );
        assert_eq!(
            crate::analysis::semantic::SelectedApplication::from_evidence(&evidence)
                .revalidate(
                    &db,
                    TraitSolveCx::new(&db, func.scope()).with_assumptions(required_only),
                    goal
                )
                .unwrap()
                .associated_type(output)
                .unwrap(),
            Some(param)
        );
        // Keep the identical rigid goal and candidate while changing only the
        // available assumptions. A cached success cannot authorize its erasure.
        for order in [[true, false, true], [false, true, false]] {
            for has_premise in order {
                let cx = TraitSolveCx::new(&db, func.scope()).with_assumptions(if has_premise {
                    assumptions
                } else {
                    PredicateListId::empty_list(&db)
                });
                match crate::analysis::semantic::SelectedApplication::from_evidence(&evidence)
                    .revalidate(&db, cx, goal)
                {
                    Ok(moved) => {
                        assert!(has_premise);
                        assert_eq!(moved.associated_type(output).unwrap(), Some(param));
                        assert_eq!(moved.context().assumptions(), cx.assumptions());
                    }
                    Err(error) => {
                        assert!(!has_premise);
                        assert_eq!(error, SelectedEvidenceError::NotProven);
                    }
                }
                assert_eq!(
                    restricted_goal_query_result(&db, cx, candidate, goal).status,
                    if has_premise {
                        TargetSolutionStatus::Found
                    } else {
                        TargetSolutionStatus::NotFound
                    }
                );
            }
        }
    }

    #[test]
    fn selected_evidence_instantiates_generic_outputs_and_rejects_unresolved_goals() {
        use crate::analysis::ty::ty_def::{AssocTy, TyData};
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "selected_evidence_consumer.fe".into(),
            r#"
trait Produce { type Output }
struct Wrapper<T> {}
struct Value {}
impl<T> Produce for Wrapper<T> { type Output = T }
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);
        let produce = named_trait(&db, top_mod, "Produce");
        let value = named_struct_ty(&db, top_mod, "Value");
        let wrapper = named_struct_ty(&db, top_mod, "Wrapper");
        let goal = TraitInstId::new(
            &db,
            produce,
            vec![TyId::app(&db, wrapper, value)],
            IndexMap::new(),
        );
        let candidate = candidates_for_goal(&db, top_mod, goal)[0];
        let cx = TraitSolveCx::new(&db, top_mod.scope());
        let output = IdentId::new(&db, "Output".to_string());
        let evidence = prove_selected_impl_evidence(&db, cx, candidate, goal).unwrap();
        assert_eq!(evidence.associated_type(output).unwrap(), Some(value));
        assert_eq!(
            evidence
                .associated_type(IdentId::new(&db, "Missing".to_string()))
                .unwrap(),
            None
        );
        let mut consumer = UnificationTable::new(&db);
        let inferred = consumer.new_var(TyVarSort::General, &Kind::Star);
        let projection = TyId::new(
            &db,
            TyData::AssocTy(AssocTy {
                trait_: goal.trait_ref(&db),
                name: output,
            }),
        );
        let unresolved_assumption = TraitInstId::new(&db, produce, vec![inferred], IndexMap::new());
        assert!(matches!(
            prove_selected_impl_evidence(
                &db,
                cx.with_assumptions(PredicateListId::new(&db, vec![unresolved_assumption])),
                candidate,
                goal
            ),
            Err(SelectedEvidenceError::Unsupported)
        ));
        for unresolved in [inferred, projection] {
            let goal = TraitInstId::new(
                &db,
                produce,
                vec![TyId::app(&db, wrapper, unresolved)],
                IndexMap::new(),
            );
            assert!(matches!(
                prove_selected_impl_evidence(&db, cx, candidate, goal),
                Err(SelectedEvidenceError::Unsupported)
            ));
        }
    }

    #[test]
    fn selected_evidence_does_not_erase_an_unresolved_output_projection() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "selected_evidence_projection.fe".into(),
            r#"
trait Source { type Item }
trait Produce { type Output }
struct Wrapper<T> {}
impl<T: Source> Produce for Wrapper<T> { type Output = T::Item }
fn context<T: Source>() {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);
        let func = top_mod.all_funcs(&db)[0];
        let param = collect_generic_params(&db, func.into()).explicit_params(&db)[0];
        let assumptions =
            collect_func_def_constraints(&db, func.into(), true).instantiate_identity();
        let self_ty = TyId::app(&db, named_struct_ty(&db, top_mod, "Wrapper"), param);
        let goal = TraitInstId::new(
            &db,
            named_trait(&db, top_mod, "Produce"),
            vec![self_ty],
            IndexMap::new(),
        );
        let candidate = candidates_for_goal(&db, top_mod, goal)[0];
        let evidence = prove_selected_impl_evidence(
            &db,
            TraitSolveCx::new(&db, func.scope()).with_assumptions(assumptions),
            candidate,
            goal,
        )
        .unwrap();
        assert_eq!(
            evidence.associated_type(IdentId::new(&db, "Output".to_string())),
            Err(SelectedEvidenceError::Unsupported)
        );
    }

    #[test]
    fn selected_recursive_candidate_uses_ordinary_premise_resolution() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "selected_recursive_candidate_uses_ordinary_premise_resolution.fe".into(),
            r#"
trait Proof {}

struct Grounded {}
impl Proof for Grounded where Grounded: Proof {}
impl Proof for Grounded {}

struct Dead {}
impl Proof for Dead where Dead: Proof {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let proof = named_trait(&db, top_mod, "Proof");
        let grounded = named_struct_ty(&db, top_mod, "Grounded");
        let dead = named_struct_ty(&db, top_mod, "Dead");
        let solve_cx = TraitSolveCx::new(&db, top_mod.scope());
        let grounded_goal = TraitInstId::new(&db, proof, vec![grounded], IndexMap::new());
        let dead_goal = TraitInstId::new(&db, proof, vec![dead], IndexMap::new());

        let grounded_candidates = candidates_for_goal(&db, top_mod, grounded_goal)
            .into_iter()
            .filter(|candidate| candidate.self_ty(&db) == grounded)
            .collect::<Vec<_>>();
        assert_eq!(grounded_candidates.len(), 2);
        let recursive_grounded = grounded_candidates
            .into_iter()
            .find(|candidate| !candidate.constraints(&db).is_empty(&db))
            .expect("missing recursive Grounded candidate");
        let recursive_dead = candidates_for_goal(&db, top_mod, dead_goal)
            .into_iter()
            .filter(|candidate| candidate.self_ty(&db) == dead)
            .find(|candidate| !candidate.constraints(&db).is_empty(&db))
            .expect("missing recursive Dead candidate");

        assert_eq!(
            restricted_goal_query_result(&db, solve_cx, recursive_grounded, grounded_goal).status,
            TargetSolutionStatus::Found,
            "the selected root's recursive premise may use the base impl"
        );
        assert_eq!(
            restricted_goal_query_result(&db, solve_cx, recursive_dead, dead_goal).status,
            TargetSolutionStatus::NotFound,
            "an ungrounded inductive cycle is not a proof"
        );
    }

    #[test]
    fn selected_candidate_premises_use_ambient_assumptions() {
        fn goal_for<'db>(
            db: &'db HirAnalysisTestDb,
            func: Func<'db>,
            goal_trait: Trait<'db>,
            wrapper: TyId<'db>,
        ) -> (TraitInstId<'db>, TraitSolveCx<'db>) {
            let param = collect_generic_params(db, func.into()).explicit_params(db)[0];
            let assumptions =
                collect_func_def_constraints(db, func.into(), true).instantiate_identity();
            let self_ty = TyId::app(db, wrapper, param);
            let goal = TraitInstId::new(db, goal_trait, vec![self_ty], IndexMap::new());
            (
                goal,
                TraitSolveCx::new(db, func.scope()).with_assumptions(assumptions),
            )
        }

        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "selected_candidate_premises_use_ambient_assumptions.fe".into(),
            r#"
trait Marker {}
trait Goal {}
struct Wrapper<T> {}

impl<T: Marker> Goal for Wrapper<T> {}

fn with_marker<T: Marker>() {}
fn without_marker<T>() {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);
        let goal_trait = named_trait(&db, top_mod, "Goal");
        let wrapper = named_struct_ty(&db, top_mod, "Wrapper");
        let named_func = |name: &str| {
            top_mod
                .all_funcs(&db)
                .iter()
                .copied()
                .find(|func| {
                    func.name(&db)
                        .to_opt()
                        .is_some_and(|ident| ident.data(&db) == name)
                })
                .unwrap_or_else(|| panic!("missing `{name}` function"))
        };
        let (with_goal, with_cx) = goal_for(&db, named_func("with_marker"), goal_trait, wrapper);
        let (without_goal, without_cx) =
            goal_for(&db, named_func("without_marker"), goal_trait, wrapper);
        let candidate = candidates_for_goal(&db, top_mod, with_goal)
            .into_iter()
            .next()
            .expect("missing generic Goal candidate");

        assert_eq!(
            restricted_goal_query_result(&db, with_cx, candidate, with_goal).status,
            TargetSolutionStatus::Found
        );
        assert_eq!(
            restricted_goal_query_result(&db, without_cx, candidate, without_goal).status,
            TargetSolutionStatus::NotFound
        );
    }

    #[test]
    fn selected_candidate_proof_preserves_incomplete_growth_result() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "selected_candidate_proof_preserves_incomplete_growth_result.fe".into(),
            r#"
trait Grow {}
struct Dead {}
struct Wrap<T> {}

impl<T> Grow for T where Wrap<T>: Grow {}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let grow = named_trait(&db, top_mod, "Grow");
        let dead = named_struct_ty(&db, top_mod, "Dead");
        let goal = TraitInstId::new(&db, grow, vec![dead], IndexMap::new());
        let candidate = candidates_for_goal(&db, top_mod, goal)
            .into_iter()
            .next()
            .expect("missing growing candidate");

        assert_eq!(
            restricted_goal_query_result(
                &db,
                TraitSolveCx::new(&db, top_mod.scope()),
                candidate,
                goal,
            )
            .status,
            TargetSolutionStatus::Incomplete
        );
        assert!(matches!(
            prove_selected_impl_evidence(
                &db,
                TraitSolveCx::new(&db, top_mod.scope()),
                candidate,
                goal
            ),
            Err(SelectedEvidenceError::Incomplete)
        ));
    }

    #[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/impl_const_requirements", glob: "evidence.fe")]
    fn selected_evidence_checks_instantiated_impl_const_conditions(fixture: Fixture<&str>) {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(fixture.path().into(), fixture.content());
        let (top_mod, _) = db.top_mod(file);
        // Exercise the evidence boundary directly while source admission of
        // generic impl conditions is extended separately.
        for trait_name in ["Marker", "Lifted", "Twice"] {
            let marker = named_trait(&db, top_mod, trait_name);
            for (name, expected) in [("Good", true), ("Bad", false)] {
                let carrier = top_mod
                    .all_structs(&db)
                    .iter()
                    .copied()
                    .find(|record| {
                        record
                            .name(&db)
                            .to_opt()
                            .is_some_and(|id| id.data(&db) == name)
                    })
                    .unwrap();
                let subject = AdtRef::Struct(carrier).as_adt(&db).fields(&db)[0]
                    .ty(&db, 0)
                    .instantiate_identity();
                let goal = TraitInstId::new(&db, marker, vec![subject], IndexMap::new());
                let candidate = candidates_for_goal(&db, top_mod, goal)[0];
                let result = prove_selected_impl_evidence(
                    &db,
                    TraitSolveCx::new(&db, top_mod.scope()),
                    candidate,
                    goal,
                );
                assert_eq!(result.is_ok(), expected, "{trait_name} for {name}");
                let ordinary = super::super::trait_def::resolve_ordinary_impl_evidence(
                    &db,
                    TraitSolveCx::new(&db, top_mod.scope()),
                    goal,
                );
                assert_eq!(
                    ordinary.is_ok(),
                    expected,
                    "ordinary {trait_name} for {name}"
                );
                let GoalSatisfiability::Satisfied(solution) =
                    is_goal_satisfiable(&db, TraitSolveCx::new(&db, top_mod.scope()), goal)
                else {
                    panic!("structural proof missing for {trait_name} on {name}");
                };
                assert_eq!(
                    solution.value.impl_conditions.instances(&db).len(),
                    1,
                    "const condition from Marker was not retained for {trait_name} on {name}"
                );
            }
        }
    }
}
