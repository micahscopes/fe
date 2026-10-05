//! Builds checked generation results as ordinary HIR in the output view.
//! Provider evaluation has already finished before this module is queried.

use parser::ast::{self, prelude::*};

use super::quote_errors::{QuoteConstructionFault, QuoteHygieneFault};

use super::{
    FileLowerCtxt, base_scope_graph_impl,
    hir_builder::{
        GeneratedFunctionSpec, GeneratedImplSpec, HirBuilder, QuoteBodyBindings, QuoteBudget,
        QuoteFragmentSource, QuoteMemberHole, QuoteMethodCall, copy_owner_bound,
        quote_hir_primitive,
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
            ty_check::{EffectParamSite, LocalBinding, ParamSite, ValuePathRef, check_func_body},
            ty_def::{PrimTy as SemPrimTy, TyBase, TyData, TyId},
        },
    },
    core::meta::{
        FrozenCode, FrozenCodeHole, FrozenCodeNode, FrozenGeneration, FrozenHandleKind,
        FrozenItemLocator, FrozenMetaValue, FrozenMetaVariantCase, FrozenOwner,
        FrozenQuoteConstructor, FrozenQuoteConstructorSite, FrozenQuoteFunction,
        FrozenQuoteNominal, FrozenQuoteQualifiedConst, FrozenQuoteSignatureType, FrozenQuoteTrait,
        FrozenQuoteWherePredicate, FrozenTemplateSource, MetaItem, QuoteMember,
    },
    hir_def::{
        AssocConstDef, Body, CallableDef, EffectParamListId, Expr, ExprId, Func, FuncParamListId,
        GenericArg, GenericArgListId, GenericParam, GenericParamListId, HirIngot, IdentId,
        ItemKind, Partial, PatId, PathId, PathKind, Quote, QuoteAssocType, QuoteConst,
        QuoteFragment, QuoteHolePosition, QuoteItem, QuoteName, TopLevelMod, TrackedItemVariant,
        TraitRefId, TupleTypeId, TypeBound, TypeGenericArg, TypeId, TypeKind, Visibility,
        WhereClauseId, WhereClauseOwner, WherePredicate,
        scope_graph::{EdgeKind, Scope, ScopeGraph, ScopeId},
    },
    span::{GenerationDesugared, HirOrigin},
};

/// Typed quotation failures survive lowering until diagnostic rendering.
#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub enum GeneratedQuoteFailure {
    Construction(QuoteConstructionFault),
    Hygiene(QuoteHygieneFault),
    RequestNodeLimit { actual: usize, maximum: usize },
    UnboundOpenName { name: String },
    FragmentDepthLimit { actual: usize, maximum: usize },
    TypeDepthLimit { actual: usize, maximum: usize },
    FunctionNodeLimit { actual: usize, maximum: usize },
}
impl std::fmt::Display for GeneratedQuoteFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Construction(fault) => fault.fmt(f),
            Self::Hygiene(fault) => fault.fmt(f),
            Self::UnboundOpenName { name } => {
                write!(f, "quoted open name `{name}` has no enclosing binder")
            }
            Self::FragmentDepthLimit { actual, maximum } => write!(
                f,
                "quoted expression fragment nesting limit exceeded: {actual} > {maximum}"
            ),
            Self::TypeDepthLimit { actual, maximum } => write!(
                f,
                "quoted type nesting limit exceeded: {actual} > {maximum}"
            ),
            Self::FunctionNodeLimit { actual, maximum } => write!(
                f,
                "quoted function node limit exceeded: {actual} > {maximum}"
            ),
            Self::RequestNodeLimit { actual, maximum } => write!(
                f,
                "quotation request node limit exceeded: {actual} > {maximum}"
            ),
        }
    }
}

/// A construction failure retains the authored template node that caused it.
#[derive(Debug)]
pub(super) struct QuoteConstructionError {
    failure: GeneratedQuoteFailure,
    source: Option<Box<crate::core::meta::FrozenQuoteSpan>>,
}

impl QuoteConstructionError {
    pub(super) fn unbound_open_name(name: String) -> Self {
        Self {
            failure: GeneratedQuoteFailure::UnboundOpenName { name },
            source: None,
        }
    }

    pub(super) fn fragment_depth_limit(actual: usize, maximum: usize) -> Self {
        Self {
            failure: GeneratedQuoteFailure::FragmentDepthLimit { actual, maximum },
            source: None,
        }
    }
    pub(super) fn type_depth_limit(actual: usize, maximum: usize) -> Self {
        Self {
            failure: GeneratedQuoteFailure::TypeDepthLimit { actual, maximum },
            source: None,
        }
    }
    pub(super) fn function_node_limit(actual: usize, maximum: usize) -> Self {
        Self {
            failure: GeneratedQuoteFailure::FunctionNodeLimit { actual, maximum },
            source: None,
        }
    }
    pub(super) fn request_node_limit(actual: usize, maximum: usize) -> Self {
        Self {
            failure: GeneratedQuoteFailure::RequestNodeLimit { actual, maximum },
            source: None,
        }
    }
    pub(super) fn at(mut self, source: Option<crate::core::meta::FrozenQuoteSpan>) -> Self {
        if self.source.is_none() {
            self.source = source.map(Box::new);
        }
        self
    }
}
impl From<QuoteConstructionFault> for QuoteConstructionError {
    fn from(fault: QuoteConstructionFault) -> Self {
        Self {
            failure: fault.into(),
            source: None,
        }
    }
}

impl From<QuoteConstructionFault> for GeneratedQuoteFailure {
    fn from(fault: QuoteConstructionFault) -> Self {
        Self::Construction(fault)
    }
}
impl From<QuoteHygieneFault> for GeneratedQuoteFailure {
    fn from(fault: QuoteHygieneFault) -> Self {
        Self::Hygiene(fault)
    }
}

/// Rendering context for captured identities in their destination ingot.
#[derive(Clone, Copy)]
pub(super) struct QuotePathContext<'db> {
    db: &'db dyn HirDb,
    destination: common::ingot::Ingot<'db>,
}

impl<'db> QuotePathContext<'db> {
    pub(super) fn new(db: &'db dyn HirDb, destination: common::ingot::Ingot<'db>) -> Self {
        Self { db, destination }
    }
}

fn quoted_name<'db>(
    name: QuoteName<'db>,
    node: &FrozenCodeNode,
    target: &TargetContext<'_, 'db>,
) -> Result<IdentId<'db>, QuoteConstructionFault> {
    match name {
        QuoteName::Ident(Partial::Present(name)) => Ok(name),
        QuoteName::Ident(Partial::Absent) => Err(QuoteConstructionFault::DeclarationNameIsMissing),
        QuoteName::Hole(slot) => {
            let Some(FrozenCodeHole::Value(value, _)) = node.holes.get(slot as usize) else {
                return Err(QuoteConstructionFault::NameSpliceHasNoCheckedValue);
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

#[derive(Clone, Copy)]
enum LocateError {
    Missing,
    Ambiguous,
}

fn locate_unique<T>(mut items: impl Iterator<Item = T>) -> Result<T, LocateError> {
    let first = items.next().ok_or(LocateError::Missing)?;
    if items.next().is_some() {
        Err(LocateError::Ambiguous)
    } else {
        Ok(first)
    }
}

impl LocateError {
    fn fault(self, missing: QuoteConstructionFault) -> QuoteConstructionFault {
        match self {
            Self::Missing => missing,
            Self::Ambiguous => QuoteConstructionFault::AuthoredItemLocatorIsAmbiguous,
        }
    }
}

/// Resolve only through the authored graph, never the unfinished output graph.
fn locate_authored_function<'db>(
    db: &'db dyn HirDb,
    graph: &'db ScopeGraph<'db>,
    locator: &FrozenItemLocator,
) -> Result<Func<'db>, QuoteConstructionFault> {
    locate_unique(graph.items_dfs(db).filter_map(|item| {
        let ItemKind::Func(func) = item else {
            return None;
        };
        let actual = func.frozen_locator_with_scope(
            db,
            graph.top_mod.file(db).url(db)?,
            |scope| authored_parent(graph, scope),
            |scope| authored_path(db, graph, scope),
        )?;
        (actual == *locator).then_some(func)
    }))
    .map_err(|error| match error {
        LocateError::Missing => QuoteConstructionFault::AuthoredFunctionLocatorHasNoMatch,
        LocateError::Ambiguous => QuoteConstructionFault::AuthoredFunctionLocatorIsAmbiguous,
    })
}

fn locate_authored_item<'db, T>(
    db: &'db dyn HirDb,
    owner: &FrozenOwner,
    select: impl Fn(ItemKind<'db>) -> Option<T>,
) -> Result<T, LocateError> {
    let file = db
        .workspace()
        .get(db, &owner.file)
        .ok_or(LocateError::Missing)?;
    let base = base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
    locate_unique(base.items_dfs(db).filter_map(|item| {
        (owner.path == authored_path(db, base, item.scope())?)
            .then(|| select(item))
            .flatten()
    }))
}

fn quoted_type_hole<'db>(
    builder: &mut HirBuilder<'_, 'db, GenerationDesugared>,
    quote: &Quote<'db>,
    node: &FrozenCodeNode,
    slot: u32,
    depth: usize,
    target: &TargetContext<'_, 'db>,
    generic_names: &[IdentId<'db>],
) -> Result<TypeId<'db>, QuoteConstructionError> {
    let db = builder.db();
    if quote.holes.get(slot as usize).map(|hole| hole.position) != Some(QuoteHolePosition::Type) {
        return Err(QuoteConstructionFault::TypeSpliceIsNotATypeHole.into());
    }
    let Some(FrozenCodeHole::Value(value, field_type)) = node.holes.get(slot as usize) else {
        return Err(QuoteConstructionFault::TypeHoleIsMissing.into());
    };
    match value.as_ref() {
        FrozenMetaValue::Structural {
            item: MetaItem::Record,
            owner,
            kind: FrozenHandleKind::Record,
        } if owner == target.target => {
            builder.ctxt().quote_budget.charge_auxiliary(
                1 + usize::from(!generic_names.is_empty()) + 4 * generic_names.len(),
            )?;
            builder
                .generated_record_type_with_names(target.owner, generic_names)
                .map_err(Into::into)
        }
        FrozenMetaValue::Structural {
            item: MetaItem::FieldType,
            ..
        } => {
            let (variant_index, index) = target.field_coordinate(value, true)?;
            if !generic_names.is_empty() {
                let frozen = field_type
                    .as_ref()
                    .ok_or(QuoteConstructionFault::GenericFieldTypeNeedsCheckedReification)?;
                return QuoteTypeReifier {
                    paths: QuotePathContext::new(db, builder.top_mod().ingot(db)),
                    budget: &builder.ctxt().quote_budget,
                    depth,
                    owner_names: generic_names,
                    generic_names: &[],
                }
                .reify(frozen, &mut |_, _| {
                    Err(QuoteConstructionFault::TypeSpliceNeedsACheckedTargetContext.into())
                });
            }
            target
                .field_hir_type(variant_index, index)
                .ok_or_else(|| QuoteConstructionFault::FieldTypeIsMissing.into())
        }
        _ => Err(QuoteConstructionFault::TypeLacksTargetAuthority.into()),
    }
}

