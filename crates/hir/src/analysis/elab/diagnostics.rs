use common::indexmap::IndexMap;

use crate::{
    analysis::{
        HirAnalysisDb,
        ty::{
            diagnostics::TyDiagCollection, evidence_provider::visible_evidence_providers_for_ingot,
        },
    },
    hir_def::TopLevelMod,
};

use super::{
    ProviderOutputId, ProviderOutputStatus, elaboration_requests_for_top_mod, invalid_request,
    provider_context::{
        derive_evidence_for_goal, matching_selected_providers, providers_named_in_ingot,
    },
};

pub(super) fn implicit_provider_ambiguity_diags_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<TyDiagCollection<'db>> {
    let providers = visible_evidence_providers_for_ingot(db, top_mod.ingot(db));
    let mut by_derive_goal: IndexMap<_, Vec<_>> = IndexMap::new();
    for provider in providers {
        by_derive_goal
            .entry(provider.derive_goal(db))
            .or_default()
            .push(provider);
    }

    elaboration_requests_for_top_mod(db, top_mod)
        .iter()
        .filter(|request| request.selected_provider(db).is_none())
        .filter_map(|request| {
            let derive_goal = derive_evidence_for_goal(db, request.goal(db))?;
            let providers = by_derive_goal.get(&derive_goal)?;
            if providers.len() <= 1 {
                return None;
            }
            let names = providers
                .iter()
                .map(|provider| provider.identity(db).pretty_print(db))
                .collect::<Vec<_>>()
                .join(", ");
            Some(invalid_request(
                request.span(db),
                format!(
                    "implicit derive request for `{}` is ambiguous between providers {names}; select a provider with `using`",
                    derive_goal.pretty_print(db)
                ),
            ))
        })
        .collect()
}

pub(super) fn selected_evidence_provider_diags_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<TyDiagCollection<'db>> {
    elaboration_requests_for_top_mod(db, top_mod)
        .iter()
        .filter_map(|request| {
            let selected = request.selected_provider(db)?;
            let derive_goal = derive_evidence_for_goal(db, request.goal(db))?;
            let matches =
                matching_selected_providers(db, top_mod.ingot(db), derive_goal, selected);
            if matches.len() == 1 {
                return None;
            }
            let selected_name = selected.data(db);
            let message = if matches.is_empty() {
                let named_providers = providers_named_in_ingot(db, top_mod.ingot(db), selected);
                if !named_providers.is_empty() {
                    let provided_goals = named_providers
                        .iter()
                        .map(|provider| format!("`{}`", provider.derive_goal(db).pretty_print(db)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "selected evidence provider `{selected_name}` does not provide `{}` evidence; visible provider(s) named `{selected_name}` provide {provided_goals}",
                        derive_goal.pretty_print(db),
                    )
                } else {
                    format!(
                        "selected evidence provider `{selected_name}` for `{}` was not found",
                        derive_goal.pretty_print(db)
                    )
                }
            } else {
                let count = matches.len();
                format!(
                    "selected evidence provider `{selected_name}` for `{}` is ambiguous; it matches {count} visible providers",
                    derive_goal.pretty_print(db)
                )
            };
            Some(invalid_request(request.span(db), message))
        })
        .collect()
}

pub(super) fn provider_output_diag<'db>(
    db: &'db dyn HirAnalysisDb,
    output: ProviderOutputId<'db>,
) -> Option<TyDiagCollection<'db>> {
    let message = match output.status(db) {
        ProviderOutputStatus::Succeeded { .. } => return None,
        ProviderOutputStatus::Failed => "provider execution failed".to_string(),
        ProviderOutputStatus::Skipped { reason, .. } => reason.diagnostic_message().to_string(),
    };

    let request = output.request(db);
    let provider = output.provider(db).identity(db).pretty_print(db);
    let span = match output.status(db) {
        ProviderOutputStatus::Skipped { span, .. } => span,
        ProviderOutputStatus::Failed | ProviderOutputStatus::Succeeded { .. } => {
            output.provider(db).func(db).span().name().into()
        }
    };
    Some(invalid_request(
        span,
        format!(
            "evidence provider `{provider}` for `{}` did not produce generated evidence: {message}",
            request.goal(db).pretty_print(db)
        ),
    ))
}
