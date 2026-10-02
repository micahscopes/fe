//! Compiler-owned read-back of a checked body into the frozen code carrier.

use std::collections::{HashMap, HashSet};

use hir::{
    analysis::semantic::{QuoteSite, QuotedCode, SemConstId, SemConstValue, StructuralHandle},
    analysis::ty::{
        const_ty::ConstTyData,
        ty_check::{ConstRef, LocalBinding, RecordInitLowering, ValuePathRef, check_func_body},
        ty_def::{TyBase, TyData},
    },
    core::meta::{
        FrozenCode, FrozenCodeNode, FrozenQuoteConstructor, FrozenQuoteConstructorSite,
        FrozenQuoteFunction, FrozenQuoteGenericUse, FrozenQuoteLocal, FrozenQuoteLocalType,
        FrozenQuoteOpenUse, FrozenQuoteQualifiedConst, FrozenTemplateKey, FrozenTemplateSource,
        MetaItem,
    },
    hir_def::{
        CallableDef, Expr, Func, GenericArg, GenericParamOwner, Partial, Stmt, scope_graph::ScopeId,
    },
    span::LazySpan,
};

use super::construction::{
    freeze_checked_quote_type, frozen_quote_nominal_ref, frozen_quote_trait,
};
use crate::DriverDataBase;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadBackError {
    MissingBody,
    InvalidBody,
    Effectful,
    MissingOwner,
    UnsupportedCall,
    UnsupportedName,
    UnsupportedType,
}

