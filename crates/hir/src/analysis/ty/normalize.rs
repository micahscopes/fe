//! Type normalization module
//!
//! This module provides functionality to normalize types by resolving associated types
//! to concrete types when possible. This happens before type unification to ensure
//! that types are in their most resolved form.

use std::collections::hash_map::Entry;

use crate::core::hir_def::{ImplTrait, scope_graph::ScopeId};
use common::indexmap::IndexMap;
use rustc_hash::FxHashMap;

use super::{
    binder::Binder,
    canonical::Canonical,
    canonical::Canonicalized,
    fold::{TyFoldable, TyFolder},
    trait_def::{TraitInstId, impls_for_ty_with_constraints},
    trait_resolution::{PredicateListId, ProvisionEnv},
    ty_def::{AssocTy, TyData, TyId, TyParam},
    visitor::{TyVisitable, TyVisitor, walk_ty},
};
use crate::analysis::{
    HirAnalysisDb,
    name_resolution::{FindAssociatedTypeError, find_associated_type},
};

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
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
) -> TyId<'db> {
    let mut normalizer = TypeNormalizer::new(db, scope, assumptions);
    ty.fold_with(db, &mut normalizer)
}

/// Normalizer-local step budget for GAT projection (guard G3, steering-05 sec
/// 3.3). Projection is NOT structurally decreasing (`type L<T> = Foo::L<Box<T>>`
/// mints a strictly growing chain of pairwise-distinct interned redexes that the
/// in-progress cycle marker never revisits), so termination is by counter, not by
/// structure. On exhaustion projection degrades to the OPAQUE canonical form (the
/// same "leave unresolved" degrade the cycle guard uses). Constant and tunable;
/// deliberately NOT part of any salsa memo key (`TypeNormalizer` is a plain
/// folder, so this counter cannot poison a memo). Shared across the bare and
/// applied projection routes, covering the bare-route growth cousin for free.
const PROJECTION_STEP_BUDGET: usize = 256;

pub struct TypeNormalizer<'db> {
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
    // Projection cache: None = in progress (cycle guard), Some(ty) = normalized
    // result. Keyed on the FULL canonical redex `TyId` (guard G2): the applied
    // form `B::Buffer<u32>` and `B::Buffer<u8>` must not conflate, and a bare
    // `AssocTy` head keys on its own node. Interning preserves the sharing the
    // old `AssocTy`-struct key gave.
    cache: FxHashMap<TyId<'db>, Option<TyId<'db>>>,
    // Shared projection step counter for G3 (see `PROJECTION_STEP_BUDGET`).
    projection_steps: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct AssumptionUnifyInput<T> {
    pub(crate) lhs_self: T,
    pub(crate) rhs_self: T,
    pub(crate) bound: T,
}

impl<'db, T> TyFoldable<'db> for AssumptionUnifyInput<T>
where
    T: TyFoldable<'db> + Copy,
{
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            lhs_self: self.lhs_self.fold_with(db, folder),
            rhs_self: self.rhs_self.fold_with(db, folder),
            bound: self.bound.fold_with(db, folder),
        }
    }
}

impl<'db, T> TyVisitable<'db> for AssumptionUnifyInput<T>
where
    T: TyVisitable<'db> + Copy,
{
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.lhs_self.visit_with(visitor);
        self.rhs_self.visit_with(visitor);
        self.bound.visit_with(visitor);
    }
}

impl<'db> TypeNormalizer<'db> {
    pub fn new(
        db: &'db dyn HirAnalysisDb,
        scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
    ) -> Self {
        Self {
            db,
            scope,
            assumptions,
            cache: FxHashMap::default(),
            projection_steps: 0,
        }
    }
}

