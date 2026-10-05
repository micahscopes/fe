//! Fe's adapter for the reusable [`tablesolve`] proof-forest engine.
//!
//! The external crate owns table creation, scheduling, consumer replay, and
//! answer deduplication. This module keeps the language-specific parts:
//! canonicalization, candidate lookup, unification, evidence construction, and
//! the small amount of diagnostic state needed for an unsatisfied subgoal.

use std::{cell::Cell, convert::Infallible};

use common::indexmap::{IndexMap, IndexSet};
use rustc_hash::{FxHashMap, FxHashSet};
use tablesolve::{
    AnswerlessMode, CallbackOutcome, Canonical as TabledCanonical, CanonicalizeOutcome, Completion,
    Config, ConsumerId, ContextTransition, Event, Limits, Observer, ReportOptions,
    ResolutionContext, ResumeProvenance, Scheduling, Transition, solve_with_observer_and_options,
    solve_with_options,
};

use super::{
    CanonicalGoalQuery, GoalSatisfiability, ImplConditionsId, TRAIT_SOLVER_ROOT_ANSWER_LIMIT,
    TraitGoalSolution, TraitSolveCompletion, TraitSolveCx, TraitSolverQuery,
};
use crate::analysis::{
    HirAnalysisDb,
    ty::{
        canonical::{Canonical, Solution},
        fold::TyFoldable,
        trait_def::{ImplementorId, TraitInstId, impls_for_trait_in_ingots},
        ty_def::{TyData, TyId},
        unify::{PersistentUnificationTable, UnificationError, UnificationResult},
        visitor::{TyVisitable, TyVisitor},
    },
};
use crate::hir_def::scope_graph::ScopeId;

/// Temporary bounded-term-size guard for non-regular recursive goals.
///
/// The initial query may itself contain a legitimately deep finite type, so
/// bound growth relative to that query instead of imposing an absolute depth.
const MAXIMUM_TYPE_GROWTH: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Query<'db> {
    query: Canonical<TraitSolverQuery<'db>>,
    selected_candidate: Option<ImplementorId<'db>>,
}

#[derive(Debug, Clone, Copy)]
struct Goal<'db> {
    query: TraitSolverQuery<'db>,
    selected_candidate: Option<ImplementorId<'db>>,
}
type GoalSolution<'db> = Solution<TraitGoalSolution<'db>>;
type UnsatSubgoal<'db> = Solution<TraitInstId<'db>>;

fn trait_inst_head<'db>(db: &'db dyn HirAnalysisDb, inst: TraitInstId<'db>) -> TraitInstId<'db> {
    TraitInstId::new(db, inst.def(db), inst.args(db).to_vec(), IndexMap::new())
}

/// Whether the impl `header` can match the normalized `goal`, checked before
/// the header is instantiated and normalized.
///
/// Instantiation rebuilds types structurally, and normalization of a valid
/// header never makes it invalid, so the header's base types and applications
/// reach unification unchanged, where different ones never unify. Parameters,
/// projections, consts, and error types are not compared.
fn impl_header_may_match<'db>(
    db: &'db dyn HirAnalysisDb,
    header: ImplementorId<'db>,
    goal: TraitInstId<'db>,
) -> bool {
    fn may_unify<'db>(db: &'db dyn HirAnalysisDb, header: TyId<'db>, goal: TyId<'db>) -> bool {
        match (header.data(db), goal.data(db)) {
            (TyData::TyApp(header_abs, header_arg), TyData::TyApp(goal_abs, goal_arg)) => {
                may_unify(db, *header_abs, *goal_abs) && may_unify(db, *header_arg, *goal_arg)
            }
            (TyData::TyBase(_) | TyData::TyApp(..), TyData::TyBase(_) | TyData::TyApp(..)) => {
                header == goal
            }
            _ => true,
        }
    }

    let inst = header.trait_(db);
    inst.args(db)
        .iter()
        .chain(inst.assoc_type_bindings(db).values())
        .chain(header.types(db).values())
        .any(|ty| ty.has_invalid(db))
        || inst
            .args(db)
            .iter()
            .zip(goal.args(db))
            .all(|(&header, &goal)| may_unify(db, header, goal))
}

fn normalize_assoc_binding<'db>(
    db: &'db dyn HirAnalysisDb,
    table: &mut PersistentUnificationTable<'db>,
    ty: TyId<'db>,
    scope: ScopeId<'db>,
    assumptions: super::PredicateListId<'db>,
) -> Result<TyId<'db>, UnificationError> {
    let ty = ty.fold_with(db, table);
    let normalized = crate::analysis::ty::normalize::normalize_ty(db, ty, scope, assumptions);
    if !ty.has_invalid(db) && normalized.has_invalid(db) {
        Err(UnificationError::NormalizationFault)
    } else {
        Ok(normalized)
    }
}

