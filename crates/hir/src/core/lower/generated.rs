//! Builds checked generation results as ordinary HIR in the output view.
//! Provider evaluation has already finished before this module is queried.

use parser::ast::{self, prelude::*};

use super::{
    FileLowerCtxt, base_scope_graph_impl,
    hir_builder::{
        GeneratedFunctionSpec, GeneratedImplSpec, HirBuilder, QuoteExprSource, QuoteMethodCall,
        quote_hir_primitive, rebind_owner_bound,
    },
    top_mod_ast,
};
use crate::{
    HirDb, LowerHirDb,
    analysis::{
        HirAnalysisDb,
        name_resolution::{PathRes, resolve_path},
        semantic::SemConstScalar,
        ty::{
            const_ty::ConstTyData,
            trait_resolution::PredicateListId,
            ty_check::{
                ConstRef, EffectParamSite, LocalBinding, ParamSite, ValuePathRef, check_func_body,
            },
            ty_def::{PrimTy as SemPrimTy, TyBase, TyData},
        },
    },
    core::meta::{
        FrozenCode, FrozenCodeHole, FrozenCodeNode, FrozenGeneration, FrozenHandleKind,
        FrozenItemLocator, FrozenMetaValue, FrozenMetaVariantCase, FrozenOwner,
        FrozenQuoteConstructor, FrozenQuoteConstructorSite, FrozenQuoteFieldType,
        FrozenQuoteFunction, FrozenQuoteNominal, FrozenQuoteQualifiedConst,
        FrozenQuoteSignatureType, FrozenQuoteTrait, FrozenQuoteWherePredicate,
        FrozenTemplateSource, MetaItem,
    },
    hir_def::{
        AssocConstDef, Body, CallableDef, EffectParamListId, Expr, ExprId, Func, FuncParamListId,
        GenericArg, GenericArgListId, GenericParam, GenericParamListId, GenericParamOwner, IdentId,
        ItemKind, Partial, PatId, PathId, PathKind, Quote, QuoteAssocType, QuoteConst,
        QuoteFragment, QuoteHolePosition, QuoteItem, QuoteName, TopLevelMod, TrackedItemVariant,
        Trait, TraitRefId, TupleTypeId, TypeBound, TypeGenericArg, TypeId, TypeKind, Visibility,
        WhereClauseId, WhereClauseOwner, WherePredicate,
        scope_graph::{EdgeKind, Scope, ScopeGraph, ScopeId},
    },
    span::{GenerationDesugared, HirOrigin},
};

fn invalid(message: &'static str) -> String {
    message.to_owned()
}

fn quoted_name<'db>(
    name: QuoteName<'db>,
    node: &FrozenCodeNode,
    target: &TargetContext<'_, 'db>,
) -> Result<IdentId<'db>, String> {
    match name {
        QuoteName::Ident(Partial::Present(name)) => Ok(name),
        QuoteName::Ident(Partial::Absent) => Err(invalid("quoted declaration name is missing")),
        QuoteName::Hole(slot) => {
            let Some(FrozenCodeHole::Value(value, _)) = node.holes.get(slot as usize) else {
                return Err(invalid("quoted name splice has no checked value"));
            };
            match value.as_ref() {
                FrozenMetaValue::Structural {
                    item: MetaItem::Field,
                    ..
                } => target.field_name(target.field_index(value, false)?),
                _ => target.name(value),
            }
        }
    }
}

fn authored_parent<'db>(graph: &ScopeGraph<'db>, scope: ScopeId<'db>) -> Option<ScopeId<'db>> {
    let edges = &graph.scopes.get(&scope)?.edges;
    edges
        .iter()
        .find_map(|edge| matches!(edge.kind, EdgeKind::Lex(_)).then_some(edge.dest))
        .or_else(|| {
            edges
                .iter()
                .find_map(|edge| matches!(edge.kind, EdgeKind::Super(_)).then_some(edge.dest))
        })
}

/// Locate an authored function without reading the merged scope graph. The
/// output view may be building generated functions for this very file.
fn locate_authored_function<'db>(
    db: &'db dyn HirDb,
    graph: &'db ScopeGraph<'db>,
    locator: &FrozenItemLocator,
) -> Result<Func<'db>, String> {
    let matches: Vec<_> = match locator {
        FrozenItemLocator::Named(owner) => graph
            .items_dfs(db)
            .filter_map(|item| {
                let ItemKind::Func(func) = item else {
                    return None;
                };
                (authored_path(db, graph, func.scope())? == owner.path).then_some(func)
            })
            .collect(),
        FrozenItemLocator::ImplMember {
            module_path,
            trait_path,
            self_type_path,
            selection_name,
            member_name,
            ..
        } => graph
            .items_dfs(db)
            .filter_map(|item| {
                let (scope, actual_trait, actual_self, actual_selection) = match item {
                    ItemKind::ImplTrait(implementation) => (
                        implementation.scope(),
                        Some(implementation.hir_trait_ref(db).to_opt()?.pretty_print(db)),
                        implementation.hir_type_ref(db).to_opt()?.pretty_print(db),
                        implementation
                            .selection_name(db)
                            .map(|name| name.data(db).clone()),
                    ),
                    ItemKind::Impl(implementation) => (
                        implementation.scope(),
                        None,
                        implementation.hir_type_ref(db).to_opt()?.pretty_print(db),
                        None,
                    ),
                    _ => return None,
                };
                let parent = authored_parent(graph, scope)?;
                if authored_path(db, graph, parent)?.as_str() != module_path.as_str()
                    || actual_trait.as_ref() != trait_path.as_ref()
                    || actual_self.as_str() != self_type_path.as_str()
                    || actual_selection.as_ref() != selection_name.as_ref()
                {
                    return None;
                }
                Some(scope)
            })
            .flat_map(|scope| {
                graph.child_items(scope).filter_map(|item| {
                    let ItemKind::Func(func) = item else {
                        return None;
                    };
                    func.name(db)
                        .to_opt()
                        .is_some_and(|name| name.data(db).as_str() == member_name.as_str())
                        .then_some(func)
                })
            })
            .collect(),
        FrozenItemLocator::TraitMember {
            trait_path,
            member_name,
            ..
        } => graph
            .items_dfs(db)
            .filter_map(|item| {
                let ItemKind::Trait(trait_) = item else {
                    return None;
                };
                if authored_path(db, graph, trait_.scope())?.as_str() != trait_path.as_str() {
                    return None;
                }
                Some(trait_.scope())
            })
            .flat_map(|scope| {
                graph.child_items(scope).filter_map(|item| {
                    let ItemKind::Func(func) = item else {
                        return None;
                    };
                    func.name(db)
                        .to_opt()
                        .is_some_and(|name| name.data(db).as_str() == member_name.as_str())
                        .then_some(func)
                })
            })
            .collect(),
    };
    match matches.as_slice() {
        [func] => Ok(*func),
        [] => Err(invalid("quoted authored function locator has no match")),
        _ => Err(invalid("quoted authored function locator is ambiguous")),
    }
}

fn locate_authored_nominal<'db>(
    db: &'db dyn HirDb,
    root: &FrozenQuoteNominal,
) -> Option<crate::analysis::ty::adt_def::AdtRef<'db>> {
    let file = db.workspace().get(db, &root.owner.file)?;
    let base = base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
    let matches = base
        .items_dfs(db)
        .filter_map(|item| {
            let adt = crate::analysis::ty::adt_def::AdtRef::try_from_item(item)?;
            let path = authored_path(db, base, adt.scope())?;
            authored_locator_path_matches(&root.owner.path, &path).then_some(adt)
        })
        .collect::<Vec<_>>();
    matches
        .as_slice()
        .first()
        .copied()
        .filter(|_| matches.len() == 1)
}

fn locate_authored_trait<'db>(db: &'db dyn HirDb, root: &FrozenQuoteTrait) -> Option<Trait<'db>> {
    let file = db.workspace().get(db, &root.owner.file)?;
    let base = base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
    let matches = base
        .scopes
        .keys()
        .filter_map(|scope| scope.to_item())
        .filter_map(|item| {
            let ItemKind::Trait(trait_) = item else {
                return None;
            };
            let path = authored_path(db, base, trait_.scope())?;
            authored_locator_path_matches(&root.owner.path, &path).then_some(trait_)
        })
        .collect::<Vec<_>>();
    matches
        .as_slice()
        .first()
        .copied()
        .filter(|_| matches.len() == 1)
}

fn authored_locator_path_matches(full_path: &str, file_path: &str) -> bool {
    full_path == file_path || full_path.ends_with(&format!("::{file_path}"))
}

fn quoted_field_type<'db>(
    db: &'db dyn HirDb,
    frozen: &FrozenQuoteFieldType,
    generic_names: &[IdentId<'db>],
) -> Result<TypeId<'db>, String> {
    match frozen {
        FrozenQuoteFieldType::TargetParam(index) => {
            let name = generic_names
                .get(*index)
                .ok_or_else(|| invalid("quoted target parameter index is missing"))?;
            Ok(TypeId::new(
                db,
                TypeKind::Path(Partial::Present(PathId::from_ident(db, *name))),
            ))
        }
        FrozenQuoteFieldType::Primitive(primitive) => {
            let primitive = quote_hir_primitive(*primitive)
                .ok_or_else(|| invalid("quoted field primitive is unsupported"))?;
            Ok(TypeId::new(db, TypeKind::Primitive(primitive)))
        }
        FrozenQuoteFieldType::Tuple(elements) => Ok(TypeId::new(
            db,
            TypeKind::Tuple(TupleTypeId::new(
                db,
                elements
                    .iter()
                    .map(|element| {
                        quoted_field_type(db, element, generic_names).map(Partial::Present)
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            )),
        )),
        FrozenQuoteFieldType::Ptr(inner) => Ok(TypeId::new(
            db,
            TypeKind::Ptr(Partial::Present(quoted_field_type(
                db,
                inner,
                generic_names,
            )?)),
        )),
    }
}

fn quoted_type_hole<'db>(
    builder: &mut HirBuilder<'_, 'db, GenerationDesugared>,
    quote: &Quote<'db>,
    node: &FrozenCodeNode,
    slot: u32,
    target: &TargetContext<'_, 'db>,
    generic_names: &[IdentId<'db>],
) -> Result<TypeId<'db>, String> {
    let db = builder.db();
    if quote.holes.get(slot as usize).map(|hole| hole.position) != Some(QuoteHolePosition::Type) {
        return Err(invalid("quoted type splice is not a type hole"));
    }
    let Some(FrozenCodeHole::Value(value, field_type)) = node.holes.get(slot as usize) else {
        return Err(invalid("quoted type hole is missing"));
    };
    match value.as_ref() {
        FrozenMetaValue::Structural {
            item: MetaItem::Record,
            owner,
            kind: FrozenHandleKind::Record,
        } if owner == target.target => {
            builder.generated_record_type_with_names(target.owner, generic_names)
        }
        FrozenMetaValue::Structural {
            item: MetaItem::FieldType,
            ..
        } => {
            let (variant_index, index) = target.field_coordinate(value, true)?;
            if !generic_names.is_empty() {
                return field_type
                    .as_ref()
                    .ok_or_else(|| invalid("quoted generic field type needs checked reification"))
                    .and_then(|frozen| quoted_field_type(db, frozen, generic_names));
            }
            target
                .field_hir_type(variant_index, index)
                .ok_or_else(|| invalid("quoted field type is missing"))
        }
        _ => Err(invalid("quoted type lacks target authority")),
    }
}

