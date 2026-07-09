//! The algorithm for the trait resolution here is based on [`Tabled Typeclass Resolution`](https://arxiv.org/abs/2001.04301).
//! Also, [`XSB: Extending Prolog with Tabled Logic Programming`](https://arxiv.org/pdf/1012.5123) is a nice entry point for more detailed discussions about tabled logic solver.

use std::collections::BinaryHeap;

use common::indexmap::IndexSet;
use cranelift_entity::{PrimaryMap, entity_impl};
use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    CanonicalGoalQuery, GoalSatisfiability, TraitGoalSolution, TraitSolveCx, TraitSolverQuery,
    normalize_trait_inst_preserving_validity,
};
use crate::analysis::{
    HirAnalysisDb,
    ty::{
        binder::Binder,
        canonical::Canonical,
        const_ty::ConstTyData,
        fold::{TyFoldable, TyFolder},
        trait_def::{ImplementorId, TraitInstId, impls_for_trait_in_ingots},
        ty_def::{TyData, TyId, TyParam},
        unify::PersistentUnificationTable,
        visitor::{TyVisitable, TyVisitor},
    },
};
use crate::hir_def::scope_graph::ScopeId;
const MAXIMUM_SOLUTION_NUM: usize = 2;
/// The maximum depth of any type that the solver will consider.
///
/// This constant defines the upper limit on the depth of types that the solver
/// will handle. It is used as a termination condition to prevent the solver
/// from entering infinite loops when encountering coinductive cycles. If a
/// solution for subgoal or goal exceeds this limit, the solver stops search and
/// giveup.
const MAXIMUM_TYPE_DEPTH: usize = 256;

/// The query goal.
/// Since `TraitInstId` contains `Self` type as its first argument,
/// the query for `Implements<Ty, Trait<i32>>` is represented as
/// `Trait<Ty, i32>`.
type Query<'db> = Canonical<TraitSolverQuery<'db>>;
type Solution<'db> = crate::analysis::ty::canonical::Solution<TraitGoalSolution<'db>>;
type UnsatSubgoal<'db> = crate::analysis::ty::canonical::Solution<TraitInstId<'db>>;

/// A structure representing a proof forest used for solving trait goals.
///
/// The `ProofForest` contains generator and consumer nodes which work together
/// to find solutions to trait goals. It maintains stacks for generator and
/// consumer nodes to keep track of the solving process, and a mapping from
/// goals to generator nodes to avoid redundant computations.
pub(super) struct ProofForest<'db> {
    origin_ingot: crate::Ingot<'db>,

    /// The root generator node.
    root: GeneratorNode,

    /// An arena of generator nodes.
    g_nodes: PrimaryMap<GeneratorNode, GeneratorNodeData<'db>>,
    /// An arena of consumer nodes.
    c_nodes: PrimaryMap<ConsumerNode, ConsumerNodeData<'db>>,
    /// A stack of generator nodes to be processed.
    g_stack: Vec<GeneratorNode>,
    /// A binary heap used for managing consumer nodes and their solutions.
    ///
    /// This heap stores tuples of [`OrderedConsumerNode`] and [`Solution`],
    /// allowing the solver to efficiently retrieve and prioritize
    /// consumer nodes that are closer to the original goal.
    c_heap: BinaryHeap<(OrderedConsumerNode, Solution<'db>)>,

    /// A mapping from canonical solver queries to generator nodes.
    query_to_node: FxHashMap<Query<'db>, GeneratorNode>,

    /// The maximum number of solutions.
    maximum_solution_num: usize,
    /// The database for HIR analysis.
    db: &'db dyn HirAnalysisDb,
}

/// A structure representing an ordered consumer node in the proof forest.
///
/// The `OrderedConsumerNode` contains a consumer node and its root generator
/// node. It is used to prioritize consumer nodes based on their proximity to
/// the original goal. This allows the solver to efficiently retrieve and
/// process consumer nodes that are closer to the original goal, improving the
/// overall solving process.
#[derive(Debug, PartialEq, Eq)]
struct OrderedConsumerNode {
    node: ConsumerNode,
    root: GeneratorNode,
}
impl PartialOrd for OrderedConsumerNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OrderedConsumerNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.root.cmp(&self.root)
    }
}