fn normalize_trait_inst_for_matching<'db>(
    db: &'db dyn HirAnalysisDb,
    inst: TraitInstId<'db>,
    scope: crate::hir_def::scope_graph::ScopeId<'db>,
    assumptions: super::PredicateListId<'db>,
) -> Result<TraitInstId<'db>, UnificationError> {
    // RD74 permits bounded pure normalization here, but a fault must stop
    // proof search rather than make this candidate disappear.
    let normalized = inst.normalize(db, scope, assumptions);
    let has_invalid = |inst: TraitInstId<'db>| {
        inst.args(db).iter().any(|ty| ty.has_invalid(db))
            || inst
                .assoc_type_bindings(db)
                .values()
                .any(|ty| ty.has_invalid(db))
    };
    if !has_invalid(inst) && has_invalid(normalized) {
        Err(UnificationError::NormalizationFault)
    } else {
        Ok(normalized)
    }
}

fn unify_trait_inst_with_normalized_assoc_bindings<'db>(
    db: &'db dyn HirAnalysisDb,
    table: &mut PersistentUnificationTable<'db>,
    candidate: TraitInstId<'db>,
    goal: TraitInstId<'db>,
    scope: ScopeId<'db>,
    assumptions: super::PredicateListId<'db>,
) -> UnificationResult {
    table.unify(trait_inst_head(db, candidate), trait_inst_head(db, goal))?;

    for (name, &candidate_assoc_ty) in candidate.assoc_type_bindings(db) {
        if let Some(&goal_assoc_ty) = goal.assoc_type_bindings(db).get(name) {
            let candidate_assoc_ty =
                normalize_assoc_binding(db, table, candidate_assoc_ty, scope, assumptions)?;
            let goal_assoc_ty =
                normalize_assoc_binding(db, table, goal_assoc_ty, scope, assumptions)?;
            table.unify(candidate_assoc_ty, goal_assoc_ty)?;
        }
    }

    if goal
        .assoc_type_bindings(db)
        .keys()
        .any(|name| !candidate.assoc_type_bindings(db).contains_key(name))
    {
        return Err(UnificationError::TypeMismatch);
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReason {
    MaximumTypeDepth,
    TargetFound,
    NormalizationFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, salsa::Update)]
pub enum TargetSolutionStatus {
    Found,
    NotFound,
    Incomplete,
    NormalizationFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, salsa::Update)]
pub(crate) struct TargetSearchResult<'db> {
    pub(crate) status: TargetSolutionStatus,
    pub(crate) impl_conditions: Option<ImplConditionsId<'db>>,
}

impl<'db> TargetSearchResult<'db> {
    fn without_conditions(status: TargetSolutionStatus) -> Self {
        Self {
            status,
            impl_conditions: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum TargetSolutionMatch {
    Equal,
    NotEqual,
}

#[derive(Clone, Copy)]
enum Clause<'db> {
    Implementor(ImplementorId<'db>),
    Assumption(usize),
    FamilyBound {
        bound: TraitInstId<'db>,
        requirements: super::PredicateListId<'db>,
    },
}

#[derive(Clone)]
struct Branch<'db> {
    table: PersistentUnificationTable<'db>,
    root_goal: TraitInstId<'db>,
    remaining_goals: Vec<TraitInstId<'db>>,
    selected_impl: ImplementorId<'db>,
    establishes_goal: bool,
    impl_conditions: Vec<ImplementorId<'db>>,
    trait_conditions: Vec<TraitInstId<'db>>,
}

#[derive(Clone)]
struct PreparedQuery<'db> {
    table: PersistentUnificationTable<'db>,
    query: TraitSolverQuery<'db>,
    scope: ScopeId<'db>,
    normalized_goal: TraitInstId<'db>,
}

#[derive(Clone, Copy)]
struct TargetAnswer<'db> {
    root: Query<'db>,
    inst: Canonical<TraitInstId<'db>>,
    relation: TargetSolutionMatch,
}

#[derive(Debug, Clone, Copy)]
struct TypeDepthBudget {
    initial_max_depth: usize,
}

impl TypeDepthBudget {
    fn new<'db>(
        db: &'db dyn HirAnalysisDb,
        root: TraitSolverQuery<'db>,
        target: Option<TargetAnswer<'db>>,
    ) -> Self {
        let mut initial_max_depth = maximum_ty_depth(db, root);
        if let Some(target) = target {
            initial_max_depth = initial_max_depth.max(maximum_ty_depth(db, target.inst.value()));
        }
        Self { initial_max_depth }
    }

    fn exceeded<'db, V>(self, db: &'db dyn HirAnalysisDb, value: V) -> bool
    where
        V: TyVisitable<'db>,
    {
        type_depth_growth_exceeded(self.initial_max_depth, maximum_ty_depth(db, value))
    }
}

