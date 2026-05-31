use crate::{
    analysis::{
        HirAnalysisDb,
        name_resolution::{PathRes, resolve_path},
        ty::{adt_def::AdtRef, constraint::ConstraintId, ty_def::TyId},
    },
    hir_def::{
        Attr, AttrArg, AttrArgValue, DeriveDecl, Enum, HirIngot, IdentId, ItemKind, NormalAttr,
        Struct, TopLevelMod, Trait,
    },
    span::DynLazySpan,
};
use common::ingot::Ingot;

use crate::analysis::ty::{
    constraint::ConstraintKind, diagnostics::TyDiagCollection, trait_def::TraitInstId,
    trait_resolution::PredicateListId,
};

use super::invalid_request;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ElaborationTarget<'db> {
    Struct(Struct<'db>),
    Enum(Enum<'db>),
}

impl<'db> ElaborationTarget<'db> {
    pub(super) fn from_item(item: ItemKind<'db>) -> Option<Self> {
        match item {
            ItemKind::Struct(struct_) => Some(Self::Struct(struct_)),
            ItemKind::Enum(enum_) => Some(Self::Enum(enum_)),
            _ => None,
        }
    }

    pub(super) fn from_adt_ref(adt: AdtRef<'db>) -> Self {
        match adt {
            AdtRef::Struct(struct_) => Self::Struct(struct_),
            AdtRef::Enum(enum_) => Self::Enum(enum_),
        }
    }

    pub(super) fn item(self) -> ItemKind<'db> {
        match self {
            Self::Struct(struct_) => ItemKind::Struct(struct_),
            Self::Enum(enum_) => ItemKind::Enum(enum_),
        }
    }

    pub(super) fn scope(self) -> crate::hir_def::scope_graph::ScopeId<'db> {
        self.item().scope()
    }

    pub(super) fn attrs(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Option<crate::hir_def::AttrListId<'db>> {
        self.item().attrs(db)
    }

    pub(super) fn attr_span(self) -> DynLazySpan<'db> {
        match self {
            Self::Struct(struct_) => struct_.span().attributes().into(),
            Self::Enum(enum_) => enum_.span().attributes().into(),
        }
    }

    pub(super) fn attr_arg_span(self, attr_index: u32, arg_index: u32) -> DynLazySpan<'db> {
        match self {
            Self::Struct(struct_) => struct_
                .span()
                .attributes()
                .attr(attr_index as usize)
                .into_normal_attr()
                .args()
                .arg(arg_index as usize)
                .into(),
            Self::Enum(enum_) => enum_
                .span()
                .attributes()
                .attr(attr_index as usize)
                .into_normal_attr()
                .args()
                .arg(arg_index as usize)
                .into(),
        }
    }

    pub(super) fn ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
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
pub(crate) enum ElaborationOrigin<'db> {
    DeriveAttr {
        attr_index: u32,
        arg_index: u32,
        selected_provider_arg_index: Option<u32>,
    },
    DeriveDecl(DeriveDecl<'db>),
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ElaborationRequestId<'db> {
    pub(super) target: ElaborationTarget<'db>,
    pub(super) goal: ConstraintId<'db>,
    pub(super) selected_provider: Option<IdentId<'db>>,
    pub(super) origin: ElaborationOrigin<'db>,
}

