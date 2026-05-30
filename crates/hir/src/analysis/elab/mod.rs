use common::{indexmap::IndexMap, ingot::Ingot};

use crate::{
    analysis::{
        HirAnalysisDb,
        analysis_pass::ModuleAnalysisPass,
        diagnostics::DiagnosticVoucher,
        name_resolution::{PathRes, resolve_path},
        ty::{
            binder::Binder,
            constraint::{
                CapabilityMode, CompilerCapabilityKind, ConstraintId, ConstraintKind,
                ConstraintListId, EffectCapabilityKey, GeneratedImplId, GeneratedImplSource,
                GeneratedRequirement, GeneratedRequirementListId,
            },
            diagnostics::{TyDiagCollection, TyLowerDiag},
            evidence_provider::{
                EvidenceProviderId, providers_for_constraint_head,
                validated_evidence_providers_for_ingot,
            },
            fold::{TyFoldable, TyFolder},
            trait_def::{ImplementorId, ImplementorOrigin, TraitInstId, does_impl_trait_conflict},
            trait_lower::collect_trait_impls,
            trait_resolution::{
                PredicateListId, constraint::collect_func_effect_capability_constraints,
            },
            ty_def::{PrimTy, TyBase, TyData, TyId},
            unify::UnificationTable,
            visitor::{TyVisitable, TyVisitor},
        },
    },
    hir_def::{
        Attr, Body, Enum, Expr, FieldParent, HirIngot, IdentId, ItemKind, NormalAttr, Partial,
        Stmt, Struct, TopLevelMod, Trait,
    },
    span::DynLazySpan,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ElaborationTarget<'db> {
    Struct(Struct<'db>),
    Enum(Enum<'db>),
}

impl<'db> ElaborationTarget<'db> {
    fn from_item(item: ItemKind<'db>) -> Option<Self> {
        match item {
            ItemKind::Struct(struct_) => Some(Self::Struct(struct_)),
            ItemKind::Enum(enum_) => Some(Self::Enum(enum_)),
            _ => None,
        }
    }

    fn item(self) -> ItemKind<'db> {
        match self {
            Self::Struct(struct_) => ItemKind::Struct(struct_),
            Self::Enum(enum_) => ItemKind::Enum(enum_),
        }
    }

    fn scope(self) -> crate::hir_def::scope_graph::ScopeId<'db> {
        self.item().scope()
    }

    fn attrs(self, db: &'db dyn HirAnalysisDb) -> Option<crate::hir_def::AttrListId<'db>> {
        self.item().attrs(db)
    }

    fn attr_span(self) -> DynLazySpan<'db> {
        match self {
            Self::Struct(struct_) => struct_.span().attributes().into(),
            Self::Enum(enum_) => enum_.span().attributes().into(),
        }
    }

    fn ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        let adt = match self {
            Self::Struct(struct_) => struct_.as_adt(db),
            Self::Enum(enum_) => enum_.as_adt(db),
        };
        let mut ty = TyId::adt(db, adt);
        for &param in adt.params(db) {
            ty = TyId::app(db, ty, param);
        }
        ty
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ElaborationOrigin {
    DeriveAttr { attr_index: u32, arg_index: u32 },
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ElaborationRequestId<'db> {
    target: ElaborationTarget<'db>,
    goal: ConstraintId<'db>,
    origin: ElaborationOrigin,
}

impl<'db> ElaborationRequestId<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        format!(
            "{} requested for {}",
            self.goal(db).pretty_print(db),
            self.target(db).ty(db).pretty_print(db)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
#[allow(dead_code)]
pub(crate) enum ElaborationError {
    ProviderExecutionNotImplemented,
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ElaborationOutputId<'db> {
    request: ElaborationRequestId<'db>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ElaborationCapabilityOrigin {
    ProviderUsesParam,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct ElaborationCapabilityWitness<'db> {
    capability: crate::analysis::ty::constraint::EffectCapabilityId<'db>,
    origin: ElaborationCapabilityOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct CapabilityEnv<'db> {
    witnesses: Vec<ElaborationCapabilityWitness<'db>>,
}

impl<'db> CapabilityEnv<'db> {
    fn from_context(db: &'db dyn HirAnalysisDb, context: ElaborationCtfeContextId<'db>) -> Self {
        Self {
            witnesses: context.capabilities(db).clone(),
        }
    }

    fn has_impl_builder(&self, db: &'db dyn HirAnalysisDb, goal: ConstraintId<'db>) -> bool {
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

    fn has_reflect_target(&self, db: &'db dyn HirAnalysisDb, target: TyId<'db>) -> bool {
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

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ElaborationCtfeContextId<'db> {
    request: ElaborationRequestId<'db>,
    provider: EvidenceProviderId<'db>,

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum GeneratedTraceFact<'db> {
    RequestedBy(ElaborationRequestId<'db>),
    GeneratedBy(ElaborationCtfeContextId<'db>),
    Source(GeneratedImplSource<'db>),
    ProvidesEvidence(ConstraintId<'db>),
    RequiresConstraint {
        constraint: ConstraintId<'db>,
        origin: RequirementOrigin<'db>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum BuilderCommand<'db> {
    Require {
        constraint: ConstraintId<'db>,
        origin: RequirementOrigin<'db>,
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
    UnsupportedProviderBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ProviderOutputStatus<'db> {
    Succeeded { commands: BuilderCommandListId<'db> },
    Failed,
    Skipped { reason: ProviderSkipReason },
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

    fn finish_commands(
        mut self,
        db: &'db dyn HirAnalysisDb,
    ) -> Result<BuilderCommandListId<'db>, BuilderError<'db>> {
        if self.finished {
            return Err(BuilderError::AlreadyFinished);
        }
        self.finished = true;
        self.commands.push(BuilderCommand::Finish);
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
        obligations: ConstraintListId::new(db, obligations),
    })
}

impl<'db> ElaborationCtfeContextId<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        let provider = self
            .provider(db)
            .func(db)
            .name(db)
            .to_opt()
            .map(|name| name.data(db).to_string())
            .unwrap_or_else(|| "<anonymous provider>".to_string());
        let capabilities = self
            .capabilities(db)
            .iter()
            .map(|witness| witness.capability.pretty_print(db))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{} via {} with [{}]",
            self.request(db).pretty_print(db),
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

impl<'db> GeneratedTraceFact<'db> {
    fn pretty_print(self, db: &'db dyn HirAnalysisDb, generated: GeneratedImplId<'db>) -> String {
        let prefix = generated.trait_inst.pretty_print(db, true);
        match self {
            Self::RequestedBy(request) => {
                format!(
                    "{prefix} requested by {}",
                    request.origin(db).pretty_print()
                )
            }
            Self::GeneratedBy(context) => {
                let provider = context
                    .provider(db)
                    .func(db)
                    .name(db)
                    .to_opt()
                    .map(|name| name.data(db).to_string())
                    .unwrap_or_else(|| "<anonymous provider>".to_string());
                format!("{prefix} generated by {provider}")
            }
            Self::Source(source) => format!(
                "{prefix} generated output source {}",
                source.pretty_print(db)
            ),
            Self::ProvidesEvidence(constraint) => {
                format!("{prefix} provides {}", constraint.pretty_print(db))
            }
            Self::RequiresConstraint { constraint, origin } => match origin {
                RequirementOrigin::ReflectedField(field) => format!(
                    "{prefix} requires {} from field {}.{}",
                    constraint.pretty_print(db),
                    field.parent.pretty_print(db),
                    field.name.data(db)
                ),
                RequirementOrigin::ProviderCode => {
                    format!(
                        "{prefix} requires {} from provider code",
                        constraint.pretty_print(db)
                    )
                }
                RequirementOrigin::Synthetic => {
                    format!(
                        "{prefix} requires {} from synthetic requirement",
                        constraint.pretty_print(db)
                    )
                }
            },
        }
    }
}

impl ElaborationOrigin {
    fn pretty_print(self) -> &'static str {
        match self {
            Self::DeriveAttr { .. } => "derive attribute",
        }
    }
}

#[allow(dead_code)]
pub(crate) fn elaborate_request<'db>(
    _db: &'db dyn HirAnalysisDb,
    _request: ElaborationRequestId<'db>,
) -> Result<ElaborationOutputId<'db>, ElaborationError> {
    Err(ElaborationError::ProviderExecutionNotImplemented)
}

#[salsa::tracked(return_ref)]
pub(crate) fn elaboration_requests_for_ingot<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
) -> Vec<ElaborationRequestId<'db>> {
    ingot
        .all_modules(db)
        .iter()
        .flat_map(|&top_mod| {
            elaboration_requests_for_top_mod(db, top_mod)
                .iter()
                .copied()
        })
        .collect()
}

#[salsa::tracked(return_ref)]
pub(crate) fn elaboration_requests_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<ElaborationRequestId<'db>> {
    top_mod
        .all_items(db)
        .iter()
        .filter_map(|&item| ElaborationTarget::from_item(item))
        .flat_map(|target| {
            derive_requests_for_target(db, target)
                .into_iter()
                .filter_map(|result| result.ok())
        })
        .collect()
}

#[salsa::tracked(return_ref)]
pub(crate) fn elaboration_request_diags_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<TyDiagCollection<'db>> {
    let mut diags: Vec<_> = top_mod
        .all_items(db)
        .iter()
        .filter_map(|&item| ElaborationTarget::from_item(item))
        .flat_map(|target| {
            derive_requests_for_target(db, target)
                .into_iter()
                .filter_map(|result| result.err())
        })
        .collect();
    diags.extend(duplicate_evidence_provider_diags_for_top_mod(db, top_mod));
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

fn duplicate_evidence_provider_diags_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<TyDiagCollection<'db>> {
    let providers = validated_evidence_providers_for_ingot(db, top_mod.ingot(db));
    let mut by_head: IndexMap<Trait<'db>, Vec<EvidenceProviderId<'db>>> = IndexMap::new();
    for provider in providers {
        by_head.entry(provider.head(db)).or_default().push(provider);
    }

    by_head
        .into_iter()
        .filter_map(|(head, providers)| {
            if providers.len() <= 1 {
                return None;
            }
            let span = providers[0].func(db).span().attributes().into();
            Some(invalid_request(
                span,
                format!(
                    "multiple evidence providers for `{}` are not supported yet",
                    trait_name(db, head)
                ),
            ))
        })
        .collect()
}

fn generated_overlay_diags_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<TyDiagCollection<'db>> {
    elaboration_requests_for_top_mod(db, top_mod)
        .iter()
        .flat_map(|&request| {
            elaboration_ctfe_contexts_for_request(db, request)
                .into_iter()
                .filter_map(move |context| {
                    let goal = request.goal(db);
                    if provider_output_trait_has_required_methods(db, goal) {
                        return Some(invalid_request(
                            request.target(db).attr_span(),
                            format!(
                                "provider output for `{}` does not generate required methods yet",
                                trait_name(db, concrete_trait_head(db, goal)?)
                            ),
                        ));
                    }
                    let generated = provider_generated_impl_candidate_for_context(db, *context)?;
                    generated_conflicts_with_authored_impl(db, generated).then(|| {
                        invalid_request(
                            request.target(db).attr_span(),
                            format!(
                                "generated implementation for `{}` conflicts with an authored implementation",
                                generated.trait_inst.pretty_print(db, true)
                            ),
                        )
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
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
    let Some(head) = concrete_trait_head(db, request.goal(db)) else {
        return Vec::new();
    };
    let ingot = request.target(db).item().top_mod(db).ingot(db);
    let providers = providers_for_constraint_head(db, ingot, head);
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
    elaboration_requests_for_ingot(db, ingot)
        .iter()
        .flat_map(|&request| {
            elaboration_ctfe_contexts_for_request(db, request)
                .iter()
                .filter_map(move |&context| provider_generated_impl_for_context(db, context))
                .collect::<Vec<_>>()
        })
        .collect()
}

pub fn generated_impl_summaries_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<String> {
    generated_impls_for_ingot(db, top_mod.ingot(db))
        .iter()
        .filter(|generated| generated.context.request(db).target(db).item().top_mod(db) == top_mod)
        .map(|generated| {
            format!(
                "generated {} {} with obligations {}",
                generated.source.pretty_print(db),
                generated.trait_inst.pretty_print(db, true),
                generated.obligations.pretty_print(db),
            )
        })
        .collect()
}

pub fn generated_trace_summaries_for_top_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<String> {
    generated_impls_for_ingot(db, top_mod.ingot(db))
        .iter()
        .filter(|generated| generated.context.request(db).target(db).item().top_mod(db) == top_mod)
        .flat_map(|&generated| {
            generated_trace_facts(db, generated)
                .into_iter()
                .map(move |fact| fact.pretty_print(db, generated))
        })
        .collect()
}

fn provider_generated_impl_for_context<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
) -> Option<GeneratedImplId<'db>> {
    let generated = provider_generated_impl_candidate_for_context(db, context)?;
    (!generated_conflicts_with_authored_impl(db, generated)).then_some(generated)
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
    let ProviderOutputStatus::Succeeded { commands } = output.status(db) else {
        return None;
    };
    if provider_output_trait_has_required_methods(db, output.request(db).goal(db)) {
        return None;
    }
    generated_impl_from_builder_commands(
        db,
        output.context(db),
        GeneratedImplSource::ProviderOutput(output),
        commands,
    )
    .ok()
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
            ProviderOutputStatus::Skipped {
                reason: ProviderSkipReason::MissingBuilderCapability,
            },
        );
    }

    let mut builder = ImplBuilderSession::new(db, context);
    if builder.emit_impl(db, goal).is_err() {
        return ProviderOutputId::new(db, request, provider, context, ProviderOutputStatus::Failed);
    }

    match provider_body_finish_count(db, provider) {
        0 => {
            return ProviderOutputId::new(
                db,
                request,
                provider,
                context,
                ProviderOutputStatus::Skipped {
                    reason: ProviderSkipReason::MissingFinish,
                },
            );
        }
        1 => {}
        _ => {
            return ProviderOutputId::new(
                db,
                request,
                provider,
                context,
                ProviderOutputStatus::Failed,
            );
        }
    }

    if provider_body_requests_field_obligations(db, provider) {
        let target_ty = request.target(db).ty(db);
        if !env.has_reflect_target(db, target_ty) {
            return ProviderOutputId::new(
                db,
                request,
                provider,
                context,
                ProviderOutputStatus::Skipped {
                    reason: ProviderSkipReason::MissingReflectCapability,
                },
            );
        }
        let Some(requirements) = derive_requirements_for_context(db, context) else {
            return ProviderOutputId::new(
                db,
                request,
                provider,
                context,
                ProviderOutputStatus::Skipped {
                    reason: ProviderSkipReason::UnsupportedProviderBody,
                },
            );
        };
        for requirement in requirements {
            if builder
                .require_with_origin(requirement.constraint, requirement.origin)
                .is_err()
            {
                return ProviderOutputId::new(
                    db,
                    request,
                    provider,
                    context,
                    ProviderOutputStatus::Failed,
                );
            }
        }
    }

    let status = match builder.finish_commands(db) {
        Ok(commands) => ProviderOutputStatus::Succeeded { commands },
        Err(_) => ProviderOutputStatus::Failed,
    };
    ProviderOutputId::new(db, request, provider, context, status)
}

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

fn generated_trace_facts<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
) -> Vec<GeneratedTraceFact<'db>> {
    let mut facts = vec![
        GeneratedTraceFact::RequestedBy(generated.context.request(db)),
        GeneratedTraceFact::GeneratedBy(generated.context),
        GeneratedTraceFact::Source(generated.source),
        GeneratedTraceFact::ProvidesEvidence(ConstraintId::from_trait(db, generated.trait_inst)),
    ];
    facts.extend(generated.requirements.list(db).iter().map(|requirement| {
        GeneratedTraceFact::RequiresConstraint {
            constraint: requirement.constraint,
            origin: requirement.origin,
        }
    }));
    facts
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

fn derive_requirements_for_context<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
) -> Option<Vec<GeneratedRequirement<'db>>> {
    let request = context.request(db);
    let ElaborationTarget::Struct(_) = request.target(db) else {
        return None;
    };
    let ConstraintKind::Trait(trait_inst) = request.goal(db).kind(db) else {
        return None;
    };
    let target_ty = request.target(db).ty(db);
    if !context_has_reflect_target(db, context, target_ty) {
        return None;
    }

    let trait_ = trait_inst.def(db);
    Some(
        reflected_fields_for_context(db, context)
            .into_iter()
            .filter(|field| tys_match(db, field.parent, target_ty))
            .map(|field| {
                let constraint = ConstraintId::from_trait(
                    db,
                    TraitInstId::new_simple(db, trait_, vec![field.ty]),
                );
                GeneratedRequirement {
                    constraint,
                    origin: RequirementOrigin::ReflectedField(field),
                }
            })
            .collect(),
    )
}

const BUILDER_FIELD_OBLIGATION_METHOD: &str = "require_fields";
const BUILDER_FINISH_METHOD: &str = "finish";

fn provider_body_requests_field_obligations<'db>(
    db: &'db dyn HirAnalysisDb,
    provider: EvidenceProviderId<'db>,
) -> bool {
    provider_body_builder_method_count(db, provider, BUILDER_FIELD_OBLIGATION_METHOD, Some(&[])) > 0
}