fn type_depth_growth_exceeded(initial_depth: usize, current_depth: usize) -> bool {
    if initial_depth == usize::MAX || current_depth == usize::MAX {
        return true;
    }
    current_depth.saturating_sub(initial_depth) > MAXIMUM_TYPE_GROWTH
}

struct TraitResolutionContext<'db> {
    db: &'db dyn HirAnalysisDb,
    origin_ingot: crate::Ingot<'db>,
    prepared_queries: FxHashMap<Query<'db>, PreparedQuery<'db>>,
    target: Option<TargetAnswer<'db>>,
    found_target_conditions: Cell<Option<ImplConditionsId<'db>>>,
    type_depth_budget: TypeDepthBudget,
}

impl<'db> TraitResolutionContext<'db> {
    fn new(
        db: &'db dyn HirAnalysisDb,
        use_scope: crate::hir_def::scope_graph::ScopeId<'db>,
        root: TraitSolverQuery<'db>,
        target: Option<TargetAnswer<'db>>,
    ) -> Self {
        Self {
            db,
            origin_ingot: use_scope.ingot(db),
            prepared_queries: FxHashMap::default(),
            target,
            found_target_conditions: Cell::new(None),
            type_depth_budget: TypeDepthBudget::new(db, root, target),
        }
    }

    fn prepare_query(&mut self, key: Query<'db>) -> Result<PreparedQuery<'db>, UnificationError> {
        if let Some(prepared) = self.prepared_queries.get(&key) {
            return Ok(prepared.clone());
        }

        let mut table = PersistentUnificationTable::new_proof(self.db);
        let query = key.query.extract_identity(&mut table);
        let scope = TraitSolveCx::normalization_scope_for_trait_inst_with_origin(
            self.db,
            self.origin_ingot,
            query.goal,
        );
        let normalized_goal =
            normalize_trait_inst_for_matching(self.db, query.goal, scope, query.assumptions)?;
        let prepared = PreparedQuery {
            table,
            query,
            scope,
            normalized_goal,
        };
        self.prepared_queries.insert(key, prepared.clone());
        Ok(prepared)
    }

    fn goal_can_use_assumptions(&self, goal: TraitInstId<'db>) -> bool {
        goal.args(self.db).iter().copied().any(|ty| {
            ty.has_param(self.db)
                || ty.has_var(self.db)
                || matches!(
                    ty.data(self.db),
                    TyData::AssocTy(_) | TyData::QualifiedTy(_)
                )
        })
    }

    fn answer(
        &self,
        parent: &Query<'db>,
        branch: &mut Branch<'db>,
        goal: TraitInstId<'db>,
    ) -> GoalSolution<'db> {
        let mut trait_conditions = branch
            .trait_conditions
            .iter()
            .map(|&inst| inst.fold_with(self.db, &mut branch.table))
            .collect::<Vec<_>>();
        let goal = goal.fold_with(self.db, &mut branch.table);
        // Structural proof only records these conditions. The checker decides
        // whether they hold after inference, outside proof search.
        if branch.establishes_goal
            && !crate::hir_def::WhereClauseOwner::Trait(goal.def(self.db))
                .where_clause(self.db)
                .const_predicates(self.db)
                .is_empty()
            && !trait_conditions.contains(&goal)
        {
            trait_conditions.push(goal);
        }
        let impl_conditions = ImplConditionsId::new(
            self.db,
            branch
                .impl_conditions
                .iter()
                .map(|&instance| instance.fold_with(self.db, &mut branch.table))
                .collect::<Vec<_>>(),
            trait_conditions,
        );
        parent.query.canonicalize_solution(
            self.db,
            &mut branch.table,
            TraitGoalSolution {
                inst: goal,
                implementor: branch.selected_impl,
                impl_conditions,
            },
        )
    }

    fn finish_branch(
        &self,
        parent: &Query<'db>,
        mut branch: Branch<'db>,
    ) -> ContextTransition<Self> {
        let root_goal = branch.root_goal;
        let answer = self.answer(parent, &mut branch, root_goal);
        if self.target.is_some_and(|target| {
            if target.root != *parent {
                return false;
            }
            let equal = Canonical::new(self.db, answer.value.inst) == target.inst;
            match target.relation {
                TargetSolutionMatch::Equal => equal,
                TargetSolutionMatch::NotEqual => !equal,
            }
        }) {
            self.found_target_conditions
                .set(Some(answer.value.impl_conditions));
            Transition::Stop(StopReason::TargetFound)
        } else {
            Transition::Answer(answer)
        }
    }

    fn continue_branch(
        &self,
        parent: &Query<'db>,
        mut branch: Branch<'db>,
        assumptions: super::PredicateListId<'db>,
    ) -> ContextTransition<Self> {
        let Some(next_goal) = branch.remaining_goals.pop() else {
            return self.finish_branch(parent, branch);
        };

        // `tablesolve` keys a suspension before retaining its branch state. Fold
        // through every substitution accumulated so far so table sharing never
        // sees a stale, pre-unification subgoal.
        let next_goal = next_goal.fold_with(self.db, &mut branch.table);
        let assumptions = assumptions.fold_with(self.db, &mut branch.table);
        Transition::Suspend {
            goal: Goal {
                query: TraitSolverQuery {
                    goal: next_goal,
                    assumptions,
                    require_impl: false,
                },
                selected_candidate: None,
            },
            state: branch,
        }
    }
}

