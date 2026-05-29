use common::ingot::Ingot;

use crate::{
    analysis::{
        HirAnalysisDb,
        analysis_pass::ModuleAnalysisPass,
        diagnostics::DiagnosticVoucher,
        name_resolution::{PathRes, resolve_path},
        ty::{
            binder::Binder,
            constraint::{ConstraintId, ConstraintKind},
            diagnostics::{TyDiagCollection, TyLowerDiag},
            evidence_provider::{EvidenceProviderId, providers_for_constraint_head},
            fold::TyFoldable,
            trait_def::TraitInstId,
            trait_resolution::{
                PredicateListId, constraint::collect_func_effect_capability_constraints,
            },
            ty_def::TyId,
            unify::UnificationTable,
        },
    },
    hir_def::{Attr, Enum, HirIngot, ItemKind, NormalAttr, Struct, TopLevelMod, Trait},
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

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ElaborationCtfeContextId<'db> {
    request: ElaborationRequestId<'db>,
    provider: EvidenceProviderId<'db>,

    #[return_ref]
    capabilities: Vec<ElaborationCapabilityWitness<'db>>,
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
    top_mod
        .all_items(db)
        .iter()
        .filter_map(|&item| ElaborationTarget::from_item(item))
        .flat_map(|target| {
            derive_requests_for_target(db, target)
                .into_iter()
                .filter_map(|result| result.err())
        })
        .collect()
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

#[salsa::tracked(return_ref)]
pub(crate) fn elaboration_ctfe_contexts_for_request<'db>(
    db: &'db dyn HirAnalysisDb,
    request: ElaborationRequestId<'db>,
) -> Vec<ElaborationCtfeContextId<'db>> {
    let Some(head) = concrete_trait_head(db, request.goal(db)) else {
        return Vec::new();
    };
    let ingot = request.target(db).item().top_mod(db).ingot(db);
    providers_for_constraint_head(db, ingot, head)
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
