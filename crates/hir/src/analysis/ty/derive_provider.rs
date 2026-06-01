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
            ty_def::{PrimTy, TyBase, TyData, TyId},
        },
    },
    hir_def::{Attr, DeriveProvider, Func, HirIngot, IdentId, ItemKind, NormalAttr, Trait},
    span::DynLazySpan,
};
use common::ingot::Ingot;
use rustc_hash::FxHashSet;

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct DeriveProviderIdentityId<'db> {
    pub(crate) name: IdentId<'db>,
    pub(crate) func: Func<'db>,
}

impl<'db> DeriveProviderIdentityId<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        self.name(db).data(db).to_string()
    }
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct DeriveProviderId<'db> {
    pub(crate) identity: DeriveProviderIdentityId<'db>,
    pub(crate) func: Func<'db>,
    pub(crate) goal: ConstraintId<'db>,
    pub(crate) derive_goal: ConstraintId<'db>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum DeriveProviderValidationResult<'db> {
    Valid(DeriveProviderId<'db>),
    Invalid,
}

pub(crate) fn obsolete_attr_derive_provider_diags<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> Vec<TyDiagCollection<'db>> {
    let mut diags = Vec::new();
    let attrs = obsolete_attr_derive_provider_attrs(db, func);
    if attrs.is_empty() {
        return diags;
    }

    let attr_span: crate::span::DynLazySpan<'db> = func.span().attributes().into();
    diags.push(invalid_provider(
        attr_span,
        "`#[evidence_provider(...)]` is obsolete; use `impl Provider: Derive for Head { const fn derive(...) { ... } }`",
    ));
    diags
}

pub(crate) fn validated_derive_providers_for_ingot<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
) -> Vec<DeriveProviderId<'db>> {
    ingot
        .all_derive_providers(db)
        .iter()
        .filter_map(
            |&provider| match validate_named_derive_provider(db, provider).0 {
                DeriveProviderValidationResult::Valid(provider) => Some(provider),
                DeriveProviderValidationResult::Invalid => None,
            },
        )
        .collect()
}

pub(crate) fn visible_derive_providers_for_ingot<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
) -> Vec<DeriveProviderId<'db>> {
    let mut providers = Vec::new();
    let mut visited = FxHashSet::default();
    collect_visible_derive_providers(db, ingot, &mut visited, &mut providers);
    providers
}

fn collect_visible_derive_providers<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
    visited: &mut FxHashSet<Ingot<'db>>,
    providers: &mut Vec<DeriveProviderId<'db>>,
) {
    if !visited.insert(ingot) {
        return;
    }

    providers.extend(validated_derive_providers_for_ingot(db, ingot));
    for &(_, dependency) in ingot.resolved_external_ingots(db) {
        collect_visible_derive_providers(db, dependency, visited, providers);
    }
}

pub(crate) fn providers_for_derive_goal<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
    derive_goal: ConstraintId<'db>,
) -> Vec<DeriveProviderId<'db>> {
    visible_derive_providers_for_ingot(db, ingot)
        .into_iter()
        .filter(|provider| provider.derive_goal(db) == derive_goal)
        .collect()
}

fn obsolete_attr_derive_provider_attrs<'db>(
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
    DeriveProviderValidationResult<'db>,
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

    let derives_derivation = provider.derive_path(db).to_opt().is_some_and(|path| {
        matches!(
            resolve_path(
                db,
                path,
                provider.scope(),
                PredicateListId::empty_list(db),
                false
            ),
            Ok(PathRes::Ty(ty))
                if matches!(ty.data(db), TyData::TyBase(TyBase::Prim(PrimTy::Derive)))
        )
    });
    if !derives_derivation {
        diags.push(invalid_provider(
            provider.span().derive_path().into(),
            "derive provider declarations must use built-in `Derive` after `:`",
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

    let (Some(func), Some(name)) = (func, name) else {
        return (DeriveProviderValidationResult::Invalid, diags);
    };

    let provider = validate_provider_function(db, func, head, name, &mut diags);

    if diags.is_empty() {
        let provider = provider.expect("validated provider");
        (DeriveProviderValidationResult::Valid(provider), diags)
    } else {
        (DeriveProviderValidationResult::Invalid, diags)
    }
}

fn validate_provider_function<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
    head: Option<Trait<'db>>,
    name: IdentId<'db>,
    diags: &mut Vec<TyDiagCollection<'db>>,
) -> Option<DeriveProviderId<'db>> {
    if !func.is_const(db) {
        diags.push(invalid_provider(
            func.span().name().into(),
            "derive provider functions must be `const fn`",
        ));
    }

    let goal = match evidence_goal_for_ty(db, func.return_ty(db)) {
        Some(goal) => Some(goal),
        None => {
            diags.push(invalid_provider(
                func.span().ret_ty().into(),
                "derive provider functions must return `Evidence<C>`",
            ));
            None
        }
    };

    if let (Some(head), Some(goal)) = (head, goal) {
        match goal.kind(db) {
            ConstraintKind::Trait(inst) if inst.def(db) == head => {
                if !provider_target_is_derive_function_param(db, func, inst.self_ty(db)) {
                    diags.push(invalid_provider(
                        func.span().ret_ty().into(),
                        "derive provider returned evidence target must be a type parameter of the `derive` function",
                    ));
                }
            }
            ConstraintKind::Trait(_) => diags.push(invalid_provider(
                func.span().ret_ty().into(),
                "returned evidence constraint does not match the provider head",
            )),
            _ => diags.push(invalid_provider(
                func.span().ret_ty().into(),
                "derive providers currently support concrete trait evidence only",
            )),
        }
    }

    if !diags.is_empty() {
        return None;
    }

    let head = head?;
    let goal = goal?;
    let identity = DeriveProviderIdentityId::new(db, name, func);
    let derive_head = ConstraintHeadId::new(db, ConstraintHeadKind::ConcreteTrait(head));
    let derive_goal = ConstraintId::new(db, ConstraintKind::Derive(derive_head));
    Some(DeriveProviderId::new(db, identity, func, goal, derive_goal))
}

fn provider_target_is_derive_function_param<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
    ty: TyId<'db>,
) -> bool {
    let TyData::TyParam(param) = ty.data(db) else {
        return false;
    };
    param.is_normal() && param.owner == func.scope()
}

fn invalid_provider<'db>(
    span: crate::span::DynLazySpan<'db>,
    message: impl Into<String>,
) -> TyDiagCollection<'db> {
    TyLowerDiag::InvalidDeriveProvider {
        span,
        message: message.into(),
    }
    .into()
}