impl<'db> ResolutionContext for TraitResolutionContext<'db> {
    type Goal = Goal<'db>;
    type Key = Query<'db>;
    type Clause = Clause<'db>;
    type Answer = GoalSolution<'db>;
    type AnswerKey = GoalSolution<'db>;
    type Output = GoalSolution<'db>;
    type State = Branch<'db>;
    type Rebase = CanonicalGoalQuery<'db>;
    type Error = Infallible;
    type StopReason = StopReason;

    fn canonicalize(
        &mut self,
        goal: Self::Goal,
    ) -> Result<CanonicalizeOutcome<Self::Key, Self::Rebase, Self::StopReason>, Self::Error> {
        // Assumptions participate in the table key, so they must be bounded
        // together with the goal; otherwise substitutions can create an
        // unbounded sequence of keys while the goal itself stays shallow.
        let exceeds_type_depth = self.type_depth_budget.exceeded(self.db, goal.query);
        let query = CanonicalGoalQuery::from_query(self.db, goal.query);
        let key = Query {
            query: query.canonical(),
            selected_candidate: goal.selected_candidate,
        };
        let canonical = TabledCanonical::new(key, query);
        if exceeds_type_depth {
            Ok(CanonicalizeOutcome::Stop {
                canonical,
                reason: StopReason::MaximumTypeDepth,
            })
        } else {
            Ok(CanonicalizeOutcome::Continue(canonical))
        }
    }

    fn clauses(
        &mut self,
        key: &Self::Key,
    ) -> Result<CallbackOutcome<Vec<Self::Clause>, Self::StopReason>, Self::Error> {
        let prepared = match self.prepare_query(*key) {
            Ok(prepared) => prepared,
            Err(UnificationError::NormalizationFault) => {
                return Ok(CallbackOutcome::Stop(StopReason::NormalizationFault));
            }
            Err(_) => unreachable!("preparing a proof query only normalizes types"),
        };
        let (primary, secondary) = TraitSolveCx::search_ingots_for_trait_inst_with_origin(
            self.db,
            self.origin_ingot,
            prepared.query.goal,
        );
        let implementors = impls_for_trait_in_ingots(
            self.db,
            primary,
            secondary,
            Canonical::new(self.db, prepared.query.goal),
            key.selected_candidate,
        );

        let mut clauses =
            Vec::with_capacity(implementors.len() + prepared.query.assumptions.list(self.db).len());
        if let Some(selected_candidate) = key.selected_candidate {
            clauses.extend(
                implementors
                    .iter()
                    .copied()
                    .filter(|candidate| *candidate == selected_candidate)
                    .map(Clause::Implementor),
            );
        } else {
            clauses.extend(implementors.iter().copied().map(Clause::Implementor));
        }
        if key.selected_candidate.is_none()
            && !prepared.query.require_impl
            && self.goal_can_use_assumptions(prepared.normalized_goal)
        {
            clauses.extend(
                (0..prepared.query.assumptions.list(self.db).len()).map(Clause::Assumption),
            );
            if let Some((requirements, bounds)) = prepared
                .normalized_goal
                .self_ty(self.db)
                .family_declared_bounds(self.db)
            {
                clauses.extend(bounds.list(self.db).iter().copied().map(|bound| {
                    Clause::FamilyBound {
                        bound,
                        requirements,
                    }
                }));
            }
        }
        Ok(CallbackOutcome::Continue(clauses))
    }