impl<'db> TyFolder<'db> for TypeNormalizer<'db> {
    fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
        match ty.data(self.db) {
            TyData::TyParam(p @ TyParam { owner, .. }) if p.is_trait_self() => {
                if let Some(impl_) = owner.resolve_to::<ImplTrait>(self.db) {
                    // Use the item method to obtain the implementor's self type.
                    let lowered = impl_.ty(self.db);
                    return self.fold_ty(db, lowered);
                }
                ty
            }
            TyData::AssocTy(assoc_ty) => {
                // Guard G1 (bare arm): a bare GAT head `B::Buffer` (decl carries
                // generic params) is UNSATURATED by definition. Resolving it in
                // isolation is meaningless: `ImplementorId::assoc_ty` returns the
                // impl RHS with the assoc def's own params dangling, so any
                // resolution here would be wrong. Leave it OPAQUE (still fold
                // internals so the trait self type normalizes). The applied form
                // is projected by the dedicated arm in `_` below; a bare GAT head
                // used AT `* -> *` kind is un-projectable by construction (its
                // diagnostic belongs to A4/A5, not here; A3 must only not mangle
                // it).
                if self.assoc_decl_arity(assoc_ty) > 0 {
                    let folded = ty.super_fold_with(db, self);
                    return folded;
                }

                // G2: key the projection cache on the node itself.
                match self.cache.entry(ty) {
                    Entry::Occupied(entry) => match entry.get() {
                        Some(cached) => return *cached,
                        None => return ty, // cycle: leave unresolved
                    },
                    Entry::Vacant(entry) => {
                        entry.insert(None);
                    }
                }

                // NOTE (deviation from steering-05 sec 3.3, measured): the G3 step
                // budget is charged on the APPLIED route only, NOT here. The bare
                // route retains its pre-A3 discipline (the in-progress `None`
                // cycle guard above). Sharing the counter across the bare route
                // regressed breadth-heavy normalizations (a single effect/contract
                // `normalize_ty` legitimately resolves well over 256 DISTINCT bare
                // associated types, which is breadth, not the divergence depth G3
                // targets). The bare-route growth cousin is UNVERIFIED/unreachable
                // today (steering's own note); if it ever surfaces it needs a
                // depth-scoped guard, not a breadth counter.
                if let Some(replacement) = self.try_resolve_assoc_ty(ty, assoc_ty) {
                    let normalized = self.fold_ty(db, replacement);
                    self.cache.insert(ty, Some(normalized));
                    return normalized;
                }

                // Not resolved; still fold internals (e.g., normalize self type)
                let folded = ty.super_fold_with(db, self);
                self.cache.insert(ty, Some(folded));
                folded
            }
            _ => {
                // Guard G1 (applied arm): the applied GAT projection form
                // `TyApp*(AssocTy(inst, name), a1..ak)`. `decompose_ty_app` peels
                // the full spine; a bare-`AssocTy` base with a nonempty spine is
                // exactly `B::Buffer<..>`. The generic `super_fold_with` path is
                // WRONG for it (it folds the bare `AssocTy` head in isolation,
                // hitting the wrong resolution and then re-applying the spine args
                // on top, an `ArgNumMismatch`/`KindMismatch`), so it gets its own
                // arm. This keeps the normalizer the single source of truth for
                // GAT projection (steering-05 sec 4).
                let (base, spine) = ty.decompose_ty_app(self.db);
                if !spine.is_empty()
                    && let TyData::AssocTy(assoc) = base.data(self.db)
                    // Only a GAT (the decl carries generic params) is a projection
                    // redex in applied form. A NON-generic associated type whose
                    // VALUE is a `* -> *` constructor (`type Foo = SomeCtor`, used
                    // as `T::Foo<X>`) is NOT a GAT: it must keep the pre-A3 path
                    // (resolve the bare head, then apply the spine), so it is left
                    // to `super_fold_with` below.
                    && self.assoc_decl_arity(assoc) > 0
                {
                    return self.project_applied(*assoc, spine);
                }

                // Second line of the S1.5 containment (the path-lowering site is
                // the first): a substitution-formed ground type-fn application
                // reduces to its concrete normal form here, so `TyBase::TypeFn`
                // does not survive body checking. Symbolic subjects stay opaque.
                let folded = ty.super_fold_with(db, self);
                if super::type_fn::type_fn_app_subject_is_ground(self.db, folded) {
                    super::type_fn::normalize_type_fn_app(self.db, folded)
                } else {
                    folded
                }
            }
        }
    }
}

impl<'db> TypeNormalizer<'db> {
    /// The number of generic parameters the associated type's DECL carries
    /// (the authority for saturation, per the A2 kind arm). Zero for a plain
    /// associated type.
    fn assoc_decl_arity(&self, assoc: &AssocTy<'db>) -> usize {
        assoc
            .trait_
            .def(self.db)
            .assoc_ty(self.db, assoc.name)
            .map(|decl| decl.generic_params.len(self.db))
            .unwrap_or(0)
    }