impl<'db> ProofForest<'db> {
    /// Creates a new `ProofForest` with the given initial goal and assumptions.
    ///
    /// This function initializes the proof forest with a root generator node
    /// for the given goal and sets up the necessary data structures for
    /// solving trait goals.
    ///
    /// # Parameters
    /// - `db`: A reference to the HIR analysis database.
    /// - `goal`: The initial trait goal to be solved.
    /// - `assumptions`: The list of assumptions to be used during the solving
    ///   process.
    ///
    /// # Returns
    /// A new instance of `ProofForest` initialized with the given goal and
    /// assumptions.
    pub(super) fn new(
        db: &'db dyn HirAnalysisDb,
        origin_ingot: crate::Ingot<'db>,
        query: Query<'db>,
    ) -> Self {
        let mut forest = Self {
            origin_ingot,
            root: GeneratorNode(0), // Set temporary root.
            g_nodes: PrimaryMap::new(),
            c_nodes: PrimaryMap::new(),
            g_stack: Vec::new(),
            c_heap: BinaryHeap::new(),
            query_to_node: FxHashMap::default(),
            maximum_solution_num: MAXIMUM_SOLUTION_NUM,
            db,
        };

        let root = forest.new_generator_node(query);
        forest.root = root;
        forest
    }

    /// Solves the trait goal using a proof forest approach.
    ///
    /// This function iteratively processes generator and consumer nodes until
    /// either the maximum number of solutions is found or no more nodes can
    /// be processed. The solving process involves:
    /// - Popping solutions from the consumer stack and applying them.
    /// - Stepping through generator nodes to find new solutions or sub-goals.
    /// - Registering solutions and propagating them to dependent consumer
    ///   nodes.
    ///
    /// The function returns `GoalSatisfiability` indicating the status of the
    /// goal:
    /// - `Satisfied` if exactly one solution is found.
    /// - `UnSat` if no solutions are found and an unresolved subgoal is
    ///   identified.
    /// - `NeedsConfirmation` if multiple solutions are found.
    pub(super) fn solve(mut self) -> GoalSatisfiability<'db> {
        loop {
            if self.g_nodes[self.root].solutions.len() >= self.maximum_solution_num {
                break;
            }

            if let Some((c_node, solution)) = self.c_heap.pop() {
                if !c_node.node.apply_solution(&mut self, solution) {
                    return GoalSatisfiability::NeedsConfirmation(IndexSet::default());
                }
                continue;
            }

            if let Some(&g_node) = self.g_stack.last() {
                if !g_node.step(&mut self) {
                    self.g_stack.pop();
                }
                continue;
            }

            break;
        }