    fn apply_clause(
        &mut self,
        key: &Self::Key,
        clause: Self::Clause,
    ) -> Result<ContextTransition<Self>, Self::Error> {
        let PreparedQuery {
            mut table,
            query,
            scope,
            normalized_goal,
        } = match self.prepare_query(*key) {
            Ok(prepared) => prepared,
            Err(UnificationError::NormalizationFault) => {
                return Ok(Transition::Stop(StopReason::NormalizationFault));
            }
            Err(_) => unreachable!("preparing a proof query only normalizes types"),
        };

        let selected_impl = match clause {
            Clause::FamilyBound {
                bound,
                requirements,
            } => {
                let scope = TraitSolveCx::normalization_scope_for_trait_inst_with_origin(
                    self.db,
                    self.origin_ingot,
                    query.goal,
                );
                if let Err(error) = unify_trait_inst_with_normalized_assoc_bindings(
                    self.db,
                    &mut table,
                    bound,
                    normalized_goal,
                    scope,
                    query.assumptions,
                ) {
                    return Ok(match error {
                        UnificationError::NormalizationFault => {
                            Transition::Stop(StopReason::NormalizationFault)
                        }
                        _ => Transition::Reject,
                    });
                }
                let selected_impl =
                    ImplementorId::assumption(self.db, query.goal.fold_with(self.db, &mut table));
                let remaining_goals = requirements
                    .list(self.db)
                    .iter()
                    .map(|goal| goal.fold_with(self.db, &mut table))
                    .collect();
                return Ok(self.continue_branch(
                    key,
                    Branch {
                        table,
                        root_goal: query.goal,
                        remaining_goals,
                        selected_impl,
                        establishes_goal: false,
                        impl_conditions: Vec::new(),
                        trait_conditions: Vec::new(),
                    },
                    query.assumptions,
                ));
            }
            Clause::Implementor(selected_impl) => {
                if !impl_header_may_match(self.db, selected_impl, normalized_goal) {
                    return Ok(Transition::Reject);
                }
                let candidate = table.instantiate_with_fresh_vars(selected_impl);
                let normalized_candidate = match normalize_trait_inst_for_matching(
                    self.db,
                    candidate.trait_inst(self.db),
                    scope,
                    query.assumptions,
                ) {
                    Ok(candidate) => candidate,
                    Err(UnificationError::NormalizationFault) => {
                        return Ok(Transition::Stop(StopReason::NormalizationFault));
                    }
                    Err(_) => unreachable!("candidate normalization only normalizes types"),
                };
                if let Err(error) = unify_trait_inst_with_normalized_assoc_bindings(
                    self.db,
                    &mut table,
                    normalized_candidate,
                    normalized_goal,
                    scope,
                    query.assumptions,
                ) {
                    return Ok(match error {
                        UnificationError::NormalizationFault => {
                            Transition::Stop(StopReason::NormalizationFault)
                        }
                        _ => Transition::Reject,
                    });
                }

                let constraints = candidate.constraints(self.db);
                let remaining_goals = constraints
                    .list(self.db)
                    .iter()
                    .map(|constraint| constraint.fold_with(self.db, &mut table))
                    .collect();
                let branch = Branch {
                    table,
                    root_goal: query.goal,
                    remaining_goals,
                    selected_impl,
                    establishes_goal: true,
                    impl_conditions: if let crate::analysis::ty::trait_def::ImplementorOrigin::Hir(
                        implementation,
                    ) = candidate.origin(self.db)
                        && !crate::hir_def::WhereClauseOwner::ImplTrait(implementation)
                            .clause(self.db)
                            .id
                            .const_predicates(self.db)
                            .is_empty()
                    {
                        vec![candidate]
                    } else {
                        Vec::new()
                    },
                    trait_conditions: Vec::new(),
                };
                return Ok(self.continue_branch(key, branch, query.assumptions));
            }
            Clause::Assumption(index) => {
                let Some(&assumption) = query.assumptions.list(self.db).get(index) else {
                    return Ok(Transition::Reject);
                };
                if let Err(error) = unify_trait_inst_with_normalized_assoc_bindings(
                    self.db,
                    &mut table,
                    assumption,
                    normalized_goal,
                    scope,
                    query.assumptions,
                ) {
                    return Ok(match error {
                        UnificationError::NormalizationFault => {
                            Transition::Stop(StopReason::NormalizationFault)
                        }
                        _ => Transition::Reject,
                    });
                }
                ImplementorId::assumption(self.db, query.goal.fold_with(self.db, &mut table))
            }
        };

        let branch = Branch {
            table,
            root_goal: query.goal,
            remaining_goals: Vec::new(),
            selected_impl,
            establishes_goal: false,
            impl_conditions: Vec::new(),
            trait_conditions: Vec::new(),
        };
        Ok(self.finish_branch(key, branch))
    }