fn quoted_record_generics<'db>(
    db: &'db dyn HirDb,
    owner: crate::analysis::ty::adt_def::AdtRef<'db>,
    const_types: &[Option<SemPrimTy>],
    budget: &QuoteBudget,
) -> Result<(GenericParamListId<'db>, Vec<IdentId<'db>>), QuoteConstructionError> {
    let source = owner.generic_owner().params_list(db);
    budget.charge_many(source.data(db).len())?;
    if source.data(db).len() != const_types.len() {
        return Err(QuoteConstructionFault::TargetParameterCountChangedAfterFreezing.into());
    }
    let original_names = source
        .data(db)
        .iter()
        .map(|param| {
            param
                .name()
                .to_opt()
                .ok_or(QuoteConstructionFault::TargetParameterHasNoName)
        })
        .collect::<Result<Vec<_>, QuoteConstructionFault>>()?;
    let mut copied = Vec::new();
    let owner_names = original_names.iter().copied().collect();
    for (index, (param, const_type)) in source.data(db).iter().zip(const_types).enumerate() {
        let name = original_names[index];
        match param {
            GenericParam::Type(source) => {
                if const_type.is_some() {
                    return Err(QuoteConstructionFault::TargetTypeParameterHasAConstType.into());
                }
                let param = crate::hir_def::TypeGenericParam {
                    name: Partial::Present(name),
                    bounds: source
                        .bounds
                        .iter()
                        .map(|bound| copy_owner_bound(db, bound, &owner_names, budget, 0))
                        .collect::<Result<Vec<_>, QuoteConstructionError>>()?,
                    default_ty: None,
                };
                copied.push(GenericParam::Type(param));
            }
            GenericParam::Const(param) => {
                let primitive = const_type
                    .and_then(quote_hir_primitive)
                    .ok_or(QuoteConstructionFault::TargetConstParameterTypeIsUnsupported)?;
                let mut param = param.clone();
                param.name = Partial::Present(name);
                param.ty = Partial::Present(TypeId::new(db, TypeKind::Primitive(primitive)));
                param.default = None;
                copied.push(GenericParam::Const(param));
            }
        }
    }
    Ok((GenericParamListId::new(db, copied), original_names))
}

fn quoted_function_generics<'db>(
    paths: QuotePathContext<'db>,
    budget: &QuoteBudget,
    function: &crate::hir_def::QuoteFunc<'db>,
    inherited: GenericParamListId<'db>,
    owner_names: &[IdentId<'db>],
    function_types: &crate::core::meta::FrozenQuoteFunctionTypes,
    roots: &[FrozenQuoteTrait],
) -> Result<(GenericParamListId<'db>, Vec<IdentId<'db>>), QuoteConstructionError> {
    let db = paths.db;
    let bound_sites = &function_types.generic_bound_sites;
    let type_defaults = &function_types.generic_type_defaults;
    let const_types = &function_types.generic_const_types;
    let mut params = inherited.data(db).to_vec();
    let mut names = Vec::new();
    // Each parameter keeps its authored name unless a parameter of the
    // function's owner, an inherited parameter or an earlier parameter already
    // has it.
    let inherited_names: Vec<IdentId<'db>> = params
        .iter()
        .filter_map(|param| param.name().to_opt())
        .chain(owner_names.iter().copied())
        .collect();
    let authored_names: Vec<IdentId<'db>> = function
        .generic_params
        .data(db)
        .iter()
        .filter_map(|param| param.name().to_opt())
        .collect();
    let generic_name = |authored: IdentId<'db>, chosen: &[IdentId<'db>]| {
        if !inherited_names.contains(&authored) && !chosen.contains(&authored) {
            return authored;
        }
        (1usize..)
            .map(|suffix| IdentId::new(db, format!("{}{suffix}", authored.data(db))))
            .find(|name| {
                !inherited_names.contains(name)
                    && !authored_names.contains(name)
                    && !chosen.contains(name)
            })
            .expect("an unused suffix exists")
    };
    if bound_sites.len() != function.generic_params.data(db).len()
        || type_defaults.len() != function.generic_params.data(db).len()
        || const_types.len() != function.generic_params.data(db).len()
    {
        return Err(QuoteConstructionFault::FunctionGenericBoundsChangedAfterFreezing.into());
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
                return Err(
                    QuoteConstructionFault::ConstParameterDefaultNeedsCheckedReification.into(),
                );
            }
            let primitive = const_ty
                .and_then(quote_hir_primitive)
                .ok_or(QuoteConstructionFault::ConstParameterTypeIsNotACheckedScalar)?;
            let name = generic_name(param.name.unwrap(), &names);
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
            return Err(QuoteConstructionFault::TypeParameterHasAConstType.into());
        }
        if !param.name.is_present()
            || param.default_ty.is_some() != default.is_some()
            || sites.len() != param.bounds.len()
        {
            return Err(
                QuoteConstructionFault::FunctionGenericBoundsAndDefaultsNeedCheckedReification
                    .into(),
            );
        }
        let bounds = param
            .bounds
            .iter()
            .zip(sites)
            .map(|(bound, site)| quoted_trait_bound(paths, bound, *site, roots, budget))
            .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
        let name = generic_name(param.name.unwrap(), &names);
        let mut param = param.clone();
        param.name = Partial::Present(name);
        param.bounds = bounds;
        param.default_ty = default
            .as_ref()
            .map(|default| quoted_signature_type(paths, budget, default, &names))
            .transpose()?;
        names.push(name);
        params.push(GenericParam::Type(param));
    }
    Ok((GenericParamListId::new(db, params), names))
}

fn quoted_trait_bound<'db>(
    paths: QuotePathContext<'db>,
    bound: &TypeBound<'db>,
    site: Option<u32>,
    roots: &[FrozenQuoteTrait],
    budget: &QuoteBudget,
) -> Result<TypeBound<'db>, QuoteConstructionError> {
    let db = paths.db;
    let TypeBound::Trait(trait_ref) = bound else {
        return Err(QuoteConstructionFault::KindBoundNeedsCheckedReification.into());
    };
    let Partial::Present(source) = trait_ref.path(db) else {
        return Err(QuoteConstructionFault::TraitBoundPathIsMissing.into());
    };
    if source.strip_generic_args(db) != source {
        return Err(QuoteConstructionFault::TraitBoundArgumentsNeedCheckedReification.into());
    }
    let site = site.ok_or(QuoteConstructionFault::TraitBoundHasNoCheckedRoot)?;
    let mut matching = roots.iter().filter(|root| root.site == site);
    let root = matching
        .next()
        .ok_or(QuoteConstructionFault::TraitBoundRootIsMissing)?;
    if matching.next().is_some() {
        return Err(QuoteConstructionFault::TraitBoundRootIsAmbiguous.into());
    }
    let path = bound_quote_trait_path(paths, root)?;
    let PathKind::BoundItem { display, .. } = path.kind(db) else {
        unreachable!("captured trait path");
    };
    budget.charge_many(2 + display.len(db))?;
    Ok(TypeBound::Trait(TraitRefId::new(
        db,
        Partial::Present(path),
    )))
}

fn quoted_signature_type<'db>(
    paths: QuotePathContext<'db>,
    budget: &QuoteBudget,
    frozen: &FrozenQuoteSignatureType,
    generic_names: &[IdentId<'db>],
) -> Result<TypeId<'db>, QuoteConstructionError> {
    quoted_signature_type_with_owner(paths, budget, frozen, &[], generic_names)
}

fn quoted_signature_type_with_owner<'db>(
    paths: QuotePathContext<'db>,
    budget: &QuoteBudget,
    frozen: &FrozenQuoteSignatureType,
    owner_names: &[IdentId<'db>],
    generic_names: &[IdentId<'db>],
) -> Result<TypeId<'db>, QuoteConstructionError> {
    quoted_signature_type_with_holes(
        paths,
        budget,
        frozen,
        owner_names,
        generic_names,
        &mut |_, _| Err(QuoteConstructionFault::TypeSpliceNeedsACheckedTargetContext.into()),
    )
}

fn quoted_signature_type_with_holes<'db>(
    paths: QuotePathContext<'db>,
    budget: &QuoteBudget,
    frozen: &FrozenQuoteSignatureType,
    owner_names: &[IdentId<'db>],
    generic_names: &[IdentId<'db>],
    hole: &mut impl FnMut(u32, usize) -> Result<TypeId<'db>, QuoteConstructionError>,
) -> Result<TypeId<'db>, QuoteConstructionError> {
    QuoteTypeReifier {
        budget,
        depth: 0,
        paths,
        owner_names,
        generic_names,
    }
    .reify(frozen, hole)
}

struct QuoteTypeReifier<'a, 'db> {
    budget: &'a QuoteBudget,
    depth: usize,
    paths: QuotePathContext<'db>,
    owner_names: &'a [IdentId<'db>],
    generic_names: &'a [IdentId<'db>],
}

