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
            table_solver::{TargetSolutionMatch, has_solution, solve},
        },
        unify::UnificationTable,
    },
};
use crate::{
    Ingot,
    hir_def::{HirIngot, scope_graph::ScopeId},
};
use common::indexmap::IndexSet;
use constraint::collect_constraints;
use rustc_hash::FxHashSet;
use salsa::Update;
use std::cell::Cell;

pub(crate) mod constraint;
mod table_solver;

pub(crate) use table_solver::TargetSearchResult;
pub use table_solver::TargetSolutionStatus;

pub(crate) const TRAIT_SOLVER_ROOT_ANSWER_LIMIT: usize = 2;

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

#[derive(Debug, Clone)]
pub enum Selection<T> {
    Unique(T),
    Ambiguous(IndexSet<T>),
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Update)]
pub struct TraitSolveCx<'db> {
    scope: ScopeId<'db>,
    origin_ingot: Ingot<'db>,
    assumptions: PredicateListId<'db>,
}

impl<'db> TraitSolveCx<'db> {
    pub fn new(db: &'db dyn HirAnalysisDb, scope: ScopeId<'db>) -> Self {
        Self {
            scope,
            origin_ingot: scope.ingot(db),
            assumptions: PredicateListId::empty_list(db),
        }
    }

    pub fn with_assumptions(self, assumptions: PredicateListId<'db>) -> Self {
        Self {
            assumptions,
            ..self
        }
    }

    pub fn assumptions(self) -> PredicateListId<'db> {
        self.assumptions
    }

    pub(crate) fn scope(self) -> ScopeId<'db> {
        self.scope
    }

    pub(crate) fn select_impl(
        self,
        db: &'db dyn HirAnalysisDb,
        inst: TraitInstId<'db>,
    ) -> Selection<ImplementorId<'db>> {
        let scope = self.normalization_scope_for_trait_inst(db, inst);
        let inst = normalize_trait_inst_preserving_validity(db, inst, scope, self.assumptions);
        // An assumption proves a bound; it is not a second implementation.
        // Keep inference goals on the ordinary proof query: an assumption may
        // select a different substitution from the implementations in scope.
        let result = if inst.args(db).iter().any(|ty| ty.has_var(db))
            || inst
                .assoc_type_bindings(db)
                .values()
                .any(|ty| ty.has_var(db))
        {
            is_goal_satisfiable(db, self, inst)
        } else {
            let query = CanonicalGoalQuery::from_query(
                db,
                TraitSolverQuery {
                    goal: inst,
                    assumptions: self.assumptions.extend_all_bounds(db),
                    require_impl: true,
                },
            );
            match is_goal_query_satisfiable(db, self, &query) {
                // A bound with no provable implementation can still be supplied
                // by the caller. Incomplete searches cannot establish this.
                GoalSatisfiability::UnSat(_) => is_goal_satisfiable(db, self, inst),
                result => result,
            }
        };
        match result {
            GoalSatisfiability::Satisfied(solution) => {
                Selection::Unique(solution.value.implementor)
            }
            GoalSatisfiability::NeedsConfirmation { solutions, .. } => {
                Selection::Ambiguous(solutions.iter().map(|s| s.value.implementor).collect())
            }
            GoalSatisfiability::ContainsInvalid | GoalSatisfiability::UnSat(_) => {
                Selection::NotFound
            }
            GoalSatisfiability::NormalizationFault => Selection::Ambiguous(IndexSet::default()),
        }
    }

    pub(crate) fn search_ingots_for_trait_inst(
        self,
        db: &'db dyn HirAnalysisDb,
        inst: TraitInstId<'db>,
    ) -> (Ingot<'db>, Option<Ingot<'db>>) {
        Self::search_ingots_for_trait_inst_with_origin(db, self.origin_ingot, inst)
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
        Self::normalization_scope_for_trait_inst_with_origin(db, self.origin_ingot, inst)
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
        self.origin_ingot.root_mod(db).scope()
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
) -> GoalSatisfiability<'db> {
    if query.flags(db).contains(TyFlags::HAS_INVALID) {
        return GoalSatisfiability::ContainsInvalid;
    };

    solve(db, use_scope, query)
}