    /// Guard G1 (applied arm): project a saturated applied GAT
    /// `TyApp*(AssocTy(inst, name), a1..ak)` through its impl.
    ///
    /// Canonical order (steering-05 sec 3.1): (1) fold the spine arguments with
    /// the SAME normalizer (a ground type-fn argument reaches combinator normal
    /// form here, UNFOLD-before-PROJECT); (2) rebuild the canonical redex `proj`
    /// and consult/enter the cycle-guard cache under its full `TyId` (G2); (3)
    /// resolve the impl and substitute the assoc def's own params with the
    /// normalized args by index; (4) re-fold the substituted RHS (catching
    /// substitution-formed ground type-fn apps); (5) on failure / arity mismatch
    /// / budget / cycle, return the OPAQUE canonical form `proj`.
    fn project_applied(&mut self, assoc: AssocTy<'db>, spine: &[TyId<'db>]) -> TyId<'db> {
        // Step 1: fold the spine arguments and the trait instance.
        let args: Vec<TyId<'db>> = spine.iter().map(|a| self.fold_ty(self.db, *a)).collect();
        let folded_trait = assoc.trait_.fold_with(self.db, self);
        let folded_assoc = AssocTy {
            trait_: folded_trait,
            name: assoc.name,
        };

        // Step 2: rebuild the canonical redex and enter the cache (G2).
        let head = TyId::assoc_ty(self.db, folded_trait, assoc.name);
        let proj = TyId::foldl(self.db, head, &args);

        match self.cache.entry(proj) {
            Entry::Occupied(entry) => match entry.get() {
                Some(cached) => return *cached,
                None => return proj, // cycle: opaque canonical form
            },
            Entry::Vacant(entry) => {
                entry.insert(None);
            }
        }

        // Arity guard: only a spine that exactly saturates the decl is a
        // projectable redex. A non-generic assoc type reached in applied form, or
        // a spine longer/shorter than `k`, is left opaque (kind-impossible
        // post-A2 for `*`-returning GATs; conformance is A4's job).
        if self.assoc_decl_arity(&folded_assoc) != args.len() {
            self.cache.insert(proj, Some(proj));
            return proj;
        }

        // Step G3: charge the shared projection budget; degrade to opaque on
        // exhaustion (projection is not structurally decreasing).
        self.projection_steps += 1;
        if self.projection_steps > PROJECTION_STEP_BUDGET {
            self.cache.insert(proj, Some(proj));
            return proj;
        }

        // Steps 3-4: resolve the impl RHS, substitute the args by param index,
        // re-fold the substituted RHS.
        if let Some(rhs) = self.try_resolve_assoc_ty(head, &folded_assoc) {
            let substituted = self.subst_gat_args(rhs, &args);
            let normalized = self.fold_ty(self.db, substituted);
            self.cache.insert(proj, Some(normalized));
            return normalized;
        }

        // Step 5: unresolved (generic `B`, ambiguity) -> opaque canonical form.
        self.cache.insert(proj, Some(proj));
        proj
    }

    /// Substitute the resolved impl RHS's free parameters (the assoc definition's
    /// own generic params, positional) with the projection arguments by index.
    ///
    /// For an argument-independent RHS (`type Ptr<T> = u256`) there are no free
    /// params and this is the identity. When the RHS threads its params
    /// (`type Buffer<T> = Store<T>`), each is a positional `TyParam` and
    /// `Binder::instantiate` maps it by index. The substitution is applied only
    /// when every free param index is in range (`< args.len()`), so it can never
    /// index out of bounds; an out-of-range RHS (a param shape A3 does not cover)
    /// is returned unsubstituted and the re-fold leaves it opaque.
    fn subst_gat_args(&self, rhs: TyId<'db>, args: &[TyId<'db>]) -> TyId<'db> {
        match max_free_param_idx(self.db, rhs) {
            None => rhs,
            Some(max) if max < args.len() => Binder::bind(rhs).instantiate(self.db, args),
            Some(_) => rhs,
        }
    }

    fn try_resolve_assoc_ty(&mut self, ty: TyId<'db>, assoc: &AssocTy<'db>) -> Option<TyId<'db>> {
        // 1) Check if the trait instance itself carries an explicit binding
        if let Some(&bound_ty) = assoc.trait_.assoc_type_bindings(self.db).get(&assoc.name) {
            return Some(bound_ty);
        }

        // 2) Check assumptions for an equivalent trait instance that carries
        //    an explicit associated type binding (e.g., from where-clauses).
        for &pred in self.assumptions.list(self.db) {
            if pred.def(self.db) != assoc.trait_.def(self.db) {
                continue;
            }

            let lhs_self = self.fold_ty(self.db, assoc.trait_.self_ty(self.db));
            let rhs_self = self.fold_ty(self.db, pred.self_ty(self.db));
            let Some(&bound) = pred.assoc_type_bindings(self.db).get(&assoc.name) else {
                continue;
            };

            // Unify in a canonicalized local table, then map the resolved
            // associated type back to the original inference environment.
            let canonical_input = Canonicalized::new(
                self.db,
                AssumptionUnifyInput {
                    lhs_self,
                    rhs_self,
                    bound,
                },
            );

            if let Some(resolved) = canonical_input.with_materialized(self.db, |cx| {
                let input = cx.query();
                if cx
                    .unify::<TyId<'db>>(input.lhs_self, input.rhs_self)
                    .is_ok()
                {
                    let resolved = cx.resolve::<TyId<'db>>(input.bound);
                    return cx.try_extract::<TyId<'db>>(resolved);
                }
                None
            }) {
                return Some(resolved);
            }
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
            self.scope,
            Canonicalized::new(self.db, self_ty),
            assoc.name,
            self.assumptions,
        ) {
            Ok(raw_cands) => raw_cands,
            Err(FindAssociatedTypeError::InfiniteBoundRecursion) => return None,
        };

        // Keep only candidates from the same trait as `assoc`.
        raw_cands.retain(|(inst, _)| inst.def(self.db) == assoc.trait_.def(self.db));

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

    fn try_resolve_assoc_ty_from_impls(&mut self, assoc: &AssocTy<'db>) -> Option<TyId<'db>> {
        let trait_inst = assoc.trait_.fold_with(self.db, self);
        let trait_def = trait_inst.def(self.db);
        let canonical_self_ty = Canonical::new(self.db, trait_inst.self_ty(self.db));

        let mut dedup: IndexMap<TyId<'db>, ()> = IndexMap::new();

        let solve_cx = ProvisionEnv::for_scope(self.scope, self.assumptions).solve_cx(self.db);
        let (primary, secondary) = solve_cx.search_ingots_for_trait_inst(self.db, trait_inst);
        let search_ingots = [Some(primary), secondary];

        // Canonicalize the target trait instance so we can unify against it in a
        // fresh table without mixing inference keys from other tables.
        let canonical_target = Canonicalized::new(self.db, trait_inst);
        canonical_target.with_materialized(self.db, |cx| {
            let target_inst = cx.query();
            for ingot in search_ingots.into_iter().flatten() {
                for implementor in impls_for_ty_with_constraints(
                    self.db,
                    ingot,
                    canonical_self_ty,
                    self.assumptions,
                ) {
                    if implementor.skip_binder().trait_def(self.db) != trait_def {
                        continue;
                    }

                    let candidate = cx.with_impl_assoc_ty(
                        implementor,
                        target_inst.self_ty(self.db),
                        assoc.name,
                        |cx, inst, assoc_ty| {
                            cx.unify::<TraitInstId<'db>>(inst, target_inst).ok()?;
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

/// The maximum index among a type's free (non-effect) type/const parameters, or
/// `None` if it has none. Used by GAT projection to decide whether the resolved
/// impl RHS can be safely instantiated with the projection arguments by index. A
/// trait-`Self` parameter forces `usize::MAX` so the caller's `max < args.len()`
/// range check declines substitution (`Self` is never a GAT argument).
fn max_free_param_idx<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> Option<usize> {
    struct ParamScan<'db> {
        db: &'db dyn HirAnalysisDb,
        max: Option<usize>,
    }
    impl<'db> ParamScan<'db> {
        fn record(&mut self, param: &TyParam<'db>) {
            let idx = if param.is_trait_self() {
                usize::MAX
            } else {
                param.idx
            };
            self.max = Some(self.max.map_or(idx, |m| m.max(idx)));
        }
    }
    impl<'db> TyVisitor<'db> for ParamScan<'db> {
        fn db(&self) -> &'db dyn HirAnalysisDb {
            self.db
        }
        fn visit_ty(&mut self, ty: TyId<'db>) {
            match ty.data(self.db) {
                TyData::TyParam(param) if !param.is_effect() => self.record(param),
                TyData::ConstTy(const_ty) => {
                    if let super::const_ty::ConstTyData::TyParam(param, _) = const_ty.data(self.db) {
                        if !param.is_effect() {
                            self.record(param);
                        }
                    } else {
                        walk_ty(self, ty);
                    }
                }
                _ => walk_ty(self, ty),
            }
        }
    }
    let mut scan = ParamScan { db, max: None };
    ty.visit_with(&mut scan);
    scan.max
}