fn provider_body_finish_count<'db>(
    db: &'db dyn HirAnalysisDb,
    provider: EvidenceProviderId<'db>,
) -> usize {
    provider_body_builder_method_count(db, provider, BUILDER_FINISH_METHOD, None)
}

fn provider_body_builder_method_count<'db>(
    db: &'db dyn HirAnalysisDb,
    provider: EvidenceProviderId<'db>,
    method_name: &str,
    required_arg_names: Option<&[IdentId<'db>]>,
) -> usize {
    let Some(body) = provider.func(db).body(db) else {
        return 0;
    };
    let builder_names = provider_impl_builder_effect_names(db, provider);
    if builder_names.is_empty() {
        return 0;
    }
    count_builder_method_calls_in_expr(
        db,
        body,
        body.expr(db),
        &builder_names,
        method_name,
        required_arg_names,
    )
}

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

fn expr_is_path_named_any<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    expr: crate::hir_def::ExprId,
    names: &[IdentId<'db>],
) -> bool {
    let Partial::Present(Expr::Path(path)) = expr.data(db, body) else {
        return false;
    };
    let Partial::Present(path) = path else {
        return false;
    };
    path.ident(db)
        .to_opt()
        .is_some_and(|ident| names.contains(&ident))
}

fn count_builder_method_calls_in_expr<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    expr: crate::hir_def::ExprId,
    builder_names: &[IdentId<'db>],
    method_name: &str,
    required_arg_names: Option<&[IdentId<'db>]>,
) -> usize {
    let Partial::Present(expr_data) = expr.data(db, body) else {
        return 0;
    };

    match expr_data {
        Expr::Block(stmts) => stmts
            .iter()
            .map(|stmt| {
                count_builder_method_calls_in_stmt(
                    db,
                    body,
                    *stmt,
                    builder_names,
                    method_name,
                    required_arg_names,
                )
            })
            .sum::<usize>(),
        Expr::Call(callee, args) => {
            count_builder_method_calls_in_expr(
                db,
                body,
                *callee,
                builder_names,
                method_name,
                required_arg_names,
            ) + args
                .iter()
                .map(|arg| {
                    count_builder_method_calls_in_expr(
                        db,
                        body,
                        arg.expr,
                        builder_names,
                        method_name,
                        required_arg_names,
                    )
                })
                .sum::<usize>()
        }
        Expr::MethodCall(receiver, method, _, args) => {
            usize::from(
                method
                    .to_opt()
                    .is_some_and(|method| method.data(db) == method_name)
                    && builder_method_args_match(db, body, args, required_arg_names)
                    && expr_is_path_named_any(db, body, *receiver, builder_names),
            ) + count_builder_method_calls_in_expr(
                db,
                body,
                *receiver,
                builder_names,
                method_name,
                required_arg_names,
            ) + args
                .iter()
                .map(|arg| {
                    count_builder_method_calls_in_expr(
                        db,
                        body,
                        arg.expr,
                        builder_names,
                        method_name,
                        required_arg_names,
                    )
                })
                .sum::<usize>()
        }
        Expr::Bin(lhs, rhs, _) | Expr::Assign(lhs, rhs) | Expr::AugAssign(lhs, rhs, _) => {
            count_builder_method_calls_in_expr(
                db,
                body,
                *lhs,
                builder_names,
                method_name,
                required_arg_names,
            ) + count_builder_method_calls_in_expr(
                db,
                body,
                *rhs,
                builder_names,
                method_name,
                required_arg_names,
            )
        }
        Expr::Un(inner, _) | Expr::Cast(inner, _) | Expr::Field(inner, _) => {
            count_builder_method_calls_in_expr(
                db,
                body,
                *inner,
                builder_names,
                method_name,
                required_arg_names,
            )
        }
        Expr::Tuple(items) | Expr::Array(items) => items
            .iter()
            .map(|item| {
                count_builder_method_calls_in_expr(
                    db,
                    body,
                    *item,
                    builder_names,
                    method_name,
                    required_arg_names,
                )
            })
            .sum(),
        Expr::ArrayRep(value, _) => count_builder_method_calls_in_expr(
            db,
            body,
            *value,
            builder_names,
            method_name,
            required_arg_names,
        ),
        Expr::If(_, then_expr, else_expr) => {
            count_builder_method_calls_in_expr(
                db,
                body,
                *then_expr,
                builder_names,
                method_name,
                required_arg_names,
            ) + else_expr
                .map(|else_expr| {
                    count_builder_method_calls_in_expr(
                        db,
                        body,
                        else_expr,
                        builder_names,
                        method_name,
                        required_arg_names,
                    )
                })
                .unwrap_or_default()
        }
        Expr::Match(scrutinee, arms) => {
            count_builder_method_calls_in_expr(
                db,
                body,
                *scrutinee,
                builder_names,
                method_name,
                required_arg_names,
            ) + match arms {
                Partial::Present(arms) => arms
                    .iter()
                    .map(|arm| {
                        count_builder_method_calls_in_expr(
                            db,
                            body,
                            arm.body,
                            builder_names,
                            method_name,
                            required_arg_names,
                        )
                    })
                    .sum(),
                Partial::Absent => 0,
            }
        }
        Expr::RecordInit(_, fields) => fields
            .iter()
            .map(|field| {
                count_builder_method_calls_in_expr(
                    db,
                    body,
                    field.expr,
                    builder_names,
                    method_name,
                    required_arg_names,
                )
            })
            .sum(),
        Expr::With(_, inner) => count_builder_method_calls_in_expr(
            db,
            body,
            *inner,
            builder_names,
            method_name,
            required_arg_names,
        ),
        Expr::Lit(_) | Expr::Path(_) => 0,
    }
}

fn count_builder_method_calls_in_stmt<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    stmt: crate::hir_def::StmtId,
    builder_names: &[IdentId<'db>],
    method_name: &str,
    required_arg_names: Option<&[IdentId<'db>]>,
) -> usize {
    let Partial::Present(stmt_data) = stmt.data(db, body) else {
        return 0;
    };
    match stmt_data {
        Stmt::Let(_, _, init) => init
            .map(|init| {
                count_builder_method_calls_in_expr(
                    db,
                    body,
                    init,
                    builder_names,
                    method_name,
                    required_arg_names,
                )
            })
            .unwrap_or_default(),
        Stmt::For(_, iterable, loop_body, _) => {
            count_builder_method_calls_in_expr(
                db,
                body,
                *iterable,
                builder_names,
                method_name,
                required_arg_names,
            ) + count_builder_method_calls_in_expr(
                db,
                body,
                *loop_body,
                builder_names,
                method_name,
                required_arg_names,
            )
        }
        Stmt::While(_, loop_body) => count_builder_method_calls_in_expr(
            db,
            body,
            *loop_body,
            builder_names,
            method_name,
            required_arg_names,
        ),
        Stmt::Return(expr) => expr
            .map(|expr| {
                count_builder_method_calls_in_expr(
                    db,
                    body,
                    expr,
                    builder_names,
                    method_name,
                    required_arg_names,
                )
            })
            .unwrap_or_default(),
        Stmt::Expr(expr) => count_builder_method_calls_in_expr(
            db,
            body,
            *expr,
            builder_names,
            method_name,
            required_arg_names,
        ),
        Stmt::Continue | Stmt::Break => 0,
    }
}

fn builder_method_args_match<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    args: &[crate::hir_def::CallArg<'db>],
    required_arg_names: Option<&[IdentId<'db>]>,
) -> bool {
    match required_arg_names {
        None => args.is_empty(),
        Some(names) => {
            let [arg] = args else {
                return false;
            };
            if names.is_empty() {
                return true;
            }
            expr_is_path_named_any(db, body, arg.expr, names)
        }
    }
}

fn generated_stub_trait_has_required_methods<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: ConstraintId<'db>,
) -> bool {
    trait_has_required_methods(db, goal)
}

fn provider_output_trait_has_required_methods<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: ConstraintId<'db>,
) -> bool {
    trait_has_required_methods(db, goal)
}

fn trait_has_required_methods<'db>(db: &'db dyn HirAnalysisDb, goal: ConstraintId<'db>) -> bool {
    let ConstraintKind::Trait(trait_inst) = goal.kind(db) else {
        return false;
    };
    trait_inst
        .def(db)
        .method_defs(db)
        .values()
        .any(|method| method.body(db).is_none())
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

fn constraints_match<'db>(
    db: &'db dyn HirAnalysisDb,
    lhs: ConstraintId<'db>,
    rhs: ConstraintId<'db>,
) -> bool {
    let mut table = UnificationTable::new(db);
    table.unify(lhs, rhs).is_ok()
}

