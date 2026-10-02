//! Compiler-owned read-back of a checked body into the frozen code carrier.

use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
};

use hir::{
    analysis::semantic::{QuoteSite, QuotedCode, SemConstId, SemConstValue, StructuralHandle},
    analysis::ty::{
        const_ty::ConstTyData,
        ty_check::{ConstRef, LocalBinding, RecordInitLowering, ValuePathRef, check_func_body},
        ty_def::{TyBase, TyData, TyId},
    },
    core::meta::{
        FrozenCode, FrozenCodeNode, FrozenQuoteConstructor, FrozenQuoteConstructorSite,
        FrozenQuoteFunction, FrozenQuoteGenericUse, FrozenQuoteLocal, FrozenQuoteLocalType,
        FrozenQuoteOpenUse, FrozenQuoteQualifiedConst, FrozenQuoteSignatureType, FrozenTemplateKey,
        FrozenTemplateSource, MetaItem, QuoteMember,
    },
    hir_def::{
        CallableDef, Expr, Func, GenericArg, GenericParamOwner, Partial, Stmt, scope_graph::ScopeId,
    },
    span::LazySpan,
};

use super::construction::{
    ConstructionError, charge_frozen_code_node, charge_frozen_node, charge_frozen_type,
    freeze_checked_quote_type, freeze_quote_constructor_arguments, frozen_quote_nominal_ref,
    frozen_quote_trait,
};
use crate::DriverDataBase;

/// Reflect a checked type in its function's declaration namespace.
pub fn read_back_type<'db>(
    db: &'db DriverDataBase,
    func: Func<'db>,
    ty: TyId<'db>,
) -> Result<FrozenQuoteSignatureType, ConstructionError> {
    freeze_checked_quote_type(db, ty, func)?.ok_or(ConstructionError::ReadBackUnsupportedType)
}

/// Supplies a checked body to an ordinary Fe const consumer as Code<Expr>.
/// Issuance stays compiler-owned; this is not public function-body discovery.
pub fn read_back_value<'db>(
    db: &'db DriverDataBase,
    func: Func<'db>,
) -> Result<SemConstId<'db>, ConstructionError> {
    let body = func
        .body(db)
        .ok_or(ConstructionError::ReadBackMissingBody)?;
    if func.has_effects(db) {
        return Err(ConstructionError::ReadBackEffectful);
    }
    if !check_func_body(db, func).0.is_empty() {
        return Err(ConstructionError::ReadBackInvalidBody);
    }
    // Issuance keeps the checked source. Transport admission and metadata
    // freezing happen once, under the consumer's shared description budget.
    let code_ty = MetaItem::Code
        .resolve(db, func.scope())
        .ok_or(ConstructionError::ReadBackMissingOwner)?;
    let expr_ty = MetaItem::Expr
        .resolve(db, func.scope())
        .ok_or(ConstructionError::ReadBackMissingOwner)?;
    let code = QuotedCode::new(
        db,
        hir::analysis::semantic::CodeSource::Template(QuoteSite {
            owner: body,
            expr: body.expr(db),
            source: FrozenTemplateSource::CheckedBody,
        }),
        Vec::new(),
    );
    Ok(SemConstId::new(
        db,
        SemConstValue::Structural {
            ty: hir::analysis::ty::ty_def::TyId::app(db, code_ty, expr_ty),
            handle: StructuralHandle::Code(code),
        },
    ))
}

/// Returns the same frozen graph shape used for checked `quote` values. The
/// template points at the function's checked body rather than a quote site.
/// The caller must keep the authored database unchanged while using the graph.
pub fn read_back<'db>(
    db: &'db DriverDataBase,
    func: Func<'db>,
) -> Result<FrozenCode, ConstructionError> {
    read_back_with_budget(db, func, &Cell::new(0))
}

