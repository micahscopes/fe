use salsa::Update;

use super::ty_def::TyId;
use super::{
    fold::{TyFoldable, TyFolder},
    trait_def::TraitInstId,
    trait_resolution::{PredicateListId, TraitSolveCx},
    visitor::{TyVisitable, TyVisitor},
};
use crate::{
    analysis::{HirAnalysisDb, semantic::SelectedApplication},
    hir_def::{IdentId, Impl, scope_graph::ScopeId},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Update)]
pub struct AssocConstUse<'db> {
    origin_scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
    inst: TraitInstId<'db>,
    name: IdentId<'db>,
    selection: Option<SelectedApplication<'db>>,
}

impl<'db> AssocConstUse<'db> {
    pub fn new(
        origin_scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
        inst: TraitInstId<'db>,
        name: IdentId<'db>,
    ) -> Self {
        Self {
            origin_scope,
            assumptions,
            inst,
            name,
            selection: None,
        }
    }

    pub fn origin_scope(self) -> ScopeId<'db> {
        self.origin_scope
    }

    pub fn assumptions(self) -> PredicateListId<'db> {
        self.assumptions
    }

    pub fn inst(self) -> TraitInstId<'db> {
        self.inst
    }

    pub fn with_inst(self, inst: TraitInstId<'db>) -> Self {
        Self { inst, ..self }
    }

    pub fn with_env(self, origin_scope: ScopeId<'db>, assumptions: PredicateListId<'db>) -> Self {
        Self {
            origin_scope,
            assumptions,
            ..self
        }
    }

    pub fn name(self) -> IdentId<'db> {
        self.name
    }

    pub(crate) fn selection(self) -> Option<SelectedApplication<'db>> {
        self.selection
    }

    pub(crate) fn with_selection(self, selection: Option<SelectedApplication<'db>>) -> Self {
        Self { selection, ..self }
    }

    /// A symbolic bound belongs to its parameter, rather than to whichever
    /// declaration supplied a field or argument type. Explicit choices retain
    /// their captured context; a goal absent from the binder is not rewritten.
    pub(crate) fn canonical_symbolic_bound(self, db: &'db dyn HirAnalysisDb) -> Self {
        if self.selection.is_some() {
            return self;
        }
        let super::ty_def::TyData::TyParam(param) = self.inst.self_ty(db).data(db) else {
            return self;
        };
        // This identity is needed while effect keys and hidden layout slots
        // are still being lowered. Completed function premises include those
        // effects, so asking for them here would re-enter their own query.
        let assumptions = match param.owner.item() {
            crate::hir_def::ItemKind::Func(func) => {
                crate::semantic::func_body_assumptions(db, func).extend_all_bounds(db)
            }
            _ => crate::semantic::context_at(db, param.owner, None).assumptions(db),
        };
        if !assumptions.list(db).contains(&self.inst) {
            return self;
        }
        self.with_env(param.owner, assumptions)
    }

    pub fn solve_cx(self, db: &'db dyn HirAnalysisDb) -> TraitSolveCx<'db> {
        TraitSolveCx::new(db, self.origin_scope).with_assumptions(self.assumptions)
    }
}

impl<'db> TyVisitable<'db> for AssocConstUse<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.assumptions.visit_with(visitor);
        self.inst.visit_with(visitor);
        if let Some(selection) = self.selection {
            selection.visit_with(visitor);
        }
    }
}

impl<'db> TyFoldable<'db> for AssocConstUse<'db> {
    fn fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        folder.fold_assoc_const_use(db, self)
    }

    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            origin_scope: self.origin_scope,
            assumptions: self.assumptions.fold_with(db, folder),
            inst: self.inst.fold_with(db, folder),
            name: self.name,
            selection: self
                .selection
                .map(|selection| selection.fold_with(db, folder)),
        }
    }
}

/// A use of an associated const defined in an inherent `impl` block,
/// e.g. `Foo::<7>::SIZE` or `Self::SIZE` inside the impl.
///
/// Carries the receiver type so the impl's generic parameters can be
/// recovered by unifying the impl self type against it once the receiver
/// is concrete (after typed-body instantiation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Update)]
pub struct InherentConstUse<'db> {
    origin_scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
    impl_: Impl<'db>,
    receiver_ty: TyId<'db>,
    name: IdentId<'db>,
}

impl<'db> InherentConstUse<'db> {
    pub fn new(
        origin_scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
        impl_: Impl<'db>,
        receiver_ty: TyId<'db>,
        name: IdentId<'db>,
    ) -> Self {
        Self {
            origin_scope,
            assumptions,
            impl_,
            receiver_ty,
            name,
        }
    }

    pub fn origin_scope(self) -> ScopeId<'db> {
        self.origin_scope
    }

    pub fn with_env(self, origin_scope: ScopeId<'db>, assumptions: PredicateListId<'db>) -> Self {
        Self {
            origin_scope,
            assumptions,
            ..self
        }
    }

    pub fn assumptions(self) -> PredicateListId<'db> {
        self.assumptions
    }

    pub fn impl_(self) -> Impl<'db> {
        self.impl_
    }

    pub fn receiver_ty(self) -> TyId<'db> {
        self.receiver_ty
    }

    pub fn name(self) -> IdentId<'db> {
        self.name
    }
}

impl<'db> TyVisitable<'db> for InherentConstUse<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.assumptions.visit_with(visitor);
        self.receiver_ty.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for InherentConstUse<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            origin_scope: self.origin_scope,
            assumptions: self.assumptions.fold_with(db, folder),
            impl_: self.impl_,
            receiver_ty: self.receiver_ty.fold_with(db, folder),
            name: self.name,
        }
    }
}
