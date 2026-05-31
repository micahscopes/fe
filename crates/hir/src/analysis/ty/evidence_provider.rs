use crate::{
    analysis::{
        HirAnalysisDb,
        name_resolution::{PathRes, resolve_path},
        ty::{
            constraint::{
                ConstraintHeadId, ConstraintHeadKind, ConstraintId, ConstraintKind,
                evidence_goal_for_ty,
            },
            diagnostics::{TyDiagCollection, TyLowerDiag},
            trait_resolution::PredicateListId,
        },
    },
    hir_def::{Attr, DeriveProvider, Func, HirIngot, IdentId, ItemKind, NormalAttr, Trait},
    span::DynLazySpan,
};
use common::ingot::Ingot;
use rustc_hash::FxHashSet;

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct EvidenceProviderIdentityId<'db> {
    pub(crate) name: IdentId<'db>,
    pub(crate) func: Func<'db>,
}

impl<'db> EvidenceProviderIdentityId<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        self.name(db).data(db).to_string()
    }
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct EvidenceProviderId<'db> {
    pub(crate) identity: EvidenceProviderIdentityId<'db>,
    pub(crate) func: Func<'db>,
    pub(crate) goal: ConstraintId<'db>,
    pub(crate) derive_goal: ConstraintId<'db>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum EvidenceProviderValidationResult<'db> {
    Valid(EvidenceProviderId<'db>),
    Invalid,
    NotProvider,
}

pub(crate) fn validate_attr_evidence_provider<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> (
    EvidenceProviderValidationResult<'db>,
    Vec<TyDiagCollection<'db>>,
) {
    let mut diags = Vec::new();
    let attrs = evidence_provider_attrs(db, func);
    if attrs.is_empty() {
        return (EvidenceProviderValidationResult::NotProvider, diags);
    }

    let attr_span: crate::span::DynLazySpan<'db> = func.span().attributes().into();
    diags.push(invalid_provider(
        attr_span,
        "`#[evidence_provider(...)]` is obsolete; use `impl Provider: Derive for Head { const fn derive(...) { ... } }`",
    ));
    (EvidenceProviderValidationResult::Invalid, diags)
}

pub(crate) fn validated_evidence_providers_for_ingot<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
) -> Vec<EvidenceProviderId<'db>> {
    ingot
        .all_derive_providers(db)
        .iter()
        .filter_map(
            |&provider| match validate_named_derive_provider(db, provider).0 {
                EvidenceProviderValidationResult::Valid(provider) => Some(provider),
                EvidenceProviderValidationResult::Invalid
                | EvidenceProviderValidationResult::NotProvider => None,
            },
        )
        .collect()
}

pub(crate) fn visible_evidence_providers_for_ingot<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
) -> Vec<EvidenceProviderId<'db>> {
    let mut providers = Vec::new();
    let mut visited = FxHashSet::default();
    collect_visible_evidence_providers(db, ingot, &mut visited, &mut providers);
    providers
}

fn collect_visible_evidence_providers<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
    visited: &mut FxHashSet<Ingot<'db>>,
    providers: &mut Vec<EvidenceProviderId<'db>>,
) {
    if !visited.insert(ingot) {
        return;
    }

    providers.extend(validated_evidence_providers_for_ingot(db, ingot));
    for &(_, dependency) in ingot.resolved_external_ingots(db) {
        collect_visible_evidence_providers(db, dependency, visited, providers);
    }
}

pub(crate) fn providers_for_derive_goal<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
    derive_goal: ConstraintId<'db>,
) -> Vec<EvidenceProviderId<'db>> {
    visible_evidence_providers_for_ingot(db, ingot)
        .into_iter()
        .filter(|provider| provider.derive_goal(db) == derive_goal)
        .collect()
}

fn evidence_provider_attrs<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> Vec<&'db NormalAttr<'db>> {
    let Some(attrs) = ItemKind::Func(func).attrs(db) else {
        return Vec::new();
    };

    attrs
        .data(db)
        .iter()
        .filter_map(|attr| {
            let Attr::Normal(normal_attr) = attr else {
                return None;
            };
            let is_provider_attr = normal_attr
                .path
                .to_opt()
                .and_then(|path| path.as_ident(db))
                .is_some_and(|ident| ident.data(db) == "evidence_provider");
            is_provider_attr.then_some(normal_attr)
        })
        .collect()
}