impl<'db> QuoteTypeReifier<'_, 'db> {
    fn reify(
        &mut self,
        frozen: &FrozenQuoteSignatureType,
        hole: &mut impl FnMut(u32, usize) -> Result<TypeId<'db>, QuoteConstructionError>,
    ) -> Result<TypeId<'db>, QuoteConstructionError> {
        self.budget.charge_emitted_type(frozen, self.depth)?;
        self.depth += 1;
        let result = self.reify_inner(frozen, hole);
        self.depth -= 1;
        result
    }

    fn reify_inner(
        &mut self,
        frozen: &FrozenQuoteSignatureType,
        hole: &mut impl FnMut(u32, usize) -> Result<TypeId<'db>, QuoteConstructionError>,
    ) -> Result<TypeId<'db>, QuoteConstructionError> {
        let paths = self.paths;
        let db = paths.db;
        let owner_names = self.owner_names;
        let generic_names = self.generic_names;
        match frozen {
            FrozenQuoteSignatureType::Primitive(primitive) => {
                let primitive = quote_hir_primitive(*primitive)
                    .ok_or(QuoteConstructionFault::PrimitiveSignatureTypeIsUnsupported)?;
                Ok(TypeId::new(db, TypeKind::Primitive(primitive)))
            }
            FrozenQuoteSignatureType::Hole(slot) => hole(*slot, self.depth - 1),
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
                    .ok_or(QuoteConstructionFault::FunctionGenericIndexIsMissing)?;
                Ok(TypeId::new(
                    db,
                    TypeKind::Path(Partial::Present(PathId::from_ident(db, *name))),
                ))
            }
            FrozenQuoteSignatureType::OwnerGeneric(index) => {
                let name = owner_names
                    .get(*index as usize)
                    .ok_or(QuoteConstructionFault::ReadBackOwnerGenericHasNoDestinationBinding)?;
                Ok(TypeId::new(
                    db,
                    TypeKind::Path(Partial::Present(PathId::from_ident(db, *name))),
                ))
            }
            FrozenQuoteSignatureType::Nominal {
                root,
                arguments,
                arguments_given,
            } => self.rooted_type(
                bound_quote_nominal_path(paths, root)?,
                arguments,
                *arguments_given,
                hole,
            ),
            FrozenQuoteSignatureType::Trait {
                root,
                arguments,
                arguments_given,
            } => self.rooted_type(
                bound_quote_trait_path(paths, root)?,
                arguments,
                *arguments_given,
                hole,
            ),
            FrozenQuoteSignatureType::Mode(mode, inner) => Ok(TypeId::new(
                db,
                TypeKind::Mode(*mode, Partial::Present(self.reify(inner, hole)?)),
            )),
            FrozenQuoteSignatureType::Ptr(inner) => Ok(TypeId::new(
                db,
                TypeKind::Ptr(Partial::Present(self.reify(inner, hole)?)),
            )),
            FrozenQuoteSignatureType::Tuple(elements) => Ok(TypeId::new(
                db,
                TypeKind::Tuple(TupleTypeId::new(
                    db,
                    elements
                        .iter()
                        .map(|element| self.reify(element, hole).map(Partial::Present))
                        .collect::<Result<Vec<_>, QuoteConstructionError>>()?,
                )),
            )),
            FrozenQuoteSignatureType::Never => Ok(TypeId::new(db, TypeKind::Never)),
        }
    }

    fn rooted_type(
        &mut self,
        mut path: PathId<'db>,
        arguments: &[FrozenQuoteSignatureType],
        arguments_given: bool,
        hole: &mut impl FnMut(u32, usize) -> Result<TypeId<'db>, QuoteConstructionError>,
    ) -> Result<TypeId<'db>, QuoteConstructionError> {
        let db = self.paths.db;
        if arguments_given || !arguments.is_empty() {
            let args = arguments
                .iter()
                .map(|argument| self.reify(argument, hole))
                .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
            path = quote_type_arguments(db, path, args)
                .ok_or(QuoteConstructionFault::RootedTypePathHasNoFinalItem)?;
        }
        Ok(TypeId::new(db, TypeKind::Path(Partial::Present(path))))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub(crate) enum QuoteHygiene<'db> {
    Function {
        func: Func<'db>,
        variants: Vec<(FrozenQuoteConstructorSite, QuoteConstructorIdentity<'db>)>,
        const_predicates: Vec<QuoteConstHygiene<'db>>,
        locals: Vec<(PatId, Vec<ExprId>)>,
        bindings: Vec<(ExprId, super::hir_builder::QuoteDestinationBinding)>,
        effects: Vec<(ExprId, usize)>,
        generics: Vec<(ExprId, IdentId<'db>)>,
        method_calls: Vec<QuoteMethodCall<'db>>,
        site_span: Option<std::ops::Range<usize>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub(crate) struct QuoteConstHygiene<'db> {
    body: Body<'db>,
    generics: Vec<(ExprId, IdentId<'db>)>,
}

fn quoted_member_holes<'db>(
    quote: &Quote<'db>,
    node: &FrozenCodeNode,
    target: &TargetContext<'_, 'db>,
    provider_body: Option<Body<'db>>,
) -> Result<Vec<QuoteMemberHole<'db>>, QuoteConstructionFault> {
    quote
        .holes
        .iter()
        .enumerate()
        .filter(|(_, hole)| {
            matches!(
                hole.position,
                QuoteHolePosition::Member | QuoteHolePosition::FieldLabel
            )
        })
        .map(|(index, hole)| {
            let Some(FrozenCodeHole::Value(value, _)) = node.holes.get(index) else {
                return Err(QuoteConstructionFault::MemberHoleIsMissing);
            };
            let field = target.field_index(value, false)?;
            let crate::analysis::ty::adt_def::AdtRef::Struct(owner) = target.owner else {
                return Err(QuoteConstructionFault::FieldNameRequiresASelectedFieldList);
            };
            Ok(QuoteMemberHole {
                slot: u32::try_from(index)
                    .map_err(|_| QuoteConstructionFault::MemberSlotExceedsU32)?,
                label: target.field_name(field)?,
                field: crate::hir_def::QuoteFieldGrant {
                    field: ScopeId::Field(crate::hir_def::FieldParent::Struct(owner), field as u16),
                    source: provider_body.map(|body| (body, hole.expr)),
                },
            })
        })
        .collect()
}

fn quoted_fragment_sources<'db>(
    db: &'db dyn HirDb,
    code: &FrozenCode,
    target: &TargetContext<'_, 'db>,
    budget: &QuoteBudget,
) -> Result<Vec<Option<QuoteFragmentSource<'db>>>, QuoteConstructionError> {
    let paths = QuotePathContext::new(db, target.owner.scope().ingot(db));
    code.nodes
        .iter()
        .map(|node| -> Result<_, QuoteConstructionError> {
            budget.charge_many(node.metadata_nodes() + node.rooted_path_nodes(None))?;
            let Some(key) = node.template() else {
                return Ok(None);
            };
            let file = db
                .workspace()
                .get(db, key.owner.file())
                .ok_or(QuoteConstructionFault::QuoteOwnerFileIsMissing)?;
            let base = base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
            let owner = locate_authored_function(db, base, &key.owner)?;
            let body = owner
                .body(db)
                .ok_or(QuoteConstructionFault::QuoteOwnerBodyIsMissing)?;
            let (template, root, open_names, member_holes, has_block_items) = match key.source {
                FrozenTemplateSource::Quote => {
                    let Partial::Present(Expr::Quote(quote)) = &body.exprs(db)[key.expr] else {
                        return Err(QuoteConstructionFault::QuoteSiteNoLongerNamesAQuote.into());
                    };
                    if !matches!(quote.fragment, QuoteFragment::Expr | QuoteFragment::Arms(_)) {
                        return Ok(None);
                    }
                    if node.members().any(|member| member != QuoteMember::Fragment) {
                        return Err(
                            QuoteConstructionFault::NameBelongsToAnotherTemplateMember.into()
                        );
                    }
                    if quote.holes.len() != node.holes.len() {
                        return Err(QuoteConstructionFault::SpliceCountChangedAfterFreezing.into());
                    }
                    let arms = match &quote.fragment {
                        QuoteFragment::Arms(arms) => arms.len(),
                        _ => 0,
                    };
                    budget.charge_many(quote.open.len() + quote.holes.len() + arms)?;
                    let member_holes = quoted_member_holes(quote, node, target, Some(body))?;
                    (
                        quote.template,
                        match &quote.fragment {
                            QuoteFragment::Arms(arms) => {
                                super::hir_builder::QuoteFragmentRoot::Arms(arms.clone())
                            }
                            _ => super::hir_builder::QuoteFragmentRoot::Expr {
                                root: quote.expression_root(db),
                                splices: quote.splice_only_body(db),
                            },
                        },
                        quote.open.clone(),
                        member_holes,
                        !quote.block_items.is_empty(),
                    )
                }
                FrozenTemplateSource::CheckedBody => {
                    if node
                        .members()
                        .any(|member| member != QuoteMember::CheckedBody)
                    {
                        return Err(
                            QuoteConstructionFault::NameBelongsToAnotherTemplateMember.into()
                        );
                    }
                    if key.expr != body.expr(db) || !node.holes.is_empty() {
                        return Err(QuoteConstructionFault::CheckedBodyChangedAfterReadBack.into());
                    }
                    budget.charge_many(owner.params(db).count())?;
                    let open_names = owner
                        .params(db)
                        .enumerate()
                        .map(|(index, param)| {
                            param.name(db).unwrap_or_else(|| {
                                IdentId::new(db, format!("<unnamed-read-back-param-{index}>"))
                            })
                        })
                        .collect();
                    (
                        body,
                        super::hir_builder::QuoteFragmentRoot::Expr {
                            root: key.expr,
                            splices: None,
                        },
                        open_names,
                        Vec::new(),
                        false,
                    )
                }
            };
            let rooted_funcs = node
                .rooted_funcs
                .iter()
                .filter(|root| {
                    root.member
                        == match key.source {
                            FrozenTemplateSource::Quote => QuoteMember::Fragment,
                            FrozenTemplateSource::CheckedBody => QuoteMember::CheckedBody,
                        }
                })
                .map(|root| {
                    let path = bound_quote_function_path(paths, root)?;
                    Ok((root.site, path))
                })
                .collect::<Result<Vec<_>, QuoteConstructionFault>>()?;
            let qualified_paths = node
                .qualified_consts
                .iter()
                .map(|qualified| {
                    if key.source != FrozenTemplateSource::CheckedBody
                        || qualified.member != QuoteMember::CheckedBody
                    {
                        return Err(
                            QuoteConstructionFault::QualifiedFragmentConstantNeedsACheckedBody,
                        );
                    }
                    let path = bound_quote_qualified_const_path(paths, template, node, qualified)?;
                    Ok((qualified.site, path))
                })
                .collect::<Result<Vec<_>, QuoteConstructionFault>>()?;
            let selected_methods = node
                .selected_methods
                .iter()
                .map(|(site, locator)| {
                    let file = db
                        .workspace()
                        .get(db, locator.file())
                        .ok_or(QuoteConstructionFault::ReadBackMethodFileIsMissing)?;
                    let base = base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
                    locate_authored_function(db, base, locator).map(|method| (*site, method))
                })
                .collect::<Result<Vec<_>, QuoteConstructionFault>>()?;
            Ok(Some(QuoteFragmentSource {
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

impl<'db> QuoteConstructorIdentity<'db> {
    fn from_variant(variant: crate::hir_def::EnumVariant<'db>) -> Self {
        Self {
            owner: crate::analysis::ty::adt_def::AdtRef::Enum(variant.enum_),
            variant_index: Some(variant.idx),
        }
    }

    fn from_ty(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> Option<Self> {
        let TyData::TyBase(TyBase::Adt(adt)) = ty.base_ty(db).data(db) else {
            return None;
        };
        Some(Self {
            owner: adt.adt_ref(db),
            variant_index: None,
        })
    }
}

fn reify_quote_constructor<'db>(
    paths: QuotePathContext<'db>,
    budget: &QuoteBudget,
    root: &FrozenQuoteConstructor,
    owner_names: &[IdentId<'db>],
    function_names: &[IdentId<'db>],
) -> Result<(PathId<'db>, QuoteConstructorIdentity<'db>), QuoteConstructionError> {
    let db = paths.db;
    let (path, identity) = reify_quote_nominal_constructor(paths, &root.owner, root.variant_index)?;
    if root.self_target {
        let mut self_path = PathId::self_ty(db, GenericArgListId::none(db));
        if root.variant_index.is_some() {
            self_path = self_path.push_str(
                db,
                path.ident(db)
                    .to_opt()
                    .ok_or(QuoteConstructionFault::VariantHasNoAuthoredName)?
                    .data(db),
            );
        }
        return Ok((self_path, identity));
    }
    let Some(arguments) = &root.type_arguments else {
        return Ok((path, identity));
    };
    let owner_path = if root.variant_index.is_some() {
        path.parent(db)
            .ok_or(QuoteConstructionFault::ConstructorOwnerIsAbsentFromAuthoredHIR)?
    } else {
        path
    };
    let ty = QuoteTypeReifier {
        budget,
        depth: 0,
        paths,
        owner_names,
        generic_names: function_names,
    }
    .rooted_type(owner_path, arguments, !arguments.is_empty(), &mut |_, _| {
        Err(QuoteConstructionFault::ReadBackRecordConstructorNeedsANominalType.into())
    })?;
    let TypeKind::Path(Partial::Present(mut owner_path)) = ty.data(db).clone() else {
        return Err(QuoteConstructionFault::ReadBackRecordConstructorNeedsANominalType.into());
    };
    if root.variant_index.is_some() {
        let PathKind::Ident { ident, .. } = path.kind(db) else {
            return Err(QuoteConstructionFault::VariantHasNoAuthoredName.into());
        };
        owner_path = owner_path.push_str(
            db,
            ident
                .to_opt()
                .ok_or(QuoteConstructionFault::VariantHasNoAuthoredName)?
                .data(db),
        );
    }
    Ok((owner_path, identity))
}

pub(super) fn reify_quote_nominal_constructor<'db>(
    paths: QuotePathContext<'db>,
    root: &FrozenQuoteNominal,
    variant_index: Option<u16>,
) -> Result<(PathId<'db>, QuoteConstructorIdentity<'db>), QuoteConstructionFault> {
    let db = paths.db;
    use crate::analysis::ty::adt_def::AdtRef;
    let owner = locate_authored_item(
        db,
        &root.owner,
        crate::analysis::ty::adt_def::AdtRef::try_from_item,
    )
    .map_err(|error| {
        error.fault(QuoteConstructionFault::ConstructorOwnerIsAbsentFromAuthoredHIR)
    })?;
    let mut path = bound_quote_declaration_path(paths, owner.scope())?;
    match (owner, variant_index) {
        (AdtRef::Struct(_), None) => {}
        (AdtRef::Enum(owner), Some(index)) => {
            if usize::from(index) >= owner.len_variants(db) {
                return Err(QuoteConstructionFault::VariantIndexIsOutsideItsAuthoredEnum);
            }
            let variant = crate::hir_def::EnumVariant::new(owner, usize::from(index));
            let name = variant
                .name(db)
                .ok_or(QuoteConstructionFault::VariantHasNoAuthoredName)?;
            path = path.push_str(db, name);
        }
        _ => {
            return Err(QuoteConstructionFault::ConstructorKindDisagreesWithItsNominalOwner);
        }
    }
    Ok((
        path,
        QuoteConstructorIdentity {
            owner,
            variant_index,
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
}

/// Reify the frozen fragment's types only after its destination binder frame
/// is known. Item signatures and body fragments share the same type reifier.
pub(super) fn reify_quote_fragment_types<'db>(
    paths: QuotePathContext<'db>,
    budget: &QuoteBudget,
    node: &FrozenCodeNode,
    source: &QuoteFragmentSource<'db>,
    owner_names: &[IdentId<'db>],
    function_names: &[IdentId<'db>],
) -> Result<QuoteExprTypes<'db>, QuoteConstructionError> {
    let db = paths.db;
    let mut reify = |ty: &FrozenQuoteSignatureType| {
        quoted_signature_type_with_owner(paths, budget, ty, owner_names, function_names)
    };
    let rooted_paths = source
        .rooted_funcs
        .iter()
        .map(|(site, path)| {
            let frozen = node
                .rooted_funcs
                .iter()
                .find(|root| root.site == *site)
                .ok_or(QuoteConstructionFault::RootedFunctionHasNoFrozenIdentity)?;
            let path = if let Some(arguments) = &frozen.type_arguments {
                let arguments = arguments
                    .iter()
                    .map(&mut reify)
                    .collect::<Result<Vec<_>, _>>()?;
                quote_type_arguments(db, *path, arguments)
                    .ok_or(QuoteConstructionFault::FunctionPathHasNoFinalItem)?
            } else {
                *path
            };
            Ok((*site, path))
        })
        .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
    let local_types = node
        .local_types
        .iter()
        .map(|local| reify(&local.ty).map(|ty| (local.stmt, ty)))
        .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
    let record_paths = node
        .record_types
        .iter()
        .map(|(site, frozen)| {
            let ty = reify(frozen)?;
            let TypeKind::Path(Partial::Present(path)) = ty.data(db) else {
                return Err(
                    QuoteConstructionFault::ReadBackRecordConstructorNeedsANominalType.into(),
                );
            };
            Ok((*site, *path))
        })
        .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
    let cast_types = quote_cast_types(db, node)?;
    let variants = node
        .rooted_constructors
        .iter()
        .map(|root| {
            reify_quote_constructor(paths, budget, root, owner_names, function_names)
                .map(|(path, variant)| (root.site, path, variant))
        })
        .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
    Ok(QuoteExprTypes {
        variants,
        rooted_paths,
        cast_types,
        local_types,
        record_paths,
    })
}

/// The checked target type of every cast in a frozen template, keyed by its
/// template expression. Fragments and quoted bodies share this one table.
pub(super) fn quote_cast_types<'db>(
    db: &'db dyn HirDb,
    node: &FrozenCodeNode,
) -> Result<Vec<(ExprId, TypeId<'db>)>, QuoteConstructionError> {
    node.cast_primitives
        .iter()
        .map(|(site, primitive)| {
            let primitive = quote_hir_primitive(*primitive)
                .ok_or(QuoteConstructionFault::ReadBackCastPrimitiveIsUnsupported)?;
            Ok((*site, TypeId::new(db, TypeKind::Primitive(primitive))))
        })
        .collect()
}

#[derive(Default)]
struct QuoteMemberSites<'a> {
    rooted_funcs: Vec<&'a crate::core::meta::FrozenQuoteFunction>,
    rooted_constructors: Vec<&'a crate::core::meta::FrozenQuoteConstructor>,
    qualified_consts: Vec<&'a crate::core::meta::FrozenQuoteQualifiedConst>,
    local_types: Vec<&'a crate::core::meta::FrozenQuoteLocalType>,
    param_uses: Vec<&'a crate::core::meta::FrozenQuoteParamUse>,
    effect_uses: Vec<&'a crate::core::meta::FrozenQuoteEffectUse>,
    generic_uses: Vec<&'a crate::core::meta::FrozenQuoteGenericUse>,
    self_uses: Vec<&'a crate::core::meta::FrozenQuoteSelfUse>,
    self_record_inits: Vec<&'a crate::core::meta::FrozenQuoteSelfUse>,
    locals: Vec<(usize, &'a crate::core::meta::FrozenQuoteLocal)>,
}

impl QuoteMemberSites<'_> {
    fn generic_bindings<'db>(
        &self,
        names: &[IdentId<'db>],
    ) -> Result<Vec<(ExprId, IdentId<'db>)>, QuoteConstructionError> {
        self.generic_uses
            .iter()
            .map(|use_| {
                let name = names
                    .get(use_.index as usize)
                    .ok_or(QuoteConstructionFault::GenericUseHasNoParameter)?;
                Ok((use_.site, *name))
            })
            .collect()
    }

    fn has_body_bindings(&self) -> bool {
        !self.rooted_constructors.is_empty()
            || !self.qualified_consts.is_empty()
            || !self.locals.is_empty()
            || !self.local_types.is_empty()
            || !self.param_uses.is_empty()
            || !self.effect_uses.is_empty()
            || !self.self_uses.is_empty()
            || !self.self_record_inits.is_empty()
    }
}

struct QuoteMemberGroups<'a> {
    members: rustc_hash::FxHashMap<QuoteMember, QuoteMemberSites<'a>>,
    empty: QuoteMemberSites<'a>,
}

impl<'a> QuoteMemberGroups<'a> {
    fn new(node: &'a FrozenCodeNode, budget: &QuoteBudget) -> Result<Self, QuoteConstructionError> {
        let mut members: rustc_hash::FxHashMap<QuoteMember, QuoteMemberSites<'a>> =
            Default::default();
        macro_rules! index_sites {
            ($field:ident) => {
                for site in &node.$field {
                    if !members.contains_key(&site.member) {
                        budget.charge()?;
                    }
                    budget.charge()?;
                    members.entry(site.member).or_default().$field.push(site);
                }
            };
        }
        index_sites!(rooted_funcs);
        index_sites!(rooted_constructors);
        index_sites!(qualified_consts);
        index_sites!(local_types);
        index_sites!(param_uses);
        index_sites!(effect_uses);
        index_sites!(generic_uses);
        index_sites!(self_uses);
        index_sites!(self_record_inits);
        for (index, site) in node.locals.iter().enumerate() {
            if !members.contains_key(&site.member) {
                budget.charge()?;
            }
            budget.charge()?;
            members
                .entry(site.member)
                .or_default()
                .locals
                .push((index, site));
        }
        Ok(Self {
            members,
            empty: QuoteMemberSites::default(),
        })
    }

    fn get(&self, member: QuoteMember) -> &QuoteMemberSites<'a> {
        self.members.get(&member).unwrap_or(&self.empty)
    }
}

fn emit_closed_quoted_items<'db>(
    db: &'db dyn HirDb,
    ctxt: &mut FileLowerCtxt<'db>,
    request: &FrozenGeneration,
    origin: GenerationDesugared,
    target: &TargetContext<'_, 'db>,
) -> Result<Vec<QuoteHygiene<'db>>, QuoteConstructionError> {
    let FrozenMetaValue::Code(code) = &request.value else {
        return Err(QuoteConstructionFault::ExpectedAQuotedGenerationResult.into());
    };
    let fragment_sources = quoted_fragment_sources(db, code, target, &ctxt.quote_budget)?;
    ctxt.quote_budget.charge_many(code.nodes.len())?;
    let member_groups = code
        .nodes
        .iter()
        .map(|node| QuoteMemberGroups::new(node, &ctxt.quote_budget))
        .collect::<Result<Vec<_>, _>>()?;
    let mut hygiene = Vec::new();
    let mut ordinal = 0usize;
    let mut pending = vec![(code.root, 0, 0, None)];
    while let Some((code_root, start, mut func_index, splice_source)) = pending.pop() {
        ctxt.quote_budget
            .charge()
            .map_err(|error| error.at(splice_source.clone()))?;
        let node = code
            .nodes
            .get(code_root)
            .ok_or(QuoteConstructionFault::CodeRootIsMissing)?;
        let Some(key) = node.template() else {
            if !matches!(node.source, crate::core::meta::FrozenCodeSource::Empty) {
                return Err(
                    QuoteConstructionFault::CompilerIssuedBinderCodeCannotStandForItems.into(),
                );
            }
            if !node.holes.is_empty() {
                return Err(QuoteConstructionFault::EmptyItemCodeHasSplices.into());
            }
            continue;
        };
        let file = db
            .workspace()
            .get(db, key.owner.file())
            .ok_or(QuoteConstructionFault::QuoteOwnerFileIsMissing)?;
        let provider_base = base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
        let owner = locate_authored_function(db, provider_base, &key.owner)?;
        let body = owner
            .body(db)
            .ok_or(QuoteConstructionFault::QuoteOwnerBodyIsMissing)?;
        let Partial::Present(Expr::Quote(quote)) = &body.exprs(db)[key.expr] else {
            return Err(QuoteConstructionFault::QuoteSiteNoLongerNamesAQuote.into());
        };
        if quote.holes.len() != node.holes.len() {
            return Err(QuoteConstructionFault::SpliceCountChangedAfterFreezing.into());
        }
        // A body made only of splices, `quote { ${a} ${b} }`, checked as item
        // code, composes its fragments in order.
        let spliced_items: Vec<QuoteItem<'db>>;
        let (items, splice_only) = match &quote.fragment {
            QuoteFragment::Items(items) => (items.as_slice(), false),
            _ => {
                let slots = quote
                    .splice_only_body(db)
                    .ok_or(QuoteConstructionFault::GenerationRequiresItemCode)?;
                spliced_items = slots.into_iter().map(QuoteItem::Hole).collect();
                (spliced_items.as_slice(), true)
            }
        };
        for member in member_groups[code_root].members.keys() {
            let valid = match *member {
                QuoteMember::Function(index) => node.function_types.get(index as usize).is_some(),
                QuoteMember::FunctionWhere(index, predicate) => node
                    .function_types
                    .get(index as usize)
                    .and_then(|types| types.where_predicates.get(predicate as usize))
                    .is_some_and(|predicate| matches!(predicate, FrozenQuoteWherePredicate::Const)),
                QuoteMember::AssocConst(index) => {
                    node.assoc_const_primitives.get(index as usize).is_some()
                }
                QuoteMember::Fragment | QuoteMember::CheckedBody => false,
            };
            if !valid {
                return Err(QuoteConstructionFault::NameBelongsToAnotherTemplateMember.into());
            }
            if matches!(member, QuoteMember::FunctionWhere(..))
                && member_groups[code_root].get(*member).has_body_bindings()
            {
                return Err(
                    QuoteConstructionFault::ConstPredicateNeedsCheckedNameTransplantation.into(),
                );
            }
        }
        let cast_types = quote_cast_types(db, node)?;
        let emit = QuoteEmitCtx {
            db,
            request,
            target,
            quote,
            node,
            member_groups: &member_groups[code_root],
            code,
            fragment_sources: &fragment_sources,
            code_root,
            provider_scope: owner.scope(),
            cast_types: &cast_types,
        };
        for (item_index, item) in items.iter().enumerate().skip(start) {
            if let QuoteItem::Hole(slot) = item {
                if !splice_only
                    && quote.holes.get(*slot as usize).map(|hole| hole.position)
                        != Some(QuoteHolePosition::Code(crate::hir_def::QuoteKind::Item))
                {
                    return Err(QuoteConstructionFault::ItemSplicePositionChanged.into());
                }
                let Some(FrozenCodeHole::Code(child, source)) = node.holes.get(*slot as usize)
                else {
                    return Err(QuoteConstructionFault::ItemSpliceIsNotItemCode.into());
                };
                // Resume this fragment after its child, preserving declaration order.
                pending.push((code_root, item_index + 1, func_index, splice_source.clone()));
                pending.push((*child, 0, 0, source.clone()));
                break;
            }
            ctxt.quote_budget
                .charge()
                .map_err(|error| error.at(splice_source.clone()))?;
            let item_ordinal = ordinal;
            ordinal = ordinal
                .checked_add(1)
                .ok_or(QuoteConstructionFault::ItemCountExceedsUsize)?;
            ctxt.enter_id_namespace(TrackedItemVariant::GeneratedSite(Box::new(
                crate::hir_def::GeneratedSiteKey {
                    target: request.target.clone(),
                    provider: request.provider.clone(),
                    attribute_index: request.attribute_index,
                    ordinal: item_ordinal,
                },
            )));
            let emitted = (|| match item {
                QuoteItem::Func(function) => {
                    let (generic_params, generic_names) = quoted_record_generics(
                        db,
                        target.owner,
                        &request.target_const_param_types,
                        &ctxt.quote_budget,
                    )?;
                    let source = top_mod_ast(db, ctxt.top_mod());
                    let mut builder = HirBuilder::new(ctxt, origin.clone(), source.syntax());
                    Ok(vec![emit_quoted_function(
                        emit,
                        &mut builder,
                        function,
                        func_index,
                        generic_params,
                        &generic_names,
                        false,
                    )?])
                }
                QuoteItem::Impl(_) if items.len() == 1 => {
                    emit_closed_quoted_impl(emit, ctxt, origin.clone(), item)
                }
                _ => Err(QuoteConstructionFault::ItemListNeedsCheckedTransplantation.into()),
            })();
            ctxt.leave_id_namespace();
            hygiene.extend(emitted.map_err(|error| error.at(splice_source.clone()))?);
            if matches!(item, QuoteItem::Func(_)) {
                func_index += 1;
            }
        }
    }
    Ok(hygiene)
}

#[derive(Clone, Copy)]
struct QuoteEmitCtx<'a, 'db> {
    db: &'db dyn HirDb,
    request: &'a FrozenGeneration,
    target: &'a TargetContext<'a, 'db>,
    quote: &'a Quote<'db>,
    node: &'a FrozenCodeNode,
    member_groups: &'a QuoteMemberGroups<'a>,
    code: &'a FrozenCode,
    fragment_sources: &'a [Option<QuoteFragmentSource<'db>>],
    code_root: usize,
    provider_scope: ScopeId<'db>,
    cast_types: &'a [(ExprId, TypeId<'db>)],
}

