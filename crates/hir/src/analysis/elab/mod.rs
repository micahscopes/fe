use common::ingot::Ingot;

#[cfg(test)]
use crate::{
    analysis::ty::generated::{
        GeneratedExprId, GeneratedExprKind, GeneratedMethodBodyKind, GeneratedStructFieldInit,
        GeneratedStructFieldInitListId,
    },
    hir_def::IdentId,
};
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
            diagnostics::{TyDiagCollection, TyLowerDiag},
            evidence_provider::{
                EvidenceProviderId, providers_for_derive_goal,
                validated_evidence_providers_for_ingot,
            },
            generated::{GeneratedImplId, GeneratedImplSource},
            ty_def::TyId,
            unify::UnificationTable,
        },
    },
    hir_def::{TopLevelMod, Trait},
    span::DynLazySpan,
};

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
use builder::generated_impl_from_builder_commands;
pub(crate) use builder::{BuilderCommandListId, BuilderError, ImplBuilderSession};
pub(crate) use capability::{
    CapabilityEnv, ElaborationCapabilityOrigin, ElaborationCapabilityWitness,
};
use coherence::{generated_conflicts_with_authored_impl, generated_conflicts_with_generated_impl};
use diagnostics::{
    duplicate_evidence_provider_diags_for_top_mod, provider_output_diag,
    selected_evidence_provider_diags_for_top_mod,
};
use generated_method::{
    generated_method_error_summary, generated_missing_required_methods,
    generated_unsupported_required_methods,
};
use provider_context::{
    derive_evidence_for_goal, elaborate_provider_context, matching_selected_providers,
};
use provider_execution::provider_output_for_context;
pub(crate) use provider_output::{ProviderOutputId, ProviderOutputStatus, ProviderSkipReason};
pub(crate) use reflect::ReflectedField;
use reflect::reflect_struct_fields;
pub(crate) use request::{
    ElaborationRequestId, elaboration_requests_for_ingot, elaboration_requests_for_top_mod,
};
pub(crate) use requirement_origin::RequirementOrigin;
pub use trace::{
    generated_impl_summaries_for_top_mod, generated_requirement_artifact_summaries_for_top_mod,
    generated_trace_summaries_for_top_mod,
};

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ElaborationCtfeContextId<'db> {
    request: ElaborationRequestId<'db>,
    provider: EvidenceProviderId<'db>,
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
    diags.extend(duplicate_evidence_provider_diags_for_top_mod(db, top_mod));
    diags.extend(selected_evidence_provider_diags_for_top_mod(db, top_mod));
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

