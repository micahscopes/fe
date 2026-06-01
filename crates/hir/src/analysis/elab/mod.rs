use crate::{
    analysis::{
        HirAnalysisDb,
        analysis_pass::ModuleAnalysisPass,
        diagnostics::DiagnosticVoucher,
        ty::{
            constraint::{
                CapabilityMode, CompilerCapabilityKind, ConstraintId, ConstraintKind,
                EffectCapabilityKey,
            },
            derive_provider::{
                DeriveProviderId, providers_for_derive_goal, validated_derive_providers_for_ingot,
            },
            diagnostics::{TyDiagCollection, TyLowerDiag},
            generated::{GeneratedImplId, GeneratedImplSource},
            ty_def::TyId,
            unify::UnificationTable,
        },
    },
    hir_def::{TopLevelMod, Trait},
    span::DynLazySpan,
};
use common::{indexmap::IndexMap, ingot::Ingot};

mod builder;
mod capability;
mod coherence;
mod cycles;
mod diagnostics;
mod generated_method;
mod provider_context;
mod provider_execution;
mod provider_output;
mod reflect;
mod request;
mod requirement_origin;
mod trace;

#[cfg(test)]
pub(crate) use builder::BuilderCommand;
use builder::builder_command_list_finish_span;
use builder::generated_impl_from_builder_commands;
pub(crate) use builder::{BuilderCommandListId, BuilderError, ImplBuilderSession};
pub(crate) use capability::{
    CapabilityEnv, ElaborationCapabilityOrigin, ElaborationCapabilityWitness,
};
use coherence::{generated_conflicting_generated_impl, generated_conflicts_with_authored_impl};
use diagnostics::{
    implicit_provider_ambiguity_diags_for_top_mod, provider_output_diag,
    selected_derive_provider_diags_for_top_mod,
};
use generated_method::{
    generated_invalid_required_method_bodies, generated_method_error_summary,
    generated_missing_required_methods,
};
use provider_context::{
    derive_evidence_for_goal, elaborate_provider_context, matching_selected_providers,
};
use provider_execution::provider_output_for_context;
pub(crate) use provider_output::{ProviderFailureReason, ProviderOutputId, ProviderOutputStatus};
pub(crate) use reflect::ReflectedField;
use reflect::reflect_struct_fields;
pub(crate) use request::{
    ElaborationRequestId, elaboration_requests_for_ingot, elaboration_requests_for_top_mod,
};
pub(crate) use requirement_origin::RequirementOrigin;
pub use trace::{
    generated_impl_summaries_for_top_mod, generated_method_artifact_summaries_for_top_mod,
    generated_requirement_artifact_summaries_for_top_mod, generated_trace_summaries_for_top_mod,
};

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ElaborationCtfeContextId<'db> {
    pub(crate) request: ElaborationRequestId<'db>,
    pub(crate) provider: DeriveProviderId<'db>,
    derive_evidence: ConstraintId<'db>,

    #[return_ref]
    capabilities: Vec<ElaborationCapabilityWitness<'db>>,
}

impl<'db> ElaborationCtfeContextId<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        let provider = self.provider(db).identity(db).pretty_print(db);
        let capabilities = self
            .capabilities(db)
            .iter()
            .map(|witness| witness.capability.pretty_print(db))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{} using {} evidence from {} with [{}]",
            self.request(db).pretty_print(db),
            self.derive_evidence(db).pretty_print(db),
            provider,
            capabilities
        )
    }
}

#[salsa::tracked(return_ref)]
pub(crate) fn elaboration_request_diags_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<TyDiagCollection<'db>> {
    let mut diags = request::elaboration_request_parse_diags_for_top_mod(db, top_mod);
    diags.extend(implicit_provider_ambiguity_diags_for_top_mod(db, top_mod));
    diags.extend(selected_derive_provider_diags_for_top_mod(db, top_mod));
    diags.extend(generated_overlay_diags_for_top_mod(db, top_mod));
    diags
}

pub fn elaboration_request_summaries_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<String> {
    elaboration_requests_for_top_mod(db, top_mod)
        .iter()
        .map(|request| request.pretty_print(db))
        .collect()
}