impl<'db> QuoteEmitCtx<'_, 'db> {
    fn signature_type(
        self,
        builder: &mut HirBuilder<'_, 'db, GenerationDesugared>,
        frozen: &FrozenQuoteSignatureType,
        owner_names: &[IdentId<'db>],
        function_names: &[IdentId<'db>],
    ) -> Result<TypeId<'db>, QuoteConstructionError> {
        quoted_signature_type_with_holes(
            QuotePathContext::new(self.db, builder.top_mod().ingot(self.db)),
            &builder.ctxt().quote_budget.clone(),
            frozen,
            &[],
            function_names,
            &mut |slot, depth| self.type_hole(builder, slot, depth, owner_names),
        )
    }

    fn type_hole(
        self,
        builder: &mut HirBuilder<'_, 'db, GenerationDesugared>,
        slot: u32,
        depth: usize,
        generic_names: &[IdentId<'db>],
    ) -> Result<TypeId<'db>, QuoteConstructionError> {
        quoted_type_hole(
            builder,
            self.quote,
            self.node,
            slot,
            depth,
            self.target,
            generic_names,
        )
    }
}

fn emit_quoted_function<'db>(
    emit: QuoteEmitCtx<'_, 'db>,
    builder: &mut HirBuilder<'_, 'db, GenerationDesugared>,
    function: &crate::hir_def::QuoteFunc<'db>,
    func_index: u32,
    generic_params: GenericParamListId<'db>,
    generic_names: &[IdentId<'db>],
    is_method: bool,
) -> Result<QuoteHygiene<'db>, QuoteConstructionError> {
    let db = emit.db;
    let type_budget = builder.ctxt().quote_budget.clone();
    let paths = QuotePathContext::new(db, builder.top_mod().ingot(db));
    let member = emit.member_groups.get(QuoteMember::Function(func_index));
    let function_types = emit
        .node
        .function_types
        .get(func_index as usize)
        .ok_or(QuoteConstructionFault::FunctionHasNoCheckedSignature)?;
    let name = quoted_name(function.name, emit.node, emit.target)?;
    let Partial::Present(params) = function.params else {
        return Err(QuoteConstructionFault::FunctionParametersAreMissing.into());
    };
    // Signature binders create scopes even when their body is tiny.
    builder.ctxt().quote_budget.charge_many(
        params.data(db).len()
            + generic_params.data(db).len()
            + function.generic_params.data(db).len()
            + function.effects.data(db).len(),
    )?;
    if !function.attrs.data(db).is_empty()
        || function.modifiers.is_unsafe
        || function.modifiers.is_extern
    {
        return Err(QuoteConstructionFault::FunctionHeaderNeedsNameAndTypeRewriting.into());
    }
    let budget = &builder.ctxt().quote_budget;
    budget.charge_many(
        function_types
            .generic_bound_sites
            .iter()
            .map(Vec::len)
            .sum::<usize>()
            + function_types.where_predicates.len()
            + function_types
                .where_predicates
                .iter()
                .map(|predicate| match predicate {
                    FrozenQuoteWherePredicate::Bound { trait_sites, .. } => trait_sites.len(),
                    FrozenQuoteWherePredicate::Const => 0,
                })
                .sum::<usize>()
            + member
                .locals
                .iter()
                .map(|(_, local)| 1 + local.uses.len())
                .sum::<usize>(),
    )?;
    budget.charge_expression_paths(emit.node, Some(func_index))?;
    budget.charge_many(function_types.generic_const_types.iter().flatten().count())?;

    let (generic_params, function_generic_names) = quoted_function_generics(
        paths,
        budget,
        function,
        generic_params,
        generic_names,
        function_types,
        &emit.node.rooted_traits,
    )?;
    if function.effects.data(db).len() != function_types.effects.len() {
        return Err(QuoteConstructionFault::MethodEffectsChangedAfterFreezing.into());
    }
    let mut effect_names = Vec::new();
    let effects = EffectParamListId::new(
        db,
        function
            .effects
            .data(db)
            .iter()
            .zip(&function_types.effects)
            .map(|(effect, frozen)| {
                let ty = frozen
                    .as_ref()
                    .ok_or(QuoteConstructionFault::EffectKeyNeedsCheckedReification)?;
                let ty = quoted_signature_type(paths, &type_budget, ty, &function_generic_names)?;
                // Effect uses are bound by position, so the authored name
                // cannot be captured.
                let name = effect.name;
                effect_names.push(name);
                let mut copied = effect.clone();
                copied.name = name;
                copied.key_ty = Partial::Present(ty);
                Ok(copied)
            })
            .collect::<Result<Vec<_>, QuoteConstructionError>>()?,
    );
    if function.where_clause.predicates.len() != function_types.where_predicates.len() {
        return Err(QuoteConstructionFault::MethodWherePredicatesChangedAfterFreezing.into());
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
                    return Err(QuoteConstructionFault::MethodWhereBoundChangedAfterFreezing.into());
                }
                let ty = quoted_signature_type(paths, &type_budget, ty, &function_generic_names)?;
                let bounds = predicate
                    .bounds
                    .iter()
                    .zip(trait_sites)
                    .map(|(bound, site)| {
                        quoted_trait_bound(
                            paths,
                            bound,
                            *site,
                            &emit.node.rooted_traits,
                            &builder.ctxt().quote_budget,
                        )
                    })
                    .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
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
                return Err(
                    QuoteConstructionFault::MethodWherePredicateChangedAfterFreezing.into(),
                );
            }
        }
    }
    let generic_uses = member.generic_bindings(&function_generic_names)?;
    let mut param_uses = member
        .param_uses
        .iter()
        .map(|use_| {
            let param = params
                .data(db)
                .get(use_.index as usize)
                .ok_or(QuoteConstructionFault::ParameterIndexIsMissing)?;
            let name = param
                .name()
                .ok_or(QuoteConstructionFault::ParameterUseHasNoName)?;
            Ok((use_.site, name, use_.index as usize))
        })
        .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
    let function_self_uses = member.self_uses.iter();
    if is_method {
        param_uses.extend(function_self_uses.map(|use_| (use_.site, IdentId::make_self(db), 0)));
    } else if function_self_uses.count() != 0 {
        return Err(QuoteConstructionFault::SelfNameRequiresAMethod.into());
    }
    let effect_uses = member
        .effect_uses
        .iter()
        .map(|use_| {
            let name = effect_names
                .get(use_.index as usize)
                .copied()
                .flatten()
                .ok_or(QuoteConstructionFault::EffectUseHasNoBoundName)?;
            Ok((use_.site, name, use_.index as usize))
        })
        .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
    if function_types.params.len() != params.data(db).len() {
        return Err(QuoteConstructionFault::ParameterTypeCountChangedAfterFreezing.into());
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
                    return Err(QuoteConstructionFault::SelfParameterNeedsAMethod.into());
                }
                let Partial::Present(_) = param.ty else {
                    return Err(QuoteConstructionFault::ParameterTypeIsMissing.into());
                };
                let ty = emit.signature_type(
                    builder,
                    primitive
                        .as_ref()
                        .ok_or(QuoteConstructionFault::ParameterNeedsCheckedRewriting)?,
                    generic_names,
                    &function_generic_names,
                )?;
                let mut copied = param.clone();
                copied.ty = Partial::Present(ty);
                Ok(copied)
            })
            .collect::<Result<Vec<_>, QuoteConstructionError>>()?,
    );
    let open_bindings = params
        .data(db)
        .iter()
        .enumerate()
        .filter_map(|(index, param)| param.name().map(|name| (name, index)))
        .collect::<Vec<_>>();
    if function.ret_ty.is_none() {
        return Err(QuoteConstructionFault::FunctionResultTypeIsMissing.into());
    }
    let ret_ty = emit.signature_type(
        builder,
        function_types
            .result
            .as_ref()
            .ok_or(QuoteConstructionFault::FunctionResultTypeNeedsCheckedRewriting)?,
        generic_names,
        &function_generic_names,
    )?;
    let root = function
        .body
        .ok_or(QuoteConstructionFault::FunctionBodyIsMissing)?;
    let rooted_funcs = member
        .rooted_funcs
        .iter()
        .map(|root| {
            Ok((
                root.site,
                quoted_function_path(
                    emit,
                    paths,
                    builder,
                    root,
                    generic_names,
                    &function_generic_names,
                )?,
            ))
        })
        .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
    let qualified_consts = member
        .qualified_consts
        .iter()
        .map(|qualified| {
            let path =
                bound_quote_qualified_const_path(paths, emit.quote.template, emit.node, qualified)?;
            Ok((qualified.site, path))
        })
        .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
    let self_record_inits = member
        .self_record_inits
        .iter()
        .map(|use_| use_.site)
        .collect::<Vec<_>>();
    let locals: Vec<(PatId, IdentId<'db>, Vec<ExprId>)> = member
        .locals
        .iter()
        .map(|(_, local)| {
            Ok((
                local.binder,
                super::hir_builder::authored_binder_name(db, emit.quote.template, local.binder)?,
                local.uses.clone(),
            ))
        })
        .collect::<Result<_, QuoteConstructionFault>>()?;
    let variants = member
        .rooted_constructors
        .iter()
        .map(|root| {
            reify_quote_constructor(
                paths,
                &type_budget,
                root,
                generic_names,
                &function_generic_names,
            )
            .map(|(path, variant)| (root.site, path, variant))
        })
        .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
    let local_types = member
        .local_types
        .iter()
        .map(|local| {
            quoted_signature_type(paths, &type_budget, &local.ty, &function_generic_names)
                .map(|ty| (local.stmt, ty))
        })
        .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
    let member_holes = quoted_member_holes(
        emit.quote,
        emit.node,
        emit.target,
        emit.provider_scope.to_item().and_then(|item| match item {
            ItemKind::Func(func) => func.body(db),
            _ => None,
        }),
    )?;
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
                builder.quoted_owner_where_clause(emit.target.owner, generic_names)?
            };
            let owner_const_count = owner_where.const_predicates(db).len();
            const_hygiene.extend(owner_where.const_predicates(db).iter().map(|body| {
                QuoteConstHygiene {
                    body: *body,
                    generics: Vec::new(),
                }
            }));
            let const_predicates = const_where
                .iter()
                .map(|(index, expr)| {
                    let predicate = emit
                        .member_groups
                        .get(QuoteMember::FunctionWhere(func_index, *index));
                    let rooted = &predicate.rooted_funcs;
                    let rooted_paths = rooted
                        .iter()
                        .map(|root| {
                            Ok((
                                root.site,
                                quoted_function_path(
                                    emit,
                                    paths,
                                    builder,
                                    root,
                                    generic_names,
                                    &function_generic_names,
                                )?,
                            ))
                        })
                        .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
                    let generics = predicate.generic_bindings(&function_generic_names)?;
                    let (body, generic_sites) = builder.generated_quote_const_predicate(
                        u32::try_from(owner_const_count)
                            .ok()
                            .and_then(|count| count.checked_add(*index))
                            .ok_or(QuoteConstructionFault::ConstPredicateCountExceedsU32)?,
                        emit.quote.template,
                        *expr,
                        emit.provider_scope,
                        QuoteBodyBindings {
                            holes: &emit.node.holes,
                            cast_types: emit.cast_types,
                            rooted_funcs: &rooted_paths,
                            generic_uses: &generics,
                            ..Default::default()
                        },
                    )?;
                    const_hygiene.push(QuoteConstHygiene {
                        body,
                        generics: generic_sites,
                    });
                    Ok(body)
                })
                .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
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
            Ok::<_, QuoteConstructionError>((spec, (emit.quote.template, root)))
        },
        |body, (template, root)| {
            copied = Some(body.adopt_closed_quote_body(
                template,
                root,
                emit.provider_scope,
                QuoteBodyBindings {
                    holes: &emit.node.holes,
                    cast_types: emit.cast_types,
                    fragments: Some((emit.code, emit.fragment_sources)),
                    open_bindings: &open_bindings,
                    owner_generic_bindings: generic_names,
                    generic_bindings: &function_generic_names,
                    allow_self: is_method,
                    rooted_funcs: &rooted_funcs,
                    qualified_paths: &qualified_consts,
                    constructor_paths: &variants,
                    self_record_inits: &self_record_inits,
                    locals: &locals,
                    local_types: &local_types,
                    param_uses: &param_uses,
                    effect_uses: &effect_uses,
                    generic_uses: &generic_uses,
                    member_holes: &member_holes,
                },
            )?);
            Ok(())
        },
    )?;
    let copied = copied.ok_or(QuoteConstructionFault::BodyCopyIsMissing)?;
    let bindings = copied.bindings;
    let effects = copied.effects;
    let generics = copied.generics;
    let checked_variants = copied.constructor_sites;
    Ok(QuoteHygiene::Function {
        func,
        variants: checked_variants,
        const_predicates: const_hygiene,
        locals: copied.locals,
        bindings,
        effects,
        generics,
        method_calls: copied.method_calls,
        site_span: emit.request.site_span.clone(),
    })
}

