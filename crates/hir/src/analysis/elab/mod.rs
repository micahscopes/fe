use common::{
    indexmap::{IndexMap, IndexSet},
    ingot::Ingot,
};

use crate::{
    analysis::{
        HirAnalysisDb,
        analysis_pass::ModuleAnalysisPass,
        diagnostics::DiagnosticVoucher,
        name_resolution::{PathRes, resolve_path},
        ty::{
            binder::Binder,
            constraint::{
                CapabilityMode, CompilerCapabilityKind, ConstraintApplicationId, ConstraintHeadId,
                ConstraintHeadKind, ConstraintId, ConstraintKind, ConstraintListId,
                EffectCapabilityKey,
            },
            diagnostics::{TyDiagCollection, TyLowerDiag},
            evidence_provider::{
                EvidenceProviderId, providers_for_derive_goal,
                validated_evidence_providers_for_ingot,
            },
            fold::{TyFoldable, TyFolder},
            generated::{
                GeneratedExprId, GeneratedExprKind, GeneratedImplId, GeneratedImplSource,
                GeneratedMethod, GeneratedMethodBodyKind, GeneratedMethodListId,
                GeneratedRequirement, GeneratedRequirementListId, GeneratedStructFieldInit,
                GeneratedStructFieldInitListId,
            },
            trait_def::{ImplementorId, ImplementorOrigin, TraitInstId, does_impl_trait_conflict},
            trait_lower::collect_trait_impls,
            trait_resolution::{
                PredicateListId, constraint::collect_func_effect_capability_constraints,
            },
            ty_def::{Kind, PrimTy, TyBase, TyData, TyId},
            ty_lower::ConstDefaultCompletion,
            unify::UnificationTable,
            visitor::{TyVisitable, TyVisitor},
        },
    },
    hir_def::{
        Body, Expr, FieldParent, GenericArg, GenericArgListId, IdentId, LitKind, Partial, Pat,
        Stmt, TopLevelMod, Trait, TypeKind,
    },
    span::DynLazySpan,
};

mod capability;
mod cycles;
mod request;
mod trace;

pub(crate) use capability::{
    CapabilityEnv, ElaborationCapabilityOrigin, ElaborationCapabilityWitness,
};
pub(crate) use request::{
    ElaborationRequestId, elaboration_requests_for_ingot, elaboration_requests_for_top_mod,
};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct ReflectedField<'db> {
    parent: TyId<'db>,
    index: u32,
    name: IdentId<'db>,
    ty: TyId<'db>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
#[allow(dead_code)]
pub(crate) enum RequirementOrigin<'db> {
    ReflectedField(ReflectedField<'db>),
    ProviderCode,
    Synthetic,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum BuilderCommand<'db> {
    Require {
        constraint: ConstraintId<'db>,
        origin: RequirementOrigin<'db>,
    },
    #[allow(dead_code)]
    EmitMethodExpr {
        name: IdentId<'db>,
        expr: GeneratedExprId<'db>,
    },
    Finish,
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct BuilderCommandListId<'db> {
    #[return_ref]
    commands: Vec<BuilderCommand<'db>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ProviderSkipReason {
    MissingBuilderCapability,
    MissingReflectCapability,
    MissingFinish,
    DuplicateFinish,
    CommandAfterFinish,
    InvalidBuilderRequirement,
    UnsupportedProviderBody,
}

impl ProviderSkipReason {
    fn diagnostic_message(self) -> &'static str {
        match self {
            Self::MissingBuilderCapability => {
                "provider does not declare mutable ImplBuilder capability"
            }
            Self::MissingReflectCapability => "provider body requires Reflect capability",
            Self::MissingFinish => "provider did not call builder.finish()",
            Self::DuplicateFinish => "provider called builder.finish() more than once",
            Self::CommandAfterFinish => "provider emitted a builder command after finish",
            Self::InvalidBuilderRequirement => "provider emitted an invalid builder requirement",
            Self::UnsupportedProviderBody => "provider body uses unsupported elaboration CTFE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ProviderOutputStatus<'db> {
    Succeeded {
        commands: BuilderCommandListId<'db>,
    },
    Failed,
    Skipped {
        reason: ProviderSkipReason,
        span: DynLazySpan<'db>,
    },
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ProviderOutputId<'db> {
    request: ElaborationRequestId<'db>,
    provider: EvidenceProviderId<'db>,
    context: ElaborationCtfeContextId<'db>,
    status: ProviderOutputStatus<'db>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum BuilderError<'db> {
    WrongTarget {
        expected: ConstraintId<'db>,
        attempted: ConstraintId<'db>,
    },
    AlreadyFinished,
    CommandAfterFinish,
    NotFinished,
    DuplicateMethod {
        name: IdentId<'db>,
    },
    UnsupportedTarget(ConstraintId<'db>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct ImplBuilderSession<'db> {
    context: ElaborationCtfeContextId<'db>,
    target: ConstraintId<'db>,
    commands: Vec<BuilderCommand<'db>>,
    finished: bool,
}

impl<'db> ImplBuilderSession<'db> {
    pub(crate) fn new(db: &'db dyn HirAnalysisDb, context: ElaborationCtfeContextId<'db>) -> Self {
        Self {
            context,
            target: context.request(db).goal(db),
            commands: Vec::new(),
            finished: false,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn require(
        &mut self,
        constraint: ConstraintId<'db>,
    ) -> Result<(), BuilderError<'db>> {
        self.require_with_origin(constraint, RequirementOrigin::Synthetic)
    }

    pub(crate) fn require_with_origin(
        &mut self,
        constraint: ConstraintId<'db>,
        origin: RequirementOrigin<'db>,
    ) -> Result<(), BuilderError<'db>> {
        if self.finished {
            return Err(BuilderError::AlreadyFinished);
        }
        self.commands
            .push(BuilderCommand::Require { constraint, origin });
        Ok(())
    }

    fn emit_method_expr(
        &mut self,
        name: IdentId<'db>,
        expr: GeneratedExprId<'db>,
    ) -> Result<(), BuilderError<'db>> {
        if self.finished {
            return Err(BuilderError::AlreadyFinished);
        }
        self.commands
            .push(BuilderCommand::EmitMethodExpr { name, expr });
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn emit_impl(
        &mut self,
        db: &'db dyn HirAnalysisDb,
        goal: ConstraintId<'db>,
    ) -> Result<(), BuilderError<'db>> {
        if self.finished {
            return Err(BuilderError::AlreadyFinished);
        }
        if !constraints_match(db, self.target, goal) {
            return Err(BuilderError::WrongTarget {
                expected: self.target,
                attempted: goal,
            });
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn finish(
        mut self,
        db: &'db dyn HirAnalysisDb,
    ) -> Result<GeneratedImplId<'db>, BuilderError<'db>> {
        if self.finished {
            return Err(BuilderError::AlreadyFinished);
        }

        self.finished = true;
        self.commands.push(BuilderCommand::Finish);
        let commands = BuilderCommandListId::new(db, self.commands);
        generated_impl_from_builder_commands(
            db,
            self.context,
            GeneratedImplSource::StubDerivedFieldObligations,
            commands,
        )
    }

    fn finish_explicit(&mut self) -> Result<(), BuilderError<'db>> {
        if self.finished {
            return Err(BuilderError::AlreadyFinished);
        }
        self.finished = true;
        self.commands.push(BuilderCommand::Finish);
        Ok(())
    }

    fn into_commands(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Result<BuilderCommandListId<'db>, BuilderError<'db>> {
        if !self.finished {
            return Err(BuilderError::NotFinished);
        }
        Ok(BuilderCommandListId::new(db, self.commands))
    }
}

fn generated_impl_from_builder_commands<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
    source: GeneratedImplSource<'db>,
    commands: BuilderCommandListId<'db>,
) -> Result<GeneratedImplId<'db>, BuilderError<'db>> {
    let target = context.request(db).goal(db);
    let ConstraintKind::Trait(trait_inst) = target.kind(db) else {
        return Err(BuilderError::UnsupportedTarget(target));
    };

    let mut finished = false;
    let mut requirements = Vec::new();
    let mut methods = Vec::new();
    let mut method_names = IndexSet::new();
    for command in commands.commands(db) {
        if finished {
            return Err(BuilderError::CommandAfterFinish);
        }
        match command {
            BuilderCommand::Require { constraint, origin } => {
                requirements.push(GeneratedRequirement {
                    constraint: *constraint,
                    origin: *origin,
                })
            }
            BuilderCommand::EmitMethodExpr { name, expr } => methods.push(GeneratedMethod {
                name: {
                    if !method_names.insert(*name) {
                        return Err(BuilderError::DuplicateMethod { name: *name });
                    }
                    *name
                },
                body: GeneratedMethodBodyKind::Expr(*expr),
            }),
            BuilderCommand::Finish => finished = true,
        }
    }

    if !finished {
        return Err(BuilderError::NotFinished);
    }

    let obligations = requirements
        .iter()
        .map(|requirement| requirement.constraint)
        .collect::<Vec<_>>();
    Ok(GeneratedImplId {
        context,
        trait_inst,
        source,
        requirements: GeneratedRequirementListId::new(db, requirements),
        methods: GeneratedMethodListId::new(db, methods),
        obligations: ConstraintListId::new(db, obligations),
    })
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

impl<'db> ReflectedField<'db> {
    pub(crate) fn field_ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        let field_ctor = TyId::new(db, TyData::TyBase(TyBase::Prim(PrimTy::Field)));
        TyId::app(db, TyId::app(db, field_ctor, self.parent), self.ty)
    }

    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        format!(
            "{}.{}: {}",
            self.parent.pretty_print(db),
            self.name.data(db),
            self.field_ty(db).pretty_print(db)
        )
    }
}

impl<'db> TyVisitable<'db> for ReflectedField<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.parent.visit_with(visitor);
        self.ty.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for ReflectedField<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            parent: self.parent.fold_with(db, folder),
            index: self.index,
            name: self.name,
            ty: self.ty.fold_with(db, folder),
        }
    }
}

