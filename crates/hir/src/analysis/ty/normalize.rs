//! Type normalization module
//!
//! This module provides functionality to normalize types by resolving associated types
//! to concrete types when possible. This happens before type unification to ensure
//! that types are in their most resolved form.

use crate::core::hir_def::{ImplTrait, scope_graph::ScopeId};
use common::indexmap::IndexMap;
use rustc_hash::FxHashMap;

use super::{
    binder::Binder,
    canonical::Canonical,
    canonical::Canonicalized,
    const_ty::{
        ConstCanonEnv, ConstTyData, concretize_const_ty_if_ground, evaluate_type_level_const_expr,
    },
    fold::{TyFoldable, TyFolder, ty_contains},
    layout_holes::LayoutRootUse,
    trait_def::{
        ImplementorOrigin, TraitInstId, TraitRefId, impls_for_trait_inst_with_possible_constraints,
        resolve_trait_impl_instance,
    },
    trait_lower::complete_candidate_impl_assoc_ty,
    trait_resolution::{PredicateListId, Selection},
    ty_def::{AssocTy, InvalidCause, TyBase, TyData, TyId, TyParam, collect_variables},
    unify::UnificationTable,
    visitor::{TyVisitable, TyVisitor, walk_const_ty, walk_ty},
};
use crate::analysis::{
    HirAnalysisDb,
    name_resolution::{FindAssociatedTypeError, find_associated_type},
    semantic::ConstRepr,
};
use crate::hir_def::TypeFnPat;

// One unit is the computed-type entry and up to 4,096 units are its unfoldings.
pub(crate) const NORMALIZATION_DEPTH_LIMIT: usize = 4098;
pub(crate) const NORMALIZATION_WORK_LIMIT: usize = 65536;
// These weights retain the previous family and projection nesting limits while
// allowing long, iterative computed-type reductions in the same root budget.
const FAMILY_DEPTH_COST: usize = 16;
const PROJECTION_DEPTH_COST: usize = 64;

#[derive(Default)]
struct ReductionBudget {
    work: usize,
}

impl ReductionBudget {
    fn visit(&mut self) -> bool {
        if self.work >= NORMALIZATION_WORK_LIMIT {
            return false;
        }
        self.work += 1;
        true
    }

    fn charge(&mut self, amount: usize) -> bool {
        let Some(total) = self.work.checked_add(amount) else {
            return false;
        };
        if total > NORMALIZATION_WORK_LIMIT {
            return false;
        }
        self.work = total;
        true
    }
}

// Checked constant occurrences carry a source tree as well as a canonical
// term. Substituting an occurrence can traverse that tree many times before
// the reducer reaches another type node, so count the transport work in the
// same budget as type reductions.
struct ConstTransportCost<'db> {
    db: &'db dyn HirAnalysisDb,
    remaining: usize,
    exceeded: bool,
}

impl<'db> ConstTransportCost<'db> {
    fn measure(db: &'db dyn HirAnalysisDb, ty: TyId<'db>, remaining: usize) -> Option<usize> {
        let mut counter = Self {
            db,
            remaining,
            exceeded: false,
        };
        counter.visit_ty(ty);
        (!counter.exceeded).then_some(remaining - counter.remaining)
    }

    fn charge(&mut self) -> bool {
        if self.remaining == 0 {
            self.exceeded = true;
            return false;
        }
        self.remaining -= 1;
        true
    }
}

impl<'db> TyVisitor<'db> for ConstTransportCost<'db> {
    fn db(&self) -> &'db dyn HirAnalysisDb {
        self.db
    }

    fn visit_ty(&mut self, ty: TyId<'db>) {
        if self.charge() {
            walk_ty(self, ty);
        }
    }

    fn visit_const_ty(&mut self, const_ty: &super::const_ty::ConstTyId<'db>) {
        if !self.charge() {
            return;
        }
        let ConstTyData::Computation {
            description,
            source,
        } = const_ty.data(self.db)
        else {
            walk_const_ty(self, const_ty);
            return;
        };
        self.visit_ty(description.ty());
        match description.repr() {
            ConstRepr::Value(value) => {
                self.visit_ty(TyId::const_ty(
                    self.db,
                    super::const_ty::const_ty_from_sem_const(self.db, value.value()),
                ));
            }
            ConstRepr::Term(term) => self.visit_ty(TyId::const_ty(self.db, *term)),
            ConstRepr::Deferred(computation) => computation.visit_with(self),
        }
        if let Some(root) = description.term_provenance() {
            let mut pending = vec![root];
            while let Some(provenance) = pending.pop() {
                if !self.charge() {
                    return;
                }
                self.visit_ty(TyId::const_ty(self.db, provenance.term));
                for frame in &provenance.frames {
                    frame.callee.visit_with(self);
                }
                pending.extend(provenance.operands.iter());
                if self.exceeded {
                    return;
                }
            }
        }
        source.visit_with(self);
    }
}