fn quoted_impl_where_bounds<'db>(
    db: &'db dyn HirDb,
    builder: &mut HirBuilder<'_, 'db, GenerationDesugared>,
    code: &FrozenCode,
    root: usize,
    target: &TargetContext<'_, 'db>,
    names: &[IdentId<'db>],
) -> Result<Vec<WherePredicate<'db>>, QuoteConstructionError> {
    let type_budget = builder.ctxt().quote_budget.clone();
    let paths = QuotePathContext::new(db, builder.top_mod().ingot(db));
    let mut work = vec![(root, None, None)];
    let mut bounds = Vec::new();
    while let Some((index, predicate_index, edge)) = work.pop() {
        builder
            .ctxt()
            .quote_budget
            .charge()
            .map_err(|error| error.at(edge.clone()))?;
        (|| -> Result<(), QuoteConstructionError> {
            let node = code
                .nodes
                .get(index)
                .ok_or(QuoteConstructionFault::WhereGraphEdgeIsMissing)?;
            if node == &FrozenCodeNode::empty(None) {
                return Ok(());
            }
            let key = node
                .template()
                .ok_or(QuoteConstructionFault::WhereFragmentHasNoTemplate)?;
            if key.source != FrozenTemplateSource::Quote {
                return Err(QuoteConstructionFault::WhereFragmentIsNotAQuote.into());
            }
            let file = db
                .workspace()
                .get(db, key.owner.file())
                .ok_or(QuoteConstructionFault::WhereOwnerFileIsMissing)?;
            let base = base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
            let owner = locate_authored_function(db, base, &key.owner)?;
            let body = owner
                .body(db)
                .ok_or(QuoteConstructionFault::WhereOwnerBodyIsMissing)?;
            let Partial::Present(Expr::Quote(quote)) = &body.exprs(db)[key.expr] else {
                return Err(QuoteConstructionFault::WhereSiteChanged.into());
            };
            if quote.holes.len() != node.holes.len() {
                return Err(QuoteConstructionFault::WhereHolesChangedAfterFreezing.into());
            }
            let clause = match &quote.fragment {
                QuoteFragment::Where(clause) => Some(clause),
                QuoteFragment::Items(items) if index == root => match items.as_slice() {
                    [QuoteItem::Impl(item)] => Some(&item.where_clause),
                    _ => None,
                },
                _ => None,
            };
            if let Some(clause) = clause {
                if clause.predicates.len() != node.where_predicates.len() {
                    return Err(QuoteConstructionFault::WherePredicatesChangedAfterFreezing.into());
                }
                let Some(predicate_index) = predicate_index else {
                    work.extend(
                        (0..clause.predicates.len())
                            .rev()
                            .map(|predicate| (index, Some(predicate), edge.clone())),
                    );
                    return Ok(());
                };
                match (
                    &clause.predicates[predicate_index],
                    &node.where_predicates[predicate_index],
                ) {
                    (crate::hir_def::QuoteWherePredicate::Hole(slot), None) => {
                        let Some(FrozenCodeHole::Code(child, source)) =
                            node.holes.get(*slot as usize)
                        else {
                            return Err(QuoteConstructionFault::WhereHoleIsNotCode.into());
                        };
                        work.push((*child, None, source.clone().or_else(|| edge.clone())));
                    }
                    (
                        crate::hir_def::QuoteWherePredicate::Bound(source),
                        Some(FrozenQuoteWherePredicate::Bound { ty, trait_sites }),
                    ) => {
                        if source.bounds.len() != trait_sites.len() {
                            return Err(
                                QuoteConstructionFault::WhereBoundChangedAfterFreezing.into()
                            );
                        }
                        builder
                            .ctxt()
                            .quote_budget
                            .charge_many(1 + trait_sites.len())?;

                        let ty = quoted_signature_type_with_holes(
                            paths,
                            &type_budget,
                            ty,
                            names,
                            &[],
                            &mut |slot, depth| {
                                quoted_type_hole(builder, quote, node, slot, depth, target, names)
                            },
                        )?;
                        let predicates = source
                            .bounds
                            .iter()
                            .zip(trait_sites)
                            .map(|(bound, site)| {
                                quoted_trait_bound(
                                    paths,
                                    bound,
                                    *site,
                                    &node.rooted_traits,
                                    &builder.ctxt().quote_budget,
                                )
                            })
                            .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
                        bounds.push(WherePredicate {
                            ty: Partial::Present(ty),
                            bounds: predicates,
                        });
                    }
                    _ => {
                        return Err(
                            QuoteConstructionFault::WherePredicateNeedsCheckedReification.into(),
                        );
                    }
                }
            } else if let Some(slots) = quote.splice_only_body(db) {
                for slot in slots.into_iter().rev() {
                    let Some(FrozenCodeHole::Code(child, source)) = node.holes.get(slot as usize)
                    else {
                        return Err(QuoteConstructionFault::WhereCompositionHoleIsNotCode.into());
                    };
                    work.push((*child, None, source.clone().or_else(|| edge.clone())));
                }
            } else {
                return Err(QuoteConstructionFault::WhereFragmentHasTheWrongKind.into());
            }
            Ok(())
        })()
        .map_err(|error| error.at(edge))?;
    }
    Ok(bounds)
}

