//! Freeze checked Fe provider values before generated HIR is published.
//! Descriptor identity and owner authority are checked in the authored view.

use std::{borrow::Cow, cell::Cell, collections::HashMap};

use hir::{
    analysis::name_resolution::{PathRes, resolve_path},
    analysis::{
        semantic::{QuotedCode, SemConstId, SemConstScalar, SemConstValue, StructuralHandle},
        ty::{
            adt_def::AdtRef,
            trait_resolution::PredicateListId,
            ty_check::{
                QuoteBinder, QuoteNameClass, QuoteNameSite, QuoteNameUse, QuoteSignatureSite,
                check_func_body,
            },
            ty_def::{PrimTy as SemPrimTy, TyBase, TyData, TyId},
            ty_lower::{lower_hir_primitive, lower_hir_ty},
        },
    },
    core::meta::{
        FrozenCode, FrozenCodeHole, FrozenCodeNode, FrozenHandleKind, FrozenItemLocator,
        FrozenMetaValue, FrozenMetaVariantCase, FrozenOwner, FrozenOwnerError,
        FrozenQuoteConstructor, FrozenQuoteConstructorSite, FrozenQuoteEffectUse,
        FrozenQuoteFunction, FrozenQuoteFunctionTypes, FrozenQuoteGenericUse, FrozenQuoteLocal,
        FrozenQuoteLocalType, FrozenQuoteNominal, FrozenQuoteOpenUse, FrozenQuoteParamUse,
        FrozenQuoteQualifiedConst, FrozenQuoteSelfUse, FrozenQuoteSignatureType, FrozenQuoteTrait,
        FrozenQuoteWherePredicate, FrozenTemplateKey, FrozenTemplateSource, MetaItem,
    },
    hir_def::{
        CallableDef, Expr, Func, GenericArg, GenericParam, Partial, PathKind, QuoteFragment,
        QuoteFunc, QuoteItem, QuoteWherePredicate, Trait, TypeBound, TypeId, TypeKind,
        scope_graph::ScopeId,
    },
    span::LazySpan,
};

use super::{
    RecordInput,
    errors::{FreezingLimitKind, FreezingProtocolFault},
};
use crate::DriverDataBase;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstructionError {
    Protocol(FreezingProtocolFault),
    ReadBackMissingBody,
    ReadBackInvalidBody,
    ReadBackEffectful,
    ReadBackMissingOwner,
    ReadBackUnsupportedCall,
    ReadBackUnsupportedName,
    ReadBackUnsupportedType,
    Authority,
    MissingItemPath,
    UnsupportedConstructorInstantiation,
    SelfConstructor {
        name: String,
    },
    Limit {
        name: FreezingLimitKind,
        actual: usize,
        maximum: usize,
    },
}

impl From<FreezingProtocolFault> for ConstructionError {
    fn from(error: FreezingProtocolFault) -> Self {
        Self::Protocol(error)
    }
}

const MAX_FROZEN_NODES: usize = 65_536;

pub(super) fn charge_frozen_node(nodes: &Cell<usize>) -> Result<(), ConstructionError> {
    charge_frozen_nodes(nodes, 1)
}

fn charge_frozen_nodes(nodes: &Cell<usize>, amount: usize) -> Result<(), ConstructionError> {
    let used = nodes.get().saturating_add(amount);
    if used > MAX_FROZEN_NODES {
        return Err(limit(
            FreezingLimitKind::MetaNodes,
            MAX_FROZEN_NODES + 1,
            MAX_FROZEN_NODES,
        ));
    }
    nodes.set(used);
    Ok(())
}

/// Charge the stored code-node shell where it is allocated, rather than
/// traversing the completed graph to estimate its description size.
pub(super) fn charge_frozen_code_node(nodes: &Cell<usize>) -> Result<(), ConstructionError> {
    charge_frozen_nodes(nodes, 16)
}

pub(super) fn frozen_quote_trait<'db>(
    db: &'db DriverDataBase,
    trait_: Trait<'db>,
    site: u32,
) -> Result<FrozenQuoteTrait, ConstructionError> {
    let owner = FrozenOwner::of_item(db, trait_.into()).map_err(|error| match error {
        FrozenOwnerError::MissingFile => FreezingProtocolFault::TraitFileIsMissing.into(),
        FrozenOwnerError::MissingPath => ConstructionError::MissingItemPath,
    })?;
    Ok(FrozenQuoteTrait { site, owner })
}

pub(super) fn frozen_quote_nominal_ref<'db>(
    db: &'db DriverDataBase,
    adt: AdtRef<'db>,
) -> Result<FrozenQuoteNominal, ConstructionError> {
    let owner = FrozenOwner::of_item(db, adt.as_item()).map_err(|error| match error {
        FrozenOwnerError::MissingFile => FreezingProtocolFault::NominalTypeFileIsMissing.into(),
        FrozenOwnerError::MissingPath => ConstructionError::MissingItemPath,
    })?;
    Ok(FrozenQuoteNominal { owner })
}

/// Freeze a checked type without resolving its spelling again. Normal generic
/// parameters retain their declaration namespace and index, not their name.
pub(super) fn freeze_checked_quote_type<'db>(
    db: &'db DriverDataBase,
    ty: TyId<'db>,
    func: Func<'db>,
) -> Result<Option<FrozenQuoteSignatureType>, ConstructionError> {
    freeze_checked_quote_type_with(db, ty, &|parameter| {
        let TyData::TyParam(param) = parameter.data(db) else {
            return None;
        };
        let ScopeId::GenericParam(owner, index) = param.declaration_scope(db)? else {
            return None;
        };
        if owner == func.into() {
            Some(Cow::Owned(FrozenQuoteSignatureType::FunctionGeneric(
                u32::from(index),
            )))
        } else if func.is_associated_func(db) && func.scope().parent_item(db) == Some(owner) {
            Some(Cow::Owned(FrozenQuoteSignatureType::OwnerGeneric(
                u32::from(index),
            )))
        } else {
            None
        }
    })
}

/// Share checked type structure while rebinding only declaration parameters.
fn freeze_checked_quote_type_with<'db, 'a>(
    db: &'db DriverDataBase,
    ty: TyId<'db>,
    parameter: &impl Fn(TyId<'db>) -> Option<Cow<'a, FrozenQuoteSignatureType>>,
) -> Result<Option<FrozenQuoteSignatureType>, ConstructionError> {
    freeze_checked_quote_type_bounded(db, ty, parameter, &mut 0, 0)
}

fn freeze_checked_quote_type_bounded<'db, 'a>(
    db: &'db DriverDataBase,
    ty: TyId<'db>,
    parameter: &impl Fn(TyId<'db>) -> Option<Cow<'a, FrozenQuoteSignatureType>>,
    nodes: &mut usize,
    depth: usize,
) -> Result<Option<FrozenQuoteSignatureType>, ConstructionError> {
    charge_checked_type(nodes, depth)?;
    let (base, args) = ty.decompose_ty_app(db);
    let mut arguments = Vec::new();
    for arg in args {
        let Some(arg) = freeze_checked_quote_type_bounded(db, *arg, parameter, nodes, depth + 1)?
        else {
            return Ok(None);
        };
        arguments.push(arg);
    }
    let frozen = match base.data(db) {
        TyData::TyParam(_) if arguments.is_empty() => {
            let substituted = parameter(base);
            if let Some(substituted) = &substituted {
                // Check borrowed substitutions before cloning them. The root
                // replaces the parameter already charged.
                let mut pending = substituted
                    .children()
                    .iter()
                    .map(|ty| (ty, depth + 1))
                    .collect::<Vec<_>>();
                while let Some((ty, depth)) = pending.pop() {
                    charge_checked_type(nodes, depth)?;
                    pending.extend(ty.children().iter().map(|ty| (ty, depth + 1)));
                }
            }
            return Ok(substituted.map(Cow::into_owned));
        }
        TyData::Never if arguments.is_empty() => FrozenQuoteSignatureType::Never,
        TyData::TyBase(TyBase::Prim(primitive))
            if arguments.is_empty()
                && (primitive.is_integral() || *primitive == SemPrimTy::Bool) =>
        {
            FrozenQuoteSignatureType::Primitive(*primitive)
        }
        TyData::TyBase(TyBase::Prim(SemPrimTy::Tuple(size))) if arguments.len() == *size => {
            FrozenQuoteSignatureType::Tuple(arguments)
        }
        TyData::TyBase(TyBase::Prim(SemPrimTy::Ptr)) if arguments.len() == 1 => {
            FrozenQuoteSignatureType::Ptr(Box::new(arguments.remove(0)))
        }
        TyData::TyBase(TyBase::Prim(primitive @ (SemPrimTy::BorrowMut | SemPrimTy::BorrowRef)))
            if arguments.len() == 1 =>
        {
            let mode = if *primitive == SemPrimTy::BorrowMut {
                hir::hir_def::TypeMode::Mut
            } else {
                hir::hir_def::TypeMode::Ref
            };
            FrozenQuoteSignatureType::Mode(mode, Box::new(arguments.remove(0)))
        }
        TyData::TyBase(TyBase::Adt(adt)) => FrozenQuoteSignatureType::Nominal {
            root: frozen_quote_nominal_ref(db, adt.adt_ref(db))?,
            arguments_given: !arguments.is_empty(),
            arguments,
        },
        _ => return Ok(None),
    };
    Ok(Some(frozen))
}

fn charge_checked_type(nodes: &mut usize, depth: usize) -> Result<(), ConstructionError> {
    const MAX_CHECKED_TYPE_DEPTH: usize = 256;
    if depth > MAX_CHECKED_TYPE_DEPTH {
        return Err(limit(
            FreezingLimitKind::TypeDepth,
            depth,
            MAX_CHECKED_TYPE_DEPTH,
        ));
    }
    *nodes = nodes.saturating_add(1);
    if *nodes > MAX_FROZEN_NODES {
        return Err(limit(
            FreezingLimitKind::TypeNodes,
            *nodes,
            MAX_FROZEN_NODES,
        ));
    }
    Ok(())
}

/// Reuse the checked type freezer while keeping each constructor owner only once.
pub(super) fn freeze_quote_constructor_arguments<'db>(
    db: &'db DriverDataBase,
    ty: TyId<'db>,
    func: Func<'db>,
) -> Result<Option<Vec<FrozenQuoteSignatureType>>, ConstructionError> {
    let Some(FrozenQuoteSignatureType::Nominal { arguments, .. }) =
        freeze_checked_quote_type(db, ty, func)?
    else {
        return Ok(None);
    };
    Ok(Some(arguments))
}

fn freeze_quote_constructor_arguments_budgeted<'db>(
    db: &'db DriverDataBase,
    ty: TyId<'db>,
    func: Func<'db>,
    budget: &Cell<usize>,
) -> Result<Option<Vec<FrozenQuoteSignatureType>>, ConstructionError> {
    let arguments = freeze_quote_constructor_arguments(db, ty, func)?;
    if let Some(arguments) = &arguments {
        for argument in arguments {
            charge_frozen_type(argument, budget)?;
        }
    }
    Ok(arguments)
}

