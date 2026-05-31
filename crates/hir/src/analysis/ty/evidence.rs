#![allow(dead_code)]

use crate::analysis::{
    HirAnalysisDb,
    ty::{
        constraint::{ConstPredicateInstId, ConstraintId, EffectCapabilityId},
        fold::{TyFoldable, TyFolder},
        generated::GeneratedImplId,
        trait_def::ImplementorId,
        visitor::{TyVisitable, TyVisitor},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct ConstProofId<'db> {
    pub(crate) predicate: ConstPredicateInstId<'db>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum BuiltinEvidenceKind {
    TypeEquality,
    WellFormedness,
    EffectCapability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct ErasedProofTermId<'db> {
    pub(crate) constraint: ConstraintId<'db>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum EvidenceKind<'db> {
    Impl(ImplementorId<'db>),
    Assumption(ConstraintId<'db>),
    ConstProof(ConstProofId<'db>),
    GeneratedImpl(GeneratedImplId<'db>),
    EffectWitness(EffectCapabilityId<'db>),
    Builtin(BuiltinEvidenceKind),
    ErasedProofTerm(ErasedProofTermId<'db>),
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct EvidenceId<'db> {
    pub(crate) kind: EvidenceKind<'db>,
}

impl<'db> TyVisitable<'db> for ConstProofId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.predicate.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for ConstProofId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            predicate: self.predicate.fold_with(db, folder),
        }
    }
}

impl<'db> TyVisitable<'db> for ErasedProofTermId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.constraint.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for ErasedProofTermId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            constraint: self.constraint.fold_with(db, folder),
        }
    }
}

impl<'db> TyVisitable<'db> for EvidenceKind<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        match self {
            Self::Impl(implementor) => implementor.visit_with(visitor),
            Self::Assumption(constraint) => constraint.visit_with(visitor),
            Self::ConstProof(proof) => proof.visit_with(visitor),
            Self::GeneratedImpl(generated) => generated.visit_with(visitor),
            Self::EffectWitness(capability) => capability.visit_with(visitor),
            Self::Builtin(_) => {}
            Self::ErasedProofTerm(proof) => proof.visit_with(visitor),
        }
    }
}

impl<'db> TyFoldable<'db> for EvidenceKind<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        match self {
            Self::Impl(implementor) => Self::Impl(implementor.fold_with(db, folder)),
            Self::Assumption(constraint) => Self::Assumption(constraint.fold_with(db, folder)),
            Self::ConstProof(proof) => Self::ConstProof(proof.fold_with(db, folder)),
            Self::GeneratedImpl(generated) => Self::GeneratedImpl(generated.fold_with(db, folder)),
            Self::EffectWitness(capability) => {
                Self::EffectWitness(capability.fold_with(db, folder))
            }
            Self::Builtin(kind) => Self::Builtin(kind),
            Self::ErasedProofTerm(proof) => Self::ErasedProofTerm(proof.fold_with(db, folder)),
        }
    }
}

impl<'db> TyVisitable<'db> for EvidenceId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.kind(visitor.db()).visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for EvidenceId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self::new(db, self.kind(db).fold_with(db, folder))
    }
}