fn emit_closed_quoted_impl<'db>(
    emit: QuoteEmitCtx<'_, 'db>,
    ctxt: &mut FileLowerCtxt<'db>,
    origin: GenerationDesugared,
    item: &QuoteItem<'db>,
) -> Result<Vec<QuoteHygiene<'db>>, QuoteConstructionError> {
    let db = emit.db;
    let type_budget = ctxt.quote_budget.clone();
    let paths = QuotePathContext::new(db, ctxt.top_mod().ingot(db));
    let QuoteItem::Impl(quoted) = item else {
        return Err(QuoteConstructionFault::GenerationCurrentlyRequiresAFunctionOrTraitImpl.into());
    };
    ctxt.quote_budget.charge_many(quoted.items.len())?;
    if !quoted.generic_params.data(db).is_empty()
        || !quoted.attrs.data(db).is_empty()
        || quoted.selection_name.is_some()
    {
        return Err(QuoteConstructionFault::ImplHeaderNeedsCheckedTransplantation.into());
    }
    if quoted.items.iter().any(|item| {
        !matches!(
            item,
            QuoteItem::Const(_) | QuoteItem::Type(_) | QuoteItem::Func(_)
        )
    }) {
        return Err(QuoteConstructionFault::ImplMemberNeedsCheckedTransplantation.into());
    }
    let method_count = quoted
        .items
        .iter()
        .filter(|item| matches!(item, QuoteItem::Func(_)))
        .count();
    if emit.node.function_types.len() != method_count {
        return Err(QuoteConstructionFault::MethodSignaturesChangedAfterFreezing.into());
    }
    if method_count == 0
        && (emit
            .node
            .rooted_funcs
            .iter()
            .any(|root| root.member.assoc_const().is_none())
            || !emit.node.locals.is_empty()
            || !emit.node.param_uses.is_empty()
            || !emit.node.self_uses.is_empty())
    {
        return Err(QuoteConstructionFault::ImplMemberNamesNeedCheckedTransplantation.into());
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
    if emit.node.assoc_const_primitives.len() != constant_count
        || emit.node.assoc_type_values.len() != type_count
    {
        return Err(QuoteConstructionFault::AssociatedMemberTypesChangedAfterFreezing.into());
    }
    let Some(Partial::Present(trait_ref)) = quoted.trait_ref else {
        return Err(QuoteConstructionFault::ImplRequiresATrait.into());
    };
    let Partial::Present(authored_trait_path) = trait_ref.path(db) else {
        return Err(QuoteConstructionFault::ImplTraitPathIsMissing.into());
    };
    for index in 0..authored_trait_path.len(db).saturating_sub(1) {
        let segment = authored_trait_path
            .segment(db, index)
            .ok_or(QuoteConstructionFault::ImplTraitPathSegmentIsMissing)?;
        if segment.generic_args(db).is_given(db) {
            return Err(
                QuoteConstructionFault::ImplTraitArgumentsOnAnIntermediatePathNeedReification
                    .into(),
            );
        }
    }
    if authored_trait_path.generic_args(db).is_given(db)
        != emit.node.impl_trait_type_arguments.is_some()
        || emit
            .node
            .impl_trait_type_arguments
            .as_ref()
            .is_some_and(|args| args.len() != authored_trait_path.generic_args(db).data(db).len())
    {
        return Err(QuoteConstructionFault::ImplTraitArgumentsChangedAfterFreezing.into());
    }
    let mut roots = emit.node.rooted_traits.iter().filter(|root| root.site == 0);
    let root = roots
        .next()
        .ok_or(QuoteConstructionFault::ImplTraitHasNoCheckedRoot)?;
    if roots.next().is_some() {
        return Err(QuoteConstructionFault::ImplTraitRootIsAmbiguous.into());
    }
    let Partial::Present(self_ty) = quoted.ty else {
        return Err(QuoteConstructionFault::ImplSelfTypeIsMissing.into());
    };
    let TypeKind::Hole(slot) = self_ty.data(db) else {
        return Err(QuoteConstructionFault::ImplSelfTypeMustSpliceTheTargetRecord.into());
    };
    let (generic_params, generic_names) = quoted_record_generics(
        db,
        emit.target.owner,
        &emit.request.target_const_param_types,
        &ctxt.quote_budget,
    )?;
    let source = top_mod_ast(db, ctxt.top_mod());
    let mut builder = HirBuilder::new(ctxt, origin, source.syntax());
    type_budget.charge_emitted_type(&FrozenQuoteSignatureType::Hole(*slot), 0)?;

    let self_ty = emit.type_hole(&mut builder, *slot, 0, &generic_names)?;
    let mut path = bound_quote_trait_path(paths, root)?;

    if let Some(arguments) = &emit.node.impl_trait_type_arguments {
        builder
            .ctxt()
            .quote_budget
            .charge_many(2 + arguments.len())?;
        let arguments = arguments
            .iter()
            .map(|argument| emit.signature_type(&mut builder, argument, &generic_names, &[]))
            .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
        path = quote_type_arguments(db, path, arguments)
            .ok_or(QuoteConstructionFault::ImplTraitPathHasNoFinalItem)?;
    }
    let mut method_hygiene = Vec::new();
    builder.generated_impl_with_children(
        |builder| {
            let types = quoted
                .items
                .iter()
                .filter_map(|item| match item {
                    QuoteItem::Type(associated) => Some(associated),
                    _ => None,
                })
                .zip(&emit.node.assoc_type_values)
                .map(|(associated, frozen)| {
                    quoted_impl_type(db, builder, associated, frozen.as_ref())
                })
                .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
            let owner_where =
                builder.quoted_owner_where_clause(emit.target.owner, &generic_names)?;
            let mut predicates = owner_where.predicates(db).to_vec();
            predicates.extend(
                quoted_impl_where_bounds(
                    db,
                    builder,
                    emit.code,
                    emit.code_root,
                    emit.target,
                    &generic_names,
                )?
                .into_iter()
                .map(crate::hir_def::WhereClausePredicate::Type),
            );
            let spec = GeneratedImplSpec {
                trait_ref: Partial::Present(TraitRefId::new(db, Partial::Present(path))),
                ty: Partial::Present(self_ty),
                selection_name: None,
                attrs: builder.empty_attrs(),
                generic_params,
                where_clause: WhereClauseId::new(db, predicates),
                types,
                consts: vec![],
            };
            Ok::<_, QuoteConstructionError>((spec, ()))
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
                    emit,
                    builder,
                    method,
                    u32::try_from(index)
                        .map_err(|_| QuoteConstructionFault::MethodCountExceedsU32)?,
                    method_params,
                    &generic_names,
                    true,
                )?);
            }
            quoted
                .items
                .iter()
                .filter_map(|item| match item {
                    QuoteItem::Const(constant) => Some(constant),
                    _ => None,
                })
                .zip(&emit.node.assoc_const_primitives)
                .enumerate()
                .map(|(index, (constant, primitive))| {
                    quoted_impl_const(
                        emit,
                        builder,
                        constant,
                        u32::try_from(index).map_err(|_| {
                            QuoteConstructionFault::AssociatedConstantCountExceedsU32
                        })?,
                        *primitive,
                        &generic_names,
                    )
                })
                .collect::<Result<Vec<_>, QuoteConstructionError>>()
        },
    )?;
    Ok(method_hygiene)
}