fn is_query_satisfiable_cycle_initial<'db>(
    _db: &'db dyn HirAnalysisDb,
    _use_scope: ScopeId<'db>,
    _query: Canonical<TraitSolverQuery<'db>>,
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
) -> salsa::CycleRecoveryAction<GoalSatisfiability<'db>> {
    salsa::CycleRecoveryAction::Iterate
}

#[salsa::tracked(
    cycle_fn=query_has_solution_cycle_recover,
    cycle_initial=query_has_solution_cycle_initial
)]
fn query_has_solution<'db>(
    db: &'db dyn HirAnalysisDb,
    use_scope: ScopeId<'db>,
    query: Canonical<TraitSolverQuery<'db>>,
    target: Canonical<TraitInstId<'db>>,
    relation: TargetSolutionMatch,
) -> TargetSolutionStatus {
    if query.flags(db).contains(TyFlags::HAS_INVALID) {
        return TargetSolutionStatus::NotFound;
    }
    has_solution(db, use_scope, query, target, relation, None).status
}

#[salsa::tracked(
    cycle_fn=query_candidate_proves_goal_cycle_recover,
    cycle_initial=query_candidate_proves_goal_cycle_initial
)]
fn query_candidate_proves_goal<'db>(
    db: &'db dyn HirAnalysisDb,
    use_scope: ScopeId<'db>,
    query: Canonical<TraitSolverQuery<'db>>,
    candidate: ImplementorId<'db>,
    target: Canonical<TraitInstId<'db>>,
) -> TargetSearchResult<'db> {
    if query.flags(db).contains(TyFlags::HAS_INVALID) {
        return TargetSearchResult {
            status: TargetSolutionStatus::NotFound,
            impl_conditions: None,
        };
    }
    has_solution(
        db,
        use_scope,
        query,
        target,
        TargetSolutionMatch::Equal,
        Some(candidate),
    )
}

fn query_candidate_proves_goal_cycle_initial<'db>(
    _db: &'db dyn HirAnalysisDb,
    _use_scope: ScopeId<'db>,
    _query: Canonical<TraitSolverQuery<'db>>,
    _candidate: ImplementorId<'db>,
    _target: Canonical<TraitInstId<'db>>,
) -> TargetSearchResult<'db> {
    TargetSearchResult {
        status: TargetSolutionStatus::Incomplete,
        impl_conditions: None,
    }
}

fn query_candidate_proves_goal_cycle_recover<'db>(
    _db: &'db dyn HirAnalysisDb,
    _value: &TargetSearchResult<'db>,
    _count: u32,
    _use_scope: ScopeId<'db>,
    _query: Canonical<TraitSolverQuery<'db>>,
    _candidate: ImplementorId<'db>,
    _target: Canonical<TraitInstId<'db>>,
) -> salsa::CycleRecoveryAction<TargetSearchResult<'db>> {
    salsa::CycleRecoveryAction::Iterate
}

/// Proves a complete trait instance through one selected implementor.
///
/// `candidate` identifies an implementor definition, rather than a separately
/// instantiated impl application. `Found` means that this implementor can
/// realize `goal` and that all of its substituted premises have proofs. It does
/// not imply that the implementor is unique or coherent with other
/// implementors. `Incomplete` means solver limits or pending cycle convergence
/// prevented a definitive answer. Complete goals may contain rigid type
/// parameters backed by `solve_cx` assumptions, but inference-bearing goals are
/// outside this entry point's contract because target equality is checked after
/// solving.
pub fn candidate_proves_goal<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    candidate: ImplementorId<'db>,
    goal: TraitInstId<'db>,
) -> TargetSolutionStatus {
    candidate_proves_goal_with_conditions(db, solve_cx, candidate, goal).status
}

