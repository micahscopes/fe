use common::ingot::Ingot;

use crate::{
    analysis::{
        HirAnalysisDb,
        ty::{
            binder::Binder,
            constraint::{ConstraintHeadId, ConstraintHeadKind, ConstraintId, ConstraintKind},
            derive_provider::{
                DeriveProviderId, providers_for_derive_goal, visible_derive_providers_for_ingot,
            },
            fold::TyFoldable,
            trait_resolution::constraint::collect_func_effect_capability_constraints,
            unify::UnificationTable,
        },
    },
    hir_def::IdentId,
};

use super::{
    ElaborationCapabilityOrigin, ElaborationCapabilityWitness, ElaborationCtfeContextId,
    ElaborationRequestId, constraints_match,
};

pub(super) fn matching_selected_providers<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
    derive_goal: ConstraintId<'db>,
    selected: IdentId<'db>,
) -> Vec<DeriveProviderId<'db>> {
    providers_for_derive_goal(db, ingot, derive_goal)
        .into_iter()
        .filter(|provider| provider.identity(db).name(db) == selected)
        .collect()
}

pub(super) fn providers_named_in_ingot<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
    selected: IdentId<'db>,
) -> Vec<DeriveProviderId<'db>> {
    visible_derive_providers_for_ingot(db, ingot)
        .into_iter()
        .filter(|provider| provider.identity(db).name(db) == selected)
        .collect()
}

pub(super) fn elaborate_provider_context<'db>(
    db: &'db dyn HirAnalysisDb,
    request: ElaborationRequestId<'db>,
    provider: DeriveProviderId<'db>,
) -> Option<ElaborationCtfeContextId<'db>> {
    let capability_constraints = collect_func_effect_capability_constraints(db, provider.func(db));
    let mut table = UnificationTable::new(db);
    let mut instantiated = Binder::bind(provider_goal_and_capabilities(
        db,
        provider,
        &capability_constraints,
    ))
    .instantiate_with(db, |ty| table.new_var_from_param(ty));

    let instantiated_goal = instantiated.first().copied()?;
    table.unify(instantiated_goal, request.goal(db)).ok()?;
    let derive_evidence = instantiated.get(1).copied()?.fold_with(db, &mut table);
    let expected_derive_evidence = derive_evidence_for_goal(db, request.goal(db))?;
    if !constraints_match(db, derive_evidence, expected_derive_evidence) {
        return None;
    }

    let capabilities: Vec<ElaborationCapabilityWitness<'db>> = instantiated
        .drain(2..)
        .filter_map(|constraint| {
            let folded = constraint.fold_with(db, &mut table);
            let ConstraintKind::EffectCapability(capability) = folded.kind(db) else {
                return None;
            };
            Some(ElaborationCapabilityWitness {
                capability,
                origin: ElaborationCapabilityOrigin::ProviderUsesParam,
            })
        })
        .collect();

    Some(ElaborationCtfeContextId::new(
        db,
        request,
        provider,
        derive_evidence,
        capabilities,
    ))
}

pub(super) fn derive_evidence_for_goal<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: ConstraintId<'db>,
) -> Option<ConstraintId<'db>> {
    let ConstraintKind::Trait(inst) = goal.kind(db) else {
        return None;
    };
    let head = ConstraintHeadId::new(db, ConstraintHeadKind::ConcreteTrait(inst.def(db)));
    Some(ConstraintId::new(db, ConstraintKind::Derive(head)))
}

fn provider_goal_and_capabilities<'db>(
    db: &'db dyn HirAnalysisDb,
    provider: DeriveProviderId<'db>,
    capabilities: &[ConstraintId<'db>],
) -> Vec<ConstraintId<'db>> {
    let mut constraints = Vec::with_capacity(capabilities.len() + 2);
    constraints.push(provider.goal(db));
    constraints.push(provider.derive_goal(db));
    constraints.extend(capabilities.iter().copied());
    constraints
}
