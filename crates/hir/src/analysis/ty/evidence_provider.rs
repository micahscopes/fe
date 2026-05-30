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
        },
    },
    hir_def::{Attr, AttrArgValue, Func, HirIngot, IdentId, ItemKind, NormalAttr, Trait},
};
use common::ingot::Ingot;

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
    pub(crate) head: Trait<'db>,
    pub(crate) goal: ConstraintId<'db>,
    pub(crate) derive_goal: ConstraintId<'db>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum EvidenceProviderValidationResult<'db> {
    Valid(EvidenceProviderId<'db>),
    Invalid,
    NotProvider,
}

pub(crate) fn validate_evidence_provider<'db>(
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
    if attrs.len() > 1 {
        diags.push(invalid_provider(
            attr_span.clone(),
            "`#[evidence_provider(...)]` may only appear once on a function",
        ));
    }

    let (head, explicit_name) = match parse_provider_attr(db, func, attrs[0]) {
        Ok((head, name)) => (Some(head), name),
        Err(message) => {
            diags.push(invalid_provider(attr_span.clone(), message));
            (None, None)
        }
    };

    if !func.is_const(db) {
        diags.push(invalid_provider(
            attr_span.clone(),
            "evidence provider functions must be `const fn`",
        ));
    }

    let goal = match evidence_goal_for_ty(db, func.return_ty(db)) {
        Some(goal) => Some(goal),
        None => {
            diags.push(invalid_provider(
                func.span().ret_ty().into(),
                "evidence provider functions must return `Evidence<C>`",
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
                "evidence providers currently support concrete trait evidence only",
            )),
        }
    }

    if diags.is_empty() {
        let name = explicit_name.unwrap_or_else(|| {
            func.name(db)
                .to_opt()
                .unwrap_or_else(|| IdentId::new(db, "<anonymous provider>".to_string()))
        });
        let identity = EvidenceProviderIdentityId::new(db, name, func);
        let head = head.expect("validated head");
        let derive_head = ConstraintHeadId::new(db, ConstraintHeadKind::ConcreteTrait(head));
        let derive_goal = ConstraintId::new(db, ConstraintKind::Derive(derive_head));
        let provider =
            EvidenceProviderId::new(db, identity, func, head, goal.unwrap(), derive_goal);
        (EvidenceProviderValidationResult::Valid(provider), diags)
    } else {
        (EvidenceProviderValidationResult::Invalid, diags)
    }
}

pub(crate) fn validated_evidence_providers_for_ingot<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
) -> Vec<EvidenceProviderId<'db>> {
    ingot
        .all_funcs(db)
        .iter()
        .filter_map(|&func| match validate_evidence_provider(db, func).0 {
            EvidenceProviderValidationResult::Valid(provider) => Some(provider),
            EvidenceProviderValidationResult::Invalid
            | EvidenceProviderValidationResult::NotProvider => None,
        })
        .collect()
}

pub(crate) fn providers_for_derive_goal<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: Ingot<'db>,
    derive_goal: ConstraintId<'db>,
) -> Vec<EvidenceProviderId<'db>> {
    validated_evidence_providers_for_ingot(db, ingot)
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

fn parse_provider_attr<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
    attr: &NormalAttr<'db>,
) -> Result<(Trait<'db>, Option<IdentId<'db>>), &'static str> {
    if attr.has_value || !attr.has_args || !matches!(attr.args.len(), 1 | 2) {
        return Err("expected `#[evidence_provider(TraitName)]`");
    }
    let arg = &attr.args[0];
    if arg.has_value || arg.value.is_some() {
        return Err("evidence provider head must be a trait path");
    }
    let Some(path) = arg.key.to_opt() else {
        return Err("evidence provider head must be a trait path");
    };

    let head = match resolve_path(db, path, func.scope(), func.assumptions(db), false) {
        Ok(PathRes::Trait(inst)) => Ok(inst.def(db)),
        _ => Err("evidence provider head must resolve to a trait"),
    }?;

    let name = match attr.args.get(1) {
        None => None,
        Some(arg) => Some(parse_provider_identity_arg(db, arg)?),
    };

    Ok((head, name))
}

fn parse_provider_identity_arg<'db>(
    db: &'db dyn HirAnalysisDb,
    arg: &crate::hir_def::AttrArg<'db>,
) -> Result<IdentId<'db>, &'static str> {
    if arg.key_str(db) != Some("name") {
        return Err(
            "evidence provider keyword arguments currently only support `name = ProviderName`",
        );
    }
    match arg.value.as_ref() {
        Some(AttrArgValue::Ident(name)) => Ok(*name),
        _ => Err("evidence provider name must be an identifier"),
    }
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