/// Normalizes a type by resolving all associated types to concrete types when possible.
///
/// This function takes a type and attempts to resolve any associated types within it
/// using the provided assumptions and scope context. It handles:
/// - Simple associated types (e.g., `T::Output`)
/// - Nested associated types (e.g., `T::Encoder::Output`)
/// - Associated types with generic parameters
pub fn normalize_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
    premises: crate::analysis::semantic::SolverPremises<'db>,
) -> TyId<'db> {
    let mut normalizer = TypeNormalizer::new(db, premises);
    normalizer.reduce(ty, 1, ConstPolicy::Preserve)
}

/// Apply declared associated equalities without implementation selection.
/// Structural slot planning uses declaration coordinates before the slots it
/// is discovering exist, so implementation lookup would create a query cycle.
pub fn normalize_from_assumptions<'db, T>(
    db: &'db dyn HirAnalysisDb,
    value: T,
    premises: crate::analysis::semantic::SolverPremises<'db>,
) -> T
where
    T: TyFoldable<'db>,
{
    let mut normalizer = TypeNormalizer::new(db, premises);
    normalizer.resolve_impls = false;
    value.fold_with(db, &mut normalizer)
}

/// Apply the associated equalities carried by a single trait predicate, as
/// [`normalize_from_assumptions`] does.
pub fn normalize_with_trait_evidence<'db, T>(
    db: &'db dyn HirAnalysisDb,
    value: T,
    scope: ScopeId<'db>,
    evidence: TraitInstId<'db>,
) -> T
where
    T: TyFoldable<'db>,
{
    // The premise is one trait predicate supplied by the caller, not the
    // premises of a position.
    normalize_from_assumptions(
        db,
        value,
        crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
            scope,
            PredicateListId::new(db, vec![evidence]),
        ),
    )
}

pub(crate) fn normalize_layout_root_uses<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
    premises: crate::analysis::semantic::SolverPremises<'db>,
) -> Vec<LayoutRootUse<'db>> {
    fn collect<'db>(
        db: &'db dyn HirAnalysisDb,
        ty: TyId<'db>,
        premises: crate::analysis::semantic::SolverPremises<'db>,
        visiting: &mut rustc_hash::FxHashSet<TyId<'db>>,
        uses: &mut Vec<LayoutRootUse<'db>>,
    ) {
        if !visiting.insert(ty) {
            return;
        }
        if let TyData::AssocTy(assoc) = ty.data(db) {
            let solve_cx = premises.solver_context(db);
            if let Selection::Unique(resolved) =
                resolve_trait_impl_instance(db, solve_cx, assoc.trait_.as_predicate(db))
                && let ImplementorOrigin::Hir(impl_trait) = resolved.selected().origin(db)
            {
                for root_use in resolved.assoc_ty_layout_root_uses(db, assoc.name) {
                    let root_use = LayoutRootUse {
                        value: Binder::bind(impl_trait.into(), root_use.value)
                            .instantiate(db, resolved.impl_args(db)),
                        owner: root_use.owner.map(|owner| {
                            normalize_ty(
                                db,
                                Binder::bind(impl_trait.into(), owner)
                                    .instantiate(db, resolved.impl_args(db)),
                                premises,
                            )
                        }),
                        selector: root_use.selector,
                    };
                    if !uses.contains(&root_use) {
                        uses.push(root_use);
                    }
                }
                if let Some(instantiated) = resolved.instantiated_assoc_ty(db, assoc.name) {
                    collect(db, instantiated, premises, visiting, uses);
                }
            }
        } else {
            let (base, args) = ty.decompose_ty_app(db);
            if base != ty {
                collect(db, base, premises, visiting, uses);
            }
            for arg in args {
                collect(db, *arg, premises, visiting, uses);
            }
        }
        visiting.remove(&ty);
    }

    let mut uses = Vec::new();
    collect(
        db,
        ty,
        premises,
        &mut rustc_hash::FxHashSet::default(),
        &mut uses,
    );
    uses
}