pub fn evidence_provider_summaries_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<String> {
    validated_evidence_providers_for_ingot(db, top_mod.ingot(db))
        .into_iter()
        .map(|provider| {
            format!(
                "provider {} for {} via {} -> {}",
                provider.identity(db).pretty_print(db),
                trait_name(db, provider.head(db)),
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
    let mut diags = Vec::new();
    diags.extend(cycles::generated_evidence_cycle_diags(
        db,
        top_mod,
        &candidates,
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
                        request.span(db),
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
            let unsupported_methods = generated_unsupported_required_methods(db, generated);
            if !missing_methods.is_empty() || !unsupported_methods.is_empty() {
                if let Some(head) = concrete_trait_head(db, request.goal(db)) {
                    diags.push(invalid_request(
                        request.span(db),
                        format!(
                            "provider output for `{}` does not generate required methods yet: {}",
                            trait_name(db, head),
                            generated_method_error_summary(
                                db,
                                &missing_methods,
                                &unsupported_methods
                            )
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
            } else if generated_conflicts_with_generated_impl(db, generated, &candidates) {
                diags.push(invalid_request(
                    request.span(db),
                    format!(
                        "generated implementation for `{}` conflicts with another generated implementation",
                        generated.trait_inst.pretty_print(db, true)
                    ),
                ));
            }
        }
    }
    diags
}

fn trait_name<'db>(db: &'db dyn HirAnalysisDb, trait_: Trait<'db>) -> String {
    trait_
        .name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
        .unwrap_or_else(|| "<anonymous trait>".to_string())
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
    candidates
        .iter()
        .copied()
        .filter(|&generated| generated_impl_is_admissible(db, generated, &candidates))
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

fn generated_impl_is_admissible<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
    candidates: &[GeneratedImplId<'db>],
) -> bool {
    let missing = generated_missing_required_methods(db, generated);
    let unsupported = generated_unsupported_required_methods(db, generated);
    if !missing.is_empty() || !unsupported.is_empty() {
        return false;
    }
    !generated_conflicts_with_authored_impl(db, generated)
        && !generated_conflicts_with_generated_impl(db, generated, candidates)
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
        BuilderError::DuplicateMethod { name } => {
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
mod tests {
    use super::*;
    use crate::{
        analysis::ty::{constraint::ConstraintId, trait_def::TraitInstId, ty_def::TyId},
        hir_def::ItemKind,
        span::LazySpan,
        test_db::HirAnalysisTestDb,
    };

    fn find_trait<'db>(
        db: &'db HirAnalysisTestDb,
        top_mod: TopLevelMod<'db>,
        name: &str,
    ) -> Trait<'db> {
        top_mod
            .all_items(db)
            .iter()
            .find_map(|item| match item {
                ItemKind::Trait(trait_)
                    if trait_
                        .name(db)
                        .to_opt()
                        .is_some_and(|ident| ident.data(db) == name) =>
                {
                    Some(*trait_)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing `{name}` trait"))
    }

    fn first_builder_context<'db>(
        db: &'db HirAnalysisTestDb,
        top_mod: TopLevelMod<'db>,
    ) -> ElaborationCtfeContextId<'db> {
        let request = *elaboration_requests_for_top_mod(db, top_mod)
            .first()
            .expect("missing elaboration request");
        *elaboration_ctfe_contexts_for_request(db, request)
            .first()
            .expect("missing elaboration context")
    }

    fn skipped_output_span_text<'db>(
        db: &'db HirAnalysisTestDb,
        output: ProviderOutputId<'db>,
    ) -> String {
        let ProviderOutputStatus::Skipped { span, .. } = output.status(db) else {
            panic!("expected skipped provider output");
        };
        let resolved = span.resolve(db).expect("skip span should resolve");
        let text = resolved.file.text(db);
        text[resolved.range.start().into()..resolved.range.end().into()].to_string()
    }

    fn provider_output_source_for_commands<'db>(
        db: &'db HirAnalysisTestDb,
        context: ElaborationCtfeContextId<'db>,
        commands: BuilderCommandListId<'db>,
    ) -> GeneratedImplSource<'db> {
        GeneratedImplSource::ProviderOutput(ProviderOutputId::new(
            db,
            context.request(db),
            context.provider(db),
            context,
            ProviderOutputStatus::Succeeded { commands },
        ))
    }

    #[test]
    fn raw_impl_builder_records_required_obligations() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "raw_impl_builder_records_required_obligations.fe".into(),
            r#"
trait Eq {}

#[derive(Eq)]
struct Foo {}

#[evidence_provider(Eq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);

        let eq = find_trait(&db, top_mod, "Eq");
        let field_obligation =
            ConstraintId::from_trait(&db, TraitInstId::new_simple(&db, eq, vec![TyId::u256(&db)]));
        let context = first_builder_context(&db, top_mod);
        let mut builder = ImplBuilderSession::new(&db, context);
        builder.require(field_obligation).unwrap();

        let generated = builder.finish(&db).unwrap();
        assert_eq!(generated.obligations.list(&db), &[field_obligation]);
    }

    #[test]
    fn raw_impl_builder_rejects_wrong_target() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "raw_impl_builder_rejects_wrong_target.fe".into(),
            r#"
trait Eq {}
trait Default {}

#[derive(Eq)]
struct Foo {}

#[evidence_provider(Eq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);

        let default = find_trait(&db, top_mod, "Default");
        let target_ty = first_builder_context(&db, top_mod)
            .request(&db)
            .target(&db)
            .ty(&db);
        let wrong_goal =
            ConstraintId::from_trait(&db, TraitInstId::new_simple(&db, default, vec![target_ty]));

        let context = first_builder_context(&db, top_mod);
        let mut builder = ImplBuilderSession::new(&db, context);
        let err = builder.emit_impl(&db, wrong_goal).unwrap_err();
        assert!(matches!(err, BuilderError::WrongTarget { .. }));
    }

    #[test]
    fn builder_command_validation_requires_finish() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "builder_command_validation_requires_finish.fe".into(),
            r#"
trait Eq {}

#[derive(Eq)]
struct Foo {}

#[evidence_provider(Eq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);

        let eq = find_trait(&db, top_mod, "Eq");
        let requirement =
            ConstraintId::from_trait(&db, TraitInstId::new_simple(&db, eq, vec![TyId::u256(&db)]));
        let context = first_builder_context(&db, top_mod);
        let commands = BuilderCommandListId::new(
            &db,
            vec![BuilderCommand::Require {
                constraint: requirement,
                origin: RequirementOrigin::Synthetic,
            }],
        );
        let err = generated_impl_from_builder_commands(
            &db,
            context,
            provider_output_source_for_commands(&db, context, commands),
            commands,
        )
        .unwrap_err();
        assert!(matches!(err, BuilderError::NotFinished));
    }

    #[test]
    fn builder_command_validation_rejects_commands_after_finish() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "builder_command_validation_rejects_commands_after_finish.fe".into(),
            r#"
trait Eq {}

#[derive(Eq)]
struct Foo {}

#[evidence_provider(Eq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);

        let eq = find_trait(&db, top_mod, "Eq");
        let requirement =
            ConstraintId::from_trait(&db, TraitInstId::new_simple(&db, eq, vec![TyId::u256(&db)]));
        let context = first_builder_context(&db, top_mod);
        let commands = BuilderCommandListId::new(
            &db,
            vec![
                BuilderCommand::Finish,
                BuilderCommand::Require {
                    constraint: requirement,
                    origin: RequirementOrigin::Synthetic,
                },
            ],
        );
        let err = generated_impl_from_builder_commands(
            &db,
            context,
            provider_output_source_for_commands(&db, context, commands),
            commands,
        )
        .unwrap_err();
        assert!(matches!(err, BuilderError::CommandAfterFinish));
    }

    #[test]
    fn provider_output_reports_missing_finish() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "provider_output_reports_missing_finish.fe".into(),
            r#"
trait Eq {}

#[derive(Eq)]
struct Foo {}

#[evidence_provider(Eq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (builder: mut ImplBuilder<Eq<T>>)
{
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);

        let context = first_builder_context(&db, top_mod);
        let output = provider_output_for_context(&db, context);
        assert!(matches!(
            output.status(&db),
            ProviderOutputStatus::Skipped {
                reason: ProviderSkipReason::MissingFinish,
                ..
            }
        ));
    }

    #[test]
    fn provider_output_reports_missing_reflect_capability() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "provider_output_reports_missing_reflect_capability.fe".into(),
            r#"
trait Eq {}

#[derive(Eq)]
struct Foo {}

#[evidence_provider(Eq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (builder: mut ImplBuilder<Eq<T>>)
{
    for field in reflect.fields() {
        builder.require<Eq>(field.ty())
    }
    builder.finish()
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);

        let context = first_builder_context(&db, top_mod);
        let output = provider_output_for_context(&db, context);
        assert!(matches!(
            output.status(&db),
            ProviderOutputStatus::Skipped {
                reason: ProviderSkipReason::MissingReflectCapability,
                ..
            }
        ));
        assert_eq!(skipped_output_span_text(&db, output), "reflect.fields()");
    }

    #[test]
    fn provider_output_reports_malformed_builder_finish_call_as_unsupported() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "provider_output_reports_malformed_builder_finish_call_as_unsupported.fe".into(),
            r#"