fn tys_match<'db>(db: &'db dyn HirAnalysisDb, lhs: TyId<'db>, rhs: TyId<'db>) -> bool {
    let mut table = UnificationTable::new(db);
    table.unify(lhs, rhs).is_ok()
}

fn derive_requests_for_target<'db>(
    db: &'db dyn HirAnalysisDb,
    target: ElaborationTarget<'db>,
) -> Vec<Result<ElaborationRequestId<'db>, TyDiagCollection<'db>>> {
    derive_attrs(db, target)
        .into_iter()
        .flat_map(|(attr_index, attr)| derive_requests_for_attr(db, target, attr_index, attr))
        .collect()
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

    let capabilities: Vec<ElaborationCapabilityWitness<'db>> = instantiated
        .drain(1..)
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
        capabilities,
    ))
}

fn provider_goal_and_capabilities<'db>(
    db: &'db dyn HirAnalysisDb,
    provider: EvidenceProviderId<'db>,
    capabilities: &[ConstraintId<'db>],
) -> Vec<ConstraintId<'db>> {
    let mut constraints = Vec::with_capacity(capabilities.len() + 1);
    constraints.push(provider.goal(db));
    constraints.extend(capabilities.iter().copied());
    constraints
}

fn derive_attrs<'db>(
    db: &'db dyn HirAnalysisDb,
    target: ElaborationTarget<'db>,
) -> Vec<(usize, &'db NormalAttr<'db>)> {
    let Some(attrs) = target.attrs(db) else {
        return Vec::new();
    };

    attrs
        .data(db)
        .iter()
        .enumerate()
        .filter_map(|(idx, attr)| {
            let Attr::Normal(normal_attr) = attr else {
                return None;
            };
            let is_derive = normal_attr
                .path
                .to_opt()
                .and_then(|path| path.as_ident(db))
                .is_some_and(|ident| ident.data(db) == "derive");
            is_derive.then_some((idx, normal_attr))
        })
        .collect()
}