impl<'db> TyVisitable<'db> for RequirementOrigin<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        match self {
            Self::ReflectedField(field) => field.visit_with(visitor),
            Self::ProviderCode | Self::Synthetic => {}
        }
    }
}

impl<'db> TyFoldable<'db> for RequirementOrigin<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        match self {
            Self::ReflectedField(field) => Self::ReflectedField(field.fold_with(db, folder)),
            Self::ProviderCode | Self::Synthetic => self,
        }
    }
}

impl<'db> RequirementOrigin<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        match self {
            RequirementOrigin::ReflectedField(field) => {
                format!(
                    "from field {}.{}",
                    field.parent.pretty_print(db),
                    field.name.data(db)
                )
            }
            RequirementOrigin::ProviderCode => "from provider code".to_string(),
            RequirementOrigin::Synthetic => "from synthetic requirement".to_string(),
        }
    }

    pub(crate) fn diagnostic_span(self, db: &'db dyn HirAnalysisDb) -> Option<DynLazySpan<'db>> {
        match self {
            RequirementOrigin::ReflectedField(field) => field
                .parent
                .field_parent(db)
                .map(|parent| parent.field_name_span(field.index as usize)),
            RequirementOrigin::ProviderCode | RequirementOrigin::Synthetic => None,
        }
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

fn duplicate_evidence_provider_diags_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<TyDiagCollection<'db>> {
    let providers = validated_evidence_providers_for_ingot(db, top_mod.ingot(db));
    let implicit_derive_goals = elaboration_requests_for_top_mod(db, top_mod)
        .iter()
        .filter(|request| request.selected_provider(db).is_none())
        .filter_map(|request| derive_evidence_for_goal(db, request.goal(db)))
        .collect::<IndexSet<_>>();
    let mut by_derive_goal: IndexMap<ConstraintId<'db>, Vec<EvidenceProviderId<'db>>> =
        IndexMap::new();
    for provider in providers {
        by_derive_goal
            .entry(provider.derive_goal(db))
            .or_default()
            .push(provider);
    }

    by_derive_goal
        .into_iter()
        .filter_map(|(derive_goal, providers)| {
            if providers.len() <= 1 {
                return None;
            }
            if !implicit_derive_goals.contains(&derive_goal) {
                return None;
            }
            let span = providers[0].func(db).span().attributes().into();
            Some(invalid_request(
                span,
                format!(
                    "multiple evidence providers for `{}` are not supported yet",
                    derive_goal.pretty_print(db)
                ),
            ))
        })
        .collect()
}

fn selected_evidence_provider_diags_for_top_mod<'db>(
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
                if !providers_named_in_ingot(db, top_mod.ingot(db), selected).is_empty() {
                    format!(
                        "selected evidence provider `{selected_name}` does not provide `{}` evidence",
                        derive_goal.pretty_print(db)
                    )
                } else {
                    format!(
                        "selected evidence provider `{selected_name}` for `{}` was not found",
                        derive_goal.pretty_print(db)
                    )
                }
            } else {
                format!(
                    "selected evidence provider `{selected_name}` for `{}` is ambiguous",
                    derive_goal.pretty_print(db)
                )
            };
            Some(invalid_request(request.span(db), message))
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

fn provider_output_diag<'db>(
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

fn matching_selected_providers<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
    derive_goal: ConstraintId<'db>,
    selected: IdentId<'db>,
) -> Vec<EvidenceProviderId<'db>> {
    providers_for_derive_goal(db, ingot, derive_goal)
        .into_iter()
        .filter(|provider| provider.identity(db).name(db) == selected)
        .collect()
}

fn providers_named_in_ingot<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
    selected: IdentId<'db>,
) -> Vec<EvidenceProviderId<'db>> {
    validated_evidence_providers_for_ingot(db, ingot)
        .into_iter()
        .filter(|provider| provider.identity(db).name(db) == selected)
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

#[salsa::tracked]
fn provider_output_for_context<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
) -> ProviderOutputId<'db> {
    let request = context.request(db);
    let provider = context.provider(db);
    let goal = request.goal(db);
    let env = CapabilityEnv::from_context(db, context);
    if !env.has_impl_builder(db, goal) {
        return ProviderOutputId::new(
            db,
            request,
            provider,
            context,
            skipped_status(
                ProviderSkipReason::MissingBuilderCapability,
                provider.func(db).span().effects().into(),
            ),
        );
    }

    let status = execute_provider_body(db, context, env);
    ProviderOutputId::new(db, request, provider, context, status)
}