trait Eq {}

#[derive(Eq)]
struct Foo {}

#[evidence_provider(Eq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (builder: mut ImplBuilder<Eq<T>>)
{
    builder.finish(1)
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let context = first_builder_context(&db, top_mod);
        let output = provider_output_for_context(&db, context);
        assert!(matches!(
            output.status(&db),
            ProviderOutputStatus::Skipped {
                reason: ProviderSkipReason::UnsupportedProviderBody,
                ..
            }
        ));
        assert_eq!(skipped_output_span_text(&db, output), "builder");
    }

    #[test]
    fn provider_output_reports_unsupported_control_flow() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "provider_output_reports_unsupported_control_flow.fe".into(),
            r#"
trait Eq {}

#[derive(Eq)]
struct Foo {}

#[evidence_provider(Eq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (builder: mut ImplBuilder<Eq<T>>)
{
    while true {
        builder.finish()
    }
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let context = first_builder_context(&db, top_mod);
        let output = provider_output_for_context(&db, context);
        assert!(matches!(
            output.status(&db),
            ProviderOutputStatus::Skipped {
                reason: ProviderSkipReason::UnsupportedProviderBody,
                ..
            }
        ));
        assert!(skipped_output_span_text(&db, output).starts_with("while true"));
    }

    #[test]
    fn provider_output_rejects_duplicate_finish_calls() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "provider_output_rejects_duplicate_finish_calls.fe".into(),
            r#"