        let solutions = std::mem::take(&mut self.g_nodes[self.root].solutions);
        match solutions.len() {
            1 => GoalSatisfiability::Satisfied(solutions.into_iter().next().unwrap()),
            0 => {
                let unresolved_subgoal = self.root.unresolved_subgoal(&mut self);
                GoalSatisfiability::UnSat(unresolved_subgoal)
            }
            _ => GoalSatisfiability::NeedsConfirmation(solutions),
        }
    }

    fn new_generator_node(&mut self, query: Query<'db>) -> GeneratorNode {
        let g_node_data = GeneratorNodeData::new(self.db, self.origin_ingot, query);
        let g_node = self.g_nodes.push(g_node_data);
        self.query_to_node.insert(query, g_node);
        self.g_stack.push(g_node);
        g_node
    }

    /// Creates a new consumer node and registers it with the proof forest.
    ///
    /// This function takes a root generator node, a list of remaining goals,
    /// and a persistent unification table. It creates a consumer node that
    /// represents a sub-goal that needs to be solved and remaining
    /// subgoals. If the goal is not already associated with a generator
    /// node, a new generator node is created for it.
    ///
    /// The consumer node is then registered as a dependent of the corresponding
    /// generator node, ensuring that solutions found for the generator node are
    /// propagated to the consumer node.
    ///
    /// # Parameters
    /// - `root`: The root generator node of the consumer node.
    /// - `remaining_goals`: A list of trait instances that represent the
    ///   remaining goals to be solved.
    /// - `table`: A persistent unification table used for managing unification
    ///   operations.
    ///
    /// # Returns
    /// A new `ConsumerNode` that is registered with the proof forest.
    fn new_consumer_node(
        &mut self,
        root: GeneratorNode,
        query: TraitSolverQuery<'db>,
        mut remaining_goals: Vec<TraitInstId<'db>>,
        table: PersistentUnificationTable<'db>,
        selected_impl: ImplementorId<'db>,
    ) -> ConsumerNode {
        let pending_goal = remaining_goals.pop().unwrap();
        debug_assert_eq!(pending_goal, query.goal);
        let query = CanonicalGoalQuery::from_query(self.db, query);
        let canonical_query = query.canonical();

        let c_node_data = ConsumerNodeData {
            applied_solutions: FxHashSet::default(),
            remaining_goals,
            root,
            selected_impl,
            query,
            table,
            children: Vec::new(),
        };

        let c_node = self.c_nodes.push(c_node_data);
        if !self.query_to_node.contains_key(&canonical_query) {
            self.new_generator_node(canonical_query);
        }

        self.query_to_node[&canonical_query].add_dependent(self, c_node);
        c_node
    }
}

/// A structure representing the data associated with a generator node in the
/// proof forest.
///
/// The `GeneratorNodeData` contains information about the goal, the unification
/// table, the candidate implementors, the solutions found, and the dependents
/// of the generator node. It also keeps track of the assumptions, the next
/// candidate to be processed, and the child consumer nodes.
struct GeneratorNodeData<'db> {
    table: PersistentUnificationTable<'db>,
    /// The canonical query associated with the generator node.
    query: Query<'db>,
    /// The solver query extracted into the node-local table.
    extracted_query: TraitSolverQuery<'db>,
    /// A set of solutions found for the goal.
    solutions: IndexSet<Solution<'db>>,
    ///  A list of consumer nodes that depend on this generator node.
    dependents: Vec<ConsumerNode>,
    ///  A list of candidate implementors for the trait.
    cands: &'db [Binder<ImplementorId<'db>>],
    /// The index of the next candidate to be tried.
    next_cand: usize,
    /// A list of child consumer nodes created for sub-goals.
    children: Vec<ConsumerNode>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct GeneratorNode(u32);
entity_impl!(GeneratorNode);

impl<'db> GeneratorNodeData<'db> {
    fn new(db: &'db dyn HirAnalysisDb, origin_ingot: crate::Ingot<'db>, query: Query<'db>) -> Self {
        let mut table = PersistentUnificationTable::new(db);
        let extracted_query = query.extract_identity(&mut table);
        let extracted_goal = extracted_query.goal;
        let (primary, secondary) = TraitSolveCx::search_ingots_for_trait_inst_with_origin(
            db,
            origin_ingot,
            extracted_goal,
        );
        let cands =
            impls_for_trait_in_ingots(db, primary, secondary, Canonical::new(db, extracted_goal));

        // S2.1 soundness tripwire (spec sec 5, ladder S2.1; sec 1.4 G1). When the
        // goal's self type is an OPAQUE `recursive type fn` application (symbolic
        // subject, so it did not eager-expand), the S2.0 impl-target ban
        // guarantees no registered impl has a type-fn self-type head. Assert it:
        // the only sound discharge routes for such a goal are a blanket impl
        // (self type a bare param, valid at every ground instantiation) and the
        // caller's assumptions leg. A candidate whose self-type head is itself a
        // `TyBase::TypeFn` would mean the ban leaked, letting coherence depend on
        // subject arithmetic (the S2.1 "gate, don't select" hazard).
        #[cfg(debug_assertions)]
        if crate::analysis::ty::type_fn::type_fn_app_head(db, extracted_goal.self_ty(db)).is_some() {
            for cand in cands.iter() {
                let cand_self = cand.instantiate_identity().self_ty(db);
                debug_assert!(
                    crate::analysis::ty::type_fn::type_fn_app_head(db, cand_self).is_none(),
                    "S2.1 soundness tripwire: an impl candidate has a `recursive \
                     type fn` self-type head for an opaque type-fn goal; the S2.0 \
                     impl-target ban must keep this goal dischargeable only via \
                     caller assumptions (blanket impls excepted)",
                );
            }
        }

        Self {
            table,
            query,
            extracted_query,
            solutions: IndexSet::default(),
            dependents: Vec::new(),
            cands: cands.as_slice(),
            next_cand: 0,
            children: Vec::new(),
        }
    }
}