pub(crate) fn validate_named_derive_provider<'db>(
    db: &'db dyn HirAnalysisDb,
    provider: DeriveProvider<'db>,
) -> (
    EvidenceProviderValidationResult<'db>,
    Vec<TyDiagCollection<'db>>,
) {
    let mut diags = Vec::new();
    let span: DynLazySpan<'db> = provider.span().into();

    let name = match provider.name(db).to_opt() {
        Some(name) => Some(name),
        None => {
            diags.push(invalid_provider(
                span.clone(),
                "derive provider declarations must have a provider name",
            ));
            None
        }
    };

    let derives_derivation = provider
        .derive_path(db)
        .to_opt()
        .and_then(|path| path.as_ident(db))
        .is_some_and(|ident| ident.data(db) == "Derive");
    if !derives_derivation {
        diags.push(invalid_provider(
            provider.span().derive_path().into(),
            "derive provider declarations must use `Derive` after `:`",
        ));
    }

    let head = match provider.head_path(db).to_opt() {
        Some(path) => match resolve_path(
            db,
            path,
            provider.scope(),
            PredicateListId::empty_list(db),
            false,
        ) {
            Ok(PathRes::Trait(inst)) => Some(inst.def(db)),
            _ => {
                diags.push(invalid_provider(
                    provider.span().head_path().into(),
                    "derive provider head must resolve to a trait",
                ));
                None
            }
        },
        None => {
            diags.push(invalid_provider(
                span.clone(),
                "derive provider declarations must specify a trait head after `for`",
            ));
            None
        }
    };

    let derive_methods: Vec<_> = provider
        .methods(db)
        .filter(|func| {
            func.name(db)
                .to_opt()
                .is_some_and(|name| name.data(db) == "derive")
        })
        .collect();
    let func = match derive_methods.as_slice() {
        [func] => Some(*func),
        [] => {
            diags.push(invalid_provider(
                provider.span().item_list().into(),
                "derive provider declarations must contain one `derive` function",
            ));
            None
        }
        _ => {
            diags.push(invalid_provider(
                provider.span().item_list().into(),
                "derive provider declarations may contain only one `derive` function",
            ));
            None
        }
    };

    let Some(func) = func else {
        return (EvidenceProviderValidationResult::Invalid, diags);
    };

    let provider = validate_provider_function(db, func, head, name, &mut diags, "derive provider");

    if diags.is_empty() {
        let provider = provider.expect("validated provider");
        (EvidenceProviderValidationResult::Valid(provider), diags)
    } else {
        (EvidenceProviderValidationResult::Invalid, diags)
    }
}

fn validate_provider_function<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
    head: Option<Trait<'db>>,
    explicit_name: Option<IdentId<'db>>,
    diags: &mut Vec<TyDiagCollection<'db>>,
    label: &'static str,
) -> Option<EvidenceProviderId<'db>> {
    if !func.is_const(db) {
        diags.push(invalid_provider(
            func.span().name().into(),
            format!("{label} functions must be `const fn`"),
        ));
    }

    let goal = match evidence_goal_for_ty(db, func.return_ty(db)) {
        Some(goal) => Some(goal),
        None => {
            diags.push(invalid_provider(
                func.span().ret_ty().into(),
                format!("{label} functions must return `Evidence<C>`"),
            ));
            None
        }
    };

    if let (Some(head), Some(goal)) = (head, goal) {
        match goal.kind(db) {
            ConstraintKind::Trait(inst) if inst.def(db) == head => {}
            ConstraintKind::Trait(_) => diags.push(invalid_provider(
                func.span().ret_ty().into(),
                "returned evidence constraint does not match the provider head",
            )),
            _ => diags.push(invalid_provider(
                func.span().ret_ty().into(),
                format!("{label}s currently support concrete trait evidence only"),
            )),
        }
    }

    if !diags.is_empty() {
        return None;
    }

    let name = explicit_name.unwrap_or_else(|| {
        func.name(db)
            .to_opt()
            .unwrap_or_else(|| IdentId::new(db, "<anonymous provider>".to_string()))
    });
    let head = head?;
    let goal = goal?;
    let identity = EvidenceProviderIdentityId::new(db, name, func);
    let derive_head = ConstraintHeadId::new(db, ConstraintHeadKind::ConcreteTrait(head));
    let derive_goal = ConstraintId::new(db, ConstraintKind::Derive(derive_head));
    Some(EvidenceProviderId::new(
        db,
        identity,
        func,
        goal,
        derive_goal,
    ))
}

fn invalid_provider<'db>(
    span: crate::span::DynLazySpan<'db>,
    message: impl Into<String>,
) -> TyDiagCollection<'db> {
    TyLowerDiag::InvalidEvidenceProvider {
        span,
        message: message.into(),
    }
    .into()
}