trait Eq {}

#[derive(Eq)]
struct Foo {}

#[evidence_provider(Eq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (builder: mut ImplBuilder<Eq<T>>)
{
    builder.finish()
    builder.finish()
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);

        let context = first_builder_context(&db, top_mod);
        let output = provider_output_for_context(&db, context);
        assert!(matches!(
            output.status(&db),
            ProviderOutputStatus::Skipped {
                reason: ProviderSkipReason::DuplicateFinish,
                ..
            }
        ));
    }

    #[test]
    fn provider_output_rejects_commands_after_finish() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "provider_output_rejects_commands_after_finish.fe".into(),
            r#"
trait Eq {}

#[derive(Eq)]
struct Foo {
    x: u256,
}

#[evidence_provider(Eq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (
        reflect: Reflect<T>,
        builder: mut ImplBuilder<Eq<T>>,
    )
{
    builder.finish()
    for field in reflect.fields() {
        builder.require<Eq>(field.ty())
    }
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);

        let context = first_builder_context(&db, top_mod);
        let output = provider_output_for_context(&db, context);
        assert!(matches!(
            output.status(&db),
            ProviderOutputStatus::Skipped {
                reason: ProviderSkipReason::CommandAfterFinish,
                ..
            }
        ));
    }

    #[test]
    fn generated_bool_method_body_satisfies_bool_required_method() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "generated_bool_method_body_satisfies_bool_required_method.fe".into(),
            r#"
trait Eq {
    fn eq(self, other: Self) -> bool
}

#[derive(Eq)]
struct Foo {}

#[evidence_provider(Eq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (builder: mut ImplBuilder<Eq<T>>)
{
    builder.finish()
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let context = first_builder_context(&db, top_mod);
        let output = provider_output_for_context(&db, context);
        let eq_trait = find_trait(&db, top_mod, "Eq");
        let method_name = *eq_trait
            .method_defs(&db)
            .keys()
            .next()
            .expect("missing required method");

        let commands = BuilderCommandListId::new(
            &db,
            vec![
                BuilderCommand::EmitMethodExpr {
                    name: method_name,
                    expr: GeneratedExprId::new(&db, GeneratedExprKind::BoolLiteral(true)),
                },
                BuilderCommand::Finish,
            ],
        );
        let generated = generated_impl_from_builder_commands(
            &db,
            context,
            GeneratedImplSource::ProviderOutput(output),
            commands,
        )
        .unwrap();

        assert!(generated_missing_required_methods(&db, generated).is_empty());
        assert!(generated_unsupported_required_methods(&db, generated).is_empty());
        let GeneratedMethodBodyKind::Expr(expr) = generated.methods.list(&db)[0].body;
        assert!(matches!(
            expr.kind(&db),
            GeneratedExprKind::BoolLiteral(true)
        ));
    }

    #[test]
    fn generated_bool_and_method_body_satisfies_bool_required_method() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "generated_bool_and_method_body_satisfies_bool_required_method.fe".into(),
            r#"
trait Eq {
    fn eq(self, other: Self) -> bool
}

#[derive(Eq)]
struct Foo {}

#[evidence_provider(Eq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (builder: mut ImplBuilder<Eq<T>>)
{
    builder.finish()
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let context = first_builder_context(&db, top_mod);
        let output = provider_output_for_context(&db, context);
        let eq_trait = find_trait(&db, top_mod, "Eq");
        let method_name = *eq_trait
            .method_defs(&db)
            .keys()
            .next()
            .expect("missing required method");
        let lhs = GeneratedExprId::new(&db, GeneratedExprKind::BoolLiteral(true));
        let rhs = GeneratedExprId::new(&db, GeneratedExprKind::BoolLiteral(false));
        let expr = GeneratedExprId::new(&db, GeneratedExprKind::BoolAnd { lhs, rhs });

        let commands = BuilderCommandListId::new(
            &db,
            vec![
                BuilderCommand::EmitMethodExpr {
                    name: method_name,
                    expr,
                },
                BuilderCommand::Finish,
            ],
        );
        let generated = generated_impl_from_builder_commands(
            &db,
            context,
            GeneratedImplSource::ProviderOutput(output),
            commands,
        )
        .unwrap();

        assert!(generated_missing_required_methods(&db, generated).is_empty());
        assert!(generated_unsupported_required_methods(&db, generated).is_empty());
    }

    #[test]
    fn generated_field_get_eq_expr_satisfies_bool_required_method() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "generated_field_get_eq_expr_satisfies_bool_required_method.fe".into(),
            r#"