pub(super) fn charge_frozen_type(
    ty: &FrozenQuoteSignatureType,
    budget: &Cell<usize>,
) -> Result<(), ConstructionError> {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        charge_frozen_node(budget)?;
        pending.extend(ty.children());
    }
    Ok(())
}

fn quoted_functions<'a, 'db>(
    fragment: &'a QuoteFragment<'db>,
) -> impl Iterator<Item = &'a QuoteFunc<'db>> {
    let items = match fragment {
        QuoteFragment::Items(items) => match items.as_slice() {
            [QuoteItem::Impl(implementation)] => implementation.items.as_slice(),
            _ => items.as_slice(),
        },
        _ => &[],
    };
    items.iter().filter_map(|item| match item {
        QuoteItem::Func(function) => Some(function),
        _ => None,
    })
}

fn freeze_quote_signature_type<'a, 'db>(
    db: &'db DriverDataBase,
    ty: TypeId<'db>,
    names: &mut impl Iterator<Item = &'a QuoteNameUse<'db>>,
    generics: &[GenericParam<'db>],
    generic_item: u32,
    checked_primitive: &impl Fn(TypeId<'db>) -> Option<SemPrimTy>,
    budget: &Cell<usize>,
) -> Result<Option<FrozenQuoteSignatureType>, ConstructionError>
where
    'db: 'a,
{
    let frozen = freeze_quote_signature_type_bounded(
        db,
        ty,
        names,
        QuoteSignatureGenerics {
            parameters: generics,
            item: generic_item,
        },
        checked_primitive,
        &mut 0,
        0,
    )?;
    if let Some(ty) = &frozen {
        charge_frozen_type(ty, budget)?;
    }
    Ok(frozen)
}

#[derive(Clone, Copy)]
struct QuoteSignatureGenerics<'g, 'db> {
    parameters: &'g [GenericParam<'db>],
    item: u32,
}

fn freeze_quote_signature_type_bounded<'a, 'db>(
    db: &'db DriverDataBase,
    ty: TypeId<'db>,
    names: &mut impl Iterator<Item = &'a QuoteNameUse<'db>>,
    owner: QuoteSignatureGenerics<'_, 'db>,
    checked_primitive: &impl Fn(TypeId<'db>) -> Option<SemPrimTy>,
    nodes: &mut usize,
    depth: usize,
) -> Result<Option<FrozenQuoteSignatureType>, ConstructionError>
where
    'db: 'a,
{
    charge_checked_type(nodes, depth)?;
    let QuoteSignatureGenerics {
        parameters: generics,
        item: generic_item,
    } = owner;
    match ty.data(db) {
        TypeKind::Mode(mode, Partial::Present(inner)) => Ok(freeze_quote_signature_type_bounded(
            db,
            *inner,
            names,
            owner,
            checked_primitive,
            nodes,
            depth + 1,
        )?
        .map(|inner| FrozenQuoteSignatureType::Mode(*mode, Box::new(inner)))),
        TypeKind::Ptr(Partial::Present(inner)) => Ok(freeze_quote_signature_type_bounded(
            db,
            *inner,
            names,
            owner,
            checked_primitive,
            nodes,
            depth + 1,
        )?
        .map(|inner| FrozenQuoteSignatureType::Ptr(Box::new(inner)))),
        TypeKind::Tuple(elements) => {
            let mut frozen = Vec::new();
            for element in elements.data(db) {
                let Some(element) = element.to_opt() else {
                    return Ok(None);
                };
                let Some(element) = freeze_quote_signature_type_bounded(
                    db,
                    element,
                    names,
                    owner,
                    checked_primitive,
                    nodes,
                    depth + 1,
                )?
                else {
                    return Ok(None);
                };
                frozen.push(element);
            }
            Ok(Some(FrozenQuoteSignatureType::Tuple(frozen)))
        }
        TypeKind::Path(Partial::Present(path)) => {
            let arguments_given = path.generic_args(db).is_given(db);
            let mut arguments = Vec::new();
            for index in 0..path.len(db) {
                let segment = path
                    .segment(db, index)
                    .ok_or(FreezingProtocolFault::SignaturePathSegmentIsMissing)?;
                let args = segment.generic_args(db);
                if index + 1 != path.len(db) && args.is_given(db) {
                    return Ok(None);
                }
                for arg in args.data(db) {
                    let GenericArg::Type(arg) = arg else {
                        return Ok(None);
                    };
                    let Some(ty) = arg.ty.to_opt() else {
                        return Ok(None);
                    };
                    let Some(frozen) = freeze_quote_signature_type_bounded(
                        db,
                        ty,
                        names,
                        owner,
                        checked_primitive,
                        nodes,
                        depth + 1,
                    )?
                    else {
                        return Ok(None);
                    };
                    arguments.push(frozen);
                }
            }
            let name_use = names
                .next()
                .ok_or(FreezingProtocolFault::SignatureTypeHasNoCheckedName)?;
            if !matches!(name_use.site, QuoteNameSite::Type(_)) {
                return Err(FreezingProtocolFault::SignatureNameChangedSite.into());
            }
            if matches!(name_use.class, QuoteNameClass::SelfName)
                && path.as_ident(db).is_some_and(|name| name.is_self_ty(db))
                && !arguments_given
            {
                return Ok(Some(FrozenQuoteSignatureType::SelfType));
            }
            if !arguments_given
                && let Some(name) = path.as_ident(db)
                && let Some((index, _)) = generics
                    .iter()
                    .enumerate()
                    .find(|(_, param)| param.name().to_opt() == Some(name))
            {
                if !matches!(
                    &name_use.class,
                    QuoteNameClass::Bound(QuoteBinder::Generic { item, index: bound })
                        if *item == generic_item && *bound as usize == index
                ) {
                    return Err(FreezingProtocolFault::GenericSignatureBindingChanged.into());
                }
                return Ok(Some(FrozenQuoteSignatureType::FunctionGeneric(
                    index as u32,
                )));
            }
            if let QuoteNameClass::Rooted(PathRes::TyAlias(alias, _)) = &name_use.class {
                let parameters = alias.params(db);
                if arguments.len() > parameters.len() {
                    return Ok(None);
                }
                let owner = hir::hir_def::GenericParamOwner::TypeAlias(alias.alias);
                for index in arguments.len()..parameters.len() {
                    let Some(view) = owner.params(db).nth(index) else {
                        return Ok(None);
                    };
                    let GenericParam::Type(parameter) = view.param else {
                        return Ok(None);
                    };
                    let Some(default) = parameter.default_ty else {
                        return Ok(None);
                    };
                    let checked = lower_hir_ty(
                        db,
                        default,
                        alias.alias.scope(),
                        hir::core::semantic::param_env(db, alias.alias.into()),
                    );
                    let Some(default) = freeze_checked_quote_type_bounded(
                        db,
                        checked,
                        &|parameter| {
                            parameters
                                .iter()
                                .position(|formal| *formal == parameter)
                                .and_then(|index| arguments.get(index).map(Cow::Borrowed))
                        },
                        nodes,
                        depth,
                    )?
                    else {
                        return Ok(None);
                    };
                    arguments.push(default);
                }
                return freeze_checked_quote_type_bounded(
                    db,
                    *alias.alias_to.skip_binder(),
                    &|parameter| {
                        parameters
                            .iter()
                            .position(|formal| *formal == parameter)
                            .and_then(|index| arguments.get(index).map(Cow::Borrowed))
                    },
                    nodes,
                    depth,
                );
            }
            if let QuoteNameClass::Rooted(PathRes::Ty(selected)) = &name_use.class
                && let TyData::TyBase(TyBase::Adt(adt)) = selected.base_ty(db).data(db)
            {
                return Ok(Some(FrozenQuoteSignatureType::Nominal {
                    root: frozen_quote_nominal_ref(db, adt.adt_ref(db))?,
                    arguments,
                    arguments_given,
                }));
            }
            if let QuoteNameClass::Rooted(PathRes::Trait(selected)) = &name_use.class {
                let QuoteNameSite::Type(site) = name_use.site else {
                    return Err(FreezingProtocolFault::TraitSignatureNameChangedSite.into());
                };
                return Ok(Some(FrozenQuoteSignatureType::Trait {
                    root: frozen_quote_trait(db, selected.key(db), site)?,
                    arguments,
                    arguments_given,
                }));
            }
            if arguments.is_empty() && !arguments_given {
                Ok(checked_primitive(ty).map(FrozenQuoteSignatureType::Primitive))
            } else {
                Ok(None)
            }
        }
        TypeKind::Primitive(_) => {
            Ok(checked_primitive(ty).map(FrozenQuoteSignatureType::Primitive))
        }
        TypeKind::Hole(slot) => Ok(Some(FrozenQuoteSignatureType::Hole(*slot))),
        TypeKind::Never => Ok(Some(FrozenQuoteSignatureType::Never)),
        TypeKind::Mode(_, _) | TypeKind::Ptr(_) | TypeKind::Path(_) | TypeKind::Array(_, _) => {
            Ok(None)
        }
    }
}

fn limit(name: FreezingLimitKind, actual: usize, maximum: usize) -> ConstructionError {
    ConstructionError::Limit {
        name,
        actual,
        maximum,
    }
}

impl std::fmt::Display for ConstructionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Protocol(fault) => fault.fmt(f),
            Self::ReadBackMissingBody => f.write_str(
                "checked-body code no longer reads back: checked function body is missing",
            ),
            Self::ReadBackInvalidBody => f.write_str(
                "checked-body code no longer reads back: checked function body is invalid",
            ),
            Self::ReadBackEffectful => {
                f.write_str("checked-body code no longer reads back: checked function has effects")
            }
            Self::ReadBackMissingOwner => {
                f.write_str("checked-body code no longer reads back: checked item owner is missing")
            }
            Self::ReadBackUnsupportedCall => f.write_str(
                "checked-body code no longer reads back: checked body contains an unsupported call",
            ),
            Self::ReadBackUnsupportedName => f.write_str(
                "checked-body code no longer reads back: checked body contains an unsupported name",
            ),
            Self::ReadBackUnsupportedType => f.write_str(
                "checked-body code no longer reads back: checked body contains an unsupported type",
            ),
            Self::SelfConstructor { name } => write!(
                f,
                "quoted Self constructor `{name}` does not match the impl self type"
            ),
            Self::UnsupportedConstructorInstantiation => {
                f.write_str("constructor alias arguments are not supported in a quote yet")
            }
            Self::MissingItemPath => f.write_str("quoted item has no ingot-relative path"),
            Self::Authority => f.write_str("generated declaration exceeds its owner's authority"),
            Self::Limit {
                name,
                actual,
                maximum,
            } => write!(f, "{name} limit exceeded: {actual} > {maximum}"),
        }
    }
}