fn derive_requests_for_attr<'db>(
    db: &'db dyn HirAnalysisDb,
    target: ElaborationTarget<'db>,
    attr_index: usize,
    attr: &NormalAttr<'db>,
) -> Vec<Result<ElaborationRequestId<'db>, TyDiagCollection<'db>>> {
    if attr.has_value || !attr.has_args || attr.args.is_empty() {
        return vec![Err(invalid_request(
            target.attr_span(),
            "expected `#[derive(TraitName)]`",
        ))];
    }

    attr.args
        .iter()
        .enumerate()
        .map(|(arg_index, arg)| {
            if arg.has_value || arg.value.is_some() {
                return Err(invalid_request(
                    target.attr_span(),
                    "derive arguments must be trait paths",
                ));
            }
            let Some(path) = arg.key.to_opt() else {
                return Err(invalid_request(
                    target.attr_span(),
                    "derive arguments must be trait paths",
                ));
            };
            let trait_ = resolve_derive_trait(db, target, path)?;
            let goal = derive_goal(db, target, trait_);
            Ok(ElaborationRequestId::new(
                db,
                target,
                goal,
                ElaborationOrigin::DeriveAttr {
                    attr_index: attr_index as u32,
                    arg_index: arg_index as u32,
                },
            ))
        })
        .collect()
}