pub struct TypeNormalizer<'db> {
    db: &'db dyn HirAnalysisDb,
    premises: crate::analysis::semantic::SolverPremises<'db>,
    resolve_impls: bool,
    cache: FxHashMap<ReductionKey<'db>, Option<Reduced<'db>>>,
    budget: ReductionBudget,
    nested_depth: usize,
    nested_policy: ConstPolicy,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ConstPolicy {
    Preserve,
    Projection,
    Computed,
}

impl ConstPolicy {
    fn in_projection(self) -> Self {
        if self == Self::Computed {
            self
        } else {
            Self::Projection
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ReductionKey<'db> {
    ty: TyId<'db>,
    policy: ConstPolicy,
}

#[derive(Clone, Copy)]
struct Reduced<'db> {
    ty: TyId<'db>,
    depth: usize,
    /// Nodes in `ty` written out as a tree. Interned types share subtrees, so
    /// a type small as a graph can be exponentially large as a tree, and later
    /// passes walk types as trees.
    size: usize,
}

enum ReductionTask<'db> {
    Visit(ReductionKey<'db>, usize),
    Rebuild {
        key: ReductionKey<'db>,
        depth: usize,
        children: Vec<TyId<'db>>,
        child_policy: ConstPolicy,
    },
    Remember {
        key: ReductionKey<'db>,
        result: ReductionKey<'db>,
        cost: usize,
        child_depth: usize,
    },
}

struct Children<'db>(Vec<TyId<'db>>);

impl<'db> TyFolder<'db> for Children<'db> {
    fn fold_ty(&mut self, _: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
        self.0.push(ty);
        ty
    }
}

struct Replacements<'a, 'db> {
    resolve_impls: bool,
    results: &'a FxHashMap<ReductionKey<'db>, Reduced<'db>>,
    policy: ConstPolicy,
}

impl<'db> TyFolder<'db> for Replacements<'_, 'db> {
    fn fold_ty_app(
        &mut self,
        db: &'db dyn HirAnalysisDb,
        abs: TyId<'db>,
        arg: TyId<'db>,
    ) -> TyId<'db> {
        if self.resolve_impls {
            TyId::app(db, abs, arg)
        } else {
            TyId::app_structural(db, abs, arg)
        }
    }

    fn fold_ty(&mut self, _: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
        self.results[&ReductionKey {
            ty,
            policy: self.policy,
        }]
            .ty
    }
}

impl<'db> TypeNormalizer<'db> {
    pub fn new(
        db: &'db dyn HirAnalysisDb,
        premises: crate::analysis::semantic::SolverPremises<'db>,
    ) -> Self {
        Self {
            db,
            premises,
            resolve_impls: true,
            cache: FxHashMap::default(),
            budget: ReductionBudget::default(),
            nested_depth: 1,
            nested_policy: ConstPolicy::Preserve,
        }
    }
}

impl<'db> TyFolder<'db> for TypeNormalizer<'db> {
    fn fold_ty_app(
        &mut self,
        db: &'db dyn HirAnalysisDb,
        abs: TyId<'db>,
        arg: TyId<'db>,
    ) -> TyId<'db> {
        if self.resolve_impls {
            TyId::app(db, abs, arg)
        } else {
            TyId::app_structural(db, abs, arg)
        }
    }

    fn fold_ty(&mut self, _: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
        self.reduce(ty, self.nested_depth, self.nested_policy)
    }
}

impl<'db> TypeNormalizer<'db> {
    fn abort(&mut self, active: &[ReductionKey<'db>], cause: InvalidCause<'db>) -> TyId<'db> {
        let invalid = TyId::invalid(self.db, cause);
        for &key in active {
            if self.cache.get(&key).is_some_and(Option::is_none) {
                self.cache.insert(
                    key,
                    Some(Reduced {
                        ty: invalid,
                        depth: 0,
                        size: 1,
                    }),
                );
            }
        }
        invalid
    }