pub(crate) fn candidate_proves_goal_with_conditions<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    candidate: ImplementorId<'db>,
    goal: TraitInstId<'db>,
) -> TargetSearchResult<'db> {
    let scope = solve_cx.normalization_scope_for_trait_inst(db, goal);
    let goal = normalize_trait_inst_preserving_validity(db, goal, scope, solve_cx.assumptions());
    let query = CanonicalGoalQuery::new(db, goal, solve_cx.assumptions());
    query_candidate_proves_goal(
        db,
        solve_cx.origin_scope(db),
        query.canonical(),
        candidate,
        Canonical::new(db, goal),
    )
}

fn query_has_solution_cycle_initial<'db>(
    _db: &'db dyn HirAnalysisDb,
    _use_scope: ScopeId<'db>,
    _query: Canonical<TraitSolverQuery<'db>>,
    _target: Canonical<TraitInstId<'db>>,
    _relation: TargetSolutionMatch,
) -> TargetSolutionStatus {
    TargetSolutionStatus::Incomplete
}

fn query_has_solution_cycle_recover<'db>(
    _db: &'db dyn HirAnalysisDb,
    _value: &TargetSolutionStatus,
    _count: u32,
    _use_scope: ScopeId<'db>,
    _query: Canonical<TraitSolverQuery<'db>>,
    _target: Canonical<TraitInstId<'db>>,
    _relation: TargetSolutionMatch,
) -> salsa::CycleRecoveryAction<TargetSolutionStatus> {
    salsa::CycleRecoveryAction::Iterate
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
            target,
            TargetSolutionMatch::Equal,
        ),
        TargetSolutionStatus::Found
    )
}

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
            target,
            TargetSolutionMatch::NotEqual,
        ),
        TargetSolutionStatus::NotFound
    )
}

pub fn is_goal_query_satisfiable<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    query: &CanonicalGoalQuery<'db>,
) -> GoalSatisfiability<'db> {
    is_query_satisfiable(db, solve_cx.origin_scope(db), query.canonical()).clone()
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
        let family_wf = check_type_application_requirements(db, solve_cx, ty);
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
            let mut table = UnificationTable::new(db);
            let query = CanonicalGoalQuery::new(db, goal, assumptions);

            if let GoalSatisfiability::UnSat(subgoal) =
                is_goal_query_satisfiable(db, solve_cx, &query)
            {
                if super::type_fn::induction::discharge(db, solve_cx, goal) {
                    continue;
                }
                let subgoal = subgoal.map(|subgoal| query.extract_subgoal(&mut table, subgoal));
                return WellFormedness::IllFormed { goal, subgoal };
            }
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
pub(crate) fn check_type_application_requirements<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    ty: TyId<'db>,
) -> WellFormedness<'db> {
    if ty.has_unsaturated_family_guards(db) {
        return WellFormedness::UnsaturatedGuardedFamily;
    }
    let requirements = if matches!(
        ty.decompose_ty_app(db).0.data(db),
        TyData::TyBase(super::ty_def::TyBase::TypeFn(_))
    ) {
        Some(ty_constraints(db, ty))
    } else {
        ty.associated_family_requirements(db)
    };
    if let Some(requirements) = requirements {
        for &goal in requirements.list(db) {
            if !matches!(
                is_goal_satisfiable(db, solve_cx, goal),
                GoalSatisfiability::Satisfied(_)
            ) {
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
            if super::type_fn::induction::discharge(db, solve_cx, goal) {
                return WellFormedness::WellFormed;
            }
            let subgoal = subgoal.map(|subgoal| query.extract_subgoal(&mut table, subgoal));
            WellFormedness::IllFormed { goal, subgoal }
        }
        _ => WellFormedness::WellFormed,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Update)]
