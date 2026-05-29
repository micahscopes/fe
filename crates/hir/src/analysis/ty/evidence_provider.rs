use crate::{
    analysis::{
        HirAnalysisDb,
        name_resolution::{PathRes, resolve_path},
        ty::{
            constraint::{ConstraintId, ConstraintKind, evidence_goal_for_ty},
            diagnostics::{TyDiagCollection, TyLowerDiag},
        },
    },
    hir_def::{Attr, Func, ItemKind, NormalAttr, Trait},
};

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct EvidenceProviderId<'db> {
    pub(crate) func: Func<'db>,
    pub(crate) head: Trait<'db>,
    pub(crate) goal: ConstraintId<'db>,
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

    let head = match parse_provider_head(db, func, attrs[0]) {
        Ok(head) => Some(head),
        Err(message) => {
            diags.push(invalid_provider(attr_span.clone(), message));
            None
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
        let provider =
            EvidenceProviderId::new(db, func, head.expect("validated head"), goal.unwrap());
        (EvidenceProviderValidationResult::Valid(provider), diags)
    } else {
        (EvidenceProviderValidationResult::Invalid, diags)
    }
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

fn parse_provider_head<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
    attr: &NormalAttr<'db>,
) -> Result<Trait<'db>, &'static str> {
    if attr.has_value || !attr.has_args || attr.args.len() != 1 {
        return Err("expected `#[evidence_provider(TraitName)]`");
    }
    let arg = &attr.args[0];
    if arg.has_value || arg.value.is_some() {
        return Err("evidence provider head must be a trait path");
    }
    let Some(path) = arg.key.to_opt() else {
        return Err("evidence provider head must be a trait path");
    };

    match resolve_path(db, path, func.scope(), func.assumptions(db), false) {
        Ok(PathRes::Trait(inst)) => Ok(inst.def(db)),
        _ => Err("evidence provider head must resolve to a trait"),
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