enum ProviderExecutionFailure<'db> {
    Skipped {
        reason: ProviderSkipReason,
        span: DynLazySpan<'db>,
    },
    Failed,
}

fn skipped_status<'db>(
    reason: ProviderSkipReason,
    span: DynLazySpan<'db>,
) -> ProviderOutputStatus<'db> {
    ProviderOutputStatus::Skipped { reason, span }
}

fn skipped_failure<'db>(
    reason: ProviderSkipReason,
    span: DynLazySpan<'db>,
) -> ProviderExecutionFailure<'db> {
    ProviderExecutionFailure::Skipped { reason, span }
}

#[derive(Clone, Copy)]
enum ElabValue<'db> {
    Field(ReflectedField<'db>),

    /// Internal provider-CTFE type witness produced by reflection operations
    /// such as `field.ty()`.
    ///
    /// This is not a public runtime value. If type witnesses become part of
    /// Fe's surface language, they should be modeled as explicit
    /// compile-time-only values instead of exposing raw `TyId`.
    TypeWitness(TyId<'db>),
    GeneratedExpr(GeneratedExprId<'db>),
}

#[derive(Clone, Copy)]
enum BuilderRequirementHead<'db> {
    ConcreteTrait(Trait<'db>),
    GenericConstraint(TyId<'db>),
}

impl<'db> BuilderRequirementHead<'db> {
    fn apply(self, db: &'db dyn HirAnalysisDb, arg: TyId<'db>) -> ConstraintId<'db> {
        match self {
            Self::ConcreteTrait(trait_) => {
                ConstraintId::from_trait(db, TraitInstId::new_simple(db, trait_, vec![arg]))
            }
            Self::GenericConstraint(head_ty) => {
                let head = ConstraintHeadId::new(db, ConstraintHeadKind::GenericParam(head_ty));
                let application = ConstraintApplicationId::new(db, head, vec![arg]);
                ConstraintId::new(db, ConstraintKind::ConstraintApplication(application))
            }
        }
    }
}

struct ProviderBodyExecutor<'db> {
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
    env: CapabilityEnv<'db>,
    builder: ImplBuilderSession<'db>,
    builder_names: Vec<IdentId<'db>>,
    reflect_names: Vec<IdentId<'db>>,
    field_bindings: Vec<(IdentId<'db>, ReflectedField<'db>)>,
    value_bindings: Vec<(IdentId<'db>, ElabValue<'db>)>,
}

impl<'db> ProviderBodyExecutor<'db> {
    fn new(
        db: &'db dyn HirAnalysisDb,
        context: ElaborationCtfeContextId<'db>,
        env: CapabilityEnv<'db>,
    ) -> Self {
        let provider = context.provider(db);
        Self {
            db,
            context,
            env,
            builder: ImplBuilderSession::new(db, context),
            builder_names: provider_impl_builder_effect_names(db, provider),
            reflect_names: provider_reflect_effect_names(db, provider),
            field_bindings: Vec::new(),
            value_bindings: Vec::new(),
        }
    }

    fn execute_body(mut self) -> ProviderOutputStatus<'db> {
        let request = self.context.request(self.db);
        let provider = self.context.provider(self.db);
        if self
            .builder
            .emit_impl(self.db, request.goal(self.db))
            .is_err()
        {
            return ProviderOutputStatus::Failed;
        }

        let Some(body) = provider.func(self.db).body(self.db) else {
            return skipped_status(
                ProviderSkipReason::MissingFinish,
                provider.func(self.db).span().name().into(),
            );
        };

        match self.execute_expr(body, body.expr(self.db)) {
            Ok(()) => match self.builder.into_commands(self.db) {
                Ok(commands) => ProviderOutputStatus::Succeeded { commands },
                Err(BuilderError::NotFinished) => skipped_status(
                    ProviderSkipReason::MissingFinish,
                    body.expr(self.db).span(body).into(),
                ),
                Err(_) => ProviderOutputStatus::Failed,
            },
            Err(ProviderExecutionFailure::Skipped { reason, span }) => skipped_status(reason, span),
            Err(ProviderExecutionFailure::Failed) => ProviderOutputStatus::Failed,
        }
    }

    fn execute_stmt(
        &mut self,
        body: Body<'db>,
        stmt: crate::hir_def::StmtId,
    ) -> Result<(), ProviderExecutionFailure<'db>> {
        let Partial::Present(stmt_data) = stmt.data(self.db, body) else {
            return Ok(());
        };
        match stmt_data {
            Stmt::Let(_, _, init) => {
                if let Some(init) = init {
                    if let Stmt::Let(pat, _, _) = stmt_data
                        && let Some(binding) = simple_pat_binding_name(self.db, body, *pat)
                        && let Some(value) = self.eval_expr_value(body, *init)
                    {
                        self.value_bindings.push((binding, value));
                        return Ok(());
                    }
                    self.execute_expr(body, *init)?;
                }
            }
            Stmt::For(pat, iterable, loop_body, _) => {
                if let Some(fields) = self.reflect_fields_iterable(body, *iterable)? {
                    let Some(binding) = simple_pat_binding_name(self.db, body, *pat) else {
                        return Err(skipped_failure(
                            ProviderSkipReason::UnsupportedProviderBody,
                            stmt.span(body).into(),
                        ));
                    };
                    for field in fields {
                        self.field_bindings.push((binding, field));
                        self.execute_expr(body, *loop_body)?;
                        self.field_bindings.pop();
                    }
                } else {
                    return Err(skipped_failure(
                        ProviderSkipReason::UnsupportedProviderBody,
                        (*iterable).span(body).into(),
                    ));
                }
            }
            Stmt::While(_, _) => {
                return Err(skipped_failure(
                    ProviderSkipReason::UnsupportedProviderBody,
                    stmt.span(body).into(),
                ));
            }
            Stmt::Return(expr) => {
                if let Some(expr) = expr {
                    self.execute_expr(body, *expr)?;
                }
            }
            Stmt::Expr(expr) => self.execute_expr(body, *expr)?,
            Stmt::Continue | Stmt::Break => {
                return Err(skipped_failure(
                    ProviderSkipReason::UnsupportedProviderBody,
                    stmt.span(body).into(),
                ));
            }
        }
        Ok(())
    }

    fn execute_expr(
        &mut self,
        body: Body<'db>,
        expr: crate::hir_def::ExprId,
    ) -> Result<(), ProviderExecutionFailure<'db>> {
        let Partial::Present(expr_data) = expr.data(self.db, body) else {
            return Ok(());
        };

        match expr_data {
            Expr::Block(stmts) => {
                let old_value_bindings = self.value_bindings.len();
                let result = (|| {
                    for &stmt in stmts {
                        self.execute_stmt(body, stmt)?;
                    }
                    Ok(())
                })();
                self.value_bindings.truncate(old_value_bindings);
                result?;
            }
            Expr::Assign(lhs, rhs) => {
                let Some(value) = self.eval_expr_value(body, *rhs) else {
                    return Err(skipped_failure(
                        ProviderSkipReason::UnsupportedProviderBody,
                        (*rhs).span(body).into(),
                    ));
                };
                if !self.assign_value_binding(body, *lhs, value) {
                    return Err(skipped_failure(
                        ProviderSkipReason::UnsupportedProviderBody,
                        (*lhs).span(body).into(),
                    ));
                }
            }
            Expr::Call(_, _) => {
                return Err(skipped_failure(
                    ProviderSkipReason::UnsupportedProviderBody,
                    expr.span(body).into(),
                ));
            }
            Expr::MethodCall(receiver, method, generic_args, args) => {
                if !self.execute_method_call(body, *receiver, *method, *generic_args, args)? {
                    if self.eval_expr_value(body, expr).is_none() {
                        return Err(skipped_failure(
                            ProviderSkipReason::UnsupportedProviderBody,
                            expr.span(body).into(),
                        ));
                    }
                }
            }
            Expr::Bin(_, _, _)
            | Expr::AugAssign(_, _, _)
            | Expr::Un(_, _)
            | Expr::Cast(_, _)
            | Expr::Field(_, _)
            | Expr::Tuple(_)
            | Expr::Array(_)
            | Expr::ArrayRep(_, _)
            | Expr::If(_, _, _)
            | Expr::Match(_, _)
            | Expr::RecordInit(_, _)
            | Expr::With(_, _) => {
                return Err(skipped_failure(
                    ProviderSkipReason::UnsupportedProviderBody,
                    expr.span(body).into(),
                ));
            }
            Expr::Lit(_) | Expr::Path(_) => {}
        }
        Ok(())
    }

    fn assign_value_binding(
        &mut self,
        body: Body<'db>,
        lhs: crate::hir_def::ExprId,
        value: ElabValue<'db>,
    ) -> bool {
        let Some(name) = simple_expr_path_ident(self.db, body, lhs) else {
            return false;
        };
        if let Some((_, binding)) = self
            .value_bindings
            .iter_mut()
            .rev()
            .find(|(candidate, _)| *candidate == name)
        {
            *binding = value;
            true
        } else {
            false
        }
    }

    fn execute_method_call(
        &mut self,
        body: Body<'db>,
        receiver: crate::hir_def::ExprId,
        method: Partial<IdentId<'db>>,
        generic_args: GenericArgListId<'db>,
        args: &[crate::hir_def::CallArg<'db>],
    ) -> Result<bool, ProviderExecutionFailure<'db>> {
        if !expr_is_path_named_any(self.db, body, receiver, &self.builder_names) {
            return Ok(false);
        }
        let Some(method) = method.to_opt() else {
            return Err(skipped_failure(
                ProviderSkipReason::UnsupportedProviderBody,
                receiver.span(body).into(),
            ));
        };
        match method.data(self.db).as_str() {
            BUILDER_REQUIRE_METHOD => {
                let [arg] = args else {
                    return Err(skipped_failure(
                        ProviderSkipReason::UnsupportedProviderBody,
                        receiver.span(body).into(),
                    ));
                };
                self.execute_require(body, generic_args, arg.expr)?;
                Ok(true)
            }
            BUILDER_FINISH_METHOD => {
                if !args.is_empty() {
                    return Err(skipped_failure(
                        ProviderSkipReason::UnsupportedProviderBody,
                        receiver.span(body).into(),
                    ));
                }
                self.builder.finish_explicit().map_err(|err| match err {
                    BuilderError::AlreadyFinished => skipped_failure(
                        ProviderSkipReason::DuplicateFinish,
                        receiver.span(body).into(),
                    ),
                    _ => ProviderExecutionFailure::Failed,
                })?;
                Ok(true)
            }
            BUILDER_BOOL_METHOD
            | BUILDER_AND_METHOD
            | BUILDER_SELF_REF_METHOD
            | BUILDER_ARG_REF_METHOD
            | BUILDER_FIELD_GET_METHOD
            | BUILDER_EQ_METHOD
            | BUILDER_DEFAULT_METHOD
            | BUILDER_STRUCT_INIT_METHOD
            | BUILDER_WITH_FIELD_METHOD => Ok(false),
            BUILDER_EMIT_METHOD => {
                let [arg] = args else {
                    return Err(skipped_failure(
                        ProviderSkipReason::UnsupportedProviderBody,
                        receiver.span(body).into(),
                    ));
                };
                self.execute_emit_method(body, arg.expr)?;
                Ok(true)
            }
            _ => Err(skipped_failure(
                ProviderSkipReason::UnsupportedProviderBody,
                receiver.span(body).into(),
            )),
        }
    }

    fn execute_require(
        &mut self,
        body: Body<'db>,
        generic_args: GenericArgListId<'db>,
        constraint_arg: crate::hir_def::ExprId,
    ) -> Result<(), ProviderExecutionFailure<'db>> {
        let Some(head) = self.resolve_requirement_head_generic_arg(generic_args) else {
            return Err(skipped_failure(
                ProviderSkipReason::InvalidBuilderRequirement,
                constraint_arg.span(body).into(),
            ));
        };
        let Some(ElabValue::TypeWitness(arg_ty)) = self.eval_expr_value(body, constraint_arg)
        else {
            return Err(skipped_failure(
                ProviderSkipReason::InvalidBuilderRequirement,
                constraint_arg.span(body).into(),
            ));
        };

        let constraint = head.apply(self.db, arg_ty);
        let origin = self
            .requirement_origin_for_expr(body, constraint_arg)
            .unwrap_or(RequirementOrigin::ProviderCode);
        self.builder
            .require_with_origin(constraint, origin)
            .map_err(|err| match err {
                BuilderError::AlreadyFinished => skipped_failure(
                    ProviderSkipReason::CommandAfterFinish,
                    constraint_arg.span(body).into(),
                ),
                _ => ProviderExecutionFailure::Failed,
            })
    }

    fn execute_emit_method(
        &mut self,
        body: Body<'db>,
        expr_arg: crate::hir_def::ExprId,
    ) -> Result<(), ProviderExecutionFailure<'db>> {
        let required = required_method_names(self.db, self.context.request(self.db).goal(self.db));
        let [method_name] = required.as_slice() else {
            return Err(skipped_failure(
                ProviderSkipReason::UnsupportedProviderBody,
                expr_arg.span(body).into(),
            ));
        };
        let Some(ElabValue::GeneratedExpr(expr)) = self.eval_expr_value(body, expr_arg) else {
            return Err(skipped_failure(
                ProviderSkipReason::UnsupportedProviderBody,
                expr_arg.span(body).into(),
            ));
        };
        self.builder
            .emit_method_expr(*method_name, expr)
            .map_err(|err| match err {
                BuilderError::AlreadyFinished => skipped_failure(
                    ProviderSkipReason::CommandAfterFinish,
                    expr_arg.span(body).into(),
                ),
                _ => ProviderExecutionFailure::Failed,
            })
    }

    fn reflect_fields_iterable(
        &self,
        body: Body<'db>,
        iterable: crate::hir_def::ExprId,
    ) -> Result<Option<Vec<ReflectedField<'db>>>, ProviderExecutionFailure<'db>> {
        let Partial::Present(Expr::MethodCall(receiver, method, _, args)) =
            iterable.data(self.db, body)
        else {
            return Ok(None);
        };
        if method
            .to_opt()
            .is_none_or(|method| method.data(self.db) != REFLECT_FIELDS_METHOD)
        {
            return Ok(None);
        }
        if !args.is_empty() {
            return Ok(None);
        }

        let target_ty = self.context.request(self.db).target(self.db).ty(self.db);
        if !self.env.has_reflect_target(self.db, target_ty)
            || !expr_is_path_named_any(self.db, body, *receiver, &self.reflect_names)
        {
            return Err(skipped_failure(
                ProviderSkipReason::MissingReflectCapability,
                iterable.span(body).into(),
            ));
        }
        Ok(Some(reflect_struct_fields(self.db, target_ty)))
    }

    fn field_value_for_expr(
        &self,
        body: Body<'db>,
        expr: crate::hir_def::ExprId,
    ) -> Option<ReflectedField<'db>> {
        match self.eval_expr_value(body, expr)? {
            ElabValue::Field(field) => Some(field),
            ElabValue::TypeWitness(_) | ElabValue::GeneratedExpr(_) => None,
        }
    }

    fn eval_expr_value(
        &self,
        body: Body<'db>,
        expr: crate::hir_def::ExprId,
    ) -> Option<ElabValue<'db>> {
        if let Some(field) = self.field_bindings.iter().rev().find_map(|(name, field)| {
            expr_is_path_named_any(self.db, body, expr, &[*name]).then_some(*field)
        }) {
            return Some(ElabValue::Field(field));
        }
        if let Some(value) = self.value_bindings.iter().rev().find_map(|(name, value)| {
            expr_is_path_named_any(self.db, body, expr, &[*name]).then_some(*value)
        }) {
            return Some(value);
        }

        let Partial::Present(Expr::MethodCall(receiver, method, _, args)) =
            expr.data(self.db, body)
        else {
            return None;
        };
        let method = method.to_opt()?;
        match method.data(self.db).as_str() {
            BUILDER_BOOL_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                let [arg] = args.as_slice() else {
                    return None;
                };
                let Partial::Present(Expr::Lit(LitKind::Bool(value))) =
                    arg.expr.data(self.db, body)
                else {
                    return None;
                };
                Some(ElabValue::GeneratedExpr(GeneratedExprId::new(
                    self.db,
                    GeneratedExprKind::BoolLiteral(*value),
                )))
            }
            BUILDER_AND_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                let [lhs_arg, rhs_arg] = args.as_slice() else {
                    return None;
                };
                let ElabValue::GeneratedExpr(lhs) = self.eval_expr_value(body, lhs_arg.expr)?
                else {
                    return None;
                };
                let ElabValue::GeneratedExpr(rhs) = self.eval_expr_value(body, rhs_arg.expr)?
                else {
                    return None;
                };
                Some(ElabValue::GeneratedExpr(GeneratedExprId::new(
                    self.db,
                    GeneratedExprKind::BoolAnd { lhs, rhs },
                )))
            }
            BUILDER_SELF_REF_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                if !args.is_empty() {
                    return None;
                }
                Some(ElabValue::GeneratedExpr(GeneratedExprId::new(
                    self.db,
                    GeneratedExprKind::SelfRef {
                        ty: self.context.request(self.db).target(self.db).ty(self.db),
                    },
                )))
            }
            BUILDER_ARG_REF_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                let [arg] = args.as_slice() else {
                    return None;
                };
                let name = self.string_literal_ident_arg(body, arg.expr)?;
                let ty = self
                    .required_method_arg_ty(name)
                    .unwrap_or_else(|| self.context.request(self.db).target(self.db).ty(self.db));
                Some(ElabValue::GeneratedExpr(GeneratedExprId::new(
                    self.db,
                    GeneratedExprKind::MethodArgRef { name, ty },
                )))
            }
            BUILDER_FIELD_GET_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                let [base_arg, field_arg] = args.as_slice() else {
                    return None;
                };
                let ElabValue::GeneratedExpr(base) = self.eval_expr_value(body, base_arg.expr)?
                else {
                    return None;
                };
                let ElabValue::Field(field) = self.eval_expr_value(body, field_arg.expr)? else {
                    return None;
                };
                Some(ElabValue::GeneratedExpr(GeneratedExprId::new(
                    self.db,
                    GeneratedExprKind::FieldGet { base, field },
                )))
            }
            BUILDER_EQ_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                let [lhs_arg, rhs_arg] = args.as_slice() else {
                    return None;
                };
                let ElabValue::GeneratedExpr(lhs) = self.eval_expr_value(body, lhs_arg.expr)?
                else {
                    return None;
                };
                let ElabValue::GeneratedExpr(rhs) = self.eval_expr_value(body, rhs_arg.expr)?
                else {
                    return None;
                };
                Some(ElabValue::GeneratedExpr(GeneratedExprId::new(
                    self.db,
                    GeneratedExprKind::EqExpr { lhs, rhs },
                )))
            }
            BUILDER_DEFAULT_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                let [ty_arg] = args.as_slice() else {
                    return None;
                };
                let ElabValue::TypeWitness(ty) = self.eval_expr_value(body, ty_arg.expr)? else {
                    return None;
                };
                Some(ElabValue::GeneratedExpr(GeneratedExprId::new(
                    self.db,
                    GeneratedExprKind::DefaultCall { ty },
                )))
            }
            BUILDER_STRUCT_INIT_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                if !args.is_empty() {
                    return None;
                }
                let target = self.context.request(self.db).target(self.db).ty(self.db);
                Some(ElabValue::GeneratedExpr(GeneratedExprId::new(
                    self.db,
                    GeneratedExprKind::StructInit {
                        target,
                        fields: GeneratedStructFieldInitListId::new(self.db, Vec::new()),
                    },
                )))
            }
            BUILDER_WITH_FIELD_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                let [init_arg, field_arg, value_arg] = args.as_slice() else {
                    return None;
                };
                let ElabValue::GeneratedExpr(init) = self.eval_expr_value(body, init_arg.expr)?
                else {
                    return None;
                };
                let GeneratedExprKind::StructInit { target, fields } = init.kind(self.db) else {
                    return None;
                };
                let ElabValue::Field(field) = self.eval_expr_value(body, field_arg.expr)? else {
                    return None;
                };
                let ElabValue::GeneratedExpr(value) = self.eval_expr_value(body, value_arg.expr)?
                else {
                    return None;
                };
                let mut field_inits = fields.list(self.db).to_vec();
                field_inits.push(GeneratedStructFieldInit { field, value });
                Some(ElabValue::GeneratedExpr(GeneratedExprId::new(
                    self.db,
                    GeneratedExprKind::StructInit {
                        target,
                        fields: GeneratedStructFieldInitListId::new(self.db, field_inits),
                    },
                )))
            }
            FIELD_TY_METHOD => {
                if !args.is_empty() {
                    return None;
                }
                let ElabValue::Field(field) = self.eval_expr_value(body, *receiver)? else {
                    return None;
                };
                Some(ElabValue::TypeWitness(field.ty))
            }
            _ => None,
        }
    }

    fn requirement_origin_for_expr(
        &self,
        body: Body<'db>,
        expr: crate::hir_def::ExprId,
    ) -> Option<RequirementOrigin<'db>> {
        let Partial::Present(Expr::MethodCall(receiver, method, _, args)) =
            expr.data(self.db, body)
        else {
            return None;
        };
        if !args.is_empty()
            || method
                .to_opt()
                .is_none_or(|method| method.data(self.db) != FIELD_TY_METHOD)
        {
            return None;
        }
        self.field_value_for_expr(body, *receiver)
            .map(RequirementOrigin::ReflectedField)
    }

    fn resolve_requirement_head_generic_arg(
        &self,
        generic_args: GenericArgListId<'db>,
    ) -> Option<BuilderRequirementHead<'db>> {
        let [GenericArg::Type(type_arg)] = generic_args.data(self.db).as_slice() else {
            return None;
        };
        let hir_ty = type_arg.ty.to_opt()?;
        let TypeKind::Path(path) = hir_ty.data(self.db) else {
            return None;
        };
        let path = path.to_opt()?;
        let assumptions = PredicateListId::empty_list(self.db);
        let scope = self.context.provider(self.db).func(self.db).scope();
        match resolve_path(self.db, path, scope, assumptions, false).ok()? {
            PathRes::Trait(inst) => Some(BuilderRequirementHead::ConcreteTrait(inst.def(self.db))),
            PathRes::Ty(ty) if is_unary_constraint_constructor_kind(&ty.kind(self.db)) => {
                Some(BuilderRequirementHead::GenericConstraint(ty))
            }
            _ => None,
        }
    }

    fn string_literal_ident_arg(
        &self,
        body: Body<'db>,
        expr: crate::hir_def::ExprId,
    ) -> Option<IdentId<'db>> {
        let Partial::Present(Expr::Lit(LitKind::String(value))) = expr.data(self.db, body) else {
            return None;
        };
        Some(IdentId::new(self.db, value.data(self.db).to_string()))
    }

    fn required_method_arg_ty(&self, name: IdentId<'db>) -> Option<TyId<'db>> {
        let ConstraintKind::Trait(trait_inst) =
            self.context.request(self.db).goal(self.db).kind(self.db)
        else {
            return None;
        };
        let required = required_methods(self.db, ConstraintId::from_trait(self.db, trait_inst));
        let mut methods = required.values().copied();
        let method = methods.next()?;
        if methods.next().is_some() {
            return None;
        }
        required_method_arg_ty_for_trait_inst(self.db, trait_inst, method, name)
    }
}