impl<'db> ElaborationRequestId<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        let mut summary = format!(
            "{} requested for {}",
            self.goal(db).pretty_print(db),
            self.target(db).ty(db).pretty_print(db)
        );
        if let Some(provider) = self.selected_provider(db) {
            summary.push_str(" using ");
            summary.push_str(provider.data(db));
        }
        summary
    }

    pub(super) fn span(self, db: &'db dyn HirAnalysisDb) -> DynLazySpan<'db> {
        match self.origin(db) {
            ElaborationOrigin::DeriveAttr { .. } => self.target(db).attr_span(),
            ElaborationOrigin::DeriveDecl(decl) => decl.span().into(),
        }
    }

    pub(super) fn selected_provider_span(self, db: &'db dyn HirAnalysisDb) -> DynLazySpan<'db> {
        match self.origin(db) {
            ElaborationOrigin::DeriveAttr {
                attr_index,
                selected_provider_arg_index: Some(arg_index),
                ..
            } => self.target(db).attr_arg_span(attr_index, arg_index),
            ElaborationOrigin::DeriveDecl(decl)
                if decl.selected_provider_path(db).is_some()
                    && !decl.selected_provider_is_scoped(db) =>
            {
                decl.span().provider_path().into()
            }
            ElaborationOrigin::DeriveAttr { .. } | ElaborationOrigin::DeriveDecl(_) => {
                self.span(db)
            }
        }
    }
}

impl<'db> ElaborationOrigin<'db> {
    pub(super) fn pretty_print(self) -> &'static str {
        match self {
            Self::DeriveAttr { .. } => "derive attribute",
            Self::DeriveDecl(_) => "derive declaration",
        }
    }
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
        .chain(top_mod.all_derive_decls(db).iter().flat_map(|&decl| {
            derive_requests_for_decl(db, decl)
                .into_iter()
                .filter_map(|result| result.ok())
        }))
        .collect()
}

pub(super) fn elaboration_request_parse_diags_for_top_mod<'db>(
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
    diags.extend(top_mod.all_derive_decls(db).iter().flat_map(|&decl| {
        derive_requests_for_decl(db, decl)
            .into_iter()
            .filter_map(|result| result.err())
    }));
    diags
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

    let mut selected_provider = None;
    let mut selected_provider_arg_index = None;
    let mut trait_args = Vec::new();
    let mut errors = Vec::new();
    for (raw_arg_index, arg) in attr.args.iter().enumerate() {
        if arg.has_value || arg.value.is_some() {
            match parse_derive_provider_selection(db, target, arg) {
                Ok(provider) if selected_provider.replace(provider).is_none() => {
                    selected_provider_arg_index = Some(raw_arg_index as u32);
                }
                Ok(_) => errors.push(invalid_request(
                    target.attr_span(),
                    "`#[derive(...)]` may only select one provider",
                )),
                Err(diag) => errors.push(diag),
            }
        } else {
            trait_args.push(arg);
        }
    }

    if let Some(selected) = selected_provider {
        if trait_args.len() != 1 {
            errors.push(invalid_request(
                target.attr_span(),
                format!(
                    "`using = {}` requires exactly one derived trait",
                    selected.data(db)
                ),
            ));
        }
    }

    if !errors.is_empty() {
        return errors.into_iter().map(Err).collect();
    }

    trait_args
        .iter()
        .enumerate()
        .map(|(arg_index, arg)| {
            let Some(path) = arg.key.to_opt() else {
                return Err(invalid_request(
                    target.attr_span(),
                    "derive arguments must be trait paths",
                ));
            };
            let trait_ = resolve_derive_trait(db, target, path)?;
            Ok(make_derive_request(
                db,
                target,
                trait_,
                selected_provider,
                ElaborationOrigin::DeriveAttr {
                    attr_index: attr_index as u32,
                    arg_index: arg_index as u32,
                    selected_provider_arg_index,
                },
            ))
        })
        .collect()
}

fn derive_requests_for_decl<'db>(
    db: &'db dyn HirAnalysisDb,
    decl: DeriveDecl<'db>,
) -> Vec<Result<ElaborationRequestId<'db>, TyDiagCollection<'db>>> {
    vec![derive_request_for_decl(db, decl)]
}