fn quoted_record_generics<'db>(
    db: &'db dyn HirDb,
    owner: crate::analysis::ty::adt_def::AdtRef<'db>,
    const_types: &[Option<SemPrimTy>],
) -> Result<(GenericParamListId<'db>, Vec<IdentId<'db>>), String> {
    let source = match owner {
        crate::analysis::ty::adt_def::AdtRef::Struct(owner) => GenericParamOwner::Struct(owner),
        crate::analysis::ty::adt_def::AdtRef::Enum(owner) => GenericParamOwner::Enum(owner),
    }
    .params_list(db);
    if source.data(db).len() != const_types.len() {
        return Err(invalid(
            "quoted target parameter count changed after freezing",
        ));
    }
    let original_names = source
        .data(db)
        .iter()
        .map(|param| {
            param
                .name()
                .to_opt()
                .ok_or_else(|| invalid("quoted target parameter has no name"))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut copied = Vec::new();
    let mut names = Vec::new();
    for (index, (param, const_type)) in source.data(db).iter().zip(const_types).enumerate() {
        match param {
            GenericParam::Type(param) => {
                if const_type.is_some() {
                    return Err(invalid("quoted target type parameter has a const type"));
                }
                let name = IdentId::new(
                    db,
                    format!(
                        "<quote-record-type-{index}:{}>",
                        original_names[index].data(db)
                    ),
                );
                let mut param = param.clone();
                param.name = Partial::Present(name);
                param.default_ty = None;
                names.push(name);
                copied.push(GenericParam::Type(param));
            }
            GenericParam::Const(param) => {
                let primitive = const_type
                    .and_then(quote_hir_primitive)
                    .ok_or_else(|| invalid("quoted target const parameter type is unsupported"))?;
                let name = IdentId::new(
                    db,
                    format!(
                        "<quote-record-const-{index}:{}>",
                        original_names[index].data(db)
                    ),
                );
                let mut param = param.clone();
                param.name = Partial::Present(name);
                param.ty = Partial::Present(TypeId::new(db, TypeKind::Primitive(primitive)));
                param.default = None;
                names.push(name);
                copied.push(GenericParam::Const(param));
            }
        }
    }
    let renames = original_names
        .into_iter()
        .zip(names.iter().copied())
        .collect();
    for param in &mut copied {
        if let GenericParam::Type(param) = param {
            param.bounds = param
                .bounds
                .iter()
                .map(|bound| rebind_owner_bound(db, bound, &renames))
                .collect::<Result<Vec<_>, String>>()?;
        }
    }
    Ok((GenericParamListId::new(db, copied), names))
}

enum QuoteSignatureRoot {
    Nominal(FrozenQuoteNominal),
    Trait(FrozenQuoteTrait),
}

fn quoted_function_generics<'db>(
    db: &'db dyn HirDb,
    function: &crate::hir_def::QuoteFunc<'db>,
    func_index: u32,
    inherited: GenericParamListId<'db>,
    bound_sites: &[Vec<Option<u32>>],
    type_defaults: &[Option<FrozenQuoteSignatureType>],
    const_types: &[Option<SemPrimTy>],
    roots: &[FrozenQuoteTrait],
    nominal_types: &mut Vec<(TypeId<'db>, QuoteSignatureRoot)>,
) -> Result<(GenericParamListId<'db>, Vec<IdentId<'db>>), String> {
    let mut params = inherited.data(db).to_vec();
    let mut names = Vec::new();
    if bound_sites.len() != function.generic_params.data(db).len()
        || type_defaults.len() != function.generic_params.data(db).len()
        || const_types.len() != function.generic_params.data(db).len()
    {
        return Err(invalid(
            "quoted function generic bounds changed after freezing",
        ));
    }
    for (index, param) in function.generic_params.data(db).iter().enumerate() {
        let sites = &bound_sites[index];
        let default = &type_defaults[index];
        let const_ty = &const_types[index];
        if let GenericParam::Const(param) = param {
            if !param.name.is_present()
                || !sites.is_empty()
                || default.is_some()
                || param.default.is_some()
            {
                return Err(invalid(
                    "quoted const parameter default needs checked reification",
                ));
            }
            let primitive = const_ty
                .and_then(quote_hir_primitive)
                .ok_or_else(|| invalid("quoted const parameter type is not a checked scalar"))?;
            let name = IdentId::new(db, format!("<quote-function-const-{func_index}-{index}>"));
            let mut param = param.clone();
            param.name = Partial::Present(name);
            param.ty = Partial::Present(TypeId::new(db, TypeKind::Primitive(primitive)));
            names.push(name);
            params.push(GenericParam::Const(param));
            continue;
        }
        let GenericParam::Type(param) = param else {
            unreachable!();
        };
        if const_ty.is_some() {
            return Err(invalid("quoted type parameter has a const type"));
        }
        if !param.name.is_present()
            || param.default_ty.is_some() != default.is_some()
            || sites.len() != param.bounds.len()
        {
            return Err(invalid(
                "quoted function generic bounds and defaults need checked reification",
            ));
        }
        let bounds = param
            .bounds
            .iter()
            .zip(sites)
            .map(|(bound, site)| quoted_trait_bound(db, bound, *site, roots))
            .collect::<Result<Vec<_>, String>>()?;
        let name = IdentId::new(db, format!("<quote-function-type-{func_index}-{index}>"));
        let mut param = param.clone();
        param.name = Partial::Present(name);
        param.bounds = bounds;
        param.default_ty = default
            .as_ref()
            .map(|default| quoted_signature_type(db, default, &names, nominal_types))
            .transpose()?;
        names.push(name);
        params.push(GenericParam::Type(param));
    }
    Ok((GenericParamListId::new(db, params), names))
}

fn quoted_trait_bound<'db>(
    db: &'db dyn HirDb,
    bound: &TypeBound<'db>,
    site: Option<u32>,
    roots: &[FrozenQuoteTrait],
) -> Result<TypeBound<'db>, String> {
    let TypeBound::Trait(trait_ref) = bound else {
        return Err(invalid("quoted kind bound needs checked reification"));
    };
    let Partial::Present(source) = trait_ref.path(db) else {
        return Err(invalid("quoted trait bound path is missing"));
    };
    if source.strip_generic_args(db) != source {
        return Err(invalid(
            "quoted trait bound arguments need checked reification",
        ));
    }
    let site = site.ok_or_else(|| invalid("quoted trait bound has no checked root"))?;
    let mut matching = roots.iter().filter(|root| root.site == site);
    let root = matching
        .next()
        .ok_or_else(|| invalid("quoted trait bound root is missing"))?;
    if matching.next().is_some() {
        return Err(invalid("quoted trait bound root is ambiguous"));
    }
    Ok(TypeBound::Trait(TraitRefId::new(
        db,
        Partial::Present(bound_quote_trait_path(db, root)?),
    )))
}

fn quoted_signature_type<'db>(
    db: &'db dyn HirDb,
    frozen: &FrozenQuoteSignatureType,
    generic_names: &[IdentId<'db>],
    nominal_types: &mut Vec<(TypeId<'db>, QuoteSignatureRoot)>,
) -> Result<TypeId<'db>, String> {
    quoted_signature_type_with_owner(db, frozen, &[], generic_names, nominal_types)
}