impl GeneratorNode {
    /// Registers the given solution with the proof forest and propagates it to
    /// dependent consumer nodes.
    ///
    /// This function canonicalizes the solution and inserts it into the set of
    /// solutions for the generator node. If the solution is new, it
    /// propagates the solution to all dependent consumer nodes.
    ///
    /// # Parameters
    /// - `pf`: A mutable reference to the `ProofForest`.
    /// - `table`: A mutable reference to the `PersistentUnificationTable` used
    ///   for managing unification operations.
    fn register_solution_with<'db>(
        self,
        pf: &mut ProofForest<'db>,
        table: &mut PersistentUnificationTable<'db>,
        selected_impl: ImplementorId<'db>,
    ) {
        let g_node = &mut pf.g_nodes[self];
        let solution = g_node.query.canonicalize_solution(
            table.db,
            table,
            TraitGoalSolution {
                inst: g_node.extracted_query.goal,
                implementor: selected_impl,
            },
        );
        if g_node.solutions.insert(solution) {
            for &c_node in g_node.dependents.iter() {
                let ordered_c_node = OrderedConsumerNode {
                    node: c_node,
                    root: pf.c_nodes[c_node].root,
                };
                pf.c_heap.push((ordered_c_node, solution));
            }
        }
    }

    /// Advances the solving process for the generator node.
    ///
    /// This function attempts to find a new solution or sub-goal for the
    /// generator node. It iterates through the candidate implementors and
    /// assumptions, unifying them with the goal. If a solution is found, it
    /// is registered. If a sub-goal is found, a new consumer node is
    /// created to handle it.
    ///
    /// # Parameters
    /// - `pf`: A mutable reference to the `ProofForest`.
    ///
    /// # Returns
    /// `true` if a new solution or sub-goal was found and processed; `false`
    /// otherwise.
    fn step(self, pf: &mut ProofForest) -> bool {
        let g_node = &mut pf.g_nodes[self];
        let db = pf.db;
        let extracted_goal = g_node.extracted_query.goal;
        let assumptions = g_node.extracted_query.assumptions;
        let scope = TraitSolveCx::normalization_scope_for_trait_inst_with_origin(
            db,
            pf.origin_ingot,
            extracted_goal,
        );
        let normalized_goal = normalize_trait_inst_preserving_validity(
            db,
            g_node.extracted_query.goal,
            scope,
            assumptions,
        );
        let goal_needs_assumptions = normalized_goal.args(db).iter().copied().any(|ty| {
            ty.has_param(db)
                || ty.has_var(db)
                || matches!(ty.data(db), TyData::AssocTy(_) | TyData::QualifiedTy(_))
        });

        while let Some(&cand) = g_node.cands.get(g_node.next_cand) {
            g_node.next_cand += 1;

            let mut table = g_node.table.clone();
            let selected_impl = cand.instantiate_identity();
            let gen_cand = table.instantiate_with_fresh_vars(cand);

            // TODO: require candidates to be pre-normalized
            // Normalize trait instance arguments before unification
            let normalized_gen_cand = normalize_trait_inst_preserving_validity(
                db,
                gen_cand.trait_inst(db),
                scope,
                assumptions,
            );
            if let Err(_err) = table.unify(normalized_gen_cand, normalized_goal) {
                continue;
            }

            let constraints = gen_cand.constraints(db);

            if constraints.list(db).is_empty() {
                self.register_solution_with(pf, &mut table, selected_impl);
            } else {
                let sub_goals: Vec<_> = {
                    constraints
                        .list(db)
                        .iter()
                        .map(|c| c.fold_with(db, &mut table))
                        .collect()
                };
                let child_query = TraitSolverQuery {
                    goal: *sub_goals.last().unwrap(),
                    assumptions: assumptions.fold_with(db, &mut table),
                };
                let child =
                    pf.new_consumer_node(self, child_query, sub_goals, table, selected_impl);
                pf.g_nodes[self].children.push(child);
            }

            return true;
        }

        if goal_needs_assumptions {
            let mut next_cand = g_node.next_cand - g_node.cands.len();
            while let Some(&assumption) = assumptions.list(db).get(next_cand) {
                g_node.next_cand += 1;
                next_cand += 1;
                let mut table = g_node.table.clone();
                if table.unify(assumption, normalized_goal).is_ok() {
                    let selected_impl =
                        ImplementorId::assumption(db, extracted_goal.fold_with(db, &mut table));
                    self.register_solution_with(pf, &mut table, selected_impl);
                    return true;
                }

                // A7.2 leg-3 instantiate-on-match. A derived UNGUARDED-GAT
                // universal `Self::Buffer<r0..>: Functor` carries the decl's OWN
                // rigids (`extend_all_bounds`'s applied-subject revival). Rigids
                // are not vars, so it can NEVER direct-unify with a use-site goal
                // `Self::Buffer<X>: Functor`. If (and only if) the assumption is
                // EXACTLY the sanctioned applied-GAT-subject shape -- the self
                // type is `AssocTy<r0..r_{k-1}>` with the decl's own rigids in
                // order, fully saturated, and NO assoc-owned rigid appears
                // outside that spine -- generalize precisely those rigids to
                // fresh inference vars (a scoped, `Binder`-style instantiation
                // keyed on the decl def-node scope, never a blanket rigid->var
                // pass) and retry. The gate keeps every other rigid rigid, so a
                // non-matching rigid-carrying assumption is skipped entirely.
                if let Some(decl_scope) = sanctioned_applied_gat_assumption(db, assumption) {
                    let mut table = g_node.table.clone();
                    let mut inst = GatUniversalInstantiator {
                        scope: decl_scope,
                        table: &mut table,
                        cache: FxHashMap::default(),
                    };
                    let instantiated = assumption.fold_with(db, &mut inst);
                    if table.unify(instantiated, normalized_goal).is_ok() {
                        let selected_impl =
                            ImplementorId::assumption(db, extracted_goal.fold_with(db, &mut table));
                        self.register_solution_with(pf, &mut table, selected_impl);
                        return true;
                    }
                }
            }
        }

        false
    }

    fn add_dependent(self, pf: &mut ProofForest, dependent: ConsumerNode) {
        let g_node = &mut pf.g_nodes[self];
        g_node.dependents.push(dependent);
        for &solution in g_node.solutions.iter() {
            let ordered_c_node = OrderedConsumerNode {
                node: dependent,
                root: pf.c_nodes[dependent].root,
            };
            pf.c_heap.push((ordered_c_node, solution))
        }
    }

    fn unresolved_subgoal<'db>(self, pf: &mut ProofForest<'db>) -> Option<UnsatSubgoal<'db>> {
        let g_node = &pf.g_nodes[self];
        // If the child nodes branch out more than one, we give up identifying the
        // unresolved subgoal to avoid generating a large number of uncertain unresolved
        // subgoals.
        if g_node.children.len() != 1 {
            return None;
        }

        let child = g_node.children[0];
        child.unresolved_subgoal(pf)
    }
}