fn quoted_impl_type<'db>(
    db: &'db dyn HirDb,
    builder: &mut HirBuilder<'_, 'db, GenerationDesugared>,
    associated: &QuoteAssocType<'db>,
    frozen: Option<&FrozenQuoteSignatureType>,
) -> Result<crate::hir_def::AssocTyDef<'db>, QuoteConstructionError> {
    let type_budget = builder.ctxt().quote_budget.clone();
    let paths = QuotePathContext::new(db, builder.top_mod().ingot(db));
    let Partial::Present(name) = associated.name else {
        return Err(QuoteConstructionFault::AssociatedTypeNeedsALiteralName.into());
    };
    if associated.ty.is_none() {
        return Err(QuoteConstructionFault::AssociatedTypeValueIsMissing.into());
    }
    let frozen = frozen.ok_or(QuoteConstructionFault::AssociatedTypeNeedsCheckedReification)?;
    let ty = quoted_signature_type(paths, &type_budget, frozen, &[])?;
    Ok(builder.assoc_ty(name.data(db), Partial::Present(ty)))
}

fn quoted_impl_const<'db>(
    emit: QuoteEmitCtx<'_, 'db>,
    builder: &mut HirBuilder<'_, 'db, GenerationDesugared>,
    constant: &QuoteConst<'db>,
    const_index: u32,
    primitive: Option<SemPrimTy>,
    generic_names: &[IdentId<'db>],
) -> Result<AssocConstDef<'db>, QuoteConstructionError> {
    let db = emit.db;
    let paths = QuotePathContext::new(db, builder.top_mod().ingot(db));
    let name = quoted_name(constant.name, emit.node, emit.target)?;
    if !constant.attrs.data(db).is_empty() || constant.vis != Visibility::Private {
        return Err(QuoteConstructionFault::AssociatedConstantHeaderNeedsRewriting.into());
    }
    let Partial::Present(_) = constant.ty else {
        return Err(QuoteConstructionFault::AssociatedConstantTypeIsMissing.into());
    };
    let ty = primitive
        .and_then(quote_hir_primitive)
        .map(|primitive| TypeId::new(db, TypeKind::Primitive(primitive)))
        .ok_or(QuoteConstructionFault::AssociatedConstantIsNotACheckedPrimitiveType)?;
    let root = constant
        .value
        .ok_or(QuoteConstructionFault::AssociatedConstantValueIsMissing)?;
    let roots = &emit
        .member_groups
        .get(QuoteMember::AssocConst(const_index))
        .rooted_funcs;
    let rooted_paths = roots
        .iter()
        .map(|root| {
            let path = quoted_function_path(emit, paths, builder, root, generic_names, &[])?;
            Ok((root.site, path))
        })
        .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
    let mut copied = None;
    let value =
        builder.generated_body(TrackedItemVariant::Const(Partial::Present(name)), |body| {
            copied = Some(body.adopt_closed_quote_body(
                emit.quote.template,
                root,
                emit.provider_scope,
                QuoteBodyBindings {
                    holes: &emit.node.holes,
                    cast_types: emit.cast_types,
                    fragments: Some((emit.code, emit.fragment_sources)),
                    rooted_funcs: &rooted_paths,
                    ..Default::default()
                },
            )?);
            Ok::<_, QuoteConstructionError>(())
        })?;
    copied.ok_or(QuoteConstructionFault::AssociatedConstantBodyWasNotCopied)?;
    let definition = AssocConstDef {
        attributes: builder.empty_attrs(),
        name: Partial::Present(name),
        ty: Partial::Present(ty),
        value: Partial::Present(value),
        body_check_policy: Default::default(),
        vis: Visibility::Private,
    };
    Ok(definition)
}

fn locate_quote_function<'db>(
    db: &'db dyn HirDb,
    root: &FrozenQuoteFunction,
) -> Result<Func<'db>, QuoteConstructionFault> {
    let file = db
        .workspace()
        .get(db, root.owner.file())
        .ok_or(QuoteConstructionFault::RootFileIsMissing)?;
    let base = base_scope_graph_impl(db, super::map_file_to_mod_impl(db, file));
    locate_authored_function(db, base, &root.owner)
}

fn bound_quote_trait_path<'db>(
    paths: QuotePathContext<'db>,
    root: &FrozenQuoteTrait,
) -> Result<PathId<'db>, QuoteConstructionFault> {
    let declaration = locate_authored_item(paths.db, &root.owner, |item| match item {
        ItemKind::Trait(trait_) => Some(trait_),
        _ => None,
    })
    .map_err(|error| error.fault(QuoteConstructionFault::TraitIsAbsentFromAuthoredHIR))?;
    bound_quote_declaration_path(paths, declaration.scope())
}

fn bound_quote_qualified_const_path<'db>(
    paths: QuotePathContext<'db>,
    template: Body<'db>,
    node: &FrozenCodeNode,
    qualified: &FrozenQuoteQualifiedConst,
) -> Result<PathId<'db>, QuoteConstructionFault> {
    let db = paths.db;
    let trait_root = node
        .rooted_traits
        .iter()
        .find(|root| root.site == qualified.trait_site)
        .ok_or(QuoteConstructionFault::QualifiedConstantLostItsCheckedTrait)?;
    let Partial::Present(Expr::Path(Partial::Present(source))) =
        &template.exprs(db)[qualified.site]
    else {
        return Err(QuoteConstructionFault::QualifiedConstantPathDisappeared);
    };
    let name = source
        .ident(db)
        .to_opt()
        .ok_or(QuoteConstructionFault::QualifiedConstantMemberIsMissing)?;
    let receiver = quote_hir_primitive(qualified.receiver)
        .map(|primitive| TypeId::new(db, TypeKind::Primitive(primitive)))
        .ok_or(QuoteConstructionFault::QualifiedConstantReceiverIsUnsupported)?;
    let trait_path = bound_quote_trait_path(paths, trait_root)?;
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
    Ok(path)
}

fn bound_quote_function_path<'db>(
    paths: QuotePathContext<'db>,
    root: &FrozenQuoteFunction,
) -> Result<PathId<'db>, QuoteConstructionFault> {
    let declaration = locate_quote_function(paths.db, root)?;
    bound_quote_declaration_path(paths, declaration.scope())
}

fn quoted_function_path<'db>(
    emit: QuoteEmitCtx<'_, 'db>,
    paths: QuotePathContext<'db>,
    builder: &mut HirBuilder<'_, 'db, GenerationDesugared>,
    root: &FrozenQuoteFunction,
    owner_generic_names: &[IdentId<'db>],
    function_generic_names: &[IdentId<'db>],
) -> Result<PathId<'db>, QuoteConstructionError> {
    let db = paths.db;
    let mut path = bound_quote_function_path(paths, root)?;
    if let Some(arguments) = &root.type_arguments {
        let arguments = arguments
            .iter()
            .map(|argument| {
                emit.signature_type(
                    builder,
                    argument,
                    owner_generic_names,
                    function_generic_names,
                )
            })
            .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
        path = quote_type_arguments(db, path, arguments)
            .ok_or(QuoteConstructionFault::FunctionPathHasNoFinalItem)?;
    }
    Ok(path)
}

fn quote_type_arguments<'db>(
    db: &'db dyn HirDb,
    path: PathId<'db>,
    arguments: Vec<TypeId<'db>>,
) -> Option<PathId<'db>> {
    let arguments = arguments
        .into_iter()
        .map(|ty| {
            GenericArg::Type(TypeGenericArg {
                ty: Partial::Present(ty),
            })
        })
        .collect();
    path.with_generic_args(db, GenericArgListId::given(db, arguments))
}

fn bound_quote_nominal_path<'db>(
    paths: QuotePathContext<'db>,
    root: &FrozenQuoteNominal,
) -> Result<PathId<'db>, QuoteConstructionFault> {
    let declaration = locate_authored_item(
        paths.db,
        &root.owner,
        crate::analysis::ty::adt_def::AdtRef::try_from_item,
    )
    .map_err(|error| {
        error.fault(QuoteConstructionFault::ConstructorOwnerIsAbsentFromAuthoredHIR)
    })?;
    bound_quote_declaration_path(paths, declaration.scope())
}

