use crate::analysis::{
    HirAnalysisDb,
    ty::{
        constraint::{
            CapabilityMode, CompilerCapabilityKind, ConstraintId, EffectCapabilityId,
            EffectCapabilityKey,
        },
        ty_def::TyId,
    },
};

use super::{ElaborationCtfeContextId, constraints_match, tys_match};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ElaborationCapabilityOrigin {
    ProviderUsesParam,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct ElaborationCapabilityWitness<'db> {
    pub(super) capability: EffectCapabilityId<'db>,
    pub(super) origin: ElaborationCapabilityOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct CapabilityEnv<'db> {
    witnesses: Vec<ElaborationCapabilityWitness<'db>>,
}

impl<'db> CapabilityEnv<'db> {
    pub(super) fn from_context(
        db: &'db dyn HirAnalysisDb,
        context: ElaborationCtfeContextId<'db>,
    ) -> Self {
        Self {
            witnesses: context.capabilities(db).clone(),
        }
    }

    pub(super) fn has_impl_builder(
        &self,
        db: &'db dyn HirAnalysisDb,
        goal: ConstraintId<'db>,
    ) -> bool {
        self.witnesses.iter().any(|witness| {
            if witness.capability.mode(db) != CapabilityMode::Mut {
                return false;
            }
            match witness.capability.key(db) {
                EffectCapabilityKey::Compiler(CompilerCapabilityKind::ImplBuilder(
                    capability_goal,
                )) => constraints_match(db, capability_goal, goal),
                _ => false,
            }
        })
    }

    pub(super) fn has_reflect_target(&self, db: &'db dyn HirAnalysisDb, target: TyId<'db>) -> bool {
        self.witnesses.iter().any(|witness| {
            if witness.capability.mode(db) != CapabilityMode::Read {
                return false;
            }
            match witness.capability.key(db) {
                EffectCapabilityKey::Compiler(CompilerCapabilityKind::Reflect(reflected)) => {
                    tys_match(db, reflected, target)
                }
                _ => false,
            }
        })
    }
}