/// A7.2 leg-3 gate: recognize an env assumption that is EXACTLY a revived
/// unguarded-GAT universal (`Self::Buffer<r0..r_{k-1}>: Bound`), returning the
/// decl def-node scope whose rigids the solver may generalize. Returns `None`
/// (skip instantiate-on-match) for every other assumption, including any
/// rigid-carrying predicate NOT in the sanctioned shape.
///
/// Sanctioned shape (all required):
/// 1. the assumption's SUBJECT (`self_ty`) decomposes to an `AssocTy` head
///    applied to a spine that is EXACTLY that decl's own rigids `0..k` in order
///    and fully saturates the decl (`spine.len() == arity`); and
/// 2. NO `is_assoc_ty_param` rigid appears anywhere in the assumption outside
///    that decl's scope.
///
/// This rejects (leg-3 test 13) a bare/partial head (`spine.len() != arity`),
/// an out-of-order spine (`spine[j].idx != j`), and another decl's rigids
/// (owner mismatch), each of which fails a clause above. It is the operational
/// twin of the restated A5.0 pin
/// (`gat_bound_assoc_owned_rigids_only_in_applied_universal_shape`): the only
/// assoc-owned rigids the solver ever treats as universally quantified are the
/// spine of a saturated applied GAT subject.
pub(super) fn sanctioned_applied_gat_assumption<'db>(
    db: &'db dyn HirAnalysisDb,
    inst: TraitInstId<'db>,
) -> Option<ScopeId<'db>> {
    let self_ty = inst.self_ty(db);
    let (base, spine) = self_ty.decompose_ty_app(db);
    let TyData::AssocTy(assoc) = base.data(db) else {
        return None;
    };
    // The decl def-node scope (`ScopeId::TraitType(t, decl_idx)`) whose rigids
    // this universal quantifies over.
    let decl_scope = assoc.scope(db)?;
    let trait_def = assoc.trait_.def(db);
    let decl_view = trait_def
        .assoc_types(db)
        .find(|v| v.name(db) == Some(assoc.name))?;
    let arity = decl_view.generic_params(db).data(db).len();
    if arity == 0 || spine.len() != arity {
        return None;
    }
    // Spine must be the decl's own rigids 0..k in order (owner-exact).
    for (j, &arg) in spine.iter().enumerate() {
        let TyData::TyParam(param) = arg.data(db) else {
            return None;
        };
        if !param.is_assoc_ty_param() || param.owner != decl_scope || param.idx != j {
            return None;
        }
    }
    // No assoc-owned rigid may occur outside this decl's scope anywhere in the
    // predicate (belt: with the derivation rule no other rigid-carrying shape
    // exists in envs; this makes the gate self-checking).
    if !all_assoc_rigids_in_scope(db, inst, decl_scope) {
        return None;
    }
    Some(decl_scope)
}