pub(crate) enum WellFormedness<'db> {
    WellFormed,
    WithConditions(ImplConditionsId<'db>),
    UnsaturatedGuardedFamily,
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
        use super::diagnostics::{AssociatedFamilyError, TraitConstraintDiag, TyLowerDiag};
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
            Self::UnsaturatedGuardedFamily => Some(
                TyLowerDiag::AssociatedFamilyDeclaration {
                    span,
                    error: AssociatedFamilyError::GuardedPartialApplication,
                }
                .into(),
            ),
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
        let mut table = UnificationTable::new(db);
        let query = CanonicalGoalQuery::new(db, goal, assumptions);
        if let GoalSatisfiability::UnSat(subgoal) = is_goal_query_satisfiable(db, solve_cx, &query)
        {
            if super::type_fn::induction::discharge(db, solve_cx, goal) {
                continue;
            }
            let subgoal = subgoal.map(|subgoal| query.extract_subgoal(&mut table, subgoal));
            return WellFormedness::IllFormed { goal, subgoal };
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

impl GoalSatisfiability<'_> {
    pub fn is_satisfied(&self) -> bool {
        matches!(
            self,
            Self::Satisfied(_) | Self::NeedsConfirmation { .. } | Self::ContainsInvalid
        )
    }
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

    pub(super) fn merge(self, db: &'db dyn HirAnalysisDb, other: Self) -> Self {
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
                    hir_trait.scope(),
                    evidence,
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
                        hir_trait.scope(),
                        evidence,
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
        TraitSolveCompletion, TraitSolveCx, candidate_proves_goal, goal_query_has_solution,
        is_goal_query_satisfiable, is_goal_satisfiable,
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
            matches!(solve.select_impl(&db, goal), Selection::Ambiguous(implementors) if implementors.len() == 2)
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
            solve.select_impl(&db, goal),
            Selection::Ambiguous(_)
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
            candidate_proves_goal(&db, solve_cx, rejected, goal),
            TargetSolutionStatus::NotFound
        );
        assert!(matches!(
            prove_selected_impl_evidence(&db, solve_cx, rejected, goal),
            Err(SelectedEvidenceError::NotProven)
        ));
        assert!(prove_selected_impl_evidence(&db, solve_cx, accepted, goal).is_ok());
        assert_eq!(
            candidate_proves_goal(&db, solve_cx, accepted, goal),
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
            candidate_proves_goal(&db, solve_cx, select_candidate, select_a),
            TargetSolutionStatus::Found
        );
        assert_eq!(
            candidate_proves_goal(&db, solve_cx, select_candidate, select_b),
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
            candidate_proves_goal(&db, solve_cx, produce_candidate, produce_a),
            TargetSolutionStatus::Found
        );
        assert_eq!(
            candidate_proves_goal(&db, solve_cx, produce_candidate, produce_b),
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
                        candidate_proves_goal(db, cx, candidate, goal),
                        TargetSolutionStatus::Found
                    );
                    assert_eq!(
                        candidate_proves_goal(db, cx, candidate, bound),
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
            evidence
                .revalidate(TraitSolveCx::new(&db, func.scope()).with_assumptions(required_only))
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
                match evidence.revalidate(cx) {
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
                    candidate_proves_goal(&db, cx, candidate, goal),
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
            candidate_proves_goal(&db, solve_cx, recursive_grounded, grounded_goal),
            TargetSolutionStatus::Found,
            "the selected root's recursive premise may use the base impl"
        );
        assert_eq!(
            candidate_proves_goal(&db, solve_cx, recursive_dead, dead_goal),
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
            candidate_proves_goal(&db, with_cx, candidate, with_goal),
            TargetSolutionStatus::Found
        );
        assert_eq!(
            candidate_proves_goal(&db, without_cx, candidate, without_goal),
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
            candidate_proves_goal(
                &db,
                TraitSolveCx::new(&db, top_mod.scope()),
                candidate,
                goal,
            ),
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