fn is_unary_constraint_constructor_kind(kind: &Kind) -> bool {
    match kind {
        Kind::Abs(inner) => {
            inner.0.does_match(&Kind::Star) && inner.1.does_match(&Kind::Constraint)
        }
        Kind::Any => true,
        _ => false,
    }
}

fn execute_provider_body<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
    env: CapabilityEnv<'db>,
) -> ProviderOutputStatus<'db> {
    ProviderBodyExecutor::new(db, context, env).execute_body()
}

#[cfg(test)]
#[allow(dead_code)]
fn stub_generated_impl_candidate_for_context<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
) -> Option<GeneratedImplId<'db>> {
    // This is still a typed overlay stub, not general CTFE execution. The
    // context must explicitly declare builder authority, and the only emitted
    // commands here are the derive-field obligations we can compute from typed
    // reflection.
    let goal = context.request(db).goal(db);
    if !context_has_impl_builder(db, context, goal) {
        return None;
    }
    if generated_stub_trait_has_required_methods(db, goal) {
        return None;
    }

    let mut builder = ImplBuilderSession::new(db, context);
    builder.emit_impl(db, goal).ok()?;
    for requirement in derive_requirements_for_context(db, context)? {
        builder
            .require_with_origin(requirement.constraint, requirement.origin)
            .ok()?;
    }
    builder.finish(db).ok()
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