/// True iff every `is_assoc_ty_param` rigid reachable in `inst` is owned by
/// `scope`. Used to reject an assumption whose bound args smuggle a rigid from
/// a different decl into the sanctioned-shape gate.
fn all_assoc_rigids_in_scope<'db>(
    db: &'db dyn HirAnalysisDb,
    inst: TraitInstId<'db>,
    scope: ScopeId<'db>,
) -> bool {
    struct Scan<'db> {
        db: &'db dyn HirAnalysisDb,
        scope: ScopeId<'db>,
        ok: bool,
    }
    impl<'db> TyVisitor<'db> for Scan<'db> {
        fn db(&self) -> &'db dyn HirAnalysisDb {
            self.db
        }
        fn visit_param(&mut self, ty_param: &TyParam<'db>) {
            if ty_param.is_assoc_ty_param() && ty_param.owner != self.scope {
                self.ok = false;
            }
        }
    }
    let mut scan = Scan { db, scope, ok: true };
    inst.visit_with(&mut scan);
    scan.ok
}

/// A7.2 leg-3 universal instantiation: replace every rigid owned by the decl
/// def-node `scope` with a FRESH inference var (one per param index, cached so
/// the same rigid maps to the same var across both the subject spine and the
/// bound args), leaving all other rigids intact. This is the scoped analogue of
/// `PersistentUnificationTable::instantiate_with_fresh_vars`' seam-S1 closure:
/// there the whole table keeps assoc params rigid, here we deliberately lift a
/// SINGLE decl's params to vars for a universal-elimination match, and only
/// after `sanctioned_applied_gat_assumption` proved the shape.
struct GatUniversalInstantiator<'db, 'a> {
    scope: ScopeId<'db>,
    table: &'a mut PersistentUnificationTable<'db>,
    cache: FxHashMap<usize, TyId<'db>>,
}