    fn reduce(&mut self, root: TyId<'db>, depth: usize, policy: ConstPolicy) -> TyId<'db> {
        let db = self.db;
        let root_key = ReductionKey { ty: root, policy };
        let mut pending = vec![ReductionTask::Visit(root_key, depth)];
        let mut results = FxHashMap::default();
        let mut active = Vec::new();

        while let Some(task) = pending.pop() {
            match task {
                ReductionTask::Visit(key, depth) => {
                    if let Some(cached) = self.cache.get(&key).copied() {
                        let reduced = cached.unwrap_or_else(|| Reduced {
                            ty: if matches!(key.ty.data(db), TyData::AssocTy(_)) {
                                key.ty
                            } else {
                                TyId::invalid(db, InvalidCause::TypeLoweringCycle)
                            },
                            depth: 0,
                            size: 1,
                        });
                        if depth.saturating_add(reduced.depth) >= NORMALIZATION_DEPTH_LIMIT {
                            return self.abort(&active, InvalidCause::TypeNormalizationLimit);
                        }
                        results.insert(key, reduced);
                        continue;
                    }
                    if depth >= NORMALIZATION_DEPTH_LIMIT || !self.budget.visit() {
                        return self.abort(&active, InvalidCause::TypeNormalizationLimit);
                    }
                    if matches!(key.ty.data(db), TyData::ConstTy(value) if matches!(value.data(db), ConstTyData::Computation { .. }))
                    {
                        let remaining = NORMALIZATION_WORK_LIMIT - self.budget.work;
                        let Some(cost) = ConstTransportCost::measure(db, key.ty, remaining) else {
                            return self.abort(&active, InvalidCause::TypeNormalizationLimit);
                        };
                        if !self.budget.charge(cost) {
                            return self.abort(&active, InvalidCause::TypeNormalizationLimit);
                        }
                    }
                    self.cache.insert(key, None);
                    active.push(key);
                    let child_policy = if self.is_full_computed_application(key.ty) {
                        ConstPolicy::Computed
                    } else {
                        key.policy
                    };
                    let mut children = Children(Vec::new());
                    key.ty.super_fold_with(db, &mut children);
                    pending.push(ReductionTask::Rebuild {
                        key,
                        depth,
                        children: children.0.clone(),
                        child_policy,
                    });
                    pending.extend(children.0.into_iter().rev().map(|ty| {
                        ReductionTask::Visit(
                            ReductionKey {
                                ty,
                                policy: child_policy,
                            },
                            depth,
                        )
                    }));
                }
                ReductionTask::Rebuild {
                    key,
                    depth,
                    children,
                    child_policy,
                } => {
                    let ty = key.ty.super_fold_with(
                        db,
                        &mut Replacements {
                            resolve_impls: self.resolve_impls,
                            results: &results,
                            policy: child_policy,
                        },
                    );
                    let child_results = children.iter().map(|&ty| {
                        results[&ReductionKey {
                            ty,
                            policy: child_policy,
                        }]
                    });
                    let child_depth = child_results.clone().map(|r| r.depth).max().unwrap_or(0);
                    // The written-out size of a result counts against the
                    // same work limit as the reduction steps: a result too
                    // large to write out is as unusable as one that takes
                    // too many steps.
                    let size = child_results.fold(1usize, |total, r| total.saturating_add(r.size));
                    if size > NORMALIZATION_WORK_LIMIT {
                        return self.abort(&active, InvalidCause::TypeNormalizationLimit);
                    }
                    let rewrite = match self.rewrite_head(ty, depth, key.policy) {
                        Ok(rewrite) => rewrite,
                        Err(cause) => return self.abort(&active, cause),
                    };
                    if let Some((result, cost, policy)) = rewrite {
                        if depth.saturating_add(cost) >= NORMALIZATION_DEPTH_LIMIT {
                            return self.abort(&active, InvalidCause::TypeNormalizationLimit);
                        }
                        let result = ReductionKey { ty: result, policy };
                        pending.push(ReductionTask::Remember {
                            key,
                            result,
                            cost,
                            child_depth,
                        });
                        pending.push(ReductionTask::Visit(result, depth + cost));
                    } else {
                        let own_depth = if matches!(ty.data(db), TyData::AssocTy(_)) {
                            PROJECTION_DEPTH_COST
                        } else {
                            0
                        };
                        let reduced = Reduced {
                            ty,
                            depth: child_depth.max(own_depth),
                            size,
                        };
                        self.cache.insert(key, Some(reduced));
                        results.insert(key, reduced);
                    }
                }
                ReductionTask::Remember {
                    key,
                    result,
                    cost,
                    child_depth,
                } => {
                    let target = results[&result];
                    let reduced = Reduced {
                        ty: target.ty,
                        depth: child_depth.max(cost.saturating_add(target.depth)),
                        size: target.size,
                    };
                    self.cache.insert(key, Some(reduced));
                    results.insert(key, reduced);
                }
            }
        }
        results[&root_key].ty
    }

    fn is_full_computed_application(&self, ty: TyId<'db>) -> bool {
        let db = self.db;
        let (head, args) = ty.decompose_ty_app(db);
        self.resolve_impls
            && matches!(head.data(db), TyData::TyBase(TyBase::TypeFn(def))
            if args.len() == def.generic_params(db).data(db).len())
            && !super::type_fn::in_definition(db, self.premises.scope())
    }

    fn rewrite_head(
        &mut self,
        ty: TyId<'db>,
        depth: usize,
        policy: ConstPolicy,
    ) -> Result<Option<(TyId<'db>, usize, ConstPolicy)>, InvalidCause<'db>> {
        let db = self.db;
        if let TyData::ConstTy(value) = ty.data(db) {
            let evaluated = match (policy, value.data(db)) {
                (ConstPolicy::Projection, ConstTyData::UnEvaluated { .. }) => {
                    concretize_const_ty_if_ground(
                        db,
                        *value,
                        ConstCanonEnv::new(
                            self.premises.scope(),
                            self.premises.assumptions(),
                            None,
                        ),
                    )
                    // A ground value whose evaluation faulted is not ground;
                    // evaluate it to report the fault instead of keeping it.
                    .unwrap_or_else(|| {
                        let evaluated = value.evaluate(db, None);
                        if matches!(evaluated.data(db), ConstTyData::Invalid(_)) {
                            evaluated
                        } else {
                            *value
                        }
                    })
                }
                (ConstPolicy::Computed, ConstTyData::UnEvaluated { .. }) => {
                    value.evaluate(db, None)
                }
                (ConstPolicy::Computed, ConstTyData::Abstract(expr, expected)) => {
                    evaluate_type_level_const_expr(
                        db,
                        *expr,
                        *expected,
                        ConstCanonEnv::new(
                            self.premises.scope(),
                            self.premises.assumptions(),
                            None,
                        ),
                    )
                    .unwrap_or(*value)
                }
                _ => *value,
            };
            // A fault while evaluating a const is a hard error, never a value
            // that takes part in type equality.
            if let ConstTyData::Invalid(fault) = evaluated.data(db)
                && let Some(cause) = fault.invalid_cause(db)
            {
                return Err(cause);
            }
            let result = TyId::const_ty(db, evaluated);
            return Ok((result != ty).then_some((result, 0, policy)));
        }

        if let TyData::TyParam(p @ TyParam { owner, .. }) = ty.data(db)
            && p.is_trait_self()
            && let Some(impl_) = owner.resolve_to::<ImplTrait>(db)
        {
            let result = impl_.ty(db);
            return Ok((result != ty).then_some((result, 0, policy)));
        }

        let (head, args) = ty.decompose_ty_app(db);
        if let TyData::TypeFamily { owner, body, .. } = head.data(db)
            && args.len() == owner.generic_params(db).data(db).len()
        {
            let schema = super::ty_lower::ParamSchemaId::associated_type(db, *owner);
            let mapping = super::ty_lower::CompleteSubst::new(
                super::ty_lower::ParamDomainId::full(db, schema),
                db,
                args.to_vec(),
            )
            .expect("fully applied associated type binder");
            let result = super::subst::substitute_complete(db, *body, &mapping)
                .expect("associated type binder covers its declaration");
            return Ok(Some((result, FAMILY_DEPTH_COST, policy)));
        }

        if let TyData::TyBase(TyBase::TypeFn(def)) = head.data(db)
            && args.len() == def.generic_params(db).data(db).len()
            && self.resolve_impls
            && !super::type_fn::in_definition(db, self.premises.scope())
            && let Some(TyData::ConstTy(subject)) = args.last().map(|arg| arg.data(db))
            && let Some(value) = subject.integer_value(db)
        {
            if super::type_fn::definition_check_is_active(db, *def) {
                return Err(InvalidCause::ComputedTypeDefinitionInvalid);
            }
            let checked = super::type_fn::check_definition_body(db, *def);
            let Some(arms) = &checked.arms else {
                return Err(InvalidCause::ComputedTypeDefinitionInvalid);
            };
            let selected = def
                .arms(db)
                .iter()
                .position(|arm| match arm.pat.to_opt() {
                    Some(TypeFnPat::Lit(pattern)) => {
                        value.to_biguint().as_ref() == Some(pattern.data(db))
                    }
                    Some(TypeFnPat::Wild) => true,
                    None => false,
                })
                .expect("checked match is exhaustive");
            let result = Binder::bind((*def).into(), arms[selected]).instantiate(db, args);
            return Ok(Some((result, 1, ConstPolicy::Computed)));
        }

        if let TyData::AssocTy(assoc) = ty.data(db) {
            if depth.saturating_add(PROJECTION_DEPTH_COST) >= NORMALIZATION_DEPTH_LIMIT {
                return Err(InvalidCause::TypeNormalizationLimit);
            }
            let saved = (self.nested_depth, self.nested_policy);
            self.nested_depth = depth + PROJECTION_DEPTH_COST;
            self.nested_policy = policy;
            let replacement = self.try_resolve_assoc_ty(ty, assoc);
            (self.nested_depth, self.nested_policy) = saved;
            if let Some(replacement) = replacement {
                if replacement != ty && ty_contains(db, replacement, ty) {
                    return Err(InvalidCause::TypeLoweringCycle);
                }
                return Ok((replacement != ty).then_some((
                    replacement,
                    PROJECTION_DEPTH_COST,
                    policy.in_projection(),
                )));
            }
        }
        Ok(None)
    }
}

impl<'db> TypeNormalizer<'db> {
    fn try_resolve_assoc_ty(&mut self, ty: TyId<'db>, assoc: &AssocTy<'db>) -> Option<TyId<'db>> {
        // Equality evidence is separate from the projection's identity. Match
        // its entire trait reference before using an assumption's binding.
        let target = assoc.trait_.fold_with(self.db, self);
        let mut matching_bounds: IndexMap<TyId<'db>, ()> = IndexMap::new();
        for &pred in self.premises.assumptions().list(self.db) {
            let Some(bound) = pred.bound_assoc_ty(self.db, assoc.name) else {
                continue;
            };
            if self.trait_refs_match(target, pred.trait_ref(self.db)) {
                matching_bounds.insert(self.fold_ty(self.db, bound), ());
            }
        }
        if matching_bounds.len() > 1 {
            return None;
        }
        if let Some((&bound, _)) = matching_bounds.first() {
            return (bound != ty).then_some(bound);
        }

        if !self.resolve_impls {
            return None;
        }

        // 3) Fall back to the general associated type search used by path resolution,
        //    but restrict results to the same trait as `assoc` and deduplicate by
        //    the resulting type. If all viable candidates agree on a single type,
        //    normalize to that type.
        //
        // First attempt an impl-based lookup across relevant ingots (Self's + trait's),
        // mirroring trait-method resolution. This allows normalization to succeed even
        // when the calling scope is in a different ingot (e.g., core code instantiated
        // with std types).
        if let Some(resolved) = self.try_resolve_assoc_ty_from_impls(assoc) {
            return Some(resolved);
        }

        //    Search by the trait's self type: `SelfTy::assoc.name`.
        // Normalize the trait's self type before candidate search.
        let self_ty = self.fold_ty(self.db, assoc.trait_.self_ty(self.db));
        let mut raw_cands = match find_associated_type(
            self.db,
            self.premises.scope(),
            Canonicalized::new(self.db, self_ty),
            assoc.name,
            self.premises.assumptions(),
        ) {
            Ok(raw_cands) => raw_cands,
            Err(FindAssociatedTypeError::InfiniteBoundRecursion) => return None,
        };

        raw_cands.retain(|(inst, _)| self.trait_refs_match(target, inst.trait_ref(self.db)));

        // Deduplicate by normalized result type (to handle cases where multiple
        // impls yield the same associated type, e.g., Output = Self for all impls).
        let mut dedup: IndexMap<TyId<'db>, ()> = IndexMap::new();
        for (_, t) in raw_cands.into_iter() {
            // Continue folding so nested associated types are also normalized
            let norm_t = self.fold_ty(self.db, t);
            dedup.entry(norm_t).or_insert(());
        }

        match dedup.len() {
            0 => None,
            1 => {
                let (unique, _) = dedup.first().unwrap();
                // Only replace if we're actually making progress
                if *unique != ty { Some(*unique) } else { None }
            }
            _ => None,
        }
    }

    /// A pure normalization query may observe established equality, but may
    /// not choose a binding by assigning an unresolved caller inference var.
    fn trait_refs_match(&mut self, target: TraitRefId<'db>, candidate: TraitRefId<'db>) -> bool {
        if target.def(self.db) != candidate.def(self.db) {
            return false;
        }
        let candidate = candidate.fold_with(self.db, self);
        if target == candidate {
            return true;
        }
        if !collect_variables(self.db, &target).is_empty()
            || !collect_variables(self.db, &candidate).is_empty()
        {
            return false;
        }
        UnificationTable::new(self.db)
            .unify::<TraitRefId<'db>>(target, candidate)
            .is_ok()
    }

    fn try_resolve_assoc_ty_from_impls(&mut self, assoc: &AssocTy<'db>) -> Option<TyId<'db>> {
        let trait_inst = assoc.trait_.fold_with(self.db, self).as_predicate(self.db);
        let canonical_trait_inst = Canonical::new(self.db, trait_inst);

        let mut dedup: IndexMap<TyId<'db>, ()> = IndexMap::new();

        let solve_cx = self.premises.solver_context(self.db);
        let (primary, secondary) = solve_cx.search_ingots_for_trait_inst(self.db, trait_inst);
        let search_ingots = [Some(primary), secondary];

        // Canonicalize the target trait instance so we can unify against it in a
        // fresh table without mixing inference keys from other tables.
        let canonical_target = Canonicalized::new(self.db, trait_inst);
        canonical_target.with_materialized(self.db, |cx| {
            let target_inst = cx.query();
            let original_target = cx.try_extract::<TraitInstId<'db>>(target_inst);
            for ingot in search_ingots.into_iter().flatten() {
                for implementor in impls_for_trait_inst_with_possible_constraints(
                    self.db,
                    ingot,
                    canonical_trait_inst,
                    self.premises,
                ) {
                    let Some(implementor) =
                        complete_candidate_impl_assoc_ty(self.db, implementor, assoc.name)
                    else {
                        continue;
                    };
                    let candidate = cx.with_impl_assoc_ty(
                        implementor,
                        target_inst.self_ty(self.db),
                        assoc.name,
                        |cx, inst, assoc_ty| {
                            cx.unify::<TraitInstId<'db>>(inst, target_inst).ok()?;
                            if cx.try_extract::<TraitInstId<'db>>(target_inst) != original_target {
                                return None;
                            }
                            let assoc_ty = cx.resolve::<TyId<'db>>(assoc_ty);
                            cx.try_extract::<TyId<'db>>(assoc_ty)
                        },
                    );

                    // Extract into the caller's inference environment before
                    // continuing normalization, so scratch-local vars never
                    // leak into the cache.
                    if let Some(Some(folded)) = candidate {
                        let norm = self.fold_ty(self.db, folded);
                        dedup.entry(norm).or_insert(());
                    }
                }
            }
        });

        match dedup.len() {
            0 => None,
            1 => Some(*dedup.first().unwrap().0),
            _ => None,
        }
    }
}