impl<'db> RecordInput<'db> {
    fn frozen_quote_owner(
        &self,
        function: Func<'db>,
    ) -> Result<FrozenItemLocator, ConstructionError> {
        function
            .frozen_locator(self.db)
            .ok_or(FreezingProtocolFault::QuoteOwnerLocatorIsMissing.into())
    }

    pub(crate) fn frozen_owner(&self) -> Result<FrozenOwner, ConstructionError> {
        FrozenOwner::of_item(self.db, self.owner.as_item()).map_err(|error| {
            ConstructionError::Protocol(match error {
                FrozenOwnerError::MissingFile => {
                    FreezingProtocolFault::GenerationOwnerFileIsMissing
                }
                FrozenOwnerError::MissingPath => {
                    FreezingProtocolFault::GenerationOwnerPathIsMissing
                }
            })
        })
    }

    pub(crate) fn frozen_const_param_types(
        &self,
    ) -> Result<Vec<Option<SemPrimTy>>, ConstructionError> {
        let params = match self.owner {
            AdtRef::Struct(owner) => owner.hir_generic_params(self.db),
            AdtRef::Enum(owner) => owner.hir_generic_params(self.db),
        };
        params
            .data(self.db)
            .iter()
            .enumerate()
            .map(|(index, param)| match param {
                GenericParam::Type(_) => Ok(None),
                GenericParam::Const(param) => {
                    param
                        .ty
                        .to_opt()
                        .ok_or(FreezingProtocolFault::TargetConstParameterHasNoType)?;
                    let primitive = self
                        .owner
                        .as_adt(self.db)
                        .params(self.db)
                        .get(index)
                        .and_then(|parameter| parameter.const_ty_ty(self.db))
                        .and_then(|ty| match ty.base_ty(self.db).data(self.db) {
                            TyData::TyBase(TyBase::Prim(primitive)) => Some(*primitive),
                            _ => None,
                        })
                        .ok_or(FreezingProtocolFault::TargetConstParameterTypeIsNotScalar)?;
                    Ok(Some(primitive))
                }
            })
            .collect()
    }