impl<'db> TyFolder<'db> for GatUniversalInstantiator<'db, '_> {
    fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
        match ty.data(db) {
            TyData::TyParam(param)
                if param.is_assoc_ty_param() && param.owner == self.scope =>
            {
                if let Some(&var) = self.cache.get(&param.idx) {
                    var
                } else {
                    let var = self.table.new_var_from_param(ty);
                    self.cache.insert(param.idx, var);
                    var
                }
            }
            TyData::ConstTy(const_ty) => match const_ty.data(db) {
                ConstTyData::TyParam(param, _)
                    if param.is_assoc_ty_param() && param.owner == self.scope =>
                {
                    if let Some(&var) = self.cache.get(&param.idx) {
                        var
                    } else {
                        let var = self.table.new_var_from_param(ty);
                        self.cache.insert(param.idx, var);
                        var
                    }
                }
                _ => ty.super_fold_with(db, self),
            },
            _ => ty.super_fold_with(db, self),
        }
    }
}

struct ConsumerNodeData<'db> {
    /// Holds solutions that are already applied.
    applied_solutions: FxHashSet<Solution<'db>>,
    remaining_goals: Vec<TraitInstId<'db>>,
    /// The root generator node of the consumer node.
    root: GeneratorNode,
    selected_impl: ImplementorId<'db>,

    /// The current pending query that is resolved by another [`GeneratorNode`].
    query: CanonicalGoalQuery<'db>,
    table: PersistentUnificationTable<'db>,
    children: Vec<ConsumerNode>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ConsumerNode(u32);
entity_impl!(ConsumerNode);

impl ConsumerNode {
    /// Applies a given solution to the consumer node.
    ///
    /// This function checks if the solution has already been applied. If not,
    /// it attempts to unify the solution with the pending query of the
    /// consumer node. If the unification is successful and there are no
    /// remaining goals, the solution is registered with the root generator
    /// node. If there are remaining goals, a new consumer node is created
    /// to handle them.
    ///
    /// # Parameters
    /// - `pf`: A mutable reference to the `ProofForest`.
    /// - `solution`: The solution to be applied.
    fn apply_solution<'db>(self, pf: &mut ProofForest<'db>, solution: Solution<'db>) -> bool {
        let c_node = &mut pf.c_nodes[self];

        // If the solutions is already applied, do nothing.
        if !c_node.applied_solutions.insert(solution) {
            return true;
        }

        let mut table = c_node.table.clone();
        let db = pf.db;

        // Extract solution to the current env.
        let pending_query = c_node.query.clone();
        let pending_inst = pending_query.goal();
        let solution = pending_query.extract_solution(&mut table, solution).inst;

        // Normalize both instances before unification
        let normalized_pending = {
            let scope = TraitSolveCx::normalization_scope_for_trait_inst_with_origin(
                db,
                pf.origin_ingot,
                pending_inst,
            );
            let assumptions = pending_query.assumptions();
            normalize_trait_inst_preserving_validity(
                db,
                pending_inst.fold_with(db, &mut table),
                scope,
                assumptions,
            )
        };

        let normalized_solution = {
            let scope = TraitSolveCx::normalization_scope_for_trait_inst_with_origin(
                db,
                pf.origin_ingot,
                solution,
            );
            let assumptions = pending_query.assumptions();
            normalize_trait_inst_preserving_validity(
                db,
                solution.fold_with(db, &mut table),
                scope,
                assumptions,
            )
        };

        // Try to unifies pending inst and solution.
        if table
            .unify(normalized_pending, normalized_solution)
            .is_err()
        {
            return true;
        }

        let tree_root = c_node.root;
        let selected_impl = c_node.selected_impl;
        let remaining_goals = c_node.remaining_goals.clone();
        let _ = c_node;