#[cfg(test)]
#[allow(dead_code)]
fn derive_requirements_for_context<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
) -> Option<Vec<GeneratedRequirement<'db>>> {
    let request = context.request(db);
    let target_ty = request.target(db).ty(db);
    if !context_has_reflect_target(db, context, target_ty) {
        return None;
    }
    derive_requirements_for_reflected_target(db, context, target_ty)
}

#[cfg(test)]
#[allow(dead_code)]
fn derive_requirements_for_reflected_target<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
    target_ty: TyId<'db>,
) -> Option<Vec<GeneratedRequirement<'db>>> {
    let request = context.request(db);
    let request::ElaborationTarget::Struct(_) = request.target(db) else {
        return None;
    };
    let ConstraintKind::Trait(trait_inst) = request.goal(db).kind(db) else {
        return None;
    };

    let trait_ = trait_inst.def(db);
    Some(
        reflect_struct_fields(db, target_ty)
            .into_iter()
            .filter(|field| tys_match(db, field.parent, target_ty))
            .map(|field| derive_requirement_for_trait_field(db, trait_, field))
            .collect(),
    )
}

#[cfg(test)]
#[allow(dead_code)]
fn derive_requirement_for_trait_field<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_: Trait<'db>,
    field: ReflectedField<'db>,
) -> GeneratedRequirement<'db> {
    let constraint =
        ConstraintId::from_trait(db, TraitInstId::new_simple(db, trait_, vec![field.ty]));
    GeneratedRequirement {
        constraint,
        origin: RequirementOrigin::ReflectedField(field),
    }
}