fn derive_request_for_decl<'db>(
    db: &'db dyn HirAnalysisDb,
    decl: DeriveDecl<'db>,
) -> Result<ElaborationRequestId<'db>, TyDiagCollection<'db>> {
    let span: DynLazySpan<'db> = decl.span().into();
    let Some(head_path) = decl.head_path(db).to_opt() else {
        return Err(invalid_request(
            span.clone(),
            "derive declarations require a trait head",
        ));
    };
    let Some(target_path) = decl.target_path(db).to_opt() else {
        return Err(invalid_request(
            span.clone(),
            "derive declarations require a target after `for`",
        ));
    };

    let target = resolve_derive_target(db, decl, target_path)?;
    let trait_ = resolve_derive_trait_in_scope(db, decl.scope(), span.clone(), head_path)?;
    let selected_provider = match decl.selected_provider_path(db) {
        None => None,
        Some(path) => {
            let Some(path) = path.to_opt() else {
                return Err(invalid_request(
                    span.clone(),
                    "`using` must name a derive provider",
                ));
            };
            let Some(provider) = path.as_ident(db) else {
                return Err(invalid_request(
                    span.clone(),
                    "`using` must name one derive provider",
                ));
            };
            Some(provider)
        }
    };

    Ok(make_derive_request(
        db,
        target,
        trait_,
        selected_provider,
        ElaborationOrigin::DeriveDecl(decl),
    ))
}

fn make_derive_request<'db>(
    db: &'db dyn HirAnalysisDb,
    target: ElaborationTarget<'db>,
    trait_: Trait<'db>,
    selected_provider: Option<IdentId<'db>>,
    origin: ElaborationOrigin<'db>,
) -> ElaborationRequestId<'db> {
    ElaborationRequestId::new(
        db,
        target,
        derive_goal(db, target, trait_),
        selected_provider,
        origin,
    )
}

fn parse_derive_provider_selection<'db>(
    db: &'db dyn HirAnalysisDb,
    target: ElaborationTarget<'db>,
    arg: &AttrArg<'db>,
) -> Result<IdentId<'db>, TyDiagCollection<'db>> {
    if arg.key_str(db) != Some("using") {
        return Err(invalid_request(
            target.attr_span(),
            "derive keyword arguments currently only support `using = Provider`",
        ));
    }
    match arg.value.as_ref() {
        Some(AttrArgValue::Ident(provider)) => Ok(*provider),
        _ => Err(invalid_request(
            target.attr_span(),
            "`using` must name a derive provider",
        )),
    }
}

fn resolve_derive_trait<'db>(
    db: &'db dyn HirAnalysisDb,
    target: ElaborationTarget<'db>,
    path: crate::hir_def::PathId<'db>,
) -> Result<Trait<'db>, TyDiagCollection<'db>> {
    resolve_derive_trait_in_scope(db, target.scope(), target.attr_span(), path)
}

fn resolve_derive_trait_in_scope<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: crate::hir_def::scope_graph::ScopeId<'db>,
    span: DynLazySpan<'db>,
    path: crate::hir_def::PathId<'db>,
) -> Result<Trait<'db>, TyDiagCollection<'db>> {
    let assumptions = PredicateListId::empty_list(db);
    match resolve_path(db, path, scope, assumptions, false) {
        Ok(PathRes::Trait(inst)) => Ok(inst.def(db)),
        Ok(res) => Err(invalid_request(
            span,
            format!(
                "derive head must resolve to a trait, but resolved to {}",
                res.kind_name()
            ),
        )),
        Err(_) => Err(invalid_request(span, "derive head must resolve to a trait")),
    }
}

fn resolve_derive_target<'db>(
    db: &'db dyn HirAnalysisDb,
    decl: DeriveDecl<'db>,
    path: crate::hir_def::PathId<'db>,
) -> Result<ElaborationTarget<'db>, TyDiagCollection<'db>> {
    let span: DynLazySpan<'db> = decl.span().into();
    let assumptions = PredicateListId::empty_list(db);
    match resolve_path(db, path, decl.scope(), assumptions, false) {
        Ok(PathRes::Ty(ty)) => {
            let Some(adt) = ty.adt_ref(db) else {
                return Err(invalid_request(
                    span,
                    "derive target must resolve to a struct or enum",
                ));
            };
            Ok(ElaborationTarget::from_adt_ref(adt))
        }
        Ok(PathRes::TyAlias(..)) => Err(invalid_request(
            span,
            "derive target must be a nominal struct or enum, not a type alias",
        )),
        Ok(res) => Err(invalid_request(
            span,
            format!(
                "derive target must resolve to a struct or enum, but resolved to {}",
                res.kind_name()
            ),
        )),
        Err(_) => Err(invalid_request(
            span,
            "derive target must resolve to a struct or enum",
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
