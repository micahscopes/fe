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
    /// The proof context the use was recorded with. Identity is unchanged:
    /// premises compare and hash as the scope and assumptions they replace.
    premises: crate::analysis::semantic::SolverPremises<'db>,
    inst: TraitInstId<'db>,
    name: IdentId<'db>,
    provision: Option<crate::semantic::Provision<'db>>,
    /// The other provisions in force where the use was written that proving
    /// its goal reaches through impl prerequisites. Only these are read, so
    /// only these are part of the use's identity.
    dependent: Option<crate::analysis::semantic::ImplEnv<'db>>,
}

impl<'db> AssocConstUse<'db> {
    pub fn new(
        premises: crate::analysis::semantic::SolverPremises<'db>,
        inst: TraitInstId<'db>,
        name: IdentId<'db>,
    ) -> Self {
        Self {
            premises,
            inst,
            name,
            provision: None,
            dependent: None,
        }
    }

    /// Keep the provisions of `context` that proving this use's goal can
    /// reach, other than the goal's own provision.
    pub(crate) fn with_dependent_provisions(
        self,
        db: &'db dyn HirAnalysisDb,
        context: crate::analysis::semantic::ImplEnv<'db>,
    ) -> Self {
        Self {
            dependent: context.reached_from(db, self.inst, self.premises),
            ..self
        }
    }

    /// The reached provisions of the use's position, as a context for the
    /// constant's body.
    pub(crate) fn dependent_context(self) -> Option<crate::analysis::semantic::ImplEnv<'db>> {
        self.dependent
    }

    /// Evidence still owned by this read, including reached prerequisites.
    pub(crate) fn opened_provisions(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl Iterator<Item = crate::semantic::Provision<'db>> {
        self.provision
            .into_iter()
            .chain(
                self.dependent
                    .into_iter()
                    .flat_map(move |context| context.openings(db).iter().copied()),
            )
            .filter(|provision| provision.evidence_operand().is_some())
    }

    /// Replace only the prerequisite operand whose proof was checked by CTFE.
    pub(crate) fn open_prerequisite(
        self,
        db: &'db dyn HirAnalysisDb,
        provision: crate::semantic::Provision<'db>,
        application: SelectedApplication<'db>,
        proof: &super::trait_def::SelectedImplEvidence<'db>,
    ) -> Self {
        let Some(context) = self
            .dependent
            .filter(|context| context.openings(db).contains(&provision))
        else {
            return self;
        };
        Self {
            dependent: Some(context.open_application(db, application, proof)),
            ..self
        }
    }

    pub fn premises(self) -> crate::analysis::semantic::SolverPremises<'db> {
        self.premises
    }

    pub fn origin_scope(self) -> ScopeId<'db> {
        self.premises.scope()
    }

    pub fn assumptions(self) -> PredicateListId<'db> {
        self.premises.assumptions()
    }

    pub fn inst(self) -> TraitInstId<'db> {
        self.inst
    }

    pub fn with_inst(self, inst: TraitInstId<'db>) -> Self {
        Self { inst, ..self }
    }

    pub fn with_premises(self, premises: crate::analysis::semantic::SolverPremises<'db>) -> Self {
        Self { premises, ..self }
    }

    pub fn name(self) -> IdentId<'db> {
        self.name
    }

    pub(crate) fn selection(self) -> Option<SelectedApplication<'db>> {
        match self.provision {
            Some(crate::semantic::Provision::Named(selection)) => Some(selection),
            _ => None,
        }
    }

    pub(crate) fn with_selection(self, selection: Option<SelectedApplication<'db>>) -> Self {
        self.with_provision(selection.map(crate::semantic::Provision::Named))
    }

    pub(crate) fn provision(self) -> Option<crate::semantic::Provision<'db>> {
        self.provision
    }

    pub(crate) fn with_provision(self, provision: Option<crate::semantic::Provision<'db>>) -> Self {
        Self {
            provision: provision
                .filter(|entry| !matches!(entry, crate::semantic::Provision::Bound(_))),
            ..self
        }
    }

    /// A symbolic bound belongs to its parameter, rather than to whichever
    /// declaration supplied a field or argument type. Explicit choices retain
    /// their captured context; a goal absent from the binder is not rewritten.
    pub(crate) fn canonical_symbolic_bound(self, db: &'db dyn HirAnalysisDb) -> Self {
        if self.provision.is_some() {
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
                let assumptions = crate::semantic::func_body_assumptions(db, func);
                // Source checking uses the parent's authored parameters, while
                // identity comparison sees their copies in the method binder.
                let assumptions = if func.is_associated_func(db)
                    && let Some(parent) = crate::hir_def::GenericParamOwner::Func(func).parent(db)
                {
                    let parent_len = super::ty_lower::collect_generic_params(db, parent)
                        .params(db)
                        .len();
                    let params =
                        super::ty_lower::collect_generic_params(db, func.into()).params(db);
                    super::binder::Binder::bind(parent, assumptions)
                        .instantiate(db, &params[..parent_len])
                } else {
                    assumptions
                };
                assumptions.extend_all_bounds(db)
            }
            _ => crate::semantic::context_at(db, param.owner, None).assumptions(db),
        };
        if !assumptions.list(db).contains(&self.inst) {
            return self;
        }
        self.with_premises(
            crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                param.owner,
                assumptions,
            ),
        )
    }

    pub fn solve_cx(self, db: &'db dyn HirAnalysisDb) -> TraitSolveCx<'db> {
        crate::core::semantic::captured_assoc_const_context(db, self)
    }
}

impl<'db> TyVisitable<'db> for AssocConstUse<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.premises.visit_with(visitor);
        self.inst.visit_with(visitor);
        if let Some(provision) = self.provision {
            provision.visit_with(visitor);
        }
        if let Some(dependent) = self.dependent {
            dependent.visit_with(visitor);
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
            premises: self.premises.fold_with(db, folder),
            inst: self.inst.fold_with(db, folder),
            name: self.name,
            provision: self
                .provision
                .map(|provision| provision.fold_with(db, folder)),
            dependent: self
                .dependent
                .map(|dependent| dependent.fold_with(db, folder)),
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