const BUILDER_REQUIRE_METHOD: &str = "require";
const BUILDER_FINISH_METHOD: &str = "finish";
const BUILDER_BOOL_METHOD: &str = "bool";
const BUILDER_AND_METHOD: &str = "and";
const BUILDER_SELF_REF_METHOD: &str = "self_ref";
const BUILDER_ARG_REF_METHOD: &str = "arg_ref";
const BUILDER_FIELD_GET_METHOD: &str = "field_get";
const BUILDER_EQ_METHOD: &str = "eq";
const BUILDER_DEFAULT_METHOD: &str = "default";
const BUILDER_STRUCT_INIT_METHOD: &str = "struct_init";
const BUILDER_WITH_FIELD_METHOD: &str = "with_field";
const BUILDER_EMIT_METHOD: &str = "emit_method";
const REFLECT_FIELDS_METHOD: &str = "fields";
const FIELD_TY_METHOD: &str = "ty";

fn provider_impl_builder_effect_names<'db>(
    db: &'db dyn HirAnalysisDb,
    provider: EvidenceProviderId<'db>,
) -> Vec<IdentId<'db>> {
    provider
        .func(db)
        .effect_params(db)
        .filter(|param| param.is_mut(db))
        .filter_map(|param| {
            let name = param.name(db)?;
            let key_path = param.key_path(db)?;
            key_path
                .ident(db)
                .to_opt()
                .is_some_and(|ident| ident.data(db) == "ImplBuilder")
                .then_some(name)
        })
        .collect()
}

fn provider_reflect_effect_names<'db>(
    db: &'db dyn HirAnalysisDb,
    provider: EvidenceProviderId<'db>,
) -> Vec<IdentId<'db>> {
    provider
        .func(db)
        .effect_params(db)
        .filter_map(|param| {
            let name = param.name(db)?;
            let key_path = param.key_path(db)?;
            key_path
                .ident(db)
                .to_opt()
                .is_some_and(|ident| ident.data(db) == "Reflect")
                .then_some(name)
        })
        .collect()
}