pub fn derive_provider_summaries_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<String> {
    validated_derive_providers_for_ingot(db, top_mod.ingot(db))
        .into_iter()
        .map(|provider| {
            format!(
                "provider {} via {} -> {}",
                provider.identity(db).pretty_print(db),
                provider.derive_goal(db).pretty_print(db),
                provider.goal(db).pretty_print(db)
            )
        })
        .collect()
}

fn generated_overlay_diags_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<TyDiagCollection<'db>> {
    let candidates = generated_impl_candidates_for_ingot(db, top_mod.ingot(db));
    let method_valid_candidates = generated_impl_method_valid_candidates(db, &candidates);
    let candidate_indices = method_valid_candidates
        .iter()
        .copied()
        .enumerate()
        .map(|(idx, generated)| (generated, idx))
        .collect::<IndexMap<_, _>>();
    let mut diags = Vec::new();
    diags.extend(cycles::generated_evidence_cycle_diags(
        db,
        top_mod,
        &method_valid_candidates,
    ));
    for &request in elaboration_requests_for_top_mod(db, top_mod) {
        for &context in elaboration_ctfe_contexts_for_request(db, request) {
            let output = provider_output_for_context(db, context);
            if let Some(diag) = provider_output_diag(db, output) {
                diags.push(diag);
                continue;
            }

            let generated = match provider_generated_impl_result_for_output(db, output) {
                Ok(Some(generated)) => generated,
                Ok(None) => continue,
                Err(err) => {
                    diags.push(invalid_request(
                        builder_error_span(&err).unwrap_or_else(|| request.span(db)),
                        format!(
                            "provider output for `{}` is invalid: {}",
                            request.goal(db).pretty_print(db),
                            builder_error_message(db, &err),
                        ),
                    ));
                    continue;
                }
            };
            let missing_methods = generated_missing_required_methods(db, generated);
            let invalid_body_methods = generated_invalid_required_method_bodies(db, generated);
            if !missing_methods.is_empty() || !invalid_body_methods.is_empty() {
                if let Some(head) = concrete_trait_head(db, request.goal(db)) {
                    let primary_span = invalid_body_methods
                        .first()
                        .map(|method| method.span.clone())
                        .or_else(|| provider_output_finish_span(db, output))
                        .unwrap_or_else(|| request.span(db));
                    diags.push(invalid_request(
                        primary_span,
                        format!(
                            "provider output for `{}` does not satisfy required methods: {} (generated by {} for `{}`)",
                            trait_name(db, head),
                            generated_method_error_summary(
                                db,
                                &missing_methods,
                                &invalid_body_methods
                            ),
                            context.provider(db).identity(db).pretty_print(db),
                            request.pretty_print(db)
                        ),
                    ));
                }
                continue;
            }
            if generated_conflicts_with_authored_impl(db, generated) {
                diags.push(invalid_request(
                    request.span(db),
                    format!(
                        "generated implementation for `{}` conflicts with an authored implementation",
                        generated.trait_inst.pretty_print(db, true)
                    ),
                ));
            } else if let Some(other) =
                generated_conflicting_generated_impl(db, generated, &method_valid_candidates)
            {
                if !should_report_generated_conflict(&candidate_indices, generated, other) {
                    continue;
                }
                diags.push(invalid_request(
                    request.span(db),
                    format!(
                        "generated implementation for `{}` from provider `{}` conflicts with another generated implementation from provider `{}` for `{}`",
                        generated.trait_inst.pretty_print(db, true),
                        generated_provider_name(db, generated),
                        generated_provider_name(db, other),
                        other.context.request(db).pretty_print(db),
                    ),
                ));
            }
        }
    }
    diags
}

fn provider_output_finish_span<'db>(
    db: &'db dyn HirAnalysisDb,
    output: ProviderOutputId<'db>,
) -> Option<DynLazySpan<'db>> {
    match output.status(db) {
        ProviderOutputStatus::Succeeded { commands } => {
            builder_command_list_finish_span(db, commands)
        }
        ProviderOutputStatus::Failed { .. } => None,
    }
}