trait Eq {
    fn eq(self, other: Self) -> bool
}

#[derive(Eq)]
struct Foo {
    x: u256,
}

#[evidence_provider(Eq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (builder: mut ImplBuilder<Eq<T>>)
{
    builder.finish()
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let context = first_builder_context(&db, top_mod);
        let output = provider_output_for_context(&db, context);
        let eq_trait = find_trait(&db, top_mod, "Eq");
        let method_name = *eq_trait
            .method_defs(&db)
            .keys()
            .next()
            .expect("missing required method");
        let target_ty = context.request(&db).target(&db).ty(&db);
        let field = reflect_struct_fields(&db, target_ty)
            .into_iter()
            .next()
            .expect("missing reflected field");
        let self_ref = GeneratedExprId::new(&db, GeneratedExprKind::SelfRef { ty: target_ty });
        let other_ref = GeneratedExprId::new(
            &db,
            GeneratedExprKind::MethodArgRef {
                name: IdentId::new(&db, "other".to_string()),
                ty: target_ty,
            },
        );
        let lhs = GeneratedExprId::new(
            &db,
            GeneratedExprKind::FieldGet {
                base: self_ref,
                field,
            },
        );
        let rhs = GeneratedExprId::new(
            &db,
            GeneratedExprKind::FieldGet {
                base: other_ref,
                field,
            },
        );
        let expr = GeneratedExprId::new(&db, GeneratedExprKind::EqExpr { lhs, rhs });

        let commands = BuilderCommandListId::new(
            &db,
            vec![
                BuilderCommand::EmitMethodExpr {
                    name: method_name,
                    expr,
                },
                BuilderCommand::Finish,
            ],
        );
        let generated = generated_impl_from_builder_commands(
            &db,
            context,
            GeneratedImplSource::ProviderOutput(output),
            commands,
        )
        .unwrap();

        assert!(generated_missing_required_methods(&db, generated).is_empty());
        assert!(generated_unsupported_required_methods(&db, generated).is_empty());
    }

    #[test]
    fn generated_struct_init_body_satisfies_self_returning_method() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "generated_struct_init_body_satisfies_self_returning_method.fe".into(),
            r#"
trait Default {
    fn default() -> Self
}

#[derive(Default)]
struct Foo {
    value: u256,
}

#[evidence_provider(Default)]
const fn derive_default<T>(ev: own Evidence<Default<T>>) -> Evidence<Default<T>>
    uses (builder: mut ImplBuilder<Default<T>>)
{
    builder.finish()
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let context = first_builder_context(&db, top_mod);
        let output = provider_output_for_context(&db, context);
        let default_trait = find_trait(&db, top_mod, "Default");
        let method_name = *default_trait
            .method_defs(&db)
            .keys()
            .next()
            .expect("missing required method");
        let target_ty = context.request(&db).target(&db).ty(&db);
        let field = reflect_struct_fields(&db, target_ty)
            .into_iter()
            .next()
            .expect("missing reflected field");
        let value = GeneratedExprId::new(&db, GeneratedExprKind::DefaultCall { ty: field.ty });
        let fields = GeneratedStructFieldInitListId::new(
            &db,
            vec![GeneratedStructFieldInit { field, value }],
        );
        let expr = GeneratedExprId::new(
            &db,
            GeneratedExprKind::StructInit {
                target: target_ty,
                fields,
            },
        );

        let commands = BuilderCommandListId::new(
            &db,
            vec![
                BuilderCommand::EmitMethodExpr {
                    name: method_name,
                    expr,
                },
                BuilderCommand::Finish,
            ],
        );
        let generated = generated_impl_from_builder_commands(
            &db,
            context,
            GeneratedImplSource::ProviderOutput(output),
            commands,
        )
        .unwrap();

        assert!(generated_missing_required_methods(&db, generated).is_empty());
        assert!(generated_unsupported_required_methods(&db, generated).is_empty());
    }

    #[test]
    fn generated_struct_init_body_rejects_missing_fields() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "generated_struct_init_body_rejects_missing_fields.fe".into(),
            r#"
trait Default {
    fn default() -> Self
}

#[derive(Default)]
struct Foo {
    value: u256,
}