pub(super) fn read_back_with_budget<'db>(
    db: &'db DriverDataBase,
    func: Func<'db>,
    budget: &Cell<usize>,
) -> Result<FrozenCode, ConstructionError> {
    let freeze_type = |ty| {
        let frozen = freeze_checked_quote_type(db, ty, func)?;
        if let Some(ty) = &frozen {
            charge_frozen_type(ty, budget)?;
        }
        Ok::<_, ConstructionError>(frozen)
    };
    let freeze_arguments = |ty| {
        let arguments = freeze_quote_constructor_arguments(db, ty, func)?;
        if let Some(arguments) = &arguments {
            for ty in arguments {
                charge_frozen_type(ty, budget)?;
            }
        }
        Ok::<_, ConstructionError>(arguments)
    };
    let body = func
        .body(db)
        .ok_or(ConstructionError::ReadBackMissingBody)?;
    if func.has_effects(db) {
        return Err(ConstructionError::ReadBackEffectful);
    }
    let (diagnostics, typed) = check_func_body(db, func);
    if !diagnostics.is_empty() {
        return Err(ConstructionError::ReadBackInvalidBody);
    }
    for _ in body.exprs(db).iter() {
        charge_frozen_node(budget)?;
    }
    for _ in body.pats(db).iter() {
        charge_frozen_node(budget)?;
    }
    for _ in body.stmts(db).iter() {
        charge_frozen_node(budget)?;
    }
    let owner = func
        .frozen_locator(db)
        .ok_or(ConstructionError::ReadBackMissingOwner)?;
    let root = body.expr(db);
    let template = FrozenTemplateKey {
        owner,
        expr: root,
        source: FrozenTemplateSource::CheckedBody,
        span: root.span(body).resolve(db).map(|span| span.range.into()),
    };
    charge_frozen_code_node(budget)?;
    let mut node = FrozenCodeNode::empty(Some(template));
    let mut local_uses = HashMap::new();
    let freeze_constructor = |site, ty, owner, variant_index| {
        Ok::<_, ConstructionError>(FrozenQuoteConstructor::new(
            QuoteMember::CheckedBody,
            site,
            frozen_quote_nominal_ref(db, owner)?,
            variant_index,
            Some(freeze_arguments(ty)?.ok_or(ConstructionError::ReadBackUnsupportedType)?),
        ))
    };
    let freeze_variant = |site, ty, variant: hir::hir_def::EnumVariant<'db>| {
        freeze_constructor(
            site,
            ty,
            hir::analysis::ty::adt_def::AdtRef::Enum(variant.enum_),
            Some(variant.idx),
        )
    };
    for (pat, _) in body.pats(db).iter() {
        if let Some(root) = typed.pattern_root(pat)
            && let hir::analysis::ty::pattern_ir::ValidatedPatKind::Constructor {
                ctor: hir::analysis::ty::pattern_ir::ConstructorKind::Variant(variant, ty),
                ..
            } = typed.pattern_store().node(root).kind()
        {
            node.rooted_constructors.push(freeze_variant(
                FrozenQuoteConstructorSite::Pat(pat),
                *ty,
                *variant,
            )?);
        }
    }
    for (pat, _) in body.pats(db).iter() {
        if let Some(root) = typed.pattern_root(pat)
            && let hir::analysis::ty::pattern_ir::ValidatedPatKind::Constructor {
                ctor: hir::analysis::ty::pattern_ir::ConstructorKind::Type(ty),
                ..
            } = typed.pattern_store().node(root).kind()
            && let TyData::TyBase(TyBase::Adt(adt)) = ty.base_ty(db).data(db)
            && matches!(
                adt.adt_ref(db),
                hir::analysis::ty::adt_def::AdtRef::Struct(_)
            )
        {
            node.rooted_constructors.push(freeze_constructor(
                FrozenQuoteConstructorSite::Pat(pat),
                *ty,
                adt.adt_ref(db),
                None,
            )?);
        }
    }
    let mut classified_paths = HashSet::new();
    for (pat, _) in body.pats(db).iter() {
        if typed.pat_binding(pat).is_some() {
            local_uses.insert(pat, Vec::new());
        }
    }
    for (expr, data) in body.exprs(db).iter() {
        let Partial::Present(Expr::Path(Partial::Present(_))) = data else {
            continue;
        };
        match typed.expr_binding(expr) {
            Some(LocalBinding::Param { idx, .. }) => {
                classified_paths.insert(expr);
                node.open_uses.push(FrozenQuoteOpenUse {
                    site: expr,
                    index: u32::try_from(idx)
                        .map_err(|_| ConstructionError::ReadBackUnsupportedName)?,
                });
            }
            Some(LocalBinding::Local { pat, .. }) => {
                classified_paths.insert(expr);
                local_uses
                    .get_mut(&pat)
                    .ok_or(ConstructionError::ReadBackUnsupportedName)?
                    .push(expr);
            }
            Some(LocalBinding::EffectParam { .. }) => {
                return Err(ConstructionError::ReadBackEffectful);
            }
            None => {
                if let Some(ValuePathRef::UnitVariant(variant)) = typed.value_path_ref(expr) {
                    node.rooted_constructors.push(freeze_variant(
                        FrozenQuoteConstructorSite::Expr(expr),
                        typed.expr_ty(db, expr),
                        variant.variant,
                    )?);
                    classified_paths.insert(expr);
                } else if let Some(ValuePathRef::TypeConst(ty)) = typed.value_path_ref(expr)
                    && let TyData::ConstTy(const_ty) = ty.data(db)
                    && let ConstTyData::TyParam(param, _) = const_ty.data(db)
                    && let Some(ScopeId::GenericParam(owner, index)) = param.declaration_scope(db)
                    && owner == func.into()
                {
                    node.generic_uses.push(FrozenQuoteGenericUse {
                        site: expr,
                        member: QuoteMember::CheckedBody,
                        index: u32::from(index),
                    });
                    classified_paths.insert(expr);
                } else if let Some(ConstRef::TraitConst(assoc)) = typed.expr_const_ref(expr) {
                    let inst = assoc.inst();
                    let TyData::TyBase(TyBase::Prim(receiver)) = inst.self_ty(db).data(db) else {
                        return Err(ConstructionError::ReadBackUnsupportedType);
                    };
                    if inst.args(db).len() != 1 {
                        return Err(ConstructionError::ReadBackUnsupportedType);
                    }
                    let trait_site = u32::try_from(node.rooted_traits.len())
                        .map_err(|_| ConstructionError::ReadBackUnsupportedName)?;
                    node.rooted_traits
                        .push(frozen_quote_trait(db, inst.key(db), trait_site)?);
                    node.qualified_consts.push(FrozenQuoteQualifiedConst {
                        member: QuoteMember::CheckedBody,
                        site: expr,
                        trait_site,
                        receiver: *receiver,
                    });
                    classified_paths.insert(expr);
                }
            }
        }
    }
    node.locals = body
        .pats(db)
        .iter()
        .filter_map(|(binder, _)| {
            local_uses.remove(&binder).map(|uses| FrozenQuoteLocal {
                member: QuoteMember::CheckedBody,
                binder,
                uses,
            })
        })
        .collect();

    for (stmt, data) in body.stmts(db).iter() {
        let Partial::Present(Stmt::Let(pat, Some(_), _)) = data else {
            continue;
        };
        let ty = typed.pat_ty(db, *pat);
        let ty = freeze_type(ty)?.ok_or(ConstructionError::ReadBackUnsupportedType)?;
        node.local_types.push(FrozenQuoteLocalType {
            member: QuoteMember::CheckedBody,
            stmt,
            ty,
        });
    }

    for (expr, data) in body.exprs(db).iter() {
        let Partial::Present(Expr::Call(callee, _)) = data else {
            continue;
        };
        let Partial::Present(Expr::Path(Partial::Present(path))) = &body.exprs(db)[*callee] else {
            continue;
        };
        let callable = typed
            .callable_expr(expr)
            .ok_or(ConstructionError::ReadBackUnsupportedCall)?;
        if let CallableDef::VariantCtor(variant) = callable.callable_def {
            node.rooted_constructors.push(freeze_variant(
                FrozenQuoteConstructorSite::Expr(*callee),
                typed.expr_ty(db, expr),
                variant,
            )?);
            classified_paths.insert(*callee);
            continue;
        }
        let CallableDef::Func(selected) = callable.callable_def else {
            return Err(ConstructionError::ReadBackUnsupportedCall);
        };
        let arguments = path.generic_args(db);
        let type_arguments = if arguments.is_given(db) {
            if arguments
                .data(db)
                .iter()
                .any(|arg| !matches!(arg, GenericArg::Type(_)))
                || callable.generic_args().len()
                    != GenericParamOwner::Func(selected).params(db).count()
                || arguments.data(db).len() > callable.generic_args().len()
            {
                return Err(ConstructionError::ReadBackUnsupportedCall);
            }
            Some(
                callable
                    .generic_args()
                    .iter()
                    .take(arguments.data(db).len())
                    .map(|ty| freeze_type(*ty)?.ok_or(ConstructionError::ReadBackUnsupportedType))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        } else {
            None
        };
        node.rooted_funcs.push(FrozenQuoteFunction {
            member: QuoteMember::CheckedBody,
            type_arguments,
            site: *callee,
            owner: selected
                .frozen_locator(db)
                .ok_or(ConstructionError::ReadBackMissingOwner)?,
        });
        classified_paths.insert(*callee);
    }
    for (expr, data) in body.exprs(db).iter() {
        match data {
            Partial::Present(Expr::Cast(_, Partial::Present(_))) => {
                let ty = typed.expr_ty(db, expr).base_ty(db);
                let TyData::TyBase(TyBase::Prim(primitive)) = ty.data(db) else {
                    return Err(ConstructionError::ReadBackUnsupportedType);
                };
                node.cast_primitives.push((expr, *primitive));
            }
            Partial::Present(Expr::Path(_)) if !classified_paths.contains(&expr) => {
                return Err(ConstructionError::ReadBackUnsupportedName);
            }
            Partial::Present(Expr::MethodCall(_, _, generic_args, _)) => {
                if generic_args.is_given(db) {
                    return Err(ConstructionError::ReadBackUnsupportedCall);
                }
                let callable = typed
                    .callable_expr(expr)
                    .ok_or(ConstructionError::ReadBackUnsupportedCall)?;
                let CallableDef::Func(selected) = callable.callable_def else {
                    return Err(ConstructionError::ReadBackUnsupportedCall);
                };
                node.selected_methods.push((
                    expr,
                    selected
                        .frozen_locator(db)
                        .ok_or(ConstructionError::ReadBackMissingOwner)?,
                ));
            }
            Partial::Present(Expr::RecordInit(..)) => {
                if let Some(RecordInitLowering::EnumVariant(variant)) =
                    typed.record_init_lowering(expr)
                {
                    node.rooted_constructors.push(freeze_variant(
                        FrozenQuoteConstructorSite::Expr(expr),
                        typed.expr_ty(db, expr),
                        variant.variant,
                    )?);
                    continue;
                }
                if typed.record_init_lowering(expr) != Some(RecordInitLowering::Struct) {
                    return Err(ConstructionError::ReadBackUnsupportedName);
                }
                if let TyData::TyBase(TyBase::Adt(adt)) =
                    typed.expr_ty(db, expr).base_ty(db).data(db)
                {
                    node.rooted_constructors.push(freeze_constructor(
                        FrozenQuoteConstructorSite::Expr(expr),
                        typed.expr_ty(db, expr),
                        adt.adt_ref(db),
                        None,
                    )?);
                }
                let ty = freeze_type(typed.expr_ty(db, expr))?
                    .ok_or(ConstructionError::ReadBackUnsupportedType)?;
                node.record_types.push((expr, ty));
            }
            _ => {}
        }
    }
    for _ in 0..node.metadata_nodes() {
        charge_frozen_node(budget)?;
    }
    Ok(FrozenCode {
        root: 0,
        nodes: vec![node],
    })
}