/// Supplies a checked body to an ordinary Fe const consumer as Code<Expr>.
/// Issuance stays compiler-owned; this is not public function-body discovery.
pub fn read_back_value<'db>(
    db: &'db DriverDataBase,
    func: Func<'db>,
) -> Result<SemConstId<'db>, ReadBackError> {
    read_back(db, func)?;
    let body = func.body(db).ok_or(ReadBackError::MissingBody)?;
    let code_ty = MetaItem::Code
        .resolve(db, func.scope())
        .ok_or(ReadBackError::MissingOwner)?;
    let expr_ty = MetaItem::Expr
        .resolve(db, func.scope())
        .ok_or(ReadBackError::MissingOwner)?;
    let code = QuotedCode::new(
        db,
        Some(QuoteSite {
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
) -> Result<FrozenCode, ReadBackError> {
    let body = func.body(db).ok_or(ReadBackError::MissingBody)?;
    if func.has_effects(db) {
        return Err(ReadBackError::Effectful);
    }
    let (diagnostics, typed) = check_func_body(db, func);
    if !diagnostics.is_empty() {
        return Err(ReadBackError::InvalidBody);
    }
    let owner = func.frozen_locator(db).ok_or(ReadBackError::MissingOwner)?;
    let root = body.expr(db);
    let template = FrozenTemplateKey {
        owner,
        expr: root,
        source: FrozenTemplateSource::CheckedBody,
        span: root.span(body).resolve(db).map(|span| span.range.into()),
    };
    let mut node = FrozenCodeNode::empty(Some(template));
    let mut local_uses = HashMap::new();
    let freeze_variant = |site, variant: hir::hir_def::EnumVariant<'db>| {
        Ok::<_, ReadBackError>(FrozenQuoteConstructor {
            func: Some(0),
            site,
            owner: frozen_quote_nominal_ref(
                db,
                hir::analysis::ty::adt_def::AdtRef::Enum(variant.enum_),
            )
            .map_err(|_| ReadBackError::MissingOwner)?,
            variant_index: Some(variant.idx),
        })
    };
    for (pat, _) in body.pats(db).iter() {
        if let Some(root) = typed.pattern_root(pat)
            && let hir::analysis::ty::pattern_ir::ValidatedPatKind::Constructor {
                ctor: hir::analysis::ty::pattern_ir::ConstructorKind::Variant(variant, _),
                ..
            } = typed.pattern_store().node(root).kind()
        {
            node.rooted_constructors.push(freeze_variant(
                FrozenQuoteConstructorSite::Pat(pat),
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
            node.rooted_constructors.push(FrozenQuoteConstructor {
                func: Some(0),
                site: FrozenQuoteConstructorSite::Pat(pat),
                owner: frozen_quote_nominal_ref(db, adt.adt_ref(db))
                    .map_err(|_| ReadBackError::MissingOwner)?,
                variant_index: None,
            });
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
                    index: u32::try_from(idx).map_err(|_| ReadBackError::UnsupportedName)?,
                });
            }
            Some(LocalBinding::Local { pat, .. }) => {
                classified_paths.insert(expr);
                local_uses
                    .get_mut(&pat)
                    .ok_or(ReadBackError::UnsupportedName)?
                    .push(expr);
            }
            Some(LocalBinding::EffectParam { .. }) => return Err(ReadBackError::Effectful),
            None => {
                if let Some(ValuePathRef::UnitVariant(variant)) = typed.value_path_ref(expr) {
                    node.rooted_constructors.push(freeze_variant(
                        FrozenQuoteConstructorSite::Expr(expr),
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
                        func: 0,
                        index: u32::from(index),
                        where_predicate: None,
                    });
                    classified_paths.insert(expr);
                } else if let Some(ConstRef::TraitConst(assoc)) = typed.expr_const_ref(expr) {
                    let inst = assoc.inst();
                    let TyData::TyBase(TyBase::Prim(receiver)) = inst.self_ty(db).data(db) else {
                        return Err(ReadBackError::UnsupportedType);
                    };
                    if inst.args(db).len() != 1 {
                        return Err(ReadBackError::UnsupportedType);
                    }
                    let trait_site = u32::try_from(node.rooted_traits.len())
                        .map_err(|_| ReadBackError::UnsupportedName)?;
                    node.rooted_traits.push(
                        frozen_quote_trait(db, inst.key(db), trait_site)
                            .map_err(|_| ReadBackError::MissingOwner)?,
                    );
                    node.qualified_consts.push(FrozenQuoteQualifiedConst {
                        func: 0,
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
                func: 0,
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
        let ty = freeze_checked_quote_type(db, ty, func)
            .map_err(|_| ReadBackError::MissingOwner)?
            .ok_or(ReadBackError::UnsupportedType)?;
        node.local_types
            .push(FrozenQuoteLocalType { func: 0, stmt, ty });
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
            .ok_or(ReadBackError::UnsupportedCall)?;
        if let CallableDef::VariantCtor(variant) = callable.callable_def {
            node.rooted_constructors.push(freeze_variant(
                FrozenQuoteConstructorSite::Expr(*callee),
                variant,
            )?);
            classified_paths.insert(*callee);
            continue;
        }
        let CallableDef::Func(selected) = callable.callable_def else {
            return Err(ReadBackError::UnsupportedCall);
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
                return Err(ReadBackError::UnsupportedCall);
            }
            Some(
                callable
                    .generic_args()
                    .iter()
                    .take(arguments.data(db).len())
                    .map(|ty| {
                        freeze_checked_quote_type(db, *ty, func)
                            .map_err(|_| ReadBackError::MissingOwner)?
                            .ok_or(ReadBackError::UnsupportedType)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )
        } else {
            None
        };
        let scope = selected.scope();
        let full_path = scope.pretty_path(db).ok_or(ReadBackError::MissingOwner)?;
        let module = scope
            .top_mod(db)
            .scope()
            .pretty_path(db)
            .ok_or(ReadBackError::MissingOwner)?;
        let path = full_path
            .strip_prefix(&format!("{module}::"))
            .ok_or(ReadBackError::MissingOwner)?
            .to_owned();
        node.rooted_funcs.push(FrozenQuoteFunction {
            func: None,
            assoc_const: None,
            type_arguments,
            site: *callee,
            where_predicate: None,
            ingot: scope.ingot(db).base(db).clone(),
            path,
            owner: selected
                .frozen_locator(db)
                .ok_or(ReadBackError::MissingOwner)?,
        });
        classified_paths.insert(*callee);
    }
    for (expr, data) in body.exprs(db).iter() {
        match data {
            Partial::Present(Expr::Cast(_, Partial::Present(_))) => {
                let ty = typed.expr_ty(db, expr).base_ty(db);
                let TyData::TyBase(TyBase::Prim(primitive)) = ty.data(db) else {
                    return Err(ReadBackError::UnsupportedType);
                };
                node.cast_primitives.push((expr, *primitive));
            }
            Partial::Present(Expr::Path(_)) if !classified_paths.contains(&expr) => {
                return Err(ReadBackError::UnsupportedName);
            }
            Partial::Present(Expr::MethodCall(_, _, generic_args, _)) => {
                if generic_args.is_given(db) {
                    return Err(ReadBackError::UnsupportedCall);
                }
                let callable = typed
                    .callable_expr(expr)
                    .ok_or(ReadBackError::UnsupportedCall)?;
                let CallableDef::Func(selected) = callable.callable_def else {
                    return Err(ReadBackError::UnsupportedCall);
                };
                node.selected_methods.push((
                    expr,
                    selected
                        .frozen_locator(db)
                        .ok_or(ReadBackError::MissingOwner)?,
                ));
            }
            Partial::Present(Expr::RecordInit(..)) => {
                if let Some(RecordInitLowering::EnumVariant(variant)) =
                    typed.record_init_lowering(expr)
                {
                    node.rooted_constructors.push(freeze_variant(
                        FrozenQuoteConstructorSite::Expr(expr),
                        variant.variant,
                    )?);
                    continue;
                }
                if typed.record_init_lowering(expr) != Some(RecordInitLowering::Struct) {
                    return Err(ReadBackError::UnsupportedName);
                }
                if let TyData::TyBase(TyBase::Adt(adt)) =
                    typed.expr_ty(db, expr).base_ty(db).data(db)
                {
                    node.rooted_constructors.push(FrozenQuoteConstructor {
                        func: Some(0),
                        site: FrozenQuoteConstructorSite::Expr(expr),
                        owner: frozen_quote_nominal_ref(db, adt.adt_ref(db))
                            .map_err(|_| ReadBackError::MissingOwner)?,
                        variant_index: None,
                    });
                }
                let ty = freeze_checked_quote_type(db, typed.expr_ty(db, expr), func)
                    .map_err(|_| ReadBackError::MissingOwner)?
                    .ok_or(ReadBackError::UnsupportedType)?;
                node.record_types.push((expr, ty));
            }
            _ => {}
        }
    }
    Ok(FrozenCode {
        root: 0,
        nodes: vec![node],
    })
}