fn expr_is_path_named_any<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    expr: crate::hir_def::ExprId,
    names: &[IdentId<'db>],
) -> bool {
    simple_expr_path_ident(db, body, expr).is_some_and(|ident| names.contains(&ident))
}

fn simple_expr_path_ident<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    expr: crate::hir_def::ExprId,
) -> Option<IdentId<'db>> {
    let Partial::Present(Expr::Path(path)) = expr.data(db, body) else {
        return None;
    };
    let Partial::Present(path) = path else {
        return None;
    };
    path.as_ident(db)
}

fn simple_pat_binding_name<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    pat: crate::hir_def::PatId,
) -> Option<IdentId<'db>> {
    let Partial::Present(Pat::Path(Partial::Present(path), _)) = pat.data(db, body) else {
        return None;
    };
    path.as_ident(db)
}

#[cfg(test)]
#[allow(dead_code)]
fn generated_stub_trait_has_required_methods<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: ConstraintId<'db>,
) -> bool {
    !required_method_names(db, goal).is_empty()
}

fn generated_missing_required_methods<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
) -> Vec<IdentId<'db>> {
    let provided = generated
        .methods
        .list(db)
        .iter()
        .map(|method| method.name)
        .collect::<IndexSet<_>>();
    required_method_names(db, ConstraintId::from_trait(db, generated.trait_inst))
        .into_iter()
        .filter(|name| !provided.contains(name))
        .collect()
}

fn generated_unsupported_required_methods<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
) -> Vec<IdentId<'db>> {
    let required = required_methods(db, ConstraintId::from_trait(db, generated.trait_inst));
    generated
        .methods
        .list(db)
        .iter()
        .filter_map(|method| {
            let required_method = required.get(&method.name)?;
            match method.body {
                GeneratedMethodBodyKind::Expr(expr) => {
                    let cx = GeneratedMethodValidationContext {
                        generated,
                        required_method: *required_method,
                    };
                    let expected = generated_required_method_return_ty(db, cx);
                    (!generated_expr_ty_matches(db, expr, expected, cx)).then_some(method.name)
                }
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
struct GeneratedMethodValidationContext<'db> {
    generated: GeneratedImplId<'db>,
    required_method: crate::hir_def::Func<'db>,
}

fn generated_required_method_return_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    cx: GeneratedMethodValidationContext<'db>,
) -> TyId<'db> {
    instantiate_required_method_ty(db, cx, cx.required_method.return_ty(db))
}

fn generated_required_method_param_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    cx: GeneratedMethodValidationContext<'db>,
    name: IdentId<'db>,
) -> Option<TyId<'db>> {
    required_method_arg_ty_for_trait_inst(db, cx.generated.trait_inst, cx.required_method, name)
}

fn required_method_arg_ty_for_trait_inst<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_inst: TraitInstId<'db>,
    required_method: crate::hir_def::Func<'db>,
    name: IdentId<'db>,
) -> Option<TyId<'db>> {
    let method = required_method.as_callable(db)?;
    let arg_tys = method.arg_tys(db);
    let trait_args = trait_inst_args_with_defaults(db, trait_inst);

    for (idx, param) in required_method.params(db).enumerate() {
        if param.is_self_param(db) {
            continue;
        }
        if param.name(db).is_some_and(|param_name| param_name == name) {
            let ty = arg_tys.get(idx).copied()?;
            let ty = ty.instantiate_identity();
            if ty_is_named_self(db, ty)
                && let Some(&target_ty) = trait_args.first()
            {
                return Some(target_ty);
            }
            return Some(instantiate_required_method_ty_for_trait_inst(
                db,
                trait_inst,
                required_method,
                ty,
            ));
        }
    }

    None
}

fn generated_method_target_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    cx: GeneratedMethodValidationContext<'db>,
) -> TyId<'db> {
    cx.generated.context.request(db).target(db).ty(db)
}

fn ty_is_named_self<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> bool {
    if let Some(inner) = ty.as_view(db) {
        return ty_is_named_self(db, inner);
    }
    if let Some((_, inner)) = ty.as_capability(db) {
        return ty_is_named_self(db, inner);
    }
    matches!(
        ty.base_ty(db).data(db),
        TyData::TyParam(param) if param.name.is_self_ty(db)
    )
}

fn instantiate_required_method_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    cx: GeneratedMethodValidationContext<'db>,
    ty: TyId<'db>,
) -> TyId<'db> {
    instantiate_required_method_ty_for_trait_inst(
        db,
        cx.generated.trait_inst,
        cx.required_method,
        ty,
    )
}

fn instantiate_required_method_ty_for_trait_inst<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_inst: TraitInstId<'db>,
    required_method: crate::hir_def::Func<'db>,
    ty: TyId<'db>,
) -> TyId<'db> {
    let trait_args = trait_inst_args_with_defaults(db, trait_inst);
    if ty_is_named_self(db, ty)
        && let Some(&target_ty) = trait_args.first()
    {
        return target_ty;
    }

    let mut mappings = Vec::new();
    let method = required_method.as_callable(db).unwrap();
    if let Some(self_ty) = required_method.expected_self_ty(db)
        && let Some(&target_ty) = trait_args.first()
    {
        mappings.push((self_ty, target_ty));
    }
    for (idx, &arg) in trait_args.iter().enumerate() {
        if let Some(&method_param) = method.params(db).get(idx) {
            mappings.push((method_param, arg));
        }
        if let Some(&trait_param) = trait_inst.def(db).params(db).get(idx) {
            mappings.push((trait_param, arg));
        }
    }
    Binder::bind(ty).instantiate_with(db, |ty| {
        mappings
            .iter()
            .find_map(|(param, arg)| (*param == ty).then_some(*arg))
            .unwrap_or(ty)
    })
}

fn trait_inst_args_with_defaults<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_inst: TraitInstId<'db>,
) -> Vec<TyId<'db>> {
    let args = trait_inst.args(db);
    if args.len() >= trait_inst.def(db).params(db).len() {
        return args.to_vec();
    }
    let Some((&self_ty, provided_explicit)) = args.split_first() else {
        return Vec::new();
    };
    let completed = trait_inst.def(db).param_set(db).complete_explicit_args(
        db,
        Some(self_ty),
        provided_explicit,
        PredicateListId::empty_list(db),
        ConstDefaultCompletion::evaluate(None),
    );
    let mut full_args = Vec::with_capacity(1 + completed.len());
    full_args.push(self_ty);
    full_args.extend(completed);
    full_args
}

fn generated_expr_ty_matches<'db>(
    db: &'db dyn HirAnalysisDb,
    expr: GeneratedExprId<'db>,
    expected: TyId<'db>,
    cx: GeneratedMethodValidationContext<'db>,
) -> bool {
    generated_expr_static_ty(db, expr, cx).is_some_and(|ty| tys_match(db, ty, expected))
}