fn should_report_generated_conflict<'db>(
    candidate_indices: &IndexMap<GeneratedImplId<'db>, usize>,
    generated: GeneratedImplId<'db>,
    other: GeneratedImplId<'db>,
) -> bool {
    match (
        candidate_indices.get(&generated),
        candidate_indices.get(&other),
    ) {
        (Some(current), Some(previous)) => current > previous,
        _ => true,
    }
}

fn trait_name<'db>(db: &'db dyn HirAnalysisDb, trait_: Trait<'db>) -> String {
    trait_
        .name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
        .unwrap_or_else(|| "<anonymous trait>".to_string())
}

fn generated_provider_name<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
) -> String {
    generated.context.provider(db).identity(db).pretty_print(db)
}

#[salsa::tracked(return_ref)]
pub(crate) fn elaboration_ctfe_contexts_for_request<'db>(
    db: &'db dyn HirAnalysisDb,
    request: ElaborationRequestId<'db>,
) -> Vec<ElaborationCtfeContextId<'db>> {
    let Some(derive_goal) = derive_evidence_for_goal(db, request.goal(db)) else {
        return Vec::new();
    };
    let ingot = request.target(db).item().top_mod(db).ingot(db);
    let providers = if let Some(selected) = request.selected_provider(db) {
        matching_selected_providers(db, ingot, derive_goal, selected)
    } else {
        providers_for_derive_goal(db, ingot, derive_goal)
    };
    if providers.len() != 1 {
        return Vec::new();
    }
    providers
        .into_iter()
        .filter_map(|provider| elaborate_provider_context(db, request, provider))
        .collect()
}

pub fn elaboration_ctfe_context_summaries_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<String> {
    elaboration_requests_for_top_mod(db, top_mod)
        .iter()
        .flat_map(|&request| {
            elaboration_ctfe_contexts_for_request(db, request)
                .iter()
                .map(|context| context.pretty_print(db))
                .collect::<Vec<_>>()
        })
        .collect()
}

pub fn reflected_field_summaries_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<String> {
    elaboration_requests_for_top_mod(db, top_mod)
        .iter()
        .flat_map(|&request| {
            elaboration_ctfe_contexts_for_request(db, request)
                .iter()
                .flat_map(|&context| {
                    reflected_fields_for_context(db, context)
                        .into_iter()
                        .map(|field| field.pretty_print(db))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[salsa::tracked(return_ref)]
pub(crate) fn generated_impls_for_ingot<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
) -> Vec<GeneratedImplId<'db>> {
    // Query boundary: elaboration produces an overlay from authored derive
    // requests and validated provider signatures. Trait solving consumes this
    // output later, but provider execution/builders must not be invoked from
    // inside the proof search itself.
    let candidates = generated_impl_candidates_for_ingot(db, ingot);
    let method_valid_candidates = generated_impl_method_valid_candidates(db, &candidates);
    method_valid_candidates
        .iter()
        .copied()
        .filter(|&generated| generated_impl_is_admissible(db, generated, &method_valid_candidates))
        .collect()
}

fn generated_impl_candidates_for_ingot<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
) -> Vec<GeneratedImplId<'db>> {
    elaboration_requests_for_ingot(db, ingot)
        .iter()
        .flat_map(|&request| {
            elaboration_ctfe_contexts_for_request(db, request)
                .iter()
                .filter_map(move |&context| {
                    provider_generated_impl_candidate_for_context(db, context)
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn generated_impl_method_valid_candidates<'db>(
    db: &'db dyn HirAnalysisDb,
    candidates: &[GeneratedImplId<'db>],
) -> Vec<GeneratedImplId<'db>> {
    candidates
        .iter()
        .copied()
        .filter(|&generated| generated_impl_methods_are_valid(db, generated))
        .collect()
}

fn generated_impl_methods_are_valid<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
) -> bool {
    generated_missing_required_methods(db, generated).is_empty()
        && generated_invalid_required_method_bodies(db, generated).is_empty()
}

fn generated_impl_is_admissible<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
    candidates: &[GeneratedImplId<'db>],
) -> bool {
    if !generated_impl_methods_are_valid(db, generated) {
        return false;
    }
    !generated_conflicts_with_authored_impl(db, generated)
        && generated_conflicting_generated_impl(db, generated, candidates).is_none()
}

fn provider_generated_impl_candidate_for_context<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
) -> Option<GeneratedImplId<'db>> {
    let output = provider_output_for_context(db, context);
    provider_generated_impl_for_output(db, output)
}

fn provider_generated_impl_for_output<'db>(
    db: &'db dyn HirAnalysisDb,
    output: ProviderOutputId<'db>,
) -> Option<GeneratedImplId<'db>> {
    provider_generated_impl_result_for_output(db, output)
        .ok()
        .flatten()
}