fn bound_quote_declaration_path<'db>(
    paths: QuotePathContext<'db>,
    item: ScopeId<'db>,
) -> Result<PathId<'db>, QuoteConstructionFault> {
    let db = paths.db;
    let ingot = item.ingot(db);
    let graph = base_scope_graph_impl(db, item.top_mod(db));
    let full = authored_path(db, graph, item).ok_or(QuoteConstructionFault::ItemPathHasNoItem)?;
    let root = ingot
        .root_mod(db)
        .scope()
        .name(db)
        .ok_or(QuoteConstructionFault::ItemPathHasNoItem)?;
    let relative = full
        .strip_prefix(root.data(db))
        .and_then(|rest| rest.strip_prefix("::"))
        .ok_or(QuoteConstructionFault::ItemPathIsNotASimpleItemPath)?;
    let display = bound_quote_item_path(paths, ingot, relative)?;
    Ok(PathId::new(
        paths.db,
        PathKind::BoundItem { item, display },
        None,
    ))
}

fn bound_quote_item_path<'db>(
    paths: QuotePathContext<'db>,
    ingot: common::ingot::Ingot<'db>,
    item_path: &str,
) -> Result<PathId<'db>, QuoteConstructionFault> {
    let db = paths.db;
    let ingot_url = ingot.base(db);
    let display = if ingot == paths.destination {
        "ingot".to_owned()
    } else if ingot_url.as_str() == common::stdlib::BUILTIN_CORE_BASE_URL {
        "core".to_owned()
    } else if ingot_url.as_str() == common::stdlib::BUILTIN_STD_BASE_URL {
        "std".to_owned()
    } else {
        paths
            .destination
            .dependencies(db)
            .into_iter()
            .filter(|(_, url)| *url == ingot_url)
            .map(|(alias, _)| alias.to_string())
            .min()
            .unwrap_or_else(|| "ingot".to_owned())
    };
    let mut path = PathId::new(
        db,
        PathKind::BoundIngot {
            ingot,
            display: IdentId::new(db, display),
        },
        None,
    );
    let mut count = 0;
    for segment in item_path.split("::") {
        if !identifier(segment) {
            return Err(QuoteConstructionFault::ItemPathIsNotASimpleItemPath);
        }
        path = path.push_str(db, segment);
        count += 1;
    }
    if count == 0 {
        return Err(QuoteConstructionFault::ItemPathHasNoItem);
    }
    Ok(path)
}

fn text(value: &FrozenMetaValue) -> Result<String, QuoteConstructionFault> {
    let FrozenMetaValue::Scalar {
        value: SemConstScalar::Bytes(bytes),
        ..
    } = value
    else {
        return Err(QuoteConstructionFault::ExpectedFixedStringData);
    };
    let start = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[start..])
        .map(str::to_owned)
        .map_err(|_| QuoteConstructionFault::GenerationStringIsNotUTF8)
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
    ) -> Result<(Option<u16>, usize), QuoteConstructionFault> {
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
            _ => return Err(QuoteConstructionFault::ExpectedAnAuthorizedFieldHandle),
        };
        if self.field_hir_type(coordinate.0, coordinate.1).is_none()
            || coordinate.1 > usize::from(u16::MAX)
        {
            return Err(
                QuoteConstructionFault::FieldHandleHasAnInvalidOwnerVariantOrFieldCoordinate,
            );
        }
        Ok(coordinate)
    }

    fn field_index(
        &self,
        value: &FrozenMetaValue,
        type_handle: bool,
    ) -> Result<usize, QuoteConstructionFault> {
        let (variant_index, index) = self.field_coordinate(value, type_handle)?;
        if variant_index.is_some() {
            return Err(QuoteConstructionFault::VariantFieldAccessRequiresAMatchingPatternBinder);
        }
        Ok(index)
    }

    fn field_name(&self, index: usize) -> Result<IdentId<'db>, QuoteConstructionFault> {
        let crate::analysis::ty::adt_def::AdtRef::Struct(owner) = self.owner else {
            return Err(QuoteConstructionFault::FieldNameRequiresASelectedFieldList);
        };
        owner.hir_fields(self.db).data(self.db)[index]
            .name
            .to_opt()
            .ok_or(QuoteConstructionFault::GenerationFieldNameIsMissing)
    }

    fn name(&self, value: &FrozenMetaValue) -> Result<IdentId<'db>, QuoteConstructionFault> {
        let FrozenMetaValue::DeclarationName { case, fields, .. } = value else {
            return Err(QuoteConstructionFault::InvalidGeneratedDeclarationName);
        };
        let name = match (*case, fields.as_slice()) {
            (FrozenMetaVariantCase::DeclarationNameLiteral, [name]) => text(name)?,
            (FrozenMetaVariantCase::DeclarationNameFieldPrefix, [prefix, field]) => {
                let index = self.field_index(field, false)?;
                format!("{}{}", text(prefix)?, self.field_name(index)?.data(self.db))
            }
            _ => return Err(QuoteConstructionFault::InvalidGeneratedDeclarationName),
        };
        if name.len() > 256 || !identifier(&name) {
            return Err(QuoteConstructionFault::GeneratedNameMustBeAnIdentifier);
        }
        Ok(IdentId::new(self.db, name))
    }
}

/// Build one checked quoted request in its own expansion context.
pub(crate) fn request_fragment<'db>(
    db: &'db dyn HirDb,
    top_mod: TopLevelMod<'db>,
    request: &FrozenGeneration,
) -> Result<(ScopeGraph<'db>, Vec<QuoteHygiene<'db>>), QuoteConstructionError> {
    let base = base_scope_graph_impl(db, top_mod);
    let root = top_mod_ast(db, top_mod).syntax().clone();
    let owner = locate_authored_item(
        db,
        &request.target,
        crate::analysis::ty::adt_def::AdtRef::try_from_item,
    )
    .map_err(|error| error.fault(QuoteConstructionFault::GeneratedTargetIsAbsentFromAuthoredHIR))?;
    let attribute = (|| {
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
            crate::analysis::ty::adt_def::AdtRef::Struct(_) => ast::Struct::cast(node)?.attr_list(),
            crate::analysis::ty::adt_def::AdtRef::Enum(_) => ast::Enum::cast(node)?.attr_list(),
        }?;
        attrs.into_iter().nth(request.attribute_index)
    })()
    .ok_or(QuoteConstructionFault::GeneratedTargetIsAbsentFromAuthoredHIR)?;
    let target = (owner, attribute);
    let parent = base
        .edges(target.0.scope())
        .iter()
        .find_map(|edge| matches!(edge.kind, EdgeKind::Lex(_)).then_some(edge.dest))
        .ok_or(QuoteConstructionFault::GeneratedTargetHasNoLexicalParent)?;
    let origin = GenerationDesugared {
        attribute: parser::ast::AstPtr::new(&target.1),
        source: None,
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
        // File modules have parents in the ingot module tree, not in this
        // file's authored scope graph. Reading that tree avoids asking for a
        // generated scope graph while it is being constructed.
        let parent = match scope.to_item() {
            Some(ItemKind::TopMod(module)) => module.parent(db).map(|parent| parent.scope()),
            _ => authored_parent(graph, scope),
        };
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
    pub failure: GeneratedQuoteFailure,
    pub site_span: Option<std::ops::Range<usize>>,
    pub quote_source: Option<crate::core::meta::FrozenQuoteSpan>,
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
                    quote_source: error.source.map(|source| *source),
                    failure: error.failure,
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

fn push_hygiene_fault(
    errors: &mut Vec<GeneratedHirError>,
    fault: QuoteHygieneFault,
    site_span: &Option<std::ops::Range<usize>>,
) {
    errors.push(GeneratedHirError {
        failure: fault.into(),
        quote_source: None,
        site_span: site_span.clone(),
    });
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
            variants,
            const_predicates,
            locals,
            bindings,
            effects,
            generics,
            method_calls,
            site_span,
        } = quote;
        let Some(body) = func.body(db) else {
            push_hygiene_fault(
                &mut errors,
                QuoteHygieneFault::FunctionBodyDisappearedBeforeHygieneChecking,
                site_span,
            );
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
                            } => Some(QuoteConstructorIdentity::from_variant(*variant)),
                            crate::analysis::ty::pattern_ir::ValidatedPatKind::Constructor {
                                ctor: crate::analysis::ty::pattern_ir::ConstructorKind::Type(ty),
                                ..
                            } => QuoteConstructorIdentity::from_ty(db, *ty),
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
                            Ok(PathRes::EnumVariant(variant)) => {
                                Some(QuoteConstructorIdentity::from_variant(variant.variant))
                            }
                            Ok(PathRes::Ty(ty)) => QuoteConstructorIdentity::from_ty(db, ty),
                            _ => None,
                        }
                    })
                }
            };
            if actual != Some(*expected) {
                push_hygiene_fault(
                    &mut errors,
                    QuoteHygieneFault::ConstructorChangedOwnerOrVariantIndex,
                    site_span,
                );
            }
        }
        for call in method_calls.iter().copied() {
            let Partial::Present(Expr::MethodCall(..)) = &body.exprs(db)[call.site] else {
                push_hygiene_fault(
                    &mut errors,
                    QuoteHygieneFault::MethodCallDisappearedBeforeHygieneChecking,
                    site_span,
                );
                continue;
            };
            // Ordinary checking owns receiver adjustment and method selection.
            // Verify the retained lookup scope and consume its checked call record.
            let actual = typed
                .callable_expr(call.site)
                .map(|callable| callable.callable_def());
            let scopes = body
                .quote_scopes(db)
                .iter()
                .filter(|(site, _)| *site == call.site.into())
                .map(|(_, scope)| scope.scope)
                .collect::<Vec<_>>();
            if scopes.as_slice() != [call.scope]
                || actual.is_none()
                || call
                    .selected
                    .is_some_and(|selected| actual != Some(CallableDef::Func(selected)))
            {
                push_hygiene_fault(
                    &mut errors,
                    QuoteHygieneFault::MethodCallChangedSelectionInOutput,
                    site_span,
                );
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
                push_hygiene_fault(
                    &mut errors,
                    QuoteHygieneFault::LocalBinderChangedMeaningInOutput,
                    site_span,
                );
            }
        }
        for (site, binding) in bindings {
            use super::hir_builder::QuoteDestinationBinding;
            let valid = match binding {
                QuoteDestinationBinding::Param(index) => matches!(typed.expr_binding(*site),
                    Some(LocalBinding::Param { site: ParamSite::Func(owner), idx, .. })
                        if owner == *func && idx == *index),
                QuoteDestinationBinding::Local(expected) => matches!(typed.expr_binding(*site),
                    Some(LocalBinding::Local { pat, .. }) if pat == *expected),
            };
            if !valid {
                push_hygiene_fault(
                    &mut errors,
                    QuoteHygieneFault::ParameterChangedMeaningInOutput,
                    site_span,
                );
            }
        }
        for (site, index) in effects {
            let valid = matches!(typed.expr_binding(*site),
                Some(LocalBinding::EffectParam { site: EffectParamSite::Func(owner), idx, .. })
                    if owner == *func && idx == *index);
            if !valid {
                push_hygiene_fault(
                    &mut errors,
                    QuoteHygieneFault::EffectBindingChangedMeaningInOutput,
                    site_span,
                );
            }
        }
        for (site, expected) in generics {
            let valid = matches!(typed.value_path_ref(*site),
                Some(ValuePathRef::TypeConst(ty))
                    if matches!(ty.data(db), TyData::ConstTy(const_ty)
                        if matches!(const_ty.data(db), ConstTyData::TyParam(param, _)
                            if param.owner == func.scope() && param.name == *expected)));
            if !valid {
                push_hygiene_fault(
                    &mut errors,
                    QuoteHygieneFault::GenericBodyUseChangedMeaningInOutput,
                    site_span,
                );
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
            push_hygiene_fault(
                &mut errors,
                QuoteHygieneFault::ConstWhereBodyChangedInOutput,
                site_span,
            );
            continue;
        }
        for predicate in const_predicates {
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
                    push_hygiene_fault(
                        &mut errors,
                        QuoteHygieneFault::ConstWhereGenericChangedMeaningInOutput,
                        site_span,
                    );
                }
            }
        }
    }
    errors
}