    fn resume(
        &mut self,
        parent: &Self::Key,
        mut branch: Self::State,
        answer: Self::Answer,
        rebase: Self::Rebase,
    ) -> Result<ContextTransition<Self>, Self::Error> {
        let pending_goal = rebase.goal();
        let solution = rebase.extract_solution(&mut branch.table, answer);
        let child_conditions = solution.impl_conditions.instances(self.db);
        for &instance in child_conditions {
            if !branch.impl_conditions.contains(&instance) {
                branch.impl_conditions.push(instance);
            }
        }
        for &goal in solution.impl_conditions.trait_goals(self.db) {
            if !branch.trait_conditions.contains(&goal) {
                branch.trait_conditions.push(goal);
            }
        }
        let solution = solution.inst;

        let normalized_pending = {
            let scope = TraitSolveCx::normalization_scope_for_trait_inst_with_origin(
                self.db,
                self.origin_ingot,
                pending_goal,
            );
            match normalize_trait_inst_for_matching(
                self.db,
                pending_goal.fold_with(self.db, &mut branch.table),
                scope,
                rebase.assumptions(),
            ) {
                Ok(goal) => goal,
                Err(UnificationError::NormalizationFault) => {
                    return Ok(Transition::Stop(StopReason::NormalizationFault));
                }
                Err(_) => unreachable!("pending-goal normalization only normalizes types"),
            }
        };
        let normalized_solution = {
            let scope = TraitSolveCx::normalization_scope_for_trait_inst_with_origin(
                self.db,
                self.origin_ingot,
                solution,
            );
            match normalize_trait_inst_for_matching(
                self.db,
                solution.fold_with(self.db, &mut branch.table),
                scope,
                rebase.assumptions(),
            ) {
                Ok(goal) => goal,
                Err(UnificationError::NormalizationFault) => {
                    return Ok(Transition::Stop(StopReason::NormalizationFault));
                }
                Err(_) => unreachable!("solution normalization only normalizes types"),
            }
        };
        if let Err(error) = branch.table.unify(normalized_pending, normalized_solution) {
            return Ok(match error {
                UnificationError::NormalizationFault => {
                    Transition::Stop(StopReason::NormalizationFault)
                }
                _ => Transition::Reject,
            });
        }
        let resumed_root = branch.root_goal.fold_with(self.db, &mut branch.table);
        let resumed_assumptions = rebase.assumptions().fold_with(self.db, &mut branch.table);
        if self.type_depth_budget.exceeded(
            self.db,
            TraitSolverQuery {
                goal: resumed_root,
                assumptions: resumed_assumptions,
                require_impl: parent.query.value().require_impl,
            },
        ) {
            return Ok(Transition::Stop(StopReason::MaximumTypeDepth));
        }

        Ok(self.continue_branch(parent, branch, rebase.assumptions()))
    }

    fn rebase_answer(
        &mut self,
        answer: &Self::Answer,
        _rebase: &Self::Rebase,
    ) -> Result<Self::Output, Self::Error> {
        // Solver entry points materialize an already-canonical query as the
        // root goal. Root answers therefore stay in that canonical coordinate
        // system until the outer `CanonicalGoalQuery` extracts them.
        Ok(*answer)
    }

    fn answer_key(&self, _key: &Self::Key, answer: &Self::Answer) -> Self::AnswerKey {
        *answer
    }
}

struct SuspendedGoal<'db> {
    query: CanonicalGoalQuery<'db>,
    table: PersistentUnificationTable<'db>,
    children: Vec<ConsumerId>,
}

#[derive(Default)]
struct UnresolvedGoalObserver<'db> {
    root_table: Option<tablesolve::TableId>,
    root_children: Vec<ConsumerId>,
    suspended: Vec<Option<SuspendedGoal<'db>>>,
}

impl<'db> UnresolvedGoalObserver<'db> {
    fn record_suspension(
        &mut self,
        consumer: ConsumerId,
        predecessor: Option<ResumeProvenance>,
        parent_table: tablesolve::TableId,
        state: &Branch<'db>,
        query: &CanonicalGoalQuery<'db>,
    ) {
        let index = consumer.index();
        if self.suspended.len() <= index {
            self.suspended.resize_with(index + 1, || None);
        }
        self.suspended[index] = Some(SuspendedGoal {
            query: query.clone(),
            table: state.table.clone(),
            children: Vec::new(),
        });

        if let Some(predecessor) = predecessor {
            if let Some(parent) = self
                .suspended
                .get_mut(predecessor.consumer.index())
                .and_then(Option::as_mut)
            {
                parent.children.push(consumer);
            }
        } else if self.root_table == Some(parent_table) {
            self.root_children.push(consumer);
        }
    }

    fn unresolved_subgoal(
        &mut self,
        db: &'db dyn HirAnalysisDb,
        root: Query<'db>,
    ) -> Option<UnsatSubgoal<'db>> {
        let [consumer] = self.root_children.as_slice() else {
            return None;
        };
        let mut consumer = *consumer;

        loop {
            let suspended = self
                .suspended
                .get_mut(consumer.index())
                .and_then(Option::as_mut)?;
            let [child] = suspended.children.as_slice() else {
                return Some(root.query.canonicalize_solution(
                    db,
                    &mut suspended.table,
                    suspended.query.goal(),
                ));
            };
            consumer = *child;
        }
    }
}

impl<'db> Observer<TraitResolutionContext<'db>> for UnresolvedGoalObserver<'db> {
    fn observe(&mut self, event: Event<'_, TraitResolutionContext<'db>>) {
        match event {
            Event::TableCreated { table_id, .. } if self.root_table.is_none() => {
                self.root_table = Some(table_id);
            }
            Event::Suspended {
                consumer_id,
                predecessor,
                parent_table_id,
                state,
                rebase,
                ..
            } => self.record_suspension(consumer_id, predecessor, parent_table_id, state, rebase),
            _ => {}
        }
    }
}

