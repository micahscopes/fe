//! Signature-derived suspension contracts. A possible cut and an unresolved
//! footprint are separate facts; neither is inferred from a callee's body.

use super::{
    SemanticInstance, SemanticInstanceKey, instance::row_expansion_for_key,
    instantiated_effect_env, validate_instantiated_effect_env_key,
};
use crate::{
    analysis::{
        HirAnalysisDb,
        ty::{
            corelib::{
                SuspensionFootprint, effect_key_suspension, native_suspension_primitive,
                suspension_capability_method,
            },
            effects::rows::{RowKey, RowPath},
            ty_check::BodyOwner,
            ty_def::TyId,
        },
    },
    semantic::{EffectRequirement, EffectRequirementKey},
};

/// Where a possible or unresolved suspension footprint comes from. Retaining
/// the declaration and row path allows admission to diagnose the actual
/// requirement rather than an unrelated argument at the call site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SuspensionContractOrigin<'db> {
    Method,
    NativeCut,
    Input {
        index: usize,
        ty: TyId<'db>,
    },
    Effect {
        requirement: EffectRequirement<'db>,
        path: Option<RowPath>,
    },
    AbstractRow {
        entry: u32,
        key: RowKey<'db>,
    },
    InvalidEnvironment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuspensionContractEntry<'db> {
    pub origin: SuspensionContractOrigin<'db>,
    pub footprint: SuspensionFootprint,
}

/// Only nonempty or unresolved footprints are retained. An empty contract is
/// proved absent; an unresolved entry must survive until concrete admission.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SuspensionContract<'db> {
    pub entries: Vec<SuspensionContractEntry<'db>>,
}

impl<'db> SuspensionContract<'db> {
    fn push(&mut self, origin: SuspensionContractOrigin<'db>, footprint: SuspensionFootprint) {
        if footprint != SuspensionFootprint::Absent {
            self.entries
                .push(SuspensionContractEntry { origin, footprint });
        }
    }
}

/// Read the selected instance's signature, including capability-bearing data
/// inputs and effect rows. This does not normalize or inspect its body.
pub fn suspension_contract<'db>(
    db: &'db dyn HirAnalysisDb,
    key: SemanticInstanceKey<'db>,
) -> SuspensionContract<'db> {
    let mut contract = SuspensionContract::default();
    if validate_instantiated_effect_env_key(db, key).is_err() {
        contract.push(
            SuspensionContractOrigin::InvalidEnvironment,
            SuspensionFootprint::Unresolved,
        );
        return contract;
    }
    let owner = key.owner(db);
    if let BodyOwner::Func(func, _) = owner
        && suspension_capability_method(db, func)
    {
        contract.push(
            SuspensionContractOrigin::Method,
            SuspensionFootprint::MaySuspend,
        );
    }
    if let BodyOwner::Func(func, _) = owner
        && native_suspension_primitive(db, func)
    {
        contract.push(
            SuspensionContractOrigin::NativeCut,
            SuspensionFootprint::MaySuspend,
        );
    }
    let instance = SemanticInstance::new(db, key);
    let assumptions = instance.assumptions(db);
    let typed = key.typed_body(db);
    for (index, input) in typed.input_bindings().iter().enumerate() {
        let ty = typed.binding_ty(db, *input);
        let ty = ty.as_capability(db).map_or(ty, |(_, inner)| inner);
        contract.push(
            SuspensionContractOrigin::Input { index, ty },
            effect_key_suspension(
                db,
                owner.scope(),
                assumptions,
                EffectRequirementKey::Type(ty),
            ),
        );
    }
    let rows = row_expansion_for_key(db, key);
    for &(entry, row) in &rows.abstract_rows {
        contract.push(
            SuspensionContractOrigin::AbstractRow { entry, key: row },
            SuspensionFootprint::Unresolved,
        );
    }
    if let Some(env) = instantiated_effect_env(db, instance) {
        for requirement in env.requirements(db) {
            let component = rows
                .components
                .iter()
                .find(|component| component.requirement.binding_idx == requirement.binding_idx);
            // A declared row is represented by its expanded components and
            // abstract_rows above. A nested unresolved row is itself a
            // component and must still be classified as unresolved.
            if requirement.key.key_row().is_some() && component.is_none() {
                continue;
            }
            contract.push(
                SuspensionContractOrigin::Effect {
                    requirement: requirement.clone(),
                    path: component.map(|component| component.path.clone()),
                },
                effect_key_suspension(db, owner.scope(), assumptions, requirement.key.clone()),
            );
        }
    }
    contract
}