fn provider_generated_impl_result_for_output<'db>(
    db: &'db dyn HirAnalysisDb,
    output: ProviderOutputId<'db>,
) -> Result<Option<GeneratedImplId<'db>>, BuilderError<'db>> {
    let ProviderOutputStatus::Succeeded { commands } = output.status(db) else {
        return Ok(None);
    };
    generated_impl_from_builder_commands(
        db,
        output.context(db),
        GeneratedImplSource::ProviderOutput(output),
        commands,
    )
    .map(Some)
}

fn builder_error_message<'db>(db: &'db dyn HirAnalysisDb, err: &BuilderError<'db>) -> String {
    match err {
        BuilderError::WrongTarget {
            expected,
            attempted,
        } => format!(
            "wrong target, expected `{}` but got `{}`",
            expected.pretty_print(db),
            attempted.pretty_print(db)
        ),
        BuilderError::AlreadyFinished => "builder was already finished".to_string(),
        BuilderError::CommandAfterFinish => "builder emitted a command after finish".to_string(),
        BuilderError::NotFinished => "builder did not finish".to_string(),
        BuilderError::DuplicateMethod { name, .. } => {
            format!("duplicate generated method `{}`", name.data(db))
        }
        BuilderError::UnsupportedTarget(target) => {
            format!(
                "unsupported generated evidence target `{}`",
                target.pretty_print(db)
            )
        }
    }
}

fn builder_error_span<'db>(err: &BuilderError<'db>) -> Option<DynLazySpan<'db>> {
    match err {
        BuilderError::DuplicateMethod { span, .. } => Some(span.clone()),
        _ => None,
    }
}

pub(crate) fn reflected_fields_for_context<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
) -> Vec<ReflectedField<'db>> {
    context
        .capabilities(db)
        .iter()
        .filter_map(|witness| {
            if witness.capability.mode(db) != CapabilityMode::Read {
                return None;
            }
            match witness.capability.key(db) {
                EffectCapabilityKey::Compiler(CompilerCapabilityKind::Reflect(target)) => {
                    Some(target)
                }
                _ => None,
            }
        })
        .flat_map(|target| reflect_struct_fields(db, target))
        .collect()
}

pub(super) fn constraints_match<'db>(
    db: &'db dyn HirAnalysisDb,
    lhs: ConstraintId<'db>,
    rhs: ConstraintId<'db>,
) -> bool {
    let mut table = UnificationTable::new(db);
    table.unify(lhs, rhs).is_ok()
}

pub(super) fn tys_match<'db>(db: &'db dyn HirAnalysisDb, lhs: TyId<'db>, rhs: TyId<'db>) -> bool {
    let mut table = UnificationTable::new(db);
    table.unify(lhs, rhs).is_ok()
}

fn concrete_trait_head<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: ConstraintId<'db>,
) -> Option<Trait<'db>> {
    match goal.kind(db) {
        ConstraintKind::Trait(inst) => Some(inst.def(db)),
        _ => None,
    }
}

pub(super) fn invalid_request<'db>(
    span: DynLazySpan<'db>,
    message: impl Into<String>,
) -> TyDiagCollection<'db> {
    TyLowerDiag::InvalidElaborationRequest {
        span,
        message: message.into(),
    }
    .into()
}

pub(crate) struct ElaborationRequestAnalysisPass {}

impl ModuleAnalysisPass for ElaborationRequestAnalysisPass {
    fn run_on_module<'db>(
        &mut self,
        db: &'db dyn HirAnalysisDb,
        top_mod: TopLevelMod<'db>,
    ) -> Vec<Box<dyn DiagnosticVoucher + 'db>> {
        elaboration_request_diags_for_top_mod(db, top_mod)
            .iter()
            .map(|diag| diag.to_voucher())
            .collect()
    }
}

#[cfg(test)]
mod tests;