#[evidence_provider(Default)]
const fn derive_default<T>(ev: own Evidence<Default<T>>) -> Evidence<Default<T>>
    uses (builder: mut ImplBuilder<Default<T>>)
{
    builder.finish()
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let context = first_builder_context(&db, top_mod);
        let output = provider_output_for_context(&db, context);
        let default_trait = find_trait(&db, top_mod, "Default");
        let method_name = *default_trait
            .method_defs(&db)
            .keys()
            .next()
            .expect("missing required method");
        let target_ty = context.request(&db).target(&db).ty(&db);
        let fields = GeneratedStructFieldInitListId::new(&db, Vec::new());
        let expr = GeneratedExprId::new(
            &db,
            GeneratedExprKind::StructInit {
                target: target_ty,
                fields,
            },
        );

        let commands = BuilderCommandListId::new(
            &db,
            vec![
                BuilderCommand::EmitMethodExpr {
                    name: method_name,
                    expr,
                },
                BuilderCommand::Finish,
            ],
        );
        let generated = generated_impl_from_builder_commands(
            &db,
            context,
            GeneratedImplSource::ProviderOutput(output),
            commands,
        )
        .unwrap();

        assert!(generated_missing_required_methods(&db, generated).is_empty());
        assert_eq!(
            generated_unsupported_required_methods(&db, generated),
            vec![method_name]
        );
    }

    #[test]
    fn generated_struct_init_body_rejects_wrong_field_type() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "generated_struct_init_body_rejects_wrong_field_type.fe".into(),
            r#"
trait Default {
    fn default() -> Self
}

#[derive(Default)]
struct Foo {
    value: u256,
}

#[evidence_provider(Default)]
const fn derive_default<T>(ev: own Evidence<Default<T>>) -> Evidence<Default<T>>
    uses (builder: mut ImplBuilder<Default<T>>)
{
    builder.finish()
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let context = first_builder_context(&db, top_mod);
        let output = provider_output_for_context(&db, context);
        let default_trait = find_trait(&db, top_mod, "Default");
        let method_name = *default_trait
            .method_defs(&db)
            .keys()
            .next()
            .expect("missing required method");
        let target_ty = context.request(&db).target(&db).ty(&db);
        let field = reflect_struct_fields(&db, target_ty)
            .into_iter()
            .next()
            .expect("missing reflected field");
        let value = GeneratedExprId::new(&db, GeneratedExprKind::BoolLiteral(false));
        let fields = GeneratedStructFieldInitListId::new(
            &db,
            vec![GeneratedStructFieldInit { field, value }],
        );
        let expr = GeneratedExprId::new(
            &db,
            GeneratedExprKind::StructInit {
                target: target_ty,
                fields,
            },
        );

        let commands = BuilderCommandListId::new(
            &db,
            vec![
                BuilderCommand::EmitMethodExpr {
                    name: method_name,
                    expr,
                },
                BuilderCommand::Finish,
            ],
        );
        let generated = generated_impl_from_builder_commands(
            &db,
            context,
            GeneratedImplSource::ProviderOutput(output),
            commands,
        )
        .unwrap();

        assert!(generated_missing_required_methods(&db, generated).is_empty());
        assert_eq!(
            generated_unsupported_required_methods(&db, generated),
            vec![method_name]
        );
    }

    #[test]
    fn generated_bool_method_body_rejects_non_bool_required_method() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "generated_bool_method_body_rejects_non_bool_required_method.fe".into(),
            r#"
trait Count {
    fn count(self) -> u256
}

#[derive(Count)]
struct Foo {}

#[evidence_provider(Count)]
const fn derive_count<T>(ev: own Evidence<Count<T>>) -> Evidence<Count<T>>
    uses (builder: mut ImplBuilder<Count<T>>)
{
    builder.finish()
    ev
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let context = first_builder_context(&db, top_mod);
        let output = provider_output_for_context(&db, context);
        let count_trait = find_trait(&db, top_mod, "Count");
        let method_name = *count_trait
            .method_defs(&db)
            .keys()
            .next()
            .expect("missing required method");

        let commands = BuilderCommandListId::new(
            &db,
            vec![
                BuilderCommand::EmitMethodExpr {
                    name: method_name,
                    expr: GeneratedExprId::new(&db, GeneratedExprKind::BoolLiteral(true)),
                },
                BuilderCommand::Finish,
            ],
        );
        let generated = generated_impl_from_builder_commands(
            &db,
            context,
            GeneratedImplSource::ProviderOutput(output),
            commands,
        )
        .unwrap();

        assert!(generated_missing_required_methods(&db, generated).is_empty());
        assert_eq!(
            generated_unsupported_required_methods(&db, generated),
            vec![method_name]
        );
    }
}