        if remaining_goals.is_empty() {
            // If no remaining goals in the consumer node, it's the solution for the root
            // goal.
            tree_root.register_solution_with(pf, &mut table, selected_impl);
        } else {
            // Create a child consumer node for the subgoals.
            let child_query = TraitSolverQuery {
                goal: *remaining_goals.last().unwrap(),
                assumptions: pending_query.assumptions().fold_with(db, &mut table),
            };
            let child = pf.new_consumer_node(
                tree_root,
                child_query,
                remaining_goals,
                table,
                selected_impl,
            );
            pf.c_nodes[self].children.push(child);
        }

        maximum_ty_depth(db, solution) <= MAXIMUM_TYPE_DEPTH
    }

    fn unresolved_subgoal<'db>(self, pf: &mut ProofForest<'db>) -> Option<UnsatSubgoal<'db>> {
        let c_node = &mut pf.c_nodes[self];
        if c_node.children.len() != 1 {
            let unsat = c_node.query.goal();
            let unsat = pf.g_nodes[c_node.root].query.canonicalize_solution(
                pf.db,
                &mut c_node.table,
                unsat,
            );
            return Some(unsat);
        }

        c_node.children[0].unresolved_subgoal(pf)
    }
}

/// Computes the depth of a given type.
///
/// The depth of a type is defined as the maximum depth of its subcomponents
/// plus one. For example, a simple type like `i32` has a depth of 1, while a
/// compound type like `Option<Result<i32, String>>` would have a depth
/// reflecting the nesting of its components.
///
/// # Parameters
/// - `db`: A reference to the HIR analysis database.
/// - `ty`: The type for which the depth is to be computed.
///
/// # Returns
/// The depth of the type as a `usize`.
///
/// # Note
/// This function is a stop gap solution to ensure termination when the solver
/// encounters coinductive cycles. It serves as a temporary solution until the
/// solver can properly handle coinductive cycles.
#[salsa::tracked]
pub(crate) fn ty_depth_impl<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> usize {
    match ty.data(db) {
        TyData::ConstTy(cty) => ty_depth_impl(db, cty.ty(db)),
        TyData::Invalid(_)
        | TyData::Never
        | TyData::TyBase(_)
        | TyData::TyParam(_)
        | TyData::AssocTy { .. }
        | TyData::ConstraintTerm(_)
        | TyData::TraitCtor(_)
        | TyData::TyVar(_) => 1,
        TyData::QualifiedTy(trait_inst) => ty_depth_impl(db, trait_inst.self_ty(db)) + 1,
        TyData::TyApp(lhs, rhs) => {
            let lhs_depth = ty_depth_impl(db, *lhs);
            let rhs_depth = ty_depth_impl(db, *rhs);
            std::cmp::max(lhs_depth, rhs_depth) + 1
        }
    }
}

/// Computes the maximum depth of any type within a visitable structure.
///
/// This function traverses the given visitable structure and computes the
/// maximum depth of any type it encounters. The depth of a type is defined
/// as the maximum depth of its subcomponents plus one. For example, a simple
/// type like `i32` has a depth of 1, while a compound type like
/// `Option<Result<i32, String>>` would have a depth reflecting the nesting
/// of its components.
///
/// # Parameters
/// - `db`: A reference to the HIR analysis database.
/// - `v`: The visitable structure for which the maximum type depth is to be
///   computed.
///
/// # Returns
/// The maximum depth of any type within the visitable structure as a `usize`.
///
/// # Note
/// This function is a stop gap solution to ensure termination when the solver
/// encounters coinductive cycles. It serves as a temporary solution until the
/// solver can properly handle coinductive cycles.
fn maximum_ty_depth<'db, V>(db: &'db dyn HirAnalysisDb, v: V) -> usize
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
            let depth = ty_depth_impl(self.db, ty);
            if depth > self.max_depth {
                self.max_depth = depth;
            }
        }
    }

    let mut visitor = DepthVisitor { db, max_depth: 0 };
    v.visit_with(&mut visitor);
    visitor.max_depth
}