fn resolve_derive_trait<'db>(
    db: &'db dyn HirAnalysisDb,
    target: ElaborationTarget<'db>,
    path: crate::hir_def::PathId<'db>,
) -> Result<Trait<'db>, TyDiagCollection<'db>> {
    let assumptions = PredicateListId::empty_list(db);
    match resolve_path(db, path, target.scope(), assumptions, false) {
        Ok(PathRes::Trait(inst)) => Ok(inst.def(db)),
        Ok(res) => Err(invalid_request(
            target.attr_span(),
            format!(
                "derive head must resolve to a trait, but resolved to {}",
                res.kind_name()
            ),
        )),
        Err(_) => Err(invalid_request(
            target.attr_span(),
            "derive head must resolve to a trait",
        )),
    }
}

fn derive_goal<'db>(
    db: &'db dyn HirAnalysisDb,
    target: ElaborationTarget<'db>,
    trait_: Trait<'db>,
) -> ConstraintId<'db> {
    let target_ty = target.ty(db);
    ConstraintId::new(
        db,
        ConstraintKind::Trait(TraitInstId::new_simple(db, trait_, vec![target_ty])),
    )
}

fn invalid_request<'db>(
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
                reason: ProviderSkipReason::MissingFinish
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
    builder.require_fields(reflect)
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
                reason: ProviderSkipReason::MissingReflectCapability
            }
        ));
    }

    #[test]
    fn provider_output_ignores_malformed_builder_finish_call() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "provider_output_ignores_malformed_builder_finish_call.fe".into(),
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
                reason: ProviderSkipReason::MissingFinish
            }
        ));
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
        assert!(matches!(output.status(&db), ProviderOutputStatus::Failed));
    }
}