fn generated_expr_static_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    expr: GeneratedExprId<'db>,
    cx: GeneratedMethodValidationContext<'db>,
) -> Option<TyId<'db>> {
    match expr.kind(db) {
        GeneratedExprKind::BoolLiteral(_) => Some(TyId::bool(db)),
        GeneratedExprKind::BoolAnd { lhs, rhs } => {
            if generated_expr_ty_matches(db, lhs, TyId::bool(db), cx)
                && generated_expr_ty_matches(db, rhs, TyId::bool(db), cx)
            {
                Some(TyId::bool(db))
            } else {
                None
            }
        }
        GeneratedExprKind::SelfRef { ty } => {
            if !cx.required_method.is_method(db) {
                return None;
            }
            let self_ty = generated_method_target_ty(db, cx);
            tys_match(db, self_ty, ty).then_some(ty)
        }
        GeneratedExprKind::MethodArgRef { name, ty } => {
            let arg_ty = generated_required_method_param_ty(db, cx, name)?;
            tys_match(db, arg_ty, ty).then_some(ty)
        }
        GeneratedExprKind::FieldGet { base, field } => {
            let base_ty = generated_expr_static_ty(db, base, cx)?;
            let base_ty = field_access_base_ty(db, base_ty);
            tys_match(db, base_ty, field.parent).then_some(field.ty)
        }
        GeneratedExprKind::EqExpr { lhs, rhs } => {
            let lhs_ty = generated_expr_static_ty(db, lhs, cx)?;
            let rhs_ty = generated_expr_static_ty(db, rhs, cx)?;
            tys_match(db, lhs_ty, rhs_ty).then_some(TyId::bool(db))
        }
        GeneratedExprKind::DefaultCall { ty } => Some(ty),
        GeneratedExprKind::StructInit { target, fields } => {
            generated_struct_init_ty(db, target, fields, cx)
        }
    }
}

fn field_access_base_ty<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
    if let Some(inner) = ty.as_view(db) {
        return field_access_base_ty(db, inner);
    }
    if let Some((_, inner)) = ty.as_capability(db) {
        return field_access_base_ty(db, inner);
    }
    ty
}

fn generated_struct_init_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    target: TyId<'db>,
    fields: GeneratedStructFieldInitListId<'db>,
    cx: GeneratedMethodValidationContext<'db>,
) -> Option<TyId<'db>> {
    let expected_fields = reflect_struct_fields(db, target);
    let field_inits = fields.list(db);
    if expected_fields.len() != field_inits.len() {
        return None;
    }

    for expected in expected_fields {
        let init = field_inits
            .iter()
            .find(|init| init.field.index == expected.index)?;
        if !tys_match(db, init.field.parent, target) || !tys_match(db, init.field.ty, expected.ty) {
            return None;
        }
        let value_ty = generated_expr_static_ty(db, init.value, cx)?;
        if !tys_match(db, value_ty, expected.ty) {
            return None;
        }
    }

    Some(target)
}

fn generated_method_error_summary<'db>(
    db: &'db dyn HirAnalysisDb,
    missing_methods: &[IdentId<'db>],
    unsupported_methods: &[IdentId<'db>],
) -> String {
    let mut parts = Vec::new();
    if !missing_methods.is_empty() {
        parts.push(format!(
            "missing {}",
            missing_methods
                .iter()
                .map(|name| name.data(db).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !unsupported_methods.is_empty() {
        parts.push(format!(
            "unsupported {}",
            unsupported_methods
                .iter()
                .map(|name| name.data(db).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    parts.join("; ")
}

fn required_method_names<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: ConstraintId<'db>,
) -> Vec<IdentId<'db>> {
    required_methods(db, goal).keys().copied().collect()
}

fn required_methods<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: ConstraintId<'db>,
) -> IndexMap<IdentId<'db>, crate::hir_def::Func<'db>> {
    let ConstraintKind::Trait(trait_inst) = goal.kind(db) else {
        return IndexMap::new();
    };
    trait_inst
        .def(db)
        .method_defs(db)
        .into_iter()
        .filter(|(_, method)| method.body(db).is_none())
        .collect()
}

fn generated_conflicts_with_authored_impl<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
) -> bool {
    let request = generated.context.request(db);
    let ingot = request.target(db).item().top_mod(db).ingot(db);
    let Some(authored_impls) = collect_trait_impls(db, ingot).get(&generated.trait_inst.def(db))
    else {
        return false;
    };
    let generated_impl = Binder::bind(ImplementorId::new(
        db,
        generated.trait_inst,
        generated.trait_inst.self_ty(db).generic_args(db).to_vec(),
        IndexMap::new(),
        ImplementorOrigin::Generated(generated),
    ));
    authored_impls
        .iter()
        .any(|&authored| does_impl_trait_conflict(db, authored, generated_impl))
}

fn generated_conflicts_with_generated_impl<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
    candidates: &[GeneratedImplId<'db>],
) -> bool {
    let generated_impl = Binder::bind(ImplementorId::new(
        db,
        generated.trait_inst,
        generated.trait_inst.self_ty(db).generic_args(db).to_vec(),
        IndexMap::new(),
        ImplementorOrigin::Generated(generated),
    ));
    candidates.iter().copied().any(|other| {
        if other == generated {
            return false;
        }
        let other_impl = Binder::bind(ImplementorId::new(
            db,
            other.trait_inst,
            other.trait_inst.self_ty(db).generic_args(db).to_vec(),
            IndexMap::new(),
            ImplementorOrigin::Generated(other),
        ));
        does_impl_trait_conflict(db, other_impl, generated_impl)
    })
}

#[cfg(test)]
#[allow(dead_code)]
fn context_has_impl_builder<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
    goal: ConstraintId<'db>,
) -> bool {
    context.capabilities(db).iter().any(|witness| {
        if witness.capability.mode(db) != CapabilityMode::Mut {
            return false;
        }
        match witness.capability.key(db) {
            EffectCapabilityKey::Compiler(CompilerCapabilityKind::ImplBuilder(capability_goal)) => {
                constraints_match(db, capability_goal, goal)
            }
            _ => false,
        }
    })
}

#[cfg(test)]
#[allow(dead_code)]
fn context_has_reflect_target<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
    target: TyId<'db>,
) -> bool {
    context.capabilities(db).iter().any(|witness| {
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

fn reflect_struct_fields<'db>(
    db: &'db dyn HirAnalysisDb,
    target: TyId<'db>,
) -> Vec<ReflectedField<'db>> {
    let Some(FieldParent::Struct(struct_)) = target.field_parent(db) else {
        return Vec::new();
    };

    let field_tys = target.field_types(db);
    FieldParent::Struct(struct_)
        .fields(db)
        .zip(field_tys)
        .filter_map(|(field, ty)| {
            Some(ReflectedField {
                parent: target,
                index: field.idx as u32,
                name: field.name(db)?,
                ty,
            })
        })
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

fn elaborate_provider_context<'db>(
    db: &'db dyn HirAnalysisDb,
    request: ElaborationRequestId<'db>,
    provider: EvidenceProviderId<'db>,
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

fn derive_evidence_for_goal<'db>(
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
    provider: EvidenceProviderId<'db>,
    capabilities: &[ConstraintId<'db>],
) -> Vec<ConstraintId<'db>> {
    let mut constraints = Vec::with_capacity(capabilities.len() + 2);
    constraints.push(provider.goal(db));
    constraints.push(provider.derive_goal(db));
    constraints.extend(capabilities.iter().copied());
    constraints
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
            GeneratedImplSource::StubDerivedFieldObligations,
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
            GeneratedImplSource::StubDerivedFieldObligations,
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