fn map_completion(completion: Completion<StopReason>) -> TraitSolveCompletion {
    match completion {
        Completion::Saturated => TraitSolveCompletion::Saturated,
        Completion::RootAnswerLimit { limit } => TraitSolveCompletion::RootAnswerLimit { limit },
        Completion::StepLimit { limit } => TraitSolveCompletion::StepLimit { limit },
        Completion::TableLimit { limit } => TraitSolveCompletion::TableLimit { limit },
        Completion::PendingWorkLimit { limit } => TraitSolveCompletion::PendingWorkLimit { limit },
        Completion::Adapter(StopReason::MaximumTypeDepth) => TraitSolveCompletion::MaximumTypeDepth,
        Completion::Adapter(StopReason::TargetFound) => {
            unreachable!("ordinary trait solving never installs a target")
        }
        Completion::Adapter(StopReason::NormalizationFault) => {
            unreachable!("normalization faults are handled before completion mapping")
        }
    }
}

pub(super) fn solve<'db>(
    db: &'db dyn HirAnalysisDb,
    use_scope: crate::hir_def::scope_graph::ScopeId<'db>,
    query: Canonical<TraitSolverQuery<'db>>,
) -> GoalSatisfiability<'db> {
    let _proof_search = super::enter_proof_search();
    let mut root_table = PersistentUnificationTable::new_proof(db);
    let root_goal = query.extract_identity(&mut root_table);
    let mut context = TraitResolutionContext::new(db, use_scope, root_goal, None);
    let mut observer = UnresolvedGoalObserver::default();
    let config = Config {
        limits: Limits {
            max_root_answers: Some(TRAIT_SOLVER_ROOT_ANSWER_LIMIT),
            ..Limits::default()
        },
        ..Config::default()
    };
    let options = ReportOptions::default().with_answerless(AnswerlessMode::Omit);
    let report = match solve_with_observer_and_options(
        &mut context,
        Goal {
            query: root_goal,
            selected_candidate: None,
        },
        config,
        options,
        &mut observer,
    ) {
        Ok(report) => report,
        Err(never) => match never {},
    };
    let root = report.root;
    let solutions: IndexSet<_> = report.answers.into_iter().collect();
    if matches!(
        report.completion,
        Completion::Adapter(StopReason::NormalizationFault)
    ) {
        return GoalSatisfiability::NormalizationFault;
    }
    let completion = map_completion(report.completion);

    match (completion, solutions.len()) {
        (TraitSolveCompletion::Saturated, 1) => {
            GoalSatisfiability::Satisfied(solutions.into_iter().next().unwrap())
        }
        (TraitSolveCompletion::Saturated, 0) => {
            GoalSatisfiability::UnSat(observer.unresolved_subgoal(db, root))
        }
        _ => GoalSatisfiability::NeedsConfirmation {
            solutions,
            completion,
        },
    }
}

pub(super) fn has_solution<'db>(
    db: &'db dyn HirAnalysisDb,
    use_scope: crate::hir_def::scope_graph::ScopeId<'db>,
    query: Canonical<TraitSolverQuery<'db>>,
    target: Canonical<TraitInstId<'db>>,
    relation: TargetSolutionMatch,
    selected_candidate: Option<ImplementorId<'db>>,
) -> TargetSearchResult<'db> {
    let _proof_search = super::enter_proof_search();
    let mut root_table = PersistentUnificationTable::new_proof(db);
    let root_goal = query.extract_identity(&mut root_table);
    let root_key = Query {
        query,
        selected_candidate,
    };
    let target = TargetAnswer {
        root: root_key,
        inst: target,
        relation,
    };
    let mut context = TraitResolutionContext::new(db, use_scope, root_goal, Some(target));
    let options = ReportOptions::default().with_answerless(AnswerlessMode::Omit);
    // A target can occur after a non-regular recursive clause. Fair scheduling
    // keeps that branch from starving later root clauses before the type-depth
    // guard makes the search incomplete.
    let config = Config {
        scheduling: Scheduling::Fair,
        ..Config::default()
    };
    let report = match solve_with_options(
        &mut context,
        Goal {
            query: root_goal,
            selected_candidate,
        },
        config,
        options,
    ) {
        Ok(report) => report,
        Err(never) => match never {},
    };
    match report.completion {
        Completion::Adapter(StopReason::TargetFound) => TargetSearchResult {
            status: TargetSolutionStatus::Found,
            impl_conditions: context.found_target_conditions.get(),
        },
        Completion::Adapter(StopReason::NormalizationFault) => {
            TargetSearchResult::without_conditions(TargetSolutionStatus::NormalizationFault)
        }
        Completion::Saturated => {
            TargetSearchResult::without_conditions(TargetSolutionStatus::NotFound)
        }
        Completion::RootAnswerLimit { .. }
        | Completion::StepLimit { .. }
        | Completion::TableLimit { .. }
        | Completion::PendingWorkLimit { .. }
        | Completion::Adapter(StopReason::MaximumTypeDepth) => {
            TargetSearchResult::without_conditions(TargetSolutionStatus::Incomplete)
        }
    }
}