    fn reflected_field_type(&self, variant_index: Option<u16>, index: usize) -> Option<TyId<'db>> {
        match (self.owner, variant_index) {
            (AdtRef::Struct(_), None) | (AdtRef::Enum(_), Some(_)) => {}
            _ => return None,
        }
        let adt = self.owner.as_adt(self.db);
        let shape = adt
            .fields(self.db)
            .get(usize::from(variant_index.unwrap_or(0)))?;
        (index < shape.num_types()).then(|| shape.ty(self.db, index).instantiate_identity())
    }

    /// Freeze a checked provider value before publishing any generated items.
    /// Only the designated generation protocol and this record's structural
    /// handles may cross into the output phase.
    pub fn freeze_value(
        &self,
        value: SemConstId<'db>,
    ) -> Result<FrozenMetaValue, ConstructionError> {
        self.freeze_value_counted(value).map(|(value, _)| value)
    }

    pub(super) fn freeze_value_counted(
        &self,
        value: SemConstId<'db>,
    ) -> Result<(FrozenMetaValue, usize), ConstructionError> {
        let owner = self.frozen_owner()?;
        let nodes = Cell::new(0);
        let value = self.freeze_node(value, &owner, &nodes, 0)?;
        Ok((value, nodes.get()))
    }

    fn freeze_node(
        &self,
        value: SemConstId<'db>,
        owner: &FrozenOwner,
        nodes: &Cell<usize>,
        depth: usize,
    ) -> Result<FrozenMetaValue, ConstructionError> {
        charge_frozen_node(nodes)?;
        if depth > 128 {
            return Err(limit(FreezingLimitKind::MetaDepth, depth, 128));
        }
        let freeze = |value, nodes: &Cell<usize>| self.freeze_node(value, owner, nodes, depth + 1);
        match value.value(self.db) {
            SemConstValue::Scalar { ty, value } => {
                charge_frozen_nodes(
                    nodes,
                    match &value {
                        SemConstScalar::Bool(_) => 1,
                        SemConstScalar::Int { value } => value.bits().div_ceil(8) as usize,
                        SemConstScalar::Bytes(bytes) => bytes.len(),
                    },
                )?;
                let primitive = match ty.base_ty(self.db).data(self.db) {
                    TyData::TyBase(TyBase::Prim(primitive)) => Some(*primitive),
                    _ => None,
                };
                Ok(FrozenMetaValue::Scalar {
                    value: value.clone(),
                    primitive,
                })
            }
            SemConstValue::Structural { ty, handle } => {
                let (item, kind, actual_owner) = match *handle {
                    StructuralHandle::Record(actual_owner) => {
                        (MetaItem::Record, FrozenHandleKind::Record, actual_owner)
                    }
                    StructuralHandle::Variant(variant) => (
                        MetaItem::Record,
                        FrozenHandleKind::Variant(variant.idx),
                        AdtRef::Enum(variant.enum_),
                    ),
                    StructuralHandle::Field {
                        owner: actual_owner,
                        variant_index,
                        index,
                    } => (
                        MetaItem::Field,
                        FrozenHandleKind::Field {
                            variant_index,
                            index,
                        },
                        actual_owner,
                    ),
                    StructuralHandle::FieldType {
                        owner: actual_owner,
                        variant_index,
                        index,
                    } => (
                        MetaItem::FieldType,
                        FrozenHandleKind::FieldType {
                            variant_index,
                            index,
                        },
                        actual_owner,
                    ),
                    StructuralHandle::Evidence(_) => return Err(ConstructionError::Authority),
                    StructuralHandle::CodeNode { .. } => return Err(ConstructionError::Authority),
                    StructuralHandle::Code(code) => {
                        if !self.designated(*ty, MetaItem::Code) {
                            return Err(ConstructionError::Authority);
                        }
                        return self
                            .freeze_code(code, owner, nodes)
                            .map(FrozenMetaValue::Code);
                    }
                };
                if actual_owner != self.owner || !self.designated(*ty, item) {
                    return Err(ConstructionError::Authority);
                }
                match kind {
                    FrozenHandleKind::Field {
                        variant_index,
                        index,
                    }
                    | FrozenHandleKind::FieldType {
                        variant_index,
                        index,
                    } if self.reflected_field_type(variant_index, index).is_none() => {
                        return Err(ConstructionError::Authority);
                    }
                    FrozenHandleKind::Variant(index) => {
                        let AdtRef::Enum(owner) = self.owner else {
                            return Err(ConstructionError::Authority);
                        };
                        if usize::from(index) >= owner.len_variants(self.db) {
                            return Err(ConstructionError::Authority);
                        }
                    }
                    _ => {}
                }
                Ok(FrozenMetaValue::Structural {
                    item,
                    owner: owner.clone(),
                    kind,
                })
            }
            SemConstValue::Unit
            | SemConstValue::Tuple { .. }
            | SemConstValue::Array { .. }
            | SemConstValue::Struct { .. } => {
                Err(FreezingProtocolFault::UnsupportedGeneratedValueShape.into())
            }
            SemConstValue::Enum {
                ty,
                variant,
                fields,
            } => {
                if !self.designated(*ty, MetaItem::DeclarationName) {
                    return Err(FreezingProtocolFault::UnexpectedCoreMetaVariant.into());
                }
                let TyData::TyBase(TyBase::Adt(adt)) = ty.base_ty(self.db).data(self.db) else {
                    return Err(FreezingProtocolFault::InvalidCoreMetaVariantType.into());
                };
                let AdtRef::Enum(definition) = adt.adt_ref(self.db) else {
                    return Err(FreezingProtocolFault::ExpectedCoreMetaEnumDefinition.into());
                };
                let name = definition
                    .variants(self.db)
                    .nth(usize::from(variant.0))
                    .and_then(|variant| variant.name(self.db))
                    .ok_or(FreezingProtocolFault::InvalidCoreMetaEnumVariant)?
                    .data(self.db);
                let fields = fields
                    .iter()
                    .map(|value| freeze(*value, nodes))
                    .collect::<Result<Vec<_>, _>>()?;
                // The evaluated discriminant already selected a variant of the
                // designated core enum. Freeze that case before source order is lost.
                let case = match name.as_str() {
                    "Literal" => FrozenMetaVariantCase::DeclarationNameLiteral,
                    "FieldPrefix" => FrozenMetaVariantCase::DeclarationNameFieldPrefix,
                    _ => return Err(FreezingProtocolFault::UnexpectedDeclarationNameVariant.into()),
                };
                Ok(FrozenMetaValue::DeclarationName { case, fields })
            }
            SemConstValue::Description(_) => {
                Err(FreezingProtocolFault::TypeLevelValuesAreNotGeneratedDeclarations.into())
            }
        }
    }

    fn designated(&self, ty: TyId<'db>, item: MetaItem) -> bool {
        self.meta_type(item)
            .is_some_and(|expected| expected.base_ty(self.db) == ty.base_ty(self.db))
    }

    fn freeze_field_type(
        &self,
        ty: TyId<'db>,
        budget: &Cell<usize>,
    ) -> Result<Option<FrozenQuoteSignatureType>, ConstructionError> {
        let mut nodes = budget.get();
        let result = freeze_checked_quote_type_bounded(
            self.db,
            ty,
            &|ty| {
                let TyData::TyParam(param) = ty.data(self.db) else {
                    return None;
                };
                (param.owner == self.owner.scope())
                    .then(|| {
                        u32::try_from(param.original_idx(self.db))
                            .ok()
                            .map(|index| Cow::Owned(FrozenQuoteSignatureType::OwnerGeneric(index)))
                    })
                    .flatten()
            },
            &mut nodes,
            0,
        )
        .map_err(|error| match error {
            ConstructionError::Limit {
                name: FreezingLimitKind::TypeNodes,
                actual,
                maximum,
            } => limit(FreezingLimitKind::MetaNodes, actual, maximum),
            error => error,
        })?;
        budget.set(nodes);
        Ok(result)
    }

    fn freeze_code(
        &self,
        root: QuotedCode<'db>,
        owner: &FrozenOwner,
        budget: &Cell<usize>,
    ) -> Result<FrozenCode, ConstructionError> {
        // Discover the interned graph without following the Rust call stack.
        // Shared fragments get one frozen node, while every edge is retained.
        charge_frozen_code_node(budget)?;
        let mut nodes = vec![FrozenCodeNode::empty(None)];
        let mut seen = HashMap::from([(root, 0)]);
        let mut pending = vec![root];
        let mut cursor = 0;
        while cursor < pending.len() {
            for value in pending[cursor].holes(self.db) {
                if let &SemConstValue::Structural {
                    ty,
                    handle: StructuralHandle::Code(child),
                } = value.value(self.db)
                {
                    if !self.designated(ty, MetaItem::Code) {
                        return Err(ConstructionError::Authority);
                    }
                    if let std::collections::hash_map::Entry::Vacant(entry) = seen.entry(child) {
                        charge_frozen_code_node(budget)?;
                        entry.insert(pending.len());
                        pending.push(child);
                        nodes.push(FrozenCodeNode::empty(None));
                    }
                }
            }
            cursor += 1;
        }
        for (index, code) in pending.into_iter().enumerate() {
            self.freeze_code_node(code, owner, budget, &mut nodes, &seen, index)?;
        }
        Ok(FrozenCode { root: 0, nodes })
    }

    fn freeze_code_node(
        &self,
        code: QuotedCode<'db>,
        owner: &FrozenOwner,
        budget: &Cell<usize>,
        nodes: &mut [FrozenCodeNode],
        seen: &HashMap<QuotedCode<'db>, usize>,
        index: usize,
    ) -> Result<(), ConstructionError> {
        // Edges are retained even when graph discovery has already seen their child.
        // Charge before allocating the parallel span and hole vectors.
        for _ in code.holes(self.db) {
            charge_frozen_node(budget)?;
        }
        let mut hole_spans = vec![None; code.holes(self.db).len()];
        let mut rooted_funcs = Vec::new();
        let mut rooted_constructors = Vec::new();
        let mut rooted_traits = Vec::new();
        let mut qualified_consts = Vec::new();
        let mut impl_trait_type_arguments = None;
        let mut locals: Vec<FrozenQuoteLocal> = Vec::new();
        let mut local_types = Vec::new();
        let mut cast_primitives = Vec::new();
        let mut param_uses = Vec::new();
        let mut effect_uses = Vec::new();
        let mut generic_uses = Vec::new();
        let mut self_uses = Vec::new();
        let mut self_record_inits = Vec::new();
        let mut open_uses = Vec::new();
        let mut free_self_uses = Vec::new();
        let mut function_types = Vec::new();
        let mut where_predicates = Vec::new();
        let mut assoc_const_primitives = Vec::new();
        let mut assoc_type_values = Vec::new();
        let site = match code.source(self.db) {
            hir::analysis::semantic::CodeSource::Template(site) => Some(site),
            hir::analysis::semantic::CodeSource::Empty => None,
            hir::analysis::semantic::CodeSource::VariantBinding {
                variant,
                group,
                field,
            } => {
                let adt = AdtRef::Enum(variant.enum_);
                if adt != self.owner || !code.holes(self.db).is_empty() {
                    return Err(ConstructionError::Authority);
                }
                let shape = adt.as_adt(self.db).fields(self.db);
                let Some(shape) = shape.get(usize::from(variant.idx)) else {
                    return Err(FreezingProtocolFault::BinderVariantIndexIsOutOfBounds.into());
                };
                if field.is_some_and(|index| index >= shape.num_types()) {
                    return Err(FreezingProtocolFault::BinderFieldIndexIsOutOfBounds.into());
                }
                let binding = hir::core::meta::FrozenVariantBinding {
                    owner: frozen_quote_nominal_ref(self.db, adt)?,
                    variant_index: variant.idx,
                    group,
                };
                nodes[index].source = match field {
                    Some(index) => hir::core::meta::FrozenCodeSource::Field(binding, index),
                    None => hir::core::meta::FrozenCodeSource::Pattern(binding),
                };
                return Ok(());
            }
        };
        let template = if let Some(site) = site {
            let func = site
                .owner
                .containing_func(self.db)
                .ok_or(FreezingProtocolFault::QuoteOwnerIsNotAFunction)?;
            let body = func
                .body(self.db)
                .ok_or(FreezingProtocolFault::QuoteOwnerBodyIsMissing)?;
            if site.source == FrozenTemplateSource::CheckedBody {
                if site.owner != body
                    || site.expr != body.expr(self.db)
                    || !code.holes(self.db).is_empty()
                {
                    return Err(FreezingProtocolFault::InvalidCheckedBodyCodeOrigin.into());
                }
                let mut frozen = super::read_back::read_back_with_budget(self.db, func, budget)?;
                nodes[index] = frozen.nodes.swap_remove(frozen.root);
                return Ok(());
            }
            let Partial::Present(Expr::Quote(quote)) = &body.exprs(self.db)[site.expr] else {
                return Err(FreezingProtocolFault::QuoteSiteNoLongerNamesAQuote.into());
            };
            // Bound the authored arenas before later copying nested pattern/arm
            // vectors. This is independent of how many names the body contains.
            for _ in 0..quote.template.exprs(self.db).len()
                + quote.template.pats(self.db).len()
                + quote.template.stmts(self.db).len()
                + quote.template.conds(self.db).len()
            {
                charge_frozen_node(budget)?;
            }
            // Non-body holes (including where/type positions) retain the
            // authored operand span; body-hole maps below refine their edges.
            let mut record_hole_span = |slot: usize, span: Option<common::diagnostics::Span>| {
                let destination = hole_spans
                    .get_mut(slot)
                    .ok_or(FreezingProtocolFault::QuoteHoleSlotIsOutOfBounds)?;
                *destination = span.and_then(|span| {
                    Some(hir::core::meta::FrozenQuoteSpan {
                        file: span.file.url(self.db)?,
                        range: span.range.into(),
                    })
                });
                Ok::<_, ConstructionError>(())
            };
            for (slot, hole) in quote.holes.iter().enumerate() {
                record_hole_span(slot, hole.expr.span(body).resolve(self.db))?;
            }
            for (expr, value) in quote.template.exprs(self.db).iter() {
                if let Partial::Present(Expr::Hole(slot)) = value {
                    record_hole_span(*slot as usize, expr.span(quote.template).resolve(self.db))?;
                }
            }
            for (pat, value) in quote.template.pats(self.db).iter() {
                if let Partial::Present(hir::hir_def::Pat::Hole(slot)) = value {
                    record_hole_span(*slot as usize, pat.span(quote.template).resolve(self.db))?;
                }
            }
            if let QuoteFragment::Arms(arms) = &quote.fragment {
                for (index, arm) in arms.iter().enumerate() {
                    if let hir::hir_def::MatchArm::Hole(slot) = arm {
                        let span = site
                            .expr
                            .span(body)
                            .into_quote_expr()
                            .arms()
                            .arm(index)
                            .resolve(self.db);
                        record_hole_span(*slot as usize, span)?;
                    }
                }
            }
            for (expr, value) in quote.template.exprs(self.db).iter() {
                if let Partial::Present(Expr::Match(_, Partial::Present(arms))) = value {
                    for (index, arm) in arms.iter().enumerate() {
                        if let hir::hir_def::MatchArm::Hole(slot) = arm {
                            let span = expr
                                .span(quote.template)
                                .into_match_expr()
                                .arms()
                                .arm(index)
                                .resolve(self.db);
                            record_hole_span(*slot as usize, span)?;
                        }
                    }
                }
            }
            let checked_primitive = |ty| {
                lower_hir_primitive(
                    self.db,
                    ty,
                    func.scope(),
                    PredicateListId::empty_list(self.db),
                )
            };
            let (diagnostics, typed) = check_func_body(self.db, func);
            if !diagnostics.is_empty() {
                return Err(FreezingProtocolFault::QuoteOwnerDoesNotTypeCheck.into());
            }
            let checked = typed
                .checked_quote(site.expr)
                .ok_or(FreezingProtocolFault::QuoteHasNoCheckedNameClassification)?;
            // Bound classification work before building its parallel carrier vectors.
            for _ in 0..checked.names.len()
                + checked.local_types.len()
                + checked.assoc_const_primitives.len()
            {
                charge_frozen_node(budget)?;
            }
            assoc_const_primitives = checked.assoc_const_primitives.clone();
            let checked_signature_type =
                |ty: TypeId<'db>,
                 signature: QuoteSignatureSite,
                 generics: &[GenericParam<'db>],
                 generic_item: u32,
                 extra_names: FreezingProtocolFault| {
                    let mut names = checked.names.iter().filter(|use_| {
                        use_.signature == Some(signature)
                            && matches!(use_.site, QuoteNameSite::Type(_))
                    });
                    let frozen = freeze_quote_signature_type(
                        self.db,
                        ty,
                        &mut names,
                        generics,
                        generic_item,
                        &checked_primitive,
                        budget,
                    )?;
                    if frozen.is_some() && names.next().is_some() {
                        return Err(extra_names.into());
                    }
                    Ok::<_, ConstructionError>(frozen)
                };
            let checked_trait_site = |signature| {
                let mut roots = checked.names.iter().filter_map(|name| {
                    if name.signature != Some(signature) {
                        return None;
                    }
                    match (name.site, &name.class) {
                        (QuoteNameSite::Type(site), QuoteNameClass::Rooted(PathRes::Trait(_))) => {
                            Some(site)
                        }
                        _ => None,
                    }
                });
                let site = roots.next();
                if roots.next().is_some() { None } else { site }
            };
            let fragment_where = match &quote.fragment {
                QuoteFragment::Where(clause) => Some(clause),
                QuoteFragment::Items(items) => match items.as_slice() {
                    [QuoteItem::Impl(item)] => Some(&item.where_clause),
                    _ => None,
                },
                _ => None,
            };
            if let Some(clause) = fragment_where {
                for predicate in &clause.predicates {
                    charge_frozen_node(budget)?;
                    if let QuoteWherePredicate::Bound(predicate) = predicate {
                        for _ in &predicate.bounds {
                            charge_frozen_node(budget)?;
                        }
                    }
                }
                where_predicates = clause
                    .predicates
                    .iter()
                    .enumerate()
                    .map(|(index, predicate)| match predicate {
                        QuoteWherePredicate::Hole(_) => Ok(None),
                        QuoteWherePredicate::Const(_) => Err(
                            FreezingProtocolFault::FragmentConstPredicateNeedsCheckedReification
                                .into(),
                        ),
                        QuoteWherePredicate::Bound(predicate) => {
                            let ty = predicate
                                .ty
                                .to_opt()
                                .ok_or(FreezingProtocolFault::WhereTypeIsMissing)?;
                            let signature = QuoteSignatureSite::FragmentWhereType {
                                predicate: index as u32,
                            };
                            let ty = checked_signature_type(
                                ty,
                                signature,
                                &[],
                                u32::MAX,
                                FreezingProtocolFault::WhereTypeHasExtraCheckedNames,
                            )?
                            .ok_or(FreezingProtocolFault::WhereTypeNeedsCheckedReification)?;
                            let trait_sites = predicate
                                .bounds
                                .iter()
                                .enumerate()
                                .map(|(bound, _)| {
                                    checked_trait_site(QuoteSignatureSite::FragmentWhereBound {
                                        predicate: index as u32,
                                        bound: bound as u32,
                                    })
                                })
                                .collect();
                            Ok(Some(FrozenQuoteWherePredicate::Bound { ty, trait_sites }))
                        }
                    })
                    .collect::<Result<Vec<_>, ConstructionError>>()?;
            }
            let mut freeze_function = |item: &QuoteFunc<'db>, index: u32| {
                let slots = item
                    .params
                    .to_opt()
                    .map_or(0, |params| params.data(self.db).len())
                    + item.effects.data(self.db).len()
                    + item.generic_params.data(self.db).len().saturating_mul(3)
                    + item
                        .generic_params
                        .data(self.db)
                        .iter()
                        .map(|param| match param {
                            GenericParam::Type(param) => param.bounds.len(),
                            GenericParam::Const(_) => 0,
                        })
                        .sum::<usize>()
                    + item
                        .where_clause
                        .predicates
                        .iter()
                        .map(|predicate| match predicate {
                            QuoteWherePredicate::Bound(predicate) => 1 + predicate.bounds.len(),
                            _ => 1,
                        })
                        .sum::<usize>();
                for _ in 0..slots {
                    charge_frozen_node(budget)?;
                }

                let generic_item = *checked
                    .function_generic_items
                    .get(index as usize)
                    .ok_or(FreezingProtocolFault::FunctionHasNoCheckedGenericOwner)?;
                let signature_type = |ty: Option<TypeId<'db>>, signature| {
                    ty.map(|ty| {
                        checked_signature_type(
                            ty,
                            signature,
                            item.generic_params.data(self.db),
                            generic_item,
                            FreezingProtocolFault::SignatureHasExtraCheckedNames,
                        )
                    })
                    .transpose()
                    .map(Option::flatten)
                };
                let params = item
                    .params
                    .to_opt()
                    .map(|params| {
                        params
                            .data(self.db)
                            .iter()
                            .enumerate()
                            .map(|(param_index, param)| {
                                signature_type(
                                    param.ty.to_opt(),
                                    QuoteSignatureSite::Param {
                                        func: index,
                                        index: param_index as u32,
                                    },
                                )
                            })
                            .collect::<Result<Vec<_>, ConstructionError>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let effects = item
                    .effects
                    .data(self.db)
                    .iter()
                    .enumerate()
                    .map(|(effect_index, effect)| {
                        signature_type(
                            effect.key_ty.to_opt(),
                            QuoteSignatureSite::Effect {
                                func: index,
                                index: effect_index as u32,
                            },
                        )
                    })
                    .collect::<Result<Vec<_>, ConstructionError>>()?;
                let result =
                    signature_type(item.ret_ty, QuoteSignatureSite::Result { func: index })?;
                let generic_bound_sites = item
                    .generic_params
                    .data(self.db)
                    .iter()
                    .enumerate()
                    .map(|(param_index, param)| match param {
                        GenericParam::Type(param) => param
                            .bounds
                            .iter()
                            .enumerate()
                            .map(|(bound_index, bound)| {
                                if !matches!(bound, TypeBound::Trait(_)) {
                                    return None;
                                }
                                let signature = QuoteSignatureSite::GenericBound {
                                    func: index,
                                    param: param_index as u32,
                                    bound: bound_index as u32,
                                };
                                checked_trait_site(signature)
                            })
                            .collect(),
                        GenericParam::Const(_) => Vec::new(),
                    })
                    .collect();
                let generic_type_defaults = item
                    .generic_params
                    .data(self.db)
                    .iter()
                    .enumerate()
                    .map(|(param_index, param)| match param {
                        GenericParam::Type(param) => signature_type(
                            param.default_ty,
                            QuoteSignatureSite::GenericDefault {
                                func: index,
                                param: param_index as u32,
                            },
                        ),
                        GenericParam::Const(_) => Ok(None),
                    })
                    .collect::<Result<Vec<_>, ConstructionError>>()?;
                let generic_const_types = item
                    .generic_params
                    .data(self.db)
                    .iter()
                    .map(|param| match param {
                        GenericParam::Const(param) => param.ty.to_opt().and_then(checked_primitive),
                        GenericParam::Type(_) => None,
                    })
                    .collect();
                let where_predicates = item
                    .where_clause
                    .predicates
                    .iter()
                    .enumerate()
                    .map(|(predicate_index, predicate)| {
                        let QuoteWherePredicate::Bound(predicate) = predicate else {
                            return match predicate {
                                QuoteWherePredicate::Const(_) => {
                                    Ok(FrozenQuoteWherePredicate::Const)
                                }
                                QuoteWherePredicate::Hole(_) => Err(FreezingProtocolFault::MethodWhereFragmentNeedsCheckedReification.into()),
                                QuoteWherePredicate::Bound(_) => unreachable!(),
                            };
                        };
                        let ty = predicate
                            .ty
                            .to_opt()
                            .ok_or(FreezingProtocolFault::MethodWhereBoundTypeIsMissing)?;
                        let ty = checked_signature_type(
                            ty,
                            QuoteSignatureSite::WhereType {
                                func: index,
                                predicate: predicate_index as u32,
                            },
                            item.generic_params.data(self.db),
                            generic_item,
                            FreezingProtocolFault::SignatureHasExtraCheckedNames,
                        )?
                        .ok_or(FreezingProtocolFault::MethodWhereBoundTypeNeedsCheckedReification)?;
                        let trait_sites = predicate
                            .bounds
                            .iter()
                            .enumerate()
                            .map(|(bound_index, bound)| {
                                if !matches!(bound, TypeBound::Trait(_)) {
                                    return None;
                                }
                                checked_trait_site(QuoteSignatureSite::WhereBound {
                                    func: index,
                                    predicate: predicate_index as u32,
                                    bound: bound_index as u32,
                                })
                            })
                            .collect();
                        Ok(FrozenQuoteWherePredicate::Bound { ty, trait_sites })
                    })
                    .collect::<Result<Vec<_>, ConstructionError>>()?;
                for local in checked
                    .local_types
                    .iter()
                    .filter(|local| local.member.function() == Some(index))
                {
                    let ty = checked_signature_type(
                        local.ty,
                        QuoteSignatureSite::LocalType {
                            func: Some(index),
                            stmt: local.stmt,
                        },
                        item.generic_params.data(self.db),
                        generic_item,
                        FreezingProtocolFault::SignatureHasExtraCheckedNames,
                    )?
                    .ok_or(FreezingProtocolFault::LocalTypeNeedsCheckedReification)?;
                    local_types.push(FrozenQuoteLocalType {
                        member: local.member,
                        stmt: local.stmt,
                        ty,
                    });
                }
                function_types.push(FrozenQuoteFunctionTypes {
                    params,
                    effects,
                    result,
                    generic_bound_sites,
                    generic_type_defaults,
                    generic_const_types,
                    where_predicates,
                });
                Ok::<_, ConstructionError>(())
            };
            if let QuoteFragment::Items(items) = &quote.fragment {
                if let [QuoteItem::Impl(item)] = items.as_slice() {
                    assoc_type_values = item
                        .items
                        .iter()
                        .filter_map(|child| match child {
                            QuoteItem::Type(associated) => Some(associated),
                            _ => None,
                        })
                        .enumerate()
                        .map(|(index, associated)| {
                            let Some(ty) = associated.ty else {
                                return Ok(None);
                            };
                            checked_signature_type(
                                ty,
                                QuoteSignatureSite::AssocType {
                                    index: index as u32,
                                },
                                &[],
                                0,
                                FreezingProtocolFault::AssociatedTypeHasExtraCheckedNames,
                            )
                        })
                        .collect::<Result<Vec<_>, ConstructionError>>()?;
                }
                if let [QuoteItem::Impl(item)] = items.as_slice()
                    && let Some(Partial::Present(trait_ref)) = item.trait_ref
                    && let Partial::Present(path) = trait_ref.path(self.db)
                {
                    let args = path.generic_args(self.db);
                    if args.is_given(self.db) {
                        let arguments =
                            args.data(self.db)
                                .iter()
                                .enumerate()
                                .map(|(arg_index, arg)| {
                                    let GenericArg::Type(arg) = arg else {
                                        return Err(
                                        FreezingProtocolFault::ImplTraitArgumentNeedsACheckedType
                                            .into(),
                                    );
                                    };
                                    let ty = arg.ty.to_opt().ok_or(
                                        FreezingProtocolFault::ImplTraitTypeArgumentIsMissing,
                                    )?;
                                    let signature = QuoteSignatureSite::ImplTraitArgument {
                                        index: arg_index as u32,
                                    };
                                    let frozen = checked_signature_type(
                                    ty,
                                    signature,
                                    &[],
                                    u32::MAX,
                                    FreezingProtocolFault::ImplTraitArgumentHasExtraCheckedNames,
                                )?
                                .ok_or(
                                    FreezingProtocolFault::ImplTraitArgumentNeedsCheckedReification,
                                )?;
                                    Ok(frozen)
                                })
                                .collect::<Result<Vec<_>, ConstructionError>>()?;
                        impl_trait_type_arguments = Some(arguments);
                    }
                }
            }
            for (index, item) in quoted_functions(&quote.fragment).enumerate() {
                freeze_function(item, index as u32)?;
            }
            // Every cast in the template, whichever fragment or quoted body
            // holds it, carries its checked target type.
            for (expr, data) in quote.template.exprs(self.db).iter() {
                if let Partial::Present(Expr::Cast(_, Partial::Present(ty))) = data {
                    let primitive = checked_primitive(*ty)
                        .ok_or(FreezingProtocolFault::CastNeedsACheckedPrimitiveType)?;
                    charge_frozen_node(budget)?;
                    cast_primitives.push((expr, primitive));
                }
            }
            // Standalone fragments have their own binder frame, without a
            // generated function index. Freeze annotations with the same checked
            // type path used by function headers and body annotations.
            for local in checked
                .local_types
                .iter()
                .filter(|local| local.member.function().is_none())
            {
                let signature = QuoteSignatureSite::LocalType {
                    func: None,
                    stmt: local.stmt,
                };
                let ty = checked_signature_type(
                    local.ty,
                    signature,
                    &[],
                    u32::MAX,
                    FreezingProtocolFault::LocalTypeHasExtraCheckedNames,
                )?
                .ok_or(FreezingProtocolFault::LocalTypeNeedsCheckedReification)?;
                local_types.push(FrozenQuoteLocalType {
                    member: local.member,
                    stmt: local.stmt,
                    ty,
                });
            }
            let self_owner = match &quote.fragment {
                QuoteFragment::Items(items) => match items.as_slice() {
                    [QuoteItem::Impl(item)] => item.ty.to_opt().and_then(|ty| {
                        let TypeKind::Hole(slot) = ty.data(self.db) else {
                            return None;
                        };
                        match *code.holes(self.db).get(*slot as usize)?.value(self.db) {
                            SemConstValue::Structural {
                                handle: StructuralHandle::Record(owner),
                                ..
                            } if owner == self.owner => Some(owner),
                            SemConstValue::Structural {
                                handle:
                                    StructuralHandle::FieldType {
                                        owner,
                                        variant_index,
                                        index,
                                    },
                                ..
                            } if owner == self.owner => self
                                .reflected_field_type(variant_index, index)
                                .and_then(|ty| match ty.base_ty(self.db).data(self.db) {
                                    TyData::TyBase(TyBase::Adt(adt)) => Some(adt.adt_ref(self.db)),
                                    _ => None,
                                }),
                            _ => None,
                        }
                    }),
                    _ => None,
                },
                _ => None,
            };
            for (name_index, name) in checked.names.iter().enumerate() {
                if matches!(name.class, QuoteNameClass::SelfName) {
                    let site_and_path = match name.site {
                        QuoteNameSite::Expr(expr) => match &quote.template.exprs(self.db)[expr] {
                            Partial::Present(
                                Expr::Path(Partial::Present(path))
                                | Expr::RecordInit(Partial::Present(path), _),
                            ) => Some((FrozenQuoteConstructorSite::Expr(expr), *path)),
                            _ => None,
                        },
                        QuoteNameSite::Pat(pat) => match &quote.template.pats(self.db)[pat] {
                            Partial::Present(
                                hir::hir_def::Pat::Path(Partial::Present(path), _)
                                | hir::hir_def::Pat::PathTuple(Partial::Present(path), _)
                                | hir::hir_def::Pat::Record(Partial::Present(path), _),
                            ) => Some((FrozenQuoteConstructorSite::Pat(pat), *path)),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some((site, path)) = site_and_path
                        && path
                            .segment(self.db, 0)
                            .and_then(|root| root.ident(self.db).to_opt())
                            .is_some_and(|name| name.is_self_ty(self.db))
                    {
                        let spelling = path.pretty_print(self.db);
                        let failure = || ConstructionError::SelfConstructor {
                            name: spelling.clone(),
                        };
                        let owner = self_owner.ok_or_else(failure)?;
                        if path.len(self.db) > 2
                            || (0..path.len(self.db)).any(|i| {
                                path.segment(self.db, i)
                                    .unwrap()
                                    .generic_args(self.db)
                                    .is_given(self.db)
                            })
                        {
                            return Err(failure());
                        }
                        let variant_index = match owner {
                            AdtRef::Struct(_) if path.len(self.db) == 1 => None,
                            AdtRef::Enum(enum_) if path.len(self.db) == 2 => {
                                let selected = path.ident(self.db).to_opt().ok_or_else(failure)?;
                                Some(
                                    (0..enum_.len_variants(self.db).min(usize::from(u16::MAX) + 1))
                                        .find(|index| {
                                            hir::hir_def::EnumVariant::new(enum_, *index)
                                                .name(self.db)
                                                == Some(selected.data(self.db).as_str())
                                        })
                                        .and_then(|index| u16::try_from(index).ok())
                                        .ok_or_else(failure)?,
                                )
                            }
                            _ => return Err(failure()),
                        };
                        rooted_constructors.push(
                            FrozenQuoteConstructor::new(
                                name.member,
                                site,
                                frozen_quote_nominal_ref(self.db, owner)?,
                                variant_index,
                                None,
                            )
                            .for_self(),
                        );
                        continue;
                    }
                }

                let constructor = match &name.class {
                    QuoteNameClass::Rooted(PathRes::Ty(ty) | PathRes::TyAlias(_, ty)) => {
                        match ty.base_ty(self.db).data(self.db) {
                            TyData::TyBase(TyBase::Adt(adt))
                                if matches!(adt.adt_ref(self.db), AdtRef::Struct(_)) =>
                            {
                                Some((adt.adt_ref(self.db), None, *ty))
                            }
                            _ => None,
                        }
                    }
                    QuoteNameClass::Rooted(PathRes::EnumVariant(variant)) => Some((
                        AdtRef::Enum(variant.variant.enum_),
                        Some(variant.variant.idx),
                        variant.ty,
                    )),
                    _ => None,
                };
                let site = match name.site {
                    QuoteNameSite::Expr(site) => Some(FrozenQuoteConstructorSite::Expr(site)),
                    QuoteNameSite::Pat(site) => Some(FrozenQuoteConstructorSite::Pat(site)),
                    QuoteNameSite::Type(_) => None,
                };
                if let (Some((owner, variant_index, ty)), Some(site)) = (constructor, site) {
                    rooted_constructors.push(FrozenQuoteConstructor::new(
                        name.member,
                        site,
                        frozen_quote_nominal_ref(self.db, owner)?,
                        variant_index,
                        if match &name.class {
                            QuoteNameClass::Rooted(PathRes::TyAlias(..)) => true,
                            QuoteNameClass::Rooted(PathRes::EnumVariant(variant)) => {
                                variant.path.parent(self.db).is_some_and(|parent| {
                                    matches!(
                                        resolve_path(
                                            self.db,
                                            parent,
                                            func.scope(),
                                            PredicateListId::empty_list(self.db),
                                            true
                                        ),
                                        Ok(PathRes::TyAlias(..))
                                    )
                                })
                            }
                            _ => false,
                        } {
                            Some(
                                freeze_quote_constructor_arguments_budgeted(
                                    self.db, ty, func, budget,
                                )?
                                .ok_or(ConstructionError::UnsupportedConstructorInstantiation)?,
                            )
                        } else {
                            None
                        },
                    ));
                }
                if matches!(name.signature, Some(QuoteSignatureSite::WhereConst { .. }))
                    && matches!(
                        &name.class,
                        QuoteNameClass::Binds
                            | QuoteNameClass::Bound(
                                QuoteBinder::Pat(..)
                                    | QuoteBinder::Param { .. }
                                    | QuoteBinder::EffectParam { .. }
                            )
                            | QuoteNameClass::SelfName
                    )
                {
                    return Err(
                        FreezingProtocolFault::ConstWhereLocalOrValueUseNeedsCheckedReification
                            .into(),
                    );
                }
                match (name.site, &name.class) {
                    (QuoteNameSite::Pat(binder), QuoteNameClass::Binds) => {
                        if locals.iter().any(|local| local.binder == binder) {
                            return Err(FreezingProtocolFault::QuoteBinderIsClassifiedTwice.into());
                        }
                        locals.push(FrozenQuoteLocal {
                            member: name.member,
                            binder,
                            uses: Vec::new(),
                        });
                    }
                    (
                        QuoteNameSite::Expr(expr),
                        QuoteNameClass::Bound(QuoteBinder::Pat(binder)),
                    ) => {
                        let local = locals
                            .iter_mut()
                            .find(|local| local.binder == *binder)
                            .ok_or(FreezingProtocolFault::QuoteLocalUseHasNoBinder)?;
                        if name.member != local.member {
                            return Err(FreezingProtocolFault::LocalUseChangedFunctionOwner.into());
                        }
                        local.uses.push(expr);
                    }
                    (
                        QuoteNameSite::Expr(expr),
                        QuoteNameClass::Bound(QuoteBinder::Param { func, index }),
                    ) => {
                        if name.member.function() != Some(*func) {
                            return Err(
                                FreezingProtocolFault::ParameterUseChangedFunctionOwner.into()
                            );
                        }
                        param_uses.push(FrozenQuoteParamUse {
                            site: expr,
                            member: name.member,
                            index: *index,
                        });
                    }
                    (
                        QuoteNameSite::Expr(expr),
                        QuoteNameClass::Bound(QuoteBinder::EffectParam { func, index }),
                    ) => {
                        if name.member.function() != Some(*func)
                            || !matches!(
                                &quote.template.exprs(self.db)[expr],
                                Partial::Present(Expr::Path(Partial::Present(path)))
                                    if path.as_ident(self.db).is_some()
                            )
                        {
                            return Err(
                                FreezingProtocolFault::EffectUseNeedsASimpleBoundPath.into()
                            );
                        }
                        effect_uses.push(FrozenQuoteEffectUse {
                            site: expr,
                            member: name.member,
                            index: *index,
                        });
                    }
                    (
                        QuoteNameSite::Expr(expr),
                        QuoteNameClass::Bound(QuoteBinder::Generic { item, index }),
                    ) => {
                        let func = name.member.function().ok_or(FreezingProtocolFault::GenericBodyUseOutsideAFunctionNeedsTransplantation)?;
                        if checked.function_generic_items.get(func as usize) != Some(item)
                            || function_types
                                .get(func as usize)
                                .and_then(|types| types.generic_const_types.get(*index as usize))
                                .copied()
                                .flatten()
                                .is_none()
                        {
                            return Err(
                                FreezingProtocolFault::GenericBodyUseNeedsCheckedReification.into(),
                            );
                        }
                        let simple_path = matches!(
                            &quote.template.exprs(self.db)[expr],
                            Partial::Present(Expr::Path(Partial::Present(path)))
                                if path.as_ident(self.db).is_some()
                        );
                        if !simple_path {
                            return Err(
                                FreezingProtocolFault::GenericBodyUseNeedsASimplePath.into()
                            );
                        }
                        generic_uses.push(FrozenQuoteGenericUse {
                            site: expr,
                            member: name.member,
                            index: *index,
                        });
                    }
                    (QuoteNameSite::Expr(expr), QuoteNameClass::SelfName) => {
                        if matches!(
                            &quote.template.exprs(self.db)[expr],
                            Partial::Present(Expr::RecordInit(Partial::Present(path), _))
                                if path.is_self_ty(self.db)
                        ) {
                            name.member.function().ok_or(FreezingProtocolFault::SelfConstructionOutsideAMethodNeedsTransplantation)?;
                            self_record_inits.push(FrozenQuoteSelfUse {
                                member: name.member,
                                site: expr,
                            });
                            continue;
                        }
                        let simple_self = match &quote.template.exprs(self.db)[expr] {
                            Partial::Present(Expr::Path(Partial::Present(path))) => path
                                .as_ident(self.db)
                                .is_some_and(|name| name.is_self(self.db)),
                            _ => false,
                        };
                        if !simple_self {
                            return Err(
                                FreezingProtocolFault::SelfPathNeedsCheckedTransplantation.into()
                            );
                        }
                        if name.member.function().is_some() {
                            self_uses.push(FrozenQuoteSelfUse {
                                member: name.member,
                                site: expr,
                            });
                        } else if matches!(
                            &quote.fragment,
                            QuoteFragment::Expr | QuoteFragment::Arms(_)
                        ) {
                            free_self_uses.push(expr);
                        } else {
                            return Err(
                                FreezingProtocolFault::SelfOutsideAMethodNeedsTransplantation
                                    .into(),
                            );
                        }
                    }
                    (QuoteNameSite::Expr(expr), QuoteNameClass::Open(index))
                        if name.member.function().is_none()
                            && matches!(
                                &quote.fragment,
                                QuoteFragment::Expr | QuoteFragment::Arms(_)
                            ) =>
                    {
                        open_uses.push(FrozenQuoteOpenUse {
                            site: expr,
                            index: *index,
                        });
                    }
                    _ => {}
                }
                if let (QuoteNameSite::Expr(expr), QuoteNameClass::Rooted(PathRes::Func(_))) =
                    (name.site, &name.class)
                {
                    let QuoteNameClass::Rooted(res) = &name.class else {
                        unreachable!();
                    };
                    let PathRes::Func(ty) = res else {
                        unreachable!();
                    };
                    let TyData::TyBase(TyBase::Func(CallableDef::Func(selected))) =
                        ty.base_ty(self.db).data(self.db)
                    else {
                        return Err(FreezingProtocolFault::FunctionRootHasNoAuthoredFunction.into());
                    };
                    if name.member.function().is_none()
                        && name.member.assoc_const().is_none()
                        && !matches!(quote.fragment, QuoteFragment::Expr | QuoteFragment::Arms(_))
                    {
                        return Err(FreezingProtocolFault::RootedFunctionOutsideAFunctionNeedsTransplantation.into());
                    }
                    let (generics, generic_item) =
                        if let Some(func_index) = name.member.function() {
                            let function = quoted_functions(&quote.fragment)
                                .nth(func_index as usize)
                                .ok_or(FreezingProtocolFault::FunctionGenericOwnerIsMissing)?;
                            let generic_item = *checked
                                .function_generic_items
                                .get(func_index as usize)
                                .ok_or(FreezingProtocolFault::FunctionGenericIdentityIsMissing)?;
                            (
                                function.generic_params.data(self.db).as_slice(),
                                generic_item,
                            )
                        } else {
                            (&[][..], u32::MAX)
                        };
                    let type_arguments = match &quote.template.exprs(self.db)[expr] {
                        Partial::Present(Expr::Path(Partial::Present(path))) => {
                            for index in 0..path.len(self.db).saturating_sub(1) {
                                let segment = path
                                    .segment(self.db, index)
                                    .ok_or(FreezingProtocolFault::FunctionPathSegmentIsMissing)?;
                                if segment.generic_args(self.db).is_given(self.db) {
                                    return Err(FreezingProtocolFault::FunctionIntermediateTypeArgumentsNeedReification.into());
                                }
                            }
                            let args = path.generic_args(self.db);
                            if args.is_given(self.db) {
                                let arguments = args
                                    .data(self.db)
                                    .iter()
                                    .enumerate()
                                    .map(|(index, arg)| {
                                        let GenericArg::Type(arg) = arg else {
                                            return Err(FreezingProtocolFault::FunctionArgumentNeedsAType.into());
                                        };
                                        let Some(ty) = arg.ty.to_opt() else {
                                            return Err(FreezingProtocolFault::FunctionTypeArgumentIsMissing.into());
                                        };
                                        let signature = QuoteSignatureSite::CallTypeArgument {
                                            expr,
                                            index: index as u32,
                                        };
                                        let mut names = checked.names.iter().filter(|name| {
                                            name.signature == Some(signature)
                                                && matches!(name.site, QuoteNameSite::Type(_))
                                        });
                                        let frozen = freeze_quote_signature_type(
                                            self.db,
                                            ty,
                                            &mut names,
                                            generics,
                                            generic_item,
                                            &checked_primitive, budget,
)?
                                        .ok_or(FreezingProtocolFault::FunctionTypeArgumentNeedsCheckedReification)?;
                                        if names.next().is_some() {
                                            return Err(FreezingProtocolFault::FunctionTypeArgumentHasExtraCheckedNames.into());
                                        }
                                        Ok(frozen)
                                    })
                                    .collect::<Result<Vec<_>, ConstructionError>>()?;
                                Some(arguments)
                            } else {
                                None
                            }
                        }
                        _ => {
                            return Err(FreezingProtocolFault::FunctionRootChangedShape.into());
                        }
                    };
                    rooted_funcs.push(FrozenQuoteFunction {
                        member: name.member,
                        type_arguments,
                        site: expr,
                        owner: self.frozen_quote_owner(*selected)?,
                    });
                }
                if let (QuoteNameSite::Type(site), QuoteNameClass::Rooted(PathRes::Trait(inst))) =
                    (name.site, &name.class)
                {
                    rooted_traits.push(frozen_quote_trait(self.db, inst.key(self.db), site)?);
                }
                if let (QuoteNameSite::Expr(expr), QuoteNameClass::Qualified) =
                    (name.site, &name.class)
                {
                    let Some(previous) =
                        name_index.checked_sub(1).and_then(|i| checked.names.get(i))
                    else {
                        return Err(
                            FreezingProtocolFault::QualifiedConstantHasNoCheckedTrait.into()
                        );
                    };
                    let (
                        QuoteNameSite::Type(trait_site),
                        QuoteNameClass::Rooted(PathRes::Trait(_)),
                    ) = (previous.site, &previous.class)
                    else {
                        return Err(
                            FreezingProtocolFault::QualifiedConstantNeedsARootedTrait.into()
                        );
                    };
                    let Partial::Present(Expr::Path(Partial::Present(path))) =
                        &quote.template.exprs(self.db)[expr]
                    else {
                        return Err(FreezingProtocolFault::QualifiedConstantHasNoPath.into());
                    };
                    if path.len(self.db) != 2
                        || !matches!(
                            path.kind(self.db),
                            PathKind::Ident { ident: Partial::Present(_), generic_args }
                                if generic_args.is_empty(self.db)
                        )
                    {
                        return Err(
                            FreezingProtocolFault::QualifiedConstantNeedsASimpleMember.into()
                        );
                    }
                    let Some(root) = path.segment(self.db, 0) else {
                        return Err(FreezingProtocolFault::QualifiedConstantHasNoRoot.into());
                    };
                    let PathKind::QualifiedType { type_, .. } = root.kind(self.db) else {
                        return Err(
                            FreezingProtocolFault::QualifiedConstantHasNoReceiverType.into()
                        );
                    };
                    let Some(receiver) = checked_primitive(type_) else {
                        return Err(
                            FreezingProtocolFault::QualifiedConstantNeedsAPrimitiveReceiver.into(),
                        );
                    };
                    name.member.function().ok_or(FreezingProtocolFault::QualifiedConstantOutsideAFunctionNeedsTransplantation)?;
                    qualified_consts.push(FrozenQuoteQualifiedConst {
                        member: name.member,
                        site: expr,
                        trait_site,
                        receiver,
                    });
                }
            }
            Some(FrozenTemplateKey {
                owner: self.frozen_quote_owner(func)?,
                expr: site.expr,
                source: FrozenTemplateSource::Quote,
                span: site
                    .expr
                    .span(body)
                    .resolve(self.db)
                    .map(|span| span.range.into()),
            })
        } else {
            None
        };
        let mut holes = Vec::new();
        for (slot, value) in code.holes(self.db).iter().enumerate() {
            let hole = match value.value(self.db) {
                SemConstValue::Structural {
                    ty,
                    handle: StructuralHandle::Code(inner),
                } => {
                    if !self.designated(*ty, MetaItem::Code) {
                        return Err(ConstructionError::Authority);
                    }
                    FrozenCodeHole::Code(seen[inner], hole_spans[slot].clone())
                }
                _ => {
                    let field_type = match value.value(self.db) {
                        SemConstValue::Structural {
                            handle:
                                StructuralHandle::FieldType {
                                    owner: field_owner,
                                    variant_index,
                                    index,
                                },
                            ..
                        } if *field_owner == self.owner => {
                            match self.reflected_field_type(*variant_index, *index) {
                                Some(ty) => self.freeze_field_type(ty, budget)?,
                                None => None,
                            }
                        }
                        _ => None,
                    };
                    FrozenCodeHole::Value(
                        Box::new(self.freeze_node(*value, owner, budget, 0)?),
                        field_type,
                    )
                }
            };
            holes.push(hole);
        }
        nodes[index] = FrozenCodeNode {
            holes,
            rooted_funcs,
            rooted_constructors,
            rooted_traits,
            qualified_consts,
            impl_trait_type_arguments,
            locals,
            cast_primitives,
            local_types,
            param_uses,
            effect_uses,
            generic_uses,
            self_uses,
            self_record_inits,
            open_uses,
            free_self_uses,
            function_types,
            where_predicates,
            assoc_const_primitives,
            assoc_type_values,
            ..FrozenCodeNode::empty(template)
        };
        // Hole edges were charged before allocation, types by their shared freezer.
        for _ in 0..nodes[index]
            .metadata_nodes()
            .saturating_sub(nodes[index].holes.len())
        {
            charge_frozen_node(budget)?;
        }
        Ok(())
    }
}