fn quoted_signature_type_with_owner<'db>(
    db: &'db dyn HirDb,
    frozen: &FrozenQuoteSignatureType,
    owner_names: &[IdentId<'db>],
    generic_names: &[IdentId<'db>],
    nominal_types: &mut Vec<(TypeId<'db>, QuoteSignatureRoot)>,
) -> Result<TypeId<'db>, String> {
    match frozen {
        FrozenQuoteSignatureType::Primitive(primitive) => {
            let primitive = quote_hir_primitive(*primitive)
                .ok_or_else(|| invalid("quoted primitive signature type is unsupported"))?;
            Ok(TypeId::new(db, TypeKind::Primitive(primitive)))
        }
        FrozenQuoteSignatureType::Hole(_) => {
            Err(invalid("quoted type splice needs a checked target context"))
        }
        FrozenQuoteSignatureType::SelfType => Ok(TypeId::new(
            db,
            TypeKind::Path(Partial::Present(PathId::from_ident(
                db,
                IdentId::make_self_ty(db),
            ))),
        )),
        FrozenQuoteSignatureType::FunctionGeneric(index) => {
            let name = generic_names
                .get(*index as usize)
                .ok_or_else(|| invalid("quoted function generic index is missing"))?;
            Ok(TypeId::new(
                db,
                TypeKind::Path(Partial::Present(PathId::from_ident(db, *name))),
            ))
        }
        FrozenQuoteSignatureType::OwnerGeneric(index) => {
            let name = owner_names
                .get(*index as usize)
                .ok_or_else(|| invalid("read-back owner generic has no destination binding"))?;
            Ok(TypeId::new(
                db,
                TypeKind::Path(Partial::Present(PathId::from_ident(db, *name))),
            ))
        }
        FrozenQuoteSignatureType::Nominal {
            root,
            arguments,
            arguments_given,
        } => {
            let ty = quoted_rooted_type(
                db,
                bound_quote_item_path(db, &root.ingot, &root.path)?,
                arguments,
                *arguments_given,
                owner_names,
                generic_names,
                nominal_types,
            )?;
            nominal_types.push((ty, QuoteSignatureRoot::Nominal(root.clone())));
            Ok(ty)
        }
        FrozenQuoteSignatureType::Trait {
            root,
            arguments,
            arguments_given,
        } => {
            let ty = quoted_rooted_type(
                db,
                bound_quote_trait_path(db, root)?,
                arguments,
                *arguments_given,
                owner_names,
                generic_names,
                nominal_types,
            )?;
            nominal_types.push((ty, QuoteSignatureRoot::Trait(root.clone())));
            Ok(ty)
        }
        FrozenQuoteSignatureType::Mode(mode, inner) => Ok(TypeId::new(
            db,
            TypeKind::Mode(
                *mode,
                Partial::Present(quoted_signature_type_with_owner(
                    db,
                    inner,
                    owner_names,
                    generic_names,
                    nominal_types,
                )?),
            ),
        )),
        FrozenQuoteSignatureType::Ptr(inner) => Ok(TypeId::new(
            db,
            TypeKind::Ptr(Partial::Present(quoted_signature_type_with_owner(
                db,
                inner,
                owner_names,
                generic_names,
                nominal_types,
            )?)),
        )),
        FrozenQuoteSignatureType::Tuple(elements) => Ok(TypeId::new(
            db,
            TypeKind::Tuple(TupleTypeId::new(
                db,
                elements
                    .iter()
                    .map(|element| {
                        quoted_signature_type_with_owner(
                            db,
                            element,
                            owner_names,
                            generic_names,
                            nominal_types,
                        )
                        .map(Partial::Present)
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            )),
        )),
        FrozenQuoteSignatureType::Never => Ok(TypeId::new(db, TypeKind::Never)),
    }
}

fn quoted_rooted_type<'db>(
    db: &'db dyn HirDb,
    mut path: PathId<'db>,
    arguments: &[FrozenQuoteSignatureType],
    arguments_given: bool,
    owner_names: &[IdentId<'db>],
    generic_names: &[IdentId<'db>],
    roots: &mut Vec<(TypeId<'db>, QuoteSignatureRoot)>,
) -> Result<TypeId<'db>, String> {
    if arguments_given || !arguments.is_empty() {
        let PathKind::Ident { ident, .. } = path.kind(db) else {
            return Err(invalid("quoted rooted type path has no final item"));
        };
        let args = arguments
            .iter()
            .map(|argument| {
                quoted_signature_type_with_owner(db, argument, owner_names, generic_names, roots)
                    .map(|ty| {
                        GenericArg::Type(TypeGenericArg {
                            ty: Partial::Present(ty),
                        })
                    })
            })
            .collect::<Result<Vec<_>, String>>()?;
        path = PathId::new(
            db,
            PathKind::Ident {
                ident,
                generic_args: GenericArgListId::given(db, args),
            },
            path.parent(db),
        );
    }
    Ok(TypeId::new(db, TypeKind::Path(Partial::Present(path))))
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub(crate) enum QuoteHygiene<'db> {
    Function {
        func: Func<'db>,
        rooted: Vec<(ExprId, Func<'db>)>,
        variants: Vec<(FrozenQuoteConstructorSite, QuoteConstructorIdentity<'db>)>,
        qualified_consts: Vec<(ExprId, Trait<'db>)>,
        const_predicates: Vec<QuoteConstHygiene<'db>>,
        nominal_types: Vec<(TypeId<'db>, crate::analysis::ty::adt_def::AdtRef<'db>)>,
        trait_types: Vec<(TypeId<'db>, Trait<'db>)>,
        locals: Vec<(PatId, Vec<ExprId>)>,
        params: Vec<(ExprId, usize)>,
        effects: Vec<(ExprId, usize)>,
        generics: Vec<(ExprId, IdentId<'db>)>,
        method_calls: Vec<ExprId>,
        fragment_method_calls: Vec<QuoteMethodCall<'db>>,
        provider_scope: ScopeId<'db>,
        site_span: Option<std::ops::Range<usize>>,
    },
    TraitImpl {
        implementation: crate::hir_def::ImplTrait<'db>,
        expected: Trait<'db>,
        const_rooted: Vec<(Body<'db>, Vec<(ExprId, Func<'db>)>)>,
        provider_scope: ScopeId<'db>,
        nominal_types: Vec<(TypeId<'db>, crate::analysis::ty::adt_def::AdtRef<'db>)>,
        trait_types: Vec<(TypeId<'db>, Trait<'db>)>,
        site_span: Option<std::ops::Range<usize>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub(crate) struct QuoteConstHygiene<'db> {
    body: Body<'db>,
    rooted: Vec<(ExprId, Func<'db>)>,
    generics: Vec<(ExprId, IdentId<'db>)>,
}

fn quoted_expr_sources<'db>(
    db: &'db dyn HirDb,
    code: &FrozenCode,
    target: &TargetContext<'_, 'db>,
) -> Result<Vec<Option<QuoteExprSource<'db>>>, String> {
    code.nodes
        .iter()
        .map(|node| {
            let Some(key) = &node.template else {
                return Ok(None);
            };
            let file = db
                .workspace()
                .get(db, key.owner.file())
                .ok_or_else(|| invalid("quote owner file is missing"))?;
            let base = base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
            let owner = locate_authored_function(db, base, &key.owner)?;
            let body = owner
                .body(db)
                .ok_or_else(|| invalid("quote owner body is missing"))?;
            let (template, root, open_names, member_holes, has_block_items) = match key.source {
                FrozenTemplateSource::Quote => {
                    let Partial::Present(Expr::Quote(quote)) = &body.exprs(db)[key.expr] else {
                        return Err(invalid("quote site no longer names a quote"));
                    };
                    if !matches!(quote.fragment, QuoteFragment::Expr) {
                        return Ok(None);
                    }
                    if quote.holes.len() != node.holes.len() {
                        return Err(invalid("quoted splice count changed after freezing"));
                    }
                    let mut member_holes = Vec::new();
                    for (index, hole) in quote.holes.iter().enumerate() {
                        if !matches!(
                            hole.position,
                            QuoteHolePosition::Member | QuoteHolePosition::FieldLabel
                        ) {
                            continue;
                        }
                        let Some(FrozenCodeHole::Value(value, _)) = node.holes.get(index) else {
                            return Err(invalid("quoted member hole is missing"));
                        };
                        let field = target.field_index(value, false)?;
                        member_holes.push((
                            u32::try_from(index)
                                .map_err(|_| invalid("quoted member slot exceeds u32"))?,
                            target.field_name(field)?,
                        ));
                    }
                    (
                        quote.template,
                        quote.template.expr(db),
                        quote.open.clone(),
                        member_holes,
                        !quote.block_items.is_empty(),
                    )
                }
                FrozenTemplateSource::CheckedBody => {
                    if key.expr != body.expr(db) || !node.holes.is_empty() {
                        return Err(invalid("checked body changed after read-back"));
                    }
                    let open_names = owner
                        .params(db)
                        .enumerate()
                        .map(|(index, param)| {
                            param.name(db).unwrap_or_else(|| {
                                IdentId::new(db, format!("<unnamed-read-back-param-{index}>"))
                            })
                        })
                        .collect();
                    (body, key.expr, open_names, Vec::new(), false)
                }
            };
            let rooted_funcs = node
                .rooted_funcs
                .iter()
                .filter(|root| root.func.is_none() && root.assoc_const.is_none())
                .map(|root| {
                    let path = bound_quote_function_path(db, root)?;
                    let file = db
                        .workspace()
                        .get(db, root.owner.file())
                        .ok_or_else(|| invalid("quoted root file is missing"))?;
                    let base = base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
                    let expected = locate_authored_function(db, base, &root.owner)?;
                    Ok((root.site, path, expected))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let qualified_paths = node
                .qualified_consts
                .iter()
                .map(|qualified| {
                    if key.source != FrozenTemplateSource::CheckedBody || qualified.func != 0 {
                        return Err(invalid("qualified fragment constant needs a checked body"));
                    }
                    let (path, expected) =
                        bound_quote_qualified_const_path(db, template, node, qualified)?;
                    Ok((qualified.site, path, expected))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let selected_methods = node
                .selected_methods
                .iter()
                .map(|(site, locator)| {
                    let file = db
                        .workspace()
                        .get(db, locator.file())
                        .ok_or_else(|| invalid("read-back method file is missing"))?;
                    let base = base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
                    locate_authored_function(db, base, locator).map(|method| (*site, method))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(Some(QuoteExprSource {
                template,
                root,
                open_names,
                member_holes,
                rooted_funcs,
                qualified_paths,
                selected_methods,
                provider_scope: owner.scope(),
                has_block_items,
            }))
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, salsa::Update)]
pub(super) struct QuoteConstructorIdentity<'db> {
    pub(super) owner: crate::analysis::ty::adt_def::AdtRef<'db>,
    pub(super) variant_index: Option<u16>,
}

fn reify_quote_constructor<'db>(
    db: &'db dyn HirDb,
    root: &FrozenQuoteConstructor,
) -> Result<(PathId<'db>, QuoteConstructorIdentity<'db>), String> {
    use crate::analysis::ty::adt_def::AdtRef;
    let owner = locate_authored_nominal(db, &root.owner)
        .ok_or_else(|| invalid("quoted constructor owner is absent from authored HIR"))?;
    let mut path = bound_quote_item_path(db, &root.owner.ingot, &root.owner.path)?;
    match (owner, root.variant_index) {
        (AdtRef::Struct(_), None) => {}
        (AdtRef::Enum(owner), Some(index)) => {
            if usize::from(index) >= owner.len_variants(db) {
                return Err(invalid("quoted variant index is outside its authored enum"));
            }
            let variant = crate::hir_def::EnumVariant::new(owner, usize::from(index));
            let name = variant
                .name(db)
                .ok_or_else(|| invalid("quoted variant has no authored name"))?;
            path = path.push_str(db, name);
        }
        _ => {
            return Err(invalid(
                "quoted constructor kind disagrees with its nominal owner",
            ));
        }
    }
    Ok((
        path,
        QuoteConstructorIdentity {
            owner,
            variant_index: root.variant_index,
        },
    ))
}

pub(super) struct QuoteExprTypes<'db> {
    pub(super) variants: Vec<(
        FrozenQuoteConstructorSite,
        PathId<'db>,
        QuoteConstructorIdentity<'db>,
    )>,
    pub(super) rooted_paths: Vec<(ExprId, PathId<'db>)>,
    pub(super) cast_types: Vec<(ExprId, TypeId<'db>)>,
    pub(super) local_types: Vec<(crate::hir_def::StmtId, TypeId<'db>)>,
    pub(super) record_paths: Vec<(ExprId, PathId<'db>)>,
    pub(super) nominal_types: Vec<(TypeId<'db>, crate::analysis::ty::adt_def::AdtRef<'db>)>,
}

/// Reify the frozen fragment's types only after its destination binder frame
/// is known. Item signatures and body fragments share the same type reifier.
pub(super) fn reify_quote_expr_types<'db>(
    db: &'db dyn HirDb,
    node: &FrozenCodeNode,
    source: &QuoteExprSource<'db>,
    owner_names: &[IdentId<'db>],
    function_names: &[IdentId<'db>],
) -> Result<QuoteExprTypes<'db>, String> {
    let mut roots = Vec::new();
    let mut reify = |ty: &FrozenQuoteSignatureType| {
        quoted_signature_type_with_owner(db, ty, owner_names, function_names, &mut roots)
    };
    let rooted_paths = source
        .rooted_funcs
        .iter()
        .map(|(site, path, _)| {
            let frozen = node
                .rooted_funcs
                .iter()
                .find(|root| root.site == *site)
                .ok_or_else(|| invalid("quoted rooted function has no frozen identity"))?;
            let path = if let Some(arguments) = &frozen.type_arguments {
                let arguments = arguments
                    .iter()
                    .map(&mut reify)
                    .collect::<Result<Vec<_>, _>>()?;
                quote_function_arguments(db, *path, arguments)?
            } else {
                *path
            };
            Ok((*site, path))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let local_types = node
        .local_types
        .iter()
        .map(|local| reify(&local.ty).map(|ty| (local.stmt, ty)))
        .collect::<Result<Vec<_>, String>>()?;
    let record_paths = node
        .record_types
        .iter()
        .map(|(site, frozen)| {
            let ty = reify(frozen)?;
            let TypeKind::Path(Partial::Present(path)) = ty.data(db) else {
                return Err(invalid("read-back record constructor needs a nominal type"));
            };
            Ok((*site, *path))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let nominal_types = roots
        .into_iter()
        .map(|(ty, root)| {
            let QuoteSignatureRoot::Nominal(root) = root else {
                return Err(invalid("read-back type needs a nominal identity"));
            };
            let expected = locate_authored_nominal(db, &root)
                .ok_or_else(|| invalid("read-back nominal type is absent from authored HIR"))?;
            Ok((ty, expected))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let cast_types = node
        .cast_primitives
        .iter()
        .map(|(site, primitive)| {
            let primitive = quote_hir_primitive(*primitive)
                .ok_or_else(|| invalid("read-back cast primitive is unsupported"))?;
            Ok((*site, TypeId::new(db, TypeKind::Primitive(primitive))))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let variants = node
        .rooted_constructors
        .iter()
        .map(|root| {
            reify_quote_constructor(db, root).map(|(path, variant)| (root.site, path, variant))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(QuoteExprTypes {
        variants,
        rooted_paths,
        cast_types,
        local_types,
        record_paths,
        nominal_types,
    })
}

fn emit_closed_quoted_items<'db>(
    db: &'db dyn HirDb,
    ctxt: &mut FileLowerCtxt<'db>,
    request: &FrozenGeneration,
    origin: GenerationDesugared,
    target: &TargetContext<'_, 'db>,
) -> Result<Vec<QuoteHygiene<'db>>, String> {
    let FrozenMetaValue::Code(code) = &request.value else {
        return Err(invalid("expected a quoted generation result"));
    };
    let expr_sources = quoted_expr_sources(db, code, target)?;
    let mut hygiene = Vec::new();
    let mut ordinal = 0;
    emit_quoted_item_node(
        db,
        ctxt,
        request,
        &origin,
        target,
        code,
        &expr_sources,
        code.root,
        &mut ordinal,
        &mut hygiene,
    )?;
    Ok(hygiene)
}

#[allow(clippy::too_many_arguments)]
fn emit_quoted_item_node<'db>(
    db: &'db dyn HirDb,
    ctxt: &mut FileLowerCtxt<'db>,
    request: &FrozenGeneration,
    origin: &GenerationDesugared,
    target: &TargetContext<'_, 'db>,
    code: &FrozenCode,
    expr_sources: &[Option<QuoteExprSource<'db>>],
    code_root: usize,
    ordinal: &mut usize,
    hygiene: &mut Vec<QuoteHygiene<'db>>,
) -> Result<(), String> {
    let node = code
        .nodes
        .get(code_root)
        .ok_or_else(|| invalid("quoted code root is missing"))?;
    let Some(key) = &node.template else {
        if !node.holes.is_empty() {
            return Err(invalid("empty item code has splices"));
        }
        return Ok(());
    };
    let file = db
        .workspace()
        .get(db, key.owner.file())
        .ok_or_else(|| invalid("quote owner file is missing"))?;
    let provider_base = base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
    let owner = locate_authored_function(db, provider_base, &key.owner)?;
    let body = owner
        .body(db)
        .ok_or_else(|| invalid("quote owner body is missing"))?;
    let Partial::Present(Expr::Quote(quote)) = &body.exprs(db)[key.expr] else {
        return Err(invalid("quote site no longer names a quote"));
    };
    if quote.holes.len() != node.holes.len() {
        return Err(invalid("quoted splice count changed after freezing"));
    }
    let QuoteFragment::Items(items) = &quote.fragment else {
        return Err(invalid("generation requires item code"));
    };
    let mut func_index = 0;
    for item in items {
        if let QuoteItem::Hole(slot) = item {
            if quote.holes.get(*slot as usize).map(|hole| hole.position)
                != Some(QuoteHolePosition::Items)
            {
                return Err(invalid("quoted item splice position changed"));
            }
            let Some(FrozenCodeHole::Code(child)) = node.holes.get(*slot as usize) else {
                return Err(invalid("quoted item splice is not item code"));
            };
            emit_quoted_item_node(
                db,
                ctxt,
                request,
                origin,
                target,
                code,
                expr_sources,
                *child,
                ordinal,
                hygiene,
            )?;
            continue;
        }
        let item_ordinal = *ordinal;
        *ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| invalid("quoted item count exceeds usize"))?;
        ctxt.enter_id_namespace(TrackedItemVariant::GeneratedSite(Box::new(
            crate::hir_def::GeneratedSiteKey {
                target: request.target.clone(),
                provider: request.provider.clone(),
                attribute_index: request.attribute_index,
                ordinal: item_ordinal,
            },
        )));
        let emitted = match item {
            QuoteItem::Func(function) => {
                let (generic_params, generic_names) =
                    quoted_record_generics(db, target.owner, &request.target_const_param_types)?;
                let source = top_mod_ast(db, ctxt.top_mod());
                let mut builder = HirBuilder::new(ctxt, origin.clone(), source.syntax());
                Ok(vec![emit_quoted_function(
                    db,
                    &mut builder,
                    request,
                    target,
                    quote,
                    node,
                    code,
                    expr_sources,
                    code_root,
                    function,
                    func_index,
                    generic_params,
                    &generic_names,
                    false,
                    owner.scope(),
                )?])
            }
            QuoteItem::Impl(_) if items.len() == 1 => emit_closed_quoted_impl(
                db,
                ctxt,
                request,
                origin.clone(),
                target,
                quote,
                node,
                code,
                expr_sources,
                code_root,
                item,
                owner.scope(),
            ),
            _ => Err(invalid("quoted item list needs checked transplantation")),
        };
        ctxt.leave_id_namespace();
        hygiene.extend(emitted?);
        if matches!(item, QuoteItem::Func(_)) {
            func_index += 1;
        }
    }
    Ok(())
}

fn emit_quoted_function<'db>(
    db: &'db dyn HirDb,
    builder: &mut HirBuilder<'_, 'db, GenerationDesugared>,
    request: &FrozenGeneration,
    target: &TargetContext<'_, 'db>,
    quote: &Quote<'db>,
    node: &FrozenCodeNode,
    code: &FrozenCode,
    expr_sources: &[Option<QuoteExprSource<'db>>],
    code_root: usize,
    function: &crate::hir_def::QuoteFunc<'db>,
    func_index: u32,
    generic_params: GenericParamListId<'db>,
    generic_names: &[IdentId<'db>],
    is_method: bool,
    provider_scope: ScopeId<'db>,
) -> Result<QuoteHygiene<'db>, String> {
    let function_types = node
        .function_types
        .get(func_index as usize)
        .ok_or_else(|| invalid("quoted function has no checked signature"))?;
    let name = quoted_name(function.name, node, target)?;
    let Partial::Present(params) = function.params else {
        return Err(invalid("quoted function parameters are missing"));
    };
    if !function.attrs.data(db).is_empty()
        || function.modifiers.is_unsafe
        || function.modifiers.is_extern
    {
        return Err(invalid(
            "quoted function header needs name and type rewriting",
        ));
    }
    let mut nominal_types = Vec::new();
    let (generic_params, function_generic_names) = quoted_function_generics(
        db,
        function,
        func_index,
        generic_params,
        &function_types.generic_bound_sites,
        &function_types.generic_type_defaults,
        &function_types.generic_const_types,
        &node.rooted_traits,
        &mut nominal_types,
    )?;
    if function.effects.data(db).len() != function_types.effects.len() {
        return Err(invalid("quoted method effects changed after freezing"));
    }
    let mut effect_names = Vec::new();
    let effects = EffectParamListId::new(
        db,
        function
            .effects
            .data(db)
            .iter()
            .zip(&function_types.effects)
            .enumerate()
            .map(|(index, (effect, frozen))| {
                let ty = frozen
                    .as_ref()
                    .ok_or_else(|| invalid("quoted effect key needs checked reification"))?;
                let ty =
                    quoted_signature_type(db, ty, &function_generic_names, &mut nominal_types)?;
                let name = effect
                    .name
                    .map(|_| IdentId::new(db, format!("<quote-effect-{func_index}-{index}>")));
                effect_names.push(name);
                let mut copied = effect.clone();
                copied.name = name;
                copied.key_ty = Partial::Present(ty);
                Ok(copied)
            })
            .collect::<Result<Vec<_>, String>>()?,
    );
    if function.where_clause.predicates.len() != function_types.where_predicates.len() {
        return Err(invalid(
            "quoted method where predicates changed after freezing",
        ));
    }
    let mut where_bounds = Vec::new();
    let mut const_where = Vec::new();
    for (index, (predicate, frozen)) in function
        .where_clause
        .predicates
        .iter()
        .zip(&function_types.where_predicates)
        .enumerate()
    {
        match (predicate, frozen) {
            (
                crate::hir_def::QuoteWherePredicate::Bound(predicate),
                FrozenQuoteWherePredicate::Bound { ty, trait_sites },
            ) => {
                if predicate.ty.to_opt().is_none() || predicate.bounds.len() != trait_sites.len() {
                    return Err(invalid("quoted method where bound changed after freezing"));
                }
                let ty =
                    quoted_signature_type(db, ty, &function_generic_names, &mut nominal_types)?;
                let bounds = predicate
                    .bounds
                    .iter()
                    .zip(trait_sites)
                    .map(|(bound, site)| quoted_trait_bound(db, bound, *site, &node.rooted_traits))
                    .collect::<Result<Vec<_>, String>>()?;
                where_bounds.push(WherePredicate {
                    ty: Partial::Present(ty),
                    bounds,
                });
            }
            (
                crate::hir_def::QuoteWherePredicate::Const(expr),
                FrozenQuoteWherePredicate::Const,
            ) => const_where.push((index as u32, *expr)),
            _ => {
                return Err(invalid(
                    "quoted method where predicate changed after freezing",
                ));
            }
        }
    }
    let generic_uses = node
        .generic_uses
        .iter()
        .filter(|use_| use_.func == func_index && use_.where_predicate.is_none())
        .map(|use_| {
            let name = function_generic_names
                .get(use_.index as usize)
                .ok_or_else(|| invalid("quoted generic use has no parameter"))?;
            Ok((use_.site, *name))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let function_param_uses = node
        .param_uses
        .iter()
        .filter(|use_| use_.func == func_index)
        .collect::<Vec<_>>();
    let mut param_uses = function_param_uses
        .iter()
        .map(|use_| {
            let param = params
                .data(db)
                .get(use_.index as usize)
                .ok_or_else(|| invalid("quoted parameter index is missing"))?;
            let name = param
                .name()
                .ok_or_else(|| invalid("quoted parameter use has no name"))?;
            Ok((use_.site, name))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut param_indices = function_param_uses
        .iter()
        .map(|use_| use_.index as usize)
        .collect::<Vec<_>>();
    let function_self_uses = node
        .self_uses
        .iter()
        .filter(|use_| use_.func == func_index)
        .collect::<Vec<_>>();
    if is_method {
        param_uses.extend(
            function_self_uses
                .iter()
                .map(|use_| (use_.site, IdentId::make_self(db))),
        );
        param_indices.extend(std::iter::repeat_n(0, function_self_uses.len()));
    } else if !function_self_uses.is_empty() {
        return Err(invalid("quoted self name requires a method"));
    }
    let body_param_count = param_uses.len();
    let effect_uses = node
        .effect_uses
        .iter()
        .filter(|use_| use_.func == func_index)
        .collect::<Vec<_>>();
    let effect_indices = effect_uses
        .iter()
        .map(|use_| use_.index as usize)
        .collect::<Vec<_>>();
    param_uses.extend(
        effect_uses
            .iter()
            .map(|use_| {
                let name = effect_names
                    .get(use_.index as usize)
                    .copied()
                    .flatten()
                    .ok_or_else(|| invalid("quoted effect use has no bound name"))?;
                Ok((use_.site, name))
            })
            .collect::<Result<Vec<_>, String>>()?,
    );
    if function_types.params.len() != params.data(db).len() {
        return Err(invalid(
            "quoted parameter type count changed after freezing",
        ));
    }
    let params = FuncParamListId::new(
        db,
        params
            .data(db)
            .iter()
            .zip(&function_types.params)
            .map(|(param, primitive)| {
                if param.self_ty_fallback || param.is_self_param(db) {
                    if is_method {
                        return Ok(param.clone());
                    }
                    return Err(invalid("quoted self parameter needs a method"));
                }
                let Partial::Present(source_ty) = param.ty else {
                    return Err(invalid("quoted parameter type is missing"));
                };
                let ty = match source_ty.data(db) {
                    TypeKind::Hole(slot) => {
                        quoted_type_hole(builder, quote, node, *slot, target, generic_names)?
                    }
                    _ => quoted_signature_type(
                        db,
                        primitive
                            .as_ref()
                            .ok_or_else(|| invalid("quoted parameter needs checked rewriting"))?,
                        &function_generic_names,
                        &mut nominal_types,
                    )?,
                };
                let mut copied = param.clone();
                copied.ty = Partial::Present(ty);
                Ok(copied)
            })
            .collect::<Result<Vec<_>, String>>()?,
    );
    let open_bindings = params
        .data(db)
        .iter()
        .enumerate()
        .filter_map(|(index, param)| param.name().map(|name| (name, index)))
        .collect::<Vec<_>>();
    let ret_ty = match function.ret_ty {
        Some(ty) => match ty.data(db) {
            TypeKind::Hole(slot) => {
                quoted_type_hole(builder, quote, node, *slot, target, generic_names)?
            }
            _ => quoted_signature_type(
                db,
                function_types.result.as_ref().ok_or_else(|| {
                    invalid("quoted function result type needs checked rewriting")
                })?,
                &function_generic_names,
                &mut nominal_types,
            )?,
        },
        None => return Err(invalid("quoted function result type is missing")),
    };
    let root = function
        .body
        .ok_or_else(|| invalid("quoted function body is missing"))?;
    let rooted_funcs = node
        .rooted_funcs
        .iter()
        .filter(|root| root.func == Some(func_index) && root.where_predicate.is_none())
        .map(|root| {
            Ok((
                root.site,
                quoted_function_path(
                    db,
                    builder,
                    quote,
                    node,
                    root,
                    target,
                    generic_names,
                    &function_generic_names,
                    &mut nominal_types,
                )?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let qualified_consts = node
        .qualified_consts
        .iter()
        .filter(|qualified| qualified.func == func_index)
        .map(|qualified| {
            let (path, expected) =
                bound_quote_qualified_const_path(db, quote.template, node, qualified)?;
            Ok((qualified.site, path, expected))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let qualified_paths = qualified_consts
        .iter()
        .map(|(site, path, _)| (*site, *path))
        .collect::<Vec<_>>();
    let self_record_inits = node
        .self_record_inits
        .iter()
        .filter(|use_| use_.func == func_index)
        .map(|use_| use_.site)
        .collect::<Vec<_>>();
    let expected_roots = node
        .rooted_funcs
        .iter()
        .filter(|root| root.func == Some(func_index) && root.where_predicate.is_none())
        .map(|root| {
            let file = db
                .workspace()
                .get(db, root.owner.file())
                .ok_or_else(|| invalid("quoted root file is missing"))?;
            let base = base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
            locate_authored_function(db, base, &root.owner)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let locals: Vec<(PatId, IdentId<'db>, Vec<ExprId>)> = node
        .locals
        .iter()
        .enumerate()
        .filter(|(_, local)| local.func == func_index)
        .map(|(index, local)| {
            (
                local.binder,
                IdentId::new(db, format!("<quote-local-{}-{index}>", code_root)),
                local.uses.clone(),
            )
        })
        .collect();
    let variants = node
        .rooted_constructors
        .iter()
        .filter(|root| root.func == Some(func_index))
        .map(|root| {
            reify_quote_constructor(db, root).map(|(path, variant)| (root.site, path, variant))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let constructor_paths = variants
        .iter()
        .map(|(site, path, _)| (*site, *path))
        .collect::<Vec<_>>();
    let local_types = node
        .local_types
        .iter()
        .filter(|local| local.func == func_index)
        .map(|local| {
            quoted_signature_type(db, &local.ty, &function_generic_names, &mut nominal_types)
                .map(|ty| (local.stmt, ty))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut member_holes = Vec::new();
    for (index, hole) in quote.holes.iter().enumerate() {
        if !matches!(
            hole.position,
            QuoteHolePosition::Member | QuoteHolePosition::FieldLabel
        ) {
            continue;
        }
        let Some(FrozenCodeHole::Value(value, _)) = node.holes.get(index) else {
            return Err(invalid("quoted member hole is missing"));
        };
        let field = target.field_index(value, false)?;
        member_holes.push((
            u32::try_from(index).map_err(|_| invalid("quoted member slot exceeds u32"))?,
            target.field_name(field)?,
        ));
    }
    let mut spec = GeneratedFunctionSpec {
        name,
        attrs: function.attrs,
        generic_params,
        where_clause: builder.empty_where_clause(),
        params,
        effects,
        ret_ty: Some(ret_ty),
        modifiers: function.modifiers,
    };
    let mut copied = None;
    let mut const_hygiene = Vec::new();
    let func = builder.generated_func_with_body_checked(
        name,
        |builder| {
            let owner_where = if is_method {
                builder.empty_where_clause()
            } else {
                builder.quoted_owner_where_clause(target.owner, generic_names)?
            };
            let owner_const_count = owner_where.const_predicates(db).len();
            const_hygiene.extend(owner_where.const_predicates(db).iter().map(|body| {
                QuoteConstHygiene {
                    body: *body,
                    rooted: Vec::new(),
                    generics: Vec::new(),
                }
            }));
            let const_predicates = const_where
                .iter()
                .map(|(index, expr)| {
                    let rooted = node
                        .rooted_funcs
                        .iter()
                        .filter(|root| {
                            root.func == Some(func_index) && root.where_predicate == Some(*index)
                        })
                        .collect::<Vec<_>>();
                    let rooted_paths = rooted
                        .iter()
                        .map(|root| {
                            Ok((
                                root.site,
                                quoted_function_path(
                                    db,
                                    builder,
                                    quote,
                                    node,
                                    root,
                                    target,
                                    generic_names,
                                    &function_generic_names,
                                    &mut nominal_types,
                                )?,
                            ))
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    let expected_roots = rooted
                        .iter()
                        .map(|root| {
                            let file = db
                                .workspace()
                                .get(db, root.owner.file())
                                .ok_or_else(|| invalid("quoted root file is missing"))?;
                            let base =
                                base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
                            locate_authored_function(db, base, &root.owner)
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    let generics = node
                        .generic_uses
                        .iter()
                        .filter(|use_| {
                            use_.func == func_index && use_.where_predicate == Some(*index)
                        })
                        .map(|use_| {
                            let name = function_generic_names
                                .get(use_.index as usize)
                                .ok_or_else(|| invalid("quoted generic use has no parameter"))?;
                            Ok((use_.site, *name))
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    let (body, rooted_sites, generic_sites) = builder
                        .generated_quote_const_predicate(
                            u32::try_from(owner_const_count)
                                .ok()
                                .and_then(|count| count.checked_add(*index))
                                .ok_or_else(|| {
                                    invalid("quoted const predicate count exceeds u32")
                                })?,
                            quote.template,
                            *expr,
                            provider_scope,
                            &node.holes,
                            &rooted_paths,
                            &generics,
                        )?;
                    const_hygiene.push(QuoteConstHygiene {
                        body,
                        rooted: rooted_sites.into_iter().zip(expected_roots).collect(),
                        generics: generic_sites
                            .into_iter()
                            .zip(generics.into_iter().map(|(_, name)| name))
                            .collect(),
                    });
                    Ok(body)
                })
                .collect::<Result<Vec<_>, String>>()?;
            let mut predicates = owner_where.predicates(db).clone();
            predicates.extend(
                where_bounds
                    .into_iter()
                    .map(crate::hir_def::WhereClausePredicate::Type),
            );
            predicates.extend(
                const_predicates
                    .into_iter()
                    .map(crate::hir_def::WhereClausePredicate::Const),
            );
            spec.where_clause = WhereClauseId::new(db, predicates);
            Ok::<_, String>((spec, (quote.template, root)))
        },
        |body, (template, root)| {
            copied = Some(body.adopt_closed_quote_body(
                template,
                root,
                provider_scope,
                &node.holes,
                Some((code, expr_sources)),
                &open_bindings,
                generic_names,
                &function_generic_names,
                is_method,
                &rooted_funcs,
                &qualified_paths,
                &constructor_paths,
                &self_record_inits,
                &locals,
                &local_types,
                &param_uses,
                &generic_uses,
                &member_holes,
            )?);
            Ok(())
        },
    )?;
    let copied = copied.ok_or_else(|| invalid("quoted body copy is missing"))?;
    let mut rooted: Vec<_> = copied
        .rooted_paths
        .into_iter()
        .zip(expected_roots)
        .collect();
    rooted.extend(copied.fragment_rooted_paths);
    let mut qualified_consts: Vec<_> = copied
        .qualified_paths
        .into_iter()
        .zip(
            qualified_consts
                .into_iter()
                .map(|(_, _, expected)| expected),
        )
        .collect();
    qualified_consts.extend(copied.fragment_qualified_paths);
    let mut resolved_nominal_types = copied.fragment_nominal_types;
    let mut trait_types = Vec::new();
    for (site, root) in nominal_types {
        match root {
            QuoteSignatureRoot::Nominal(root) => {
                let expected = locate_authored_nominal(db, &root)
                    .ok_or_else(|| invalid("quoted nominal type is absent from authored HIR"))?;
                resolved_nominal_types.push((site, expected));
            }
            QuoteSignatureRoot::Trait(root) => {
                let expected = locate_authored_trait(db, &root)
                    .ok_or_else(|| invalid("quoted trait type is absent from authored HIR"))?;
                trait_types.push((site, expected));
            }
        }
    }
    let mut copied_param_sites = copied.params.into_iter();
    let mut params: Vec<_> = copied_param_sites
        .by_ref()
        .take(body_param_count)
        .zip(param_indices)
        .collect();
    params.extend(copied.fragment_params);
    let effects = copied_param_sites.zip(effect_indices).collect();
    let mut generics: Vec<_> = copied
        .generics
        .into_iter()
        .zip(generic_uses.into_iter().map(|(_, name)| name))
        .collect();
    generics.extend(copied.fragment_generics);
    let mut checked_variants = copied
        .constructor_sites
        .into_iter()
        .zip(variants.into_iter().map(|(_, _, variant)| variant))
        .collect::<Vec<_>>();
    checked_variants.extend(copied.fragment_constructors);
    Ok(QuoteHygiene::Function {
        func,
        rooted,
        variants: checked_variants,
        qualified_consts,
        const_predicates: const_hygiene,
        nominal_types: resolved_nominal_types,
        trait_types,
        locals: copied.locals,
        params,
        effects,
        generics,
        method_calls: copied.method_calls,
        fragment_method_calls: copied.fragment_method_calls,
        provider_scope,
        site_span: request.site_span.clone(),
    })
}

fn emit_closed_quoted_impl<'db>(
    db: &'db dyn HirDb,
    ctxt: &mut FileLowerCtxt<'db>,
    request: &FrozenGeneration,
    origin: GenerationDesugared,
    target: &TargetContext<'_, 'db>,
    quote: &Quote<'db>,
    node: &FrozenCodeNode,
    code: &FrozenCode,
    expr_sources: &[Option<QuoteExprSource<'db>>],
    code_root: usize,
    item: &QuoteItem<'db>,
    provider_scope: ScopeId<'db>,
) -> Result<Vec<QuoteHygiene<'db>>, String> {
    let QuoteItem::Impl(quoted) = item else {
        return Err(invalid(
            "quoted generation currently requires a function or trait impl",
        ));
    };
    if !quoted.generic_params.data(db).is_empty()
        || !quoted.where_clause.predicates.is_empty()
        || !quoted.attrs.data(db).is_empty()
        || quoted.selection_name.is_some()
    {
        return Err(invalid("quoted impl header needs checked transplantation"));
    }
    if quoted.items.iter().any(|item| {
        !matches!(
            item,
            QuoteItem::Const(_) | QuoteItem::Type(_) | QuoteItem::Func(_)
        )
    }) {
        return Err(invalid("quoted impl member needs checked transplantation"));
    }
    let method_count = quoted
        .items
        .iter()
        .filter(|item| matches!(item, QuoteItem::Func(_)))
        .count();
    if node.function_types.len() != method_count {
        return Err(invalid("quoted method signatures changed after freezing"));
    }
    if method_count == 0
        && (node
            .rooted_funcs
            .iter()
            .any(|root| root.assoc_const.is_none())
            || !node.locals.is_empty()
            || !node.param_uses.is_empty()
            || !node.self_uses.is_empty())
    {
        return Err(invalid(
            "quoted impl member names need checked transplantation",
        ));
    }
    let constant_count = quoted
        .items
        .iter()
        .filter(|item| matches!(item, QuoteItem::Const(_)))
        .count();
    let type_count = quoted
        .items
        .iter()
        .filter(|item| matches!(item, QuoteItem::Type(_)))
        .count();
    if node.assoc_const_primitives.len() != constant_count
        || node.assoc_type_values.len() != type_count
    {
        return Err(invalid(
            "quoted associated member types changed after freezing",
        ));
    }
    let Some(Partial::Present(trait_ref)) = quoted.trait_ref else {
        return Err(invalid("quoted impl requires a trait"));
    };
    let Partial::Present(authored_trait_path) = trait_ref.path(db) else {
        return Err(invalid("quoted impl trait path is missing"));
    };
    for index in 0..authored_trait_path.len(db).saturating_sub(1) {
        let segment = authored_trait_path
            .segment(db, index)
            .ok_or_else(|| invalid("quoted impl trait path segment is missing"))?;
        if segment.generic_args(db).is_given(db) {
            return Err(invalid(
                "quoted impl trait arguments on an intermediate path need reification",
            ));
        }
    }
    if authored_trait_path.generic_args(db).is_given(db) != node.impl_trait_type_arguments.is_some()
        || node
            .impl_trait_type_arguments
            .as_ref()
            .is_some_and(|args| args.len() != authored_trait_path.generic_args(db).data(db).len())
    {
        return Err(invalid(
            "quoted impl trait arguments changed after freezing",
        ));
    }
    let mut roots = node.rooted_traits.iter().filter(|root| root.site == 0);
    let root = roots
        .next()
        .ok_or_else(|| invalid("quoted impl trait has no checked root"))?;
    if roots.next().is_some() {
        return Err(invalid("quoted impl trait root is ambiguous"));
    }
    let Partial::Present(self_ty) = quoted.ty else {
        return Err(invalid("quoted impl self type is missing"));
    };
    let TypeKind::Hole(slot) = self_ty.data(db) else {
        return Err(invalid(
            "quoted impl self type must splice the target record",
        ));
    };
    let (generic_params, generic_names) =
        quoted_record_generics(db, target.owner, &request.target_const_param_types)?;
    let source = top_mod_ast(db, ctxt.top_mod());
    let mut builder = HirBuilder::new(ctxt, origin, source.syntax());
    let self_ty = quoted_type_hole(&mut builder, quote, node, *slot, target, &generic_names)?;
    let mut path = bound_quote_trait_path(db, root)?;
    let mut signature_roots = Vec::new();
    let mut const_signature_roots = Vec::new();
    if let Some(arguments) = &node.impl_trait_type_arguments {
        let arguments = arguments
            .iter()
            .map(|argument| {
                let ty = match argument {
                    FrozenQuoteSignatureType::Hole(slot) => {
                        quoted_type_hole(&mut builder, quote, node, *slot, target, &generic_names)?
                    }
                    _ => quoted_signature_type(db, argument, &[], &mut signature_roots)?,
                };
                Ok::<_, String>(GenericArg::Type(TypeGenericArg {
                    ty: Partial::Present(ty),
                }))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let PathKind::Ident { ident, .. } = path.kind(db) else {
            return Err(invalid("quoted impl trait path has no final item"));
        };
        path = PathId::new(
            db,
            PathKind::Ident {
                ident,
                generic_args: GenericArgListId::given(db, arguments),
            },
            path.parent(db),
        );
    }
    let expected = locate_authored_trait(db, root)
        .ok_or_else(|| invalid("quoted trait is absent from authored HIR"))?;
    let mut method_hygiene = Vec::new();
    let mut const_hygiene = Vec::new();
    let implementation = builder.generated_impl_with_children(
        |builder| {
            let types = quoted
                .items
                .iter()
                .filter_map(|item| match item {
                    QuoteItem::Type(associated) => Some(associated),
                    _ => None,
                })
                .zip(&node.assoc_type_values)
                .map(|(associated, frozen)| {
                    quoted_impl_type(
                        db,
                        builder,
                        associated,
                        frozen.as_ref(),
                        &mut signature_roots,
                    )
                })
                .collect::<Result<Vec<_>, String>>()?;
            let spec = GeneratedImplSpec {
                trait_ref: Partial::Present(TraitRefId::new(db, Partial::Present(path))),
                ty: Partial::Present(self_ty),
                selection_name: None,
                attrs: builder.empty_attrs(),
                generic_params,
                where_clause: builder.quoted_owner_where_clause(target.owner, &generic_names)?,
                types,
                consts: vec![],
            };
            Ok::<_, String>((spec, ()))
        },
        |builder, ()| {
            for (index, method) in quoted
                .items
                .iter()
                .filter_map(|item| match item {
                    QuoteItem::Func(method) => Some(method),
                    _ => None,
                })
                .enumerate()
            {
                let method_params = builder.empty_generic_params();
                method_hygiene.push(emit_quoted_function(
                    db,
                    builder,
                    request,
                    target,
                    quote,
                    node,
                    code,
                    expr_sources,
                    code_root,
                    method,
                    u32::try_from(index).map_err(|_| invalid("quoted method count exceeds u32"))?,
                    method_params,
                    &generic_names,
                    true,
                    provider_scope,
                )?);
            }
            quoted
                .items
                .iter()
                .filter_map(|item| match item {
                    QuoteItem::Const(constant) => Some(constant),
                    _ => None,
                })
                .zip(&node.assoc_const_primitives)
                .enumerate()
                .map(|(index, (constant, primitive))| {
                    let (definition, body, roots) = quoted_impl_const(
                        db,
                        builder,
                        target,
                        quote,
                        node,
                        code,
                        expr_sources,
                        constant,
                        u32::try_from(index)
                            .map_err(|_| invalid("quoted associated constant count exceeds u32"))?,
                        *primitive,
                        &generic_names,
                        provider_scope,
                        &mut const_signature_roots,
                    )?;
                    const_hygiene.push((body, roots));
                    Ok(definition)
                })
                .collect::<Result<Vec<_>, String>>()
        },
    )?;
    signature_roots.extend(const_signature_roots);
    let mut nominal_types = Vec::new();
    let mut trait_types = Vec::new();
    for (site, root) in signature_roots {
        match root {
            QuoteSignatureRoot::Nominal(root) => {
                let expected = locate_authored_nominal(db, &root)
                    .ok_or_else(|| invalid("quoted nominal type is absent from authored HIR"))?;
                nominal_types.push((site, expected));
            }
            QuoteSignatureRoot::Trait(root) => {
                let expected = locate_authored_trait(db, &root)
                    .ok_or_else(|| invalid("quoted trait type is absent from authored HIR"))?;
                trait_types.push((site, expected));
            }
        }
    }
    let mut hygiene = vec![QuoteHygiene::TraitImpl {
        implementation,
        expected,
        const_rooted: const_hygiene,
        provider_scope,
        nominal_types,
        trait_types,
        site_span: request.site_span.clone(),
    }];
    hygiene.extend(method_hygiene);
    Ok(hygiene)
}

fn quoted_impl_type<'db>(
    db: &'db dyn HirDb,
    builder: &HirBuilder<'_, 'db, GenerationDesugared>,
    associated: &QuoteAssocType<'db>,
    frozen: Option<&FrozenQuoteSignatureType>,
    roots: &mut Vec<(TypeId<'db>, QuoteSignatureRoot)>,
) -> Result<crate::hir_def::AssocTyDef<'db>, String> {
    let Partial::Present(name) = associated.name else {
        return Err(invalid("quoted associated type needs a literal name"));
    };
    if associated.ty.is_none() {
        return Err(invalid("quoted associated type value is missing"));
    }
    let ty = quoted_signature_type(
        db,
        frozen.ok_or_else(|| invalid("quoted associated type needs checked reification"))?,
        &[],
        roots,
    )?;
    Ok(builder.assoc_ty(name.data(db), Partial::Present(ty)))
}

fn quoted_impl_const<'db>(
    db: &'db dyn HirDb,
    builder: &mut HirBuilder<'_, 'db, GenerationDesugared>,
    target: &TargetContext<'_, 'db>,
    quote: &Quote<'db>,
    node: &FrozenCodeNode,
    code: &FrozenCode,
    expr_sources: &[Option<QuoteExprSource<'db>>],
    constant: &QuoteConst<'db>,
    const_index: u32,
    primitive: Option<SemPrimTy>,
    generic_names: &[IdentId<'db>],
    provider_scope: ScopeId<'db>,
    signature_roots: &mut Vec<(TypeId<'db>, QuoteSignatureRoot)>,
) -> Result<(AssocConstDef<'db>, Body<'db>, Vec<(ExprId, Func<'db>)>), String> {
    let name = quoted_name(constant.name, node, target)?;
    if !constant.attrs.data(db).is_empty() || constant.vis != Visibility::Private {
        return Err(invalid("quoted associated constant header needs rewriting"));
    }
    let Partial::Present(_) = constant.ty else {
        return Err(invalid("quoted associated constant type is missing"));
    };
    let ty = primitive
        .and_then(quote_hir_primitive)
        .map(|primitive| TypeId::new(db, TypeKind::Primitive(primitive)))
        .ok_or_else(|| invalid("quoted associated constant is not a checked primitive type"))?;
    let root = constant
        .value
        .ok_or_else(|| invalid("quoted associated constant value is missing"))?;
    let roots = node
        .rooted_funcs
        .iter()
        .filter(|root| root.assoc_const == Some(const_index))
        .collect::<Vec<_>>();
    let rooted_paths = roots
        .iter()
        .map(|root| {
            let path = quoted_function_path(
                db,
                builder,
                quote,
                node,
                root,
                target,
                generic_names,
                &[],
                signature_roots,
            )?;
            Ok((root.site, path))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let expected_roots = roots
        .iter()
        .map(|root| {
            let file = db
                .workspace()
                .get(db, root.owner.file())
                .ok_or_else(|| invalid("quoted root file is missing"))?;
            let base = base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
            locate_authored_function(db, base, &root.owner)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut copied = None;
    let value =
        builder.generated_body(TrackedItemVariant::Const(Partial::Present(name)), |body| {
            copied = Some(body.adopt_closed_quote_body(
                quote.template,
                root,
                provider_scope,
                &node.holes,
                Some((code, expr_sources)),
                &[],
                &[],
                &[],
                false,
                &rooted_paths,
                &[],
                &[],
                &[],
                &[],
                &[],
                &[],
                &[],
                &[],
            )?);
            Ok::<_, String>(())
        })?;
    let rooted = copied
        .ok_or_else(|| invalid("quoted associated constant body was not copied"))?
        .rooted_paths
        .into_iter()
        .zip(expected_roots)
        .collect();
    let definition = AssocConstDef {
        attributes: builder.empty_attrs(),
        name: Partial::Present(name),
        ty: Partial::Present(ty),
        value: Partial::Present(value),
        body_check_policy: Default::default(),
        vis: Visibility::Private,
    };
    Ok((definition, value, rooted))
}

fn bound_quote_trait_path<'db>(
    db: &'db dyn HirDb,
    root: &FrozenQuoteTrait,
) -> Result<PathId<'db>, String> {
    bound_quote_item_path(db, &root.ingot, &root.path)
}

fn bound_quote_qualified_const_path<'db>(
    db: &'db dyn HirDb,
    template: Body<'db>,
    node: &FrozenCodeNode,
    qualified: &FrozenQuoteQualifiedConst,
) -> Result<(PathId<'db>, Trait<'db>), String> {
    let trait_root = node
        .rooted_traits
        .iter()
        .find(|root| root.site == qualified.trait_site)
        .ok_or_else(|| invalid("quoted qualified constant lost its checked trait"))?;
    let expected = locate_authored_trait(db, trait_root)
        .ok_or_else(|| invalid("quoted qualified trait is absent from authored HIR"))?;
    let Partial::Present(Expr::Path(Partial::Present(source))) =
        &template.exprs(db)[qualified.site]
    else {
        return Err(invalid("quoted qualified constant path disappeared"));
    };
    let name = source
        .ident(db)
        .to_opt()
        .ok_or_else(|| invalid("quoted qualified constant member is missing"))?;
    let receiver = quote_hir_primitive(qualified.receiver)
        .map(|primitive| TypeId::new(db, TypeKind::Primitive(primitive)))
        .ok_or_else(|| invalid("quoted qualified constant receiver is unsupported"))?;
    let trait_path = bound_quote_trait_path(db, trait_root)?;
    let trait_ref = TraitRefId::new(db, Partial::Present(trait_path));
    let path = PathId::new(
        db,
        PathKind::QualifiedType {
            type_: receiver,
            trait_: trait_ref,
        },
        None,
    )
    .push_ident(db, name);
    Ok((path, expected))
}

fn bound_quote_function_path<'db>(
    db: &'db dyn HirDb,
    root: &FrozenQuoteFunction,
) -> Result<PathId<'db>, String> {
    bound_quote_item_path(db, &root.ingot, &root.path)
}

fn quoted_function_path<'db>(
    db: &'db dyn HirDb,
    builder: &mut HirBuilder<'_, 'db, GenerationDesugared>,
    quote: &Quote<'db>,
    node: &FrozenCodeNode,
    root: &FrozenQuoteFunction,
    target: &TargetContext<'_, 'db>,
    owner_generic_names: &[IdentId<'db>],
    function_generic_names: &[IdentId<'db>],
    signature_roots: &mut Vec<(TypeId<'db>, QuoteSignatureRoot)>,
) -> Result<PathId<'db>, String> {
    let mut path = bound_quote_function_path(db, root)?;
    if let Some(arguments) = &root.type_arguments {
        let arguments = arguments
            .iter()
            .map(|argument| {
                let ty = match argument {
                    FrozenQuoteSignatureType::Hole(slot) => {
                        quoted_type_hole(builder, quote, node, *slot, target, owner_generic_names)?
                    }
                    _ => quoted_signature_type(
                        db,
                        argument,
                        function_generic_names,
                        signature_roots,
                    )?,
                };
                Ok::<_, String>(ty)
            })
            .collect::<Result<Vec<_>, String>>()?;
        path = quote_function_arguments(db, path, arguments)?;
    }
    Ok(path)
}

fn quote_function_arguments<'db>(
    db: &'db dyn HirDb,
    path: PathId<'db>,
    arguments: Vec<TypeId<'db>>,
) -> Result<PathId<'db>, String> {
    let PathKind::Ident { ident, .. } = path.kind(db) else {
        return Err(invalid("quoted function path has no final item"));
    };
    let arguments = arguments
        .into_iter()
        .map(|ty| {
            GenericArg::Type(TypeGenericArg {
                ty: Partial::Present(ty),
            })
        })
        .collect();
    Ok(PathId::new(
        db,
        PathKind::Ident {
            ident,
            generic_args: GenericArgListId::given(db, arguments),
        },
        path.parent(db),
    ))
}

fn bound_quote_item_path<'db>(
    db: &'db dyn HirDb,
    ingot_url: &url::Url,
    item_path: &str,
) -> Result<PathId<'db>, String> {
    let ingot = db
        .workspace()
        .containing_ingot(db, ingot_url.clone())
        .ok_or_else(|| invalid("quoted item ingot is missing"))?;
    let mut path = PathId::new(
        db,
        crate::hir_def::PathKind::BoundIngot {
            ingot,
            display: IdentId::make_ingot(db),
        },
        None,
    );
    let mut count = 0;
    for segment in item_path.split("::") {
        if !identifier(segment) {
            return Err(invalid("quoted item path is not a simple item path"));
        }
        path = path.push_str(db, segment);
        count += 1;
    }
    if count == 0 {
        return Err(invalid("quoted item path has no item"));
    }
    Ok(path)
}

fn text(value: &FrozenMetaValue) -> Result<String, String> {
    let FrozenMetaValue::Scalar {
        value: SemConstScalar::Bytes(bytes),
        ..
    } = value
    else {
        return Err(invalid("expected fixed string data"));
    };
    let start = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[start..])
        .map(str::to_owned)
        .map_err(|_| invalid("generation string is not UTF-8"))
}

fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
}

struct TargetContext<'a, 'db> {
    db: &'db dyn HirDb,
    owner: crate::analysis::ty::adt_def::AdtRef<'db>,
    target: &'a FrozenOwner,
}

impl<'a, 'db> TargetContext<'a, 'db> {
    fn field_hir_type(&self, variant_index: Option<u16>, index: usize) -> Option<TypeId<'db>> {
        use crate::{analysis::ty::adt_def::AdtRef, hir_def::VariantKind};
        match (self.owner, variant_index) {
            (AdtRef::Struct(owner), None) => owner
                .hir_fields(self.db)
                .data(self.db)
                .get(index)?
                .type_ref
                .to_opt(),
            (AdtRef::Enum(owner), Some(variant_index)) => {
                if usize::from(variant_index) >= owner.len_variants(self.db) {
                    return None;
                }
                match crate::hir_def::EnumVariant::new(owner, usize::from(variant_index))
                    .kind(self.db)
                {
                    VariantKind::Unit => None,
                    VariantKind::Tuple(fields) => fields.data(self.db).get(index)?.to_opt(),
                    VariantKind::Record(fields) => {
                        fields.data(self.db).get(index)?.type_ref.to_opt()
                    }
                }
            }
            _ => None,
        }
    }

    fn field_coordinate(
        &self,
        value: &FrozenMetaValue,
        type_handle: bool,
    ) -> Result<(Option<u16>, usize), String> {
        let coordinate = match value {
            FrozenMetaValue::Structural {
                item: MetaItem::FieldType,
                owner,
                kind:
                    FrozenHandleKind::FieldType {
                        variant_index,
                        index,
                    },
            } if type_handle && owner == self.target => (*variant_index, *index),
            FrozenMetaValue::Structural {
                item: MetaItem::Field,
                owner,
                kind:
                    FrozenHandleKind::Field {
                        variant_index,
                        index,
                    },
            } if !type_handle && owner == self.target => (*variant_index, *index),
            _ => return Err(invalid("expected an authorized field handle")),
        };
        if self.field_hir_type(coordinate.0, coordinate.1).is_none() {
            return Err(invalid(
                "field handle has an invalid owner, variant or field coordinate",
            ));
        }
        Ok(coordinate)
    }

    fn field_index(&self, value: &FrozenMetaValue, type_handle: bool) -> Result<usize, String> {
        let (variant_index, index) = self.field_coordinate(value, type_handle)?;
        if variant_index.is_some() {
            return Err(invalid(
                "variant field access requires a matching pattern binder",
            ));
        }
        Ok(index)
    }

    fn field_name(&self, index: usize) -> Result<IdentId<'db>, String> {
        let crate::analysis::ty::adt_def::AdtRef::Struct(owner) = self.owner else {
            return Err(invalid("field name requires a selected field list"));
        };
        owner.hir_fields(self.db).data(self.db)[index]
            .name
            .to_opt()
            .ok_or_else(|| invalid("generation field name is missing"))
    }

    fn name(&self, value: &FrozenMetaValue) -> Result<IdentId<'db>, String> {
        let FrozenMetaValue::Variant {
            item: MetaItem::DeclarationName,
            case,
            fields,
            ..
        } = value
        else {
            return Err(invalid("invalid generated declaration name"));
        };
        let name = match (*case, fields.as_slice()) {
            (FrozenMetaVariantCase::DeclarationNameLiteral, [name]) => text(name)?,
            (FrozenMetaVariantCase::DeclarationNameFieldPrefix, [prefix, field]) => {
                let index = self.field_index(field, false)?;
                format!("{}{}", text(prefix)?, self.field_name(index)?.data(self.db))
            }
            _ => return Err(invalid("invalid generated declaration name")),
        };
        if name.len() > 256 || !identifier(&name) {
            return Err(invalid("generated name must be an identifier"));
        }
        Ok(IdentId::new(self.db, name))
    }
}

/// Build one checked quoted request in its own expansion context.
pub(crate) fn request_fragment<'db>(
    db: &'db dyn HirDb,
    top_mod: TopLevelMod<'db>,
    request: &FrozenGeneration,
) -> Result<(ScopeGraph<'db>, Vec<QuoteHygiene<'db>>), String> {
    let base = base_scope_graph_impl(db, top_mod);
    let root = top_mod_ast(db, top_mod).syntax().clone();
    let target = base
        .items_dfs(db)
        .find_map(|item| {
            let owner = crate::analysis::ty::adt_def::AdtRef::try_from_item(item)?;
            if authored_path(db, base, owner.scope())? != request.target.path {
                return None;
            }
            let node = match owner {
                crate::analysis::ty::adt_def::AdtRef::Struct(owner) => {
                    let HirOrigin::Raw(ptr) = owner.origin(db) else {
                        return None;
                    };
                    ptr.syntax_node_ptr().try_to_node(&root)?
                }
                crate::analysis::ty::adt_def::AdtRef::Enum(owner) => {
                    let HirOrigin::Raw(ptr) = owner.origin(db) else {
                        return None;
                    };
                    ptr.syntax_node_ptr().try_to_node(&root)?
                }
            };
            let attrs = match owner {
                crate::analysis::ty::adt_def::AdtRef::Struct(_) => {
                    ast::Struct::cast(node)?.attr_list()
                }
                crate::analysis::ty::adt_def::AdtRef::Enum(_) => ast::Enum::cast(node)?.attr_list(),
            }?;
            let attribute = attrs.into_iter().nth(request.attribute_index)?;
            Some((owner, attribute))
        })
        .ok_or_else(|| invalid("generated target is absent from authored HIR"))?;
    let parent = base
        .edges(target.0.scope())
        .iter()
        .find_map(|edge| matches!(edge.kind, EdgeKind::Lex(_)).then_some(edge.dest))
        .ok_or_else(|| invalid("generated target has no lexical parent"))?;
    let origin = GenerationDesugared {
        attribute: parser::ast::AstPtr::new(&target.1),
    };
    let target_context = TargetContext {
        db,
        owner: target.0,
        target: &request.target,
    };
    let mut ctxt = FileLowerCtxt::enter_expansion(db, top_mod);
    ctxt.enter_shim_scope(parent, base.scope_data(&parent).vis);
    let hygiene = emit_closed_quoted_items(db, &mut ctxt, request, origin, &target_context)?;
    ctxt.leave_shim_scope();
    Ok((ctxt.build(), hygiene))
}

fn authored_path<'db>(
    db: &'db dyn HirDb,
    graph: &ScopeGraph<'db>,
    mut scope: ScopeId<'db>,
) -> Option<String> {
    let mut names = Vec::new();
    loop {
        names.push(scope.name(db)?.data(db).clone());
        let parent = authored_parent(graph, scope);
        let Some(next) = parent else {
            break;
        };
        scope = next;
    }
    names.reverse();
    Some(names.join("::"))
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct GeneratedHirError {
    pub message: String,
    pub site_span: Option<std::ops::Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub(crate) struct GeneratedFunctions<'db> {
    pub(crate) graph: ScopeGraph<'db>,
    pub(crate) errors: Vec<GeneratedHirError>,
    pub(crate) hygiene: Vec<QuoteHygiene<'db>>,
}

/// Build only from the published, completed input. This query never evaluates
/// providers and only reads the authored scope graph for target lookup.
#[salsa::tracked(return_ref)]
pub(crate) fn generated_functions_impl<'db>(
    db: &'db dyn HirDb,
    top_mod: TopLevelMod<'db>,
) -> GeneratedFunctions<'db> {
    let mut scopes = common::indexmap::IndexMap::<ScopeId<'db>, Scope<'db>>::default();
    let mut unresolved_uses = rustc_hash::FxHashSet::default();
    let mut errors = Vec::new();
    let mut hygiene = Vec::new();
    if let Some(url) = top_mod.file(db).url(db)
        && let Some(module) = db.generated_module(&url)
    {
        for request in module.requests(db) {
            match request_fragment(db, top_mod, request) {
                Ok((fragment, quote)) => {
                    for (scope_id, scope) in &fragment.scopes {
                        if let Some(existing) = scopes.get_mut(scope_id) {
                            existing.edges.extend(scope.edges.iter().cloned());
                        } else {
                            scopes.insert(*scope_id, scope.clone());
                        }
                    }
                    unresolved_uses.extend(fragment.unresolved_uses);
                    hygiene.extend(quote);
                }
                Err(error) => errors.push(GeneratedHirError {
                    message: error,
                    site_span: request.site_span.clone(),
                }),
            }
        }
    }
    GeneratedFunctions {
        graph: ScopeGraph {
            top_mod,
            scopes,
            unresolved_uses,
        },
        errors,
        hygiene,
    }
}

/// Output-stage construction failures point back to the generation request.
pub fn generated_hir_errors<'db>(
    db: &'db dyn LowerHirDb,
    top_mod: TopLevelMod<'db>,
) -> &'db [GeneratedHirError] {
    &generated_functions_impl(db, top_mod).errors
}

fn push_quoted_type_hygiene_errors<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    nominal_types: &[(TypeId<'db>, crate::analysis::ty::adt_def::AdtRef<'db>)],
    trait_types: &[(TypeId<'db>, Trait<'db>)],
    site_span: &Option<std::ops::Range<usize>>,
    errors: &mut Vec<GeneratedHirError>,
) {
    for (site, expected) in nominal_types {
        let valid = match site.data(db) {
            TypeKind::Path(Partial::Present(path)) => matches!(
                resolve_path(db, *path, scope, PredicateListId::empty_list(db), true),
                Ok(PathRes::Ty(ty))
                    if matches!(ty.base_ty(db).data(db),
                        TyData::TyBase(TyBase::Adt(actual))
                            if actual.adt_ref(db) == *expected)
            ),
            _ => false,
        };
        if !valid {
            errors.push(GeneratedHirError {
                message: invalid("quoted nominal type changed meaning in output"),
                site_span: site_span.clone(),
            });
        }
    }
    for (site, expected) in trait_types {
        let valid = match site.data(db) {
            TypeKind::Path(Partial::Present(path)) => matches!(
                resolve_path(db, *path, scope, PredicateListId::empty_list(db), true),
                Ok(PathRes::Trait(actual)) if actual.key(db) == *expected
            ),
            _ => false,
        };
        if !valid {
            errors.push(GeneratedHirError {
                message: invalid("quoted trait type changed meaning in output"),
                site_span: site_span.clone(),
            });
        }
    }
}

/// Check the meanings of transplanted names only after the output scope graph
/// is complete. Running this inside `generated_functions_impl` would ask that
/// query to resolve its own unfinished result.
pub fn generated_quote_hygiene_errors<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<GeneratedHirError> {
    let mut errors = Vec::new();
    for quote in &generated_functions_impl(db, top_mod).hygiene {
        let QuoteHygiene::Function {
            func,
            rooted,
            variants,
            qualified_consts,
            const_predicates,
            nominal_types,
            trait_types,
            locals,
            params,
            effects,
            generics,
            method_calls,
            fragment_method_calls,
            provider_scope,
            site_span,
        } = quote
        else {
            let QuoteHygiene::TraitImpl {
                implementation,
                expected,
                const_rooted,
                provider_scope,
                nominal_types,
                trait_types,
                site_span,
            } = quote
            else {
                unreachable!();
            };
            let valid = implementation
                .hir_trait_ref(db)
                .to_opt()
                .and_then(|trait_ref| trait_ref.path(db).to_opt())
                .is_some_and(|path| {
                    matches!(
                        resolve_path(
                            db,
                            path,
                            implementation.scope(),
                            PredicateListId::empty_list(db),
                            true,
                        ),
                        Ok(PathRes::Trait(actual)) if actual.key(db) == *expected
                    )
                });
            if !valid {
                errors.push(GeneratedHirError {
                    message: invalid("quoted rooted trait changed meaning in output"),
                    site_span: site_span.clone(),
                });
            }
            for (body, roots) in const_rooted {
                for (site, expected) in roots {
                    let valid = match &body.exprs(db)[*site] {
                        Partial::Present(Expr::Path(Partial::Present(path))) => matches!(
                            resolve_path(
                                db,
                                *path,
                                *provider_scope,
                                PredicateListId::empty_list(db),
                                true,
                            ),
                            Ok(PathRes::Func(ty))
                                if matches!(ty.base_ty(db).data(db),
                                    TyData::TyBase(TyBase::Func(CallableDef::Func(actual)))
                                        if actual == expected)
                        ),
                        _ => false,
                    };
                    if !valid {
                        errors.push(GeneratedHirError {
                            message: invalid(
                                "quoted associated constant function changed meaning in output",
                            ),
                            site_span: site_span.clone(),
                        });
                    }
                }
            }
            push_quoted_type_hygiene_errors(
                db,
                implementation.scope(),
                nominal_types,
                trait_types,
                site_span,
                &mut errors,
            );
            continue;
        };
        let Some(body) = func.body(db) else {
            errors.push(GeneratedHirError {
                message: invalid("quoted function body disappeared before hygiene checking"),
                site_span: site_span.clone(),
            });
            continue;
        };
        let (_, typed) = check_func_body(db, *func);
        for (site, expected) in variants {
            let actual = match site {
                FrozenQuoteConstructorSite::Pat(site) => {
                    typed.pattern_root(*site).and_then(|root| {
                        match typed.pattern_store().node(root).kind() {
                            crate::analysis::ty::pattern_ir::ValidatedPatKind::Constructor {
                                ctor:
                                    crate::analysis::ty::pattern_ir::ConstructorKind::Variant(
                                        variant,
                                        _,
                                    ),
                                ..
                            } => Some(QuoteConstructorIdentity {
                                owner: crate::analysis::ty::adt_def::AdtRef::Enum(variant.enum_),
                                variant_index: Some(variant.idx),
                            }),
                            crate::analysis::ty::pattern_ir::ValidatedPatKind::Constructor {
                                ctor: crate::analysis::ty::pattern_ir::ConstructorKind::Type(ty),
                                ..
                            } => {
                                if let TyData::TyBase(TyBase::Adt(adt)) = ty.base_ty(db).data(db) {
                                    Some(QuoteConstructorIdentity {
                                        owner: adt.adt_ref(db),
                                        variant_index: None,
                                    })
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        }
                    })
                }
                FrozenQuoteConstructorSite::Expr(site) => {
                    let path = match &body.exprs(db)[*site] {
                        Partial::Present(Expr::Path(path) | Expr::RecordInit(path, _)) => {
                            path.to_opt()
                        }
                        _ => None,
                    };
                    path.and_then(|path| {
                        match resolve_path(
                            db,
                            path,
                            func.scope(),
                            PredicateListId::empty_list(db),
                            true,
                        ) {
                            Ok(PathRes::EnumVariant(variant)) => Some(QuoteConstructorIdentity {
                                owner: crate::analysis::ty::adt_def::AdtRef::Enum(
                                    variant.variant.enum_,
                                ),
                                variant_index: Some(variant.variant.idx),
                            }),
                            Ok(PathRes::Ty(ty)) => {
                                if let TyData::TyBase(TyBase::Adt(adt)) = ty.base_ty(db).data(db) {
                                    Some(QuoteConstructorIdentity {
                                        owner: adt.adt_ref(db),
                                        variant_index: None,
                                    })
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        }
                    })
                }
            };
            if actual != Some(*expected) {
                errors.push(GeneratedHirError {
                    message: invalid("quoted constructor changed owner or variant index"),
                    site_span: site_span.clone(),
                });
            }
        }
        push_quoted_type_hygiene_errors(
            db,
            func.scope(),
            nominal_types,
            trait_types,
            site_span,
            &mut errors,
        );
        for call in method_calls
            .iter()
            .map(|site| QuoteMethodCall {
                site: *site,
                scope: *provider_scope,
                selected: None,
            })
            .chain(fragment_method_calls.iter().copied())
        {
            let Partial::Present(Expr::MethodCall(..)) = &body.exprs(db)[call.site] else {
                errors.push(GeneratedHirError {
                    message: invalid("quoted method call disappeared before hygiene checking"),
                    site_span: site_span.clone(),
                });
                continue;
            };
            // Ordinary checking owns receiver adjustment and method selection.
            // Verify the retained lookup scope and consume its checked call record.
            let actual = typed
                .callable_expr(call.site)
                .map(|callable| callable.callable_def());
            let scopes = body
                .quote_method_scopes(db)
                .iter()
                .filter(|(site, _)| *site == call.site)
                .map(|(_, scope)| *scope)
                .collect::<Vec<_>>();
            if scopes.as_slice() != [call.scope]
                || actual.is_none()
                || call
                    .selected
                    .is_some_and(|selected| actual != Some(CallableDef::Func(selected)))
            {
                errors.push(GeneratedHirError {
                    message: invalid("quoted method call changed selection in output"),
                    site_span: site_span.clone(),
                });
            }
        }
        for (site, expected) in rooted {
            let valid = match &body.exprs(db)[*site] {
                Partial::Present(Expr::Path(Partial::Present(path))) => {
                    matches!(
                        resolve_path(
                            db,
                            *path,
                            func.scope(),
                            PredicateListId::empty_list(db),
                            true,
                        ),
                        Ok(PathRes::Func(ty))
                            if matches!(ty.base_ty(db).data(db),
                                TyData::TyBase(TyBase::Func(CallableDef::Func(actual)))
                                    if actual == expected)
                    )
                }
                _ => false,
            };
            if !valid {
                errors.push(GeneratedHirError {
                    message: invalid("quoted rooted function changed meaning in output"),
                    site_span: site_span.clone(),
                });
            }
        }
        for (site, expected) in qualified_consts {
            let valid = matches!(typed.expr_const_ref(*site),
                Some(ConstRef::TraitConst(actual)) if actual.inst().key(db) == *expected);
            if !valid {
                errors.push(GeneratedHirError {
                    message: invalid("quoted qualified constant changed trait in output"),
                    site_span: site_span.clone(),
                });
            }
        }
        for (binder, uses) in locals {
            let binds = matches!(typed.pat_binding(*binder),
                Some(LocalBinding::Local { pat, .. }) if pat == *binder);
            let uses_bind = uses.iter().all(|site| {
                matches!(typed.expr_binding(*site),
                    Some(LocalBinding::Local { pat, .. }) if pat == *binder)
            });
            if !binds || !uses_bind {
                errors.push(GeneratedHirError {
                    message: invalid("quoted local binder changed meaning in output"),
                    site_span: site_span.clone(),
                });
            }
        }
        for (site, index) in params {
            let valid = matches!(typed.expr_binding(*site),
                Some(LocalBinding::Param { site: ParamSite::Func(owner), idx, .. })
                    if owner == *func && idx == *index);
            if !valid {
                errors.push(GeneratedHirError {
                    message: invalid("quoted parameter changed meaning in output"),
                    site_span: site_span.clone(),
                });
            }
        }
        for (site, index) in effects {
            let valid = matches!(typed.expr_binding(*site),
                Some(LocalBinding::EffectParam { site: EffectParamSite::Func(owner), idx, .. })
                    if owner == *func && idx == *index);
            if !valid {
                errors.push(GeneratedHirError {
                    message: invalid("quoted effect binding changed meaning in output"),
                    site_span: site_span.clone(),
                });
            }
        }
        for (site, expected) in generics {
            let valid = matches!(typed.value_path_ref(*site),
                Some(ValuePathRef::TypeConst(ty))
                    if matches!(ty.data(db), TyData::ConstTy(const_ty)
                        if matches!(const_ty.data(db), ConstTyData::TyParam(param, _)
                            if param.owner == func.scope() && param.name == *expected)));
            if !valid {
                errors.push(GeneratedHirError {
                    message: invalid("quoted generic body use changed meaning in output"),
                    site_span: site_span.clone(),
                });
            }
        }
        let actual_predicates = WhereClauseOwner::Func(*func)
            .where_clause(db)
            .const_predicates(db);
        if actual_predicates.len() != const_predicates.len()
            || actual_predicates
                .iter()
                .zip(const_predicates)
                .any(|(actual, frozen)| *actual != frozen.body)
        {
            errors.push(GeneratedHirError {
                message: invalid("quoted const where body changed in output"),
                site_span: site_span.clone(),
            });
            continue;
        }
        for predicate in const_predicates {
            for (site, expected) in &predicate.rooted {
                let valid = match &predicate.body.exprs(db)[*site] {
                    Partial::Present(Expr::Path(Partial::Present(path))) => matches!(
                        resolve_path(
                            db,
                            *path,
                            func.scope(),
                            PredicateListId::empty_list(db),
                            true,
                        ),
                        Ok(PathRes::Func(ty))
                            if matches!(ty.base_ty(db).data(db),
                                TyData::TyBase(TyBase::Func(CallableDef::Func(actual)))
                                    if actual == expected)
                    ),
                    _ => false,
                };
                if !valid {
                    errors.push(GeneratedHirError {
                        message: invalid("quoted const where function changed meaning in output"),
                        site_span: site_span.clone(),
                    });
                }
            }
            for (site, expected) in &predicate.generics {
                let valid = match &predicate.body.exprs(db)[*site] {
                    Partial::Present(Expr::Path(Partial::Present(path))) => matches!(
                        resolve_path(
                            db,
                            *path,
                            func.scope(),
                            PredicateListId::empty_list(db),
                            true,
                        ),
                        Ok(PathRes::Ty(ty))
                            if matches!(ty.base_ty(db).data(db),
                                TyData::ConstTy(const_ty)
                                    if matches!(const_ty.data(db),
                                        ConstTyData::TyParam(param, _) if param.name == *expected))
                    ),
                    _ => false,
                };
                if !valid {
                    errors.push(GeneratedHirError {
                        message: invalid("quoted const where generic changed meaning in output"),
                        site_span: site_span.clone(),
                    });
                }
            }
        }
    }
    errors
}