#[salsa::tracked]
pub(crate) fn ty_depth_impl<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> usize {
    enum Task<'db> {
        Visit(TyId<'db>),
        Finish(TyId<'db>, Vec<TyId<'db>>, usize),
    }

    let mut pending = vec![Task::Visit(ty)];
    let mut active = FxHashSet::default();
    let mut depths = FxHashMap::default();
    while let Some(task) = pending.pop() {
        match task {
            Task::Visit(ty) => {
                if depths.contains_key(&ty) {
                    continue;
                }
                if !active.insert(ty) {
                    // Interned type graphs should be acyclic. If one is not,
                    // refuse to establish a finite growth bound.
                    return usize::MAX;
                }
                let (children, cost) = match ty.data(db) {
                    TyData::TypeFamily { args, body, .. } => (
                        args.iter().copied().chain(std::iter::once(*body)).collect(),
                        1,
                    ),
                    TyData::ConstTy(cty) => (vec![cty.ty(db)], 0),
                    TyData::ConstraintTerm(goal) => (
                        goal.args(db)
                            .iter()
                            .chain(goal.assoc_type_bindings(db).values())
                            .copied()
                            .collect(),
                        1,
                    ),
                    TyData::QualifiedTy(trait_inst) => (vec![trait_inst.self_ty(db)], 1),
                    TyData::TyApp(lhs, rhs) => (vec![*lhs, *rhs], 1),
                    TyData::Invalid(_)
                    | TyData::Never
                    | TyData::TyBase(_)
                    | TyData::TyParam(_)
                    | TyData::AssocTy { .. }
                    | TyData::TyVar(_) => (Vec::new(), 1),
                };
                pending.push(Task::Finish(ty, children.clone(), cost));
                pending.extend(children.into_iter().rev().map(Task::Visit));
            }
            Task::Finish(ty, children, cost) => {
                let depth = children
                    .iter()
                    .map(|child| depths[child])
                    .max()
                    .unwrap_or(0usize)
                    .saturating_add(cost);
                depths.insert(ty, depth);
                active.remove(&ty);
            }
        }
    }
    depths[&ty]
}

fn maximum_ty_depth<'db, V>(db: &'db dyn HirAnalysisDb, value: V) -> usize
where
    V: TyVisitable<'db>,
{
    struct DepthVisitor<'db> {
        db: &'db dyn HirAnalysisDb,
        max_depth: usize,
    }

    impl<'db> TyVisitor<'db> for DepthVisitor<'db> {
        fn db(&self) -> &'db dyn HirAnalysisDb {
            self.db
        }

        fn visit_ty(&mut self, ty: TyId) {
            self.max_depth = self.max_depth.max(ty_depth_impl(self.db, ty));
        }
    }

    let mut visitor = DepthVisitor { db, max_depth: 0 };
    value.visit_with(&mut visitor);
    visitor.max_depth
}

#[cfg(test)]
mod tests {
    use super::{
        Completion, MAXIMUM_TYPE_GROWTH, StopReason, TraitSolveCompletion, map_completion,
        type_depth_growth_exceeded,
    };

    #[test]
    fn completion_mapping_preserves_engine_stop_reasons() {
        assert_eq!(
            map_completion(Completion::Saturated),
            TraitSolveCompletion::Saturated
        );
        assert_eq!(
            map_completion(Completion::RootAnswerLimit { limit: 2 }),
            TraitSolveCompletion::RootAnswerLimit { limit: 2 }
        );
        assert_eq!(
            map_completion(Completion::StepLimit { limit: 3 }),
            TraitSolveCompletion::StepLimit { limit: 3 }
        );
        assert_eq!(
            map_completion(Completion::TableLimit { limit: 4 }),
            TraitSolveCompletion::TableLimit { limit: 4 }
        );
        assert_eq!(
            map_completion(Completion::PendingWorkLimit { limit: 5 }),
            TraitSolveCompletion::PendingWorkLimit { limit: 5 }
        );
        assert_eq!(
            map_completion(Completion::Adapter(StopReason::MaximumTypeDepth)),
            TraitSolveCompletion::MaximumTypeDepth
        );
    }

    #[test]
    fn type_depth_growth_budget_includes_its_boundary() {
        let initial_depth = 300;
        assert!(!type_depth_growth_exceeded(
            initial_depth,
            initial_depth + MAXIMUM_TYPE_GROWTH,
        ));
        assert!(type_depth_growth_exceeded(
            initial_depth,
            initial_depth + MAXIMUM_TYPE_GROWTH + 1,
        ));
        assert!(!type_depth_growth_exceeded(initial_depth, 1));
    }
}
