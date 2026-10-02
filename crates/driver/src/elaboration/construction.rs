//! Freeze checked Fe provider values before generated HIR is published.
//! Descriptor identity and owner authority are checked in the authored view.

use std::collections::{BTreeMap, HashMap};

use hir::{
    analysis::name_resolution::PathRes,
    analysis::{
        semantic::{QuotedCode, SemConstId, SemConstValue, StructuralHandle},
        ty::{
            adt_def::{AdtDef, AdtRef},
            trait_resolution::PredicateListId,
            ty_check::{
                QuoteBinder, QuoteNameClass, QuoteNameSite, QuoteNameUse, QuoteSignatureSite,
                check_func_body,
            },
            ty_def::{PrimTy as SemPrimTy, TyBase, TyData, TyId},
            ty_lower::lower_hir_ty,
        },
    },
    core::meta::{
        FrozenCode, FrozenCodeHole, FrozenCodeNode, FrozenHandleKind, FrozenItemLocator,
        FrozenMetaValue, FrozenMetaVariantCase, FrozenOwner, FrozenQuoteEffectUse,
        FrozenQuoteFieldType, FrozenQuoteFunction, FrozenQuoteFunctionTypes, FrozenQuoteGenericUse,
        FrozenQuoteLocal, FrozenQuoteLocalType, FrozenQuoteNominal, FrozenQuoteOpenUse,
        FrozenQuoteParamUse, FrozenQuoteQualifiedConst, FrozenQuoteSelfUse,
        FrozenQuoteSignatureType, FrozenQuoteTrait, FrozenQuoteWherePredicate, FrozenTemplateKey,
        FrozenTemplateSource, MetaItem,
    },
    hir_def::{
        CallableDef, Expr, Func, GenericArg, GenericParam, Partial, PathKind, QuoteFragment,
        QuoteFunc, QuoteItem, QuoteWherePredicate, Trait, TypeBound, TypeId, TypeKind,
        scope_graph::ScopeId,
    },
    span::LazySpan,
};

use super::RecordInput;
use crate::DriverDataBase;

#[derive(Debug, PartialEq, Eq)]
pub enum ConstructionError {
    Protocol(&'static str),
    Authority,
    Limit {
        name: &'static str,
        actual: String,
        maximum: String,
    },
}

fn protocol(message: &'static str) -> ConstructionError {
    ConstructionError::Protocol(message)
}

pub(super) fn frozen_quote_trait<'db>(
    db: &'db DriverDataBase,
    trait_: Trait<'db>,
    site: u32,
) -> Result<FrozenQuoteTrait, ConstructionError> {
    let scope = trait_.scope();
    let file = trait_
        .span()
        .resolve(db)
        .and_then(|span| span.file.url(db))
        .ok_or(protocol("quoted trait file is missing"))?;
    let full_path = scope
        .pretty_path(db)
        .ok_or(protocol("quoted trait path is missing"))?;
    let module = scope
        .top_mod(db)
        .scope()
        .pretty_path(db)
        .ok_or(protocol("quoted trait module path is missing"))?;
    let path = full_path
        .strip_prefix(&format!("{module}::"))
        .ok_or(protocol("quoted trait is outside its defining module"))?
        .to_owned();
    Ok(FrozenQuoteTrait {
        site,
        ingot: scope.ingot(db).base(db).clone(),
        path,
        owner: FrozenOwner {
            file,
            path: full_path,
        },
    })
}

fn frozen_quote_nominal<'db>(
    db: &'db DriverDataBase,
    adt: AdtDef<'db>,
) -> Result<FrozenQuoteNominal, ConstructionError> {
    let scope = adt.scope(db);
    let full_path = scope
        .pretty_path(db)
        .ok_or(protocol("quoted nominal type path is missing"))?;
    let module = scope
        .top_mod(db)
        .scope()
        .pretty_path(db)
        .ok_or(protocol("quoted nominal type module path is missing"))?;
    let path = full_path
        .strip_prefix(&format!("{module}::"))
        .ok_or(protocol(
            "quoted nominal type is outside its defining module",
        ))?
        .to_owned();
    let file = match adt.adt_ref(db) {
        AdtRef::Struct(item) => item.span().resolve(db),
        AdtRef::Enum(item) => item.span().resolve(db),
    }
    .and_then(|span| span.file.url(db))
    .ok_or(protocol("quoted nominal type file is missing"))?;
    Ok(FrozenQuoteNominal {
        ingot: scope.ingot(db).base(db).clone(),
        path,
        owner: FrozenOwner {
            file,
            path: full_path,
        },
    })
}

/// Freeze a checked type without resolving its spelling again. Normal generic
/// parameters retain their declaration namespace and index, not their name.
pub(super) fn freeze_checked_quote_type<'db>(
    db: &'db DriverDataBase,
    ty: TyId<'db>,
    func: Func<'db>,
) -> Result<Option<FrozenQuoteSignatureType>, ConstructionError> {
    let (base, args) = ty.decompose_ty_app(db);
    let mut arguments = Vec::with_capacity(args.len());
    for arg in args {
        let Some(arg) = freeze_checked_quote_type(db, *arg, func)? else {
            return Ok(None);
        };
        arguments.push(arg);
    }
    let frozen = match base.data(db) {
        TyData::TyParam(param) if arguments.is_empty() => {
            let Some(ScopeId::GenericParam(owner, index)) = param.declaration_scope(db) else {
                return Ok(None);
            };
            if owner == func.into() {
                FrozenQuoteSignatureType::FunctionGeneric(u32::from(index))
            } else if func.is_associated_func(db) && func.scope().parent_item(db) == Some(owner) {
                FrozenQuoteSignatureType::OwnerGeneric(u32::from(index))
            } else {
                return Ok(None);
            }
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
        TyData::TyBase(TyBase::Adt(adt)) => FrozenQuoteSignatureType::Nominal {
            root: frozen_quote_nominal(db, *adt)?,
            arguments_given: !arguments.is_empty(),
            arguments,
        },
        _ => return Ok(None),
    };
    Ok(Some(frozen))
}

fn freeze_quote_signature_type<'a, 'db>(
    db: &'db DriverDataBase,
    ty: TypeId<'db>,
    names: &mut impl Iterator<Item = &'a QuoteNameUse<'db>>,
    generics: &[GenericParam<'db>],
    generic_item: u32,
    checked_primitive: &impl Fn(TypeId<'db>) -> Option<SemPrimTy>,
) -> Result<Option<FrozenQuoteSignatureType>, ConstructionError>
where
    'db: 'a,
{
    match ty.data(db) {
        TypeKind::Mode(mode, Partial::Present(inner)) => Ok(freeze_quote_signature_type(
            db,
            *inner,
            names,
            generics,
            generic_item,
            checked_primitive,
        )?
        .map(|inner| FrozenQuoteSignatureType::Mode(*mode, Box::new(inner)))),
        TypeKind::Ptr(Partial::Present(inner)) => Ok(freeze_quote_signature_type(
            db,
            *inner,
            names,
            generics,
            generic_item,
            checked_primitive,
        )?
        .map(|inner| FrozenQuoteSignatureType::Ptr(Box::new(inner)))),
        TypeKind::Tuple(elements) => {
            let mut frozen = Vec::new();
            for element in elements.data(db) {
                let Some(element) = element.to_opt() else {
                    return Ok(None);
                };
                let Some(element) = freeze_quote_signature_type(
                    db,
                    element,
                    names,
                    generics,
                    generic_item,
                    checked_primitive,
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
                    .ok_or(protocol("quoted signature path segment is missing"))?;
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
                    let Some(frozen) = freeze_quote_signature_type(
                        db,
                        ty,
                        names,
                        generics,
                        generic_item,
                        checked_primitive,
                    )?
                    else {
                        return Ok(None);
                    };
                    arguments.push(frozen);
                }
            }
            let name_use = names
                .next()
                .ok_or(protocol("quoted signature type has no checked name"))?;
            if !matches!(name_use.site, QuoteNameSite::Type(_)) {
                return Err(protocol("quoted signature name changed site"));
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
                    return Err(protocol("quoted generic signature binding changed"));
                }
                return Ok(Some(FrozenQuoteSignatureType::FunctionGeneric(
                    index as u32,
                )));
            }
            if let QuoteNameClass::Rooted(PathRes::Ty(selected)) = &name_use.class
                && let TyData::TyBase(TyBase::Adt(adt)) = selected.base_ty(db).data(db)
            {
                return Ok(Some(FrozenQuoteSignatureType::Nominal {
                    root: frozen_quote_nominal(db, *adt)?,
                    arguments,
                    arguments_given,
                }));
            }
            if let QuoteNameClass::Rooted(PathRes::Trait(selected)) = &name_use.class {
                let QuoteNameSite::Type(site) = name_use.site else {
                    return Err(protocol("quoted trait signature name changed site"));
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

fn limit(
    name: &'static str,
    actual: impl std::fmt::Display,
    maximum: impl std::fmt::Display,
) -> ConstructionError {
    ConstructionError::Limit {
        name,
        actual: actual.to_string(),
        maximum: maximum.to_string(),
    }
}

impl std::fmt::Display for ConstructionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Protocol(message) => f.write_str(message),
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
            .ok_or(protocol("quote owner locator is missing"))
    }

    pub(crate) fn frozen_owner(&self) -> Result<FrozenOwner, ConstructionError> {
        let file = self
            .owner
            .span()
            .resolve(self.db)
            .and_then(|span| span.file.url(self.db))
            .ok_or(protocol("generation owner file is missing"))?;
        let path = self
            .owner
            .scope()
            .pretty_path(self.db)
            .ok_or(protocol("generation owner path is missing"))?;
        Ok(FrozenOwner { file, path })
    }

    pub(crate) fn frozen_const_param_types(
        &self,
    ) -> Result<Vec<Option<SemPrimTy>>, ConstructionError> {
        self.owner
            .hir_generic_params(self.db)
            .data(self.db)
            .iter()
            .map(|param| match param {
                GenericParam::Type(_) => Ok(None),
                GenericParam::Const(param) => {
                    let ty = param
                        .ty
                        .to_opt()
                        .ok_or(protocol("target const parameter has no type"))?;
                    let checked = lower_hir_ty(
                        self.db,
                        ty,
                        self.owner.scope(),
                        hir::analysis::ty::trait_resolution::PredicateListId::empty_list(self.db),
                    );
                    let TyData::TyBase(TyBase::Prim(primitive)) =
                        checked.base_ty(self.db).data(self.db)
                    else {
                        return Err(protocol("target const parameter type is not scalar"));
                    };
                    Ok(Some(*primitive))
                }
            })
            .collect()
    }

    fn meta_item(&self, ty: TyId<'db>) -> Option<MetaItem> {
        MetaItem::ALL
            .into_iter()
            .find(|item| self.designated(ty, *item))
    }

    /// Freeze a checked provider value before publishing any generated items.
    /// Only the designated generation protocol and this record's structural
    /// handles may cross into the output phase.
    pub fn freeze_value(
        &self,
        value: SemConstId<'db>,
    ) -> Result<FrozenMetaValue, ConstructionError> {
        let owner = self.frozen_owner()?;
        let mut nodes = 0;
        self.freeze_node(value, &owner, &mut nodes, 0)
    }

    fn freeze_node(
        &self,
        value: SemConstId<'db>,
        owner: &FrozenOwner,
        nodes: &mut usize,
        depth: usize,
    ) -> Result<FrozenMetaValue, ConstructionError> {
        *nodes += 1;
        if *nodes > 65536 {
            return Err(limit("frozen meta nodes", *nodes, 65536));
        }
        if depth > 128 {
            return Err(limit("frozen meta depth", depth, 128));
        }
        let freeze = |value, nodes: &mut usize| self.freeze_node(value, owner, nodes, depth + 1);
        match value.value(self.db) {
            SemConstValue::Unit => Ok(FrozenMetaValue::Unit),
            SemConstValue::Scalar { ty, value } => {
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
                let (item, kind, actual_owner) = match handle {
                    StructuralHandle::Record(actual_owner) => {
                        (MetaItem::Record, FrozenHandleKind::Record, actual_owner)
                    }
                    StructuralHandle::Field {
                        owner: actual_owner,
                        index,
                    } => (
                        MetaItem::Field,
                        FrozenHandleKind::Field(*index),
                        actual_owner,
                    ),
                    StructuralHandle::FieldType {
                        owner: actual_owner,
                        index,
                    } => (
                        MetaItem::FieldType,
                        FrozenHandleKind::FieldType(*index),
                        actual_owner,
                    ),
                    StructuralHandle::Evidence(_) => return Err(ConstructionError::Authority),
                    StructuralHandle::CodeNode { .. } => return Err(ConstructionError::Authority),
                    StructuralHandle::Code(code) => {
                        if !self.designated(*ty, MetaItem::Code) {
                            return Err(ConstructionError::Authority);
                        }
                        return self
                            .freeze_code(*code, owner, nodes)
                            .map(FrozenMetaValue::Code);
                    }
                };
                if *actual_owner != self.owner || !self.designated(*ty, item) {
                    return Err(ConstructionError::Authority);
                }
                if matches!(kind, FrozenHandleKind::Field(index) | FrozenHandleKind::FieldType(index) if index >= self.fields.len())
                {
                    return Err(ConstructionError::Authority);
                }
                Ok(FrozenMetaValue::Structural {
                    item,
                    owner: owner.clone(),
                    kind,
                })
            }
            SemConstValue::Tuple { ty, elems } => {
                if !ty.is_tuple(self.db) {
                    return Err(protocol("invalid core meta tuple"));
                }
                elems
                    .iter()
                    .map(|value| freeze(*value, nodes))
                    .collect::<Result<Vec<_>, _>>()
                    .map(FrozenMetaValue::Tuple)
            }
            SemConstValue::Array { ty, elems } => {
                if !ty.is_array(self.db) {
                    return Err(protocol("invalid core meta array"));
                }
                elems
                    .iter()
                    .map(|value| freeze(*value, nodes))
                    .collect::<Result<Vec<_>, _>>()
                    .map(FrozenMetaValue::Array)
            }
            SemConstValue::Struct { ty, fields } => {
                let item = self
                    .meta_item(*ty)
                    .ok_or(protocol("unexpected core meta record"))?;
                let TyData::TyBase(TyBase::Adt(adt)) = ty.base_ty(self.db).data(self.db) else {
                    return Err(protocol("invalid core meta record type"));
                };
                let AdtRef::Struct(definition) = adt.adt_ref(self.db) else {
                    return Err(protocol("expected core meta record definition"));
                };
                let definitions = definition.hir_fields(self.db);
                let definitions = definitions.data(self.db);
                if definitions.len() != fields.len() {
                    return Err(protocol("invalid core meta record fields"));
                }
                let mut named = BTreeMap::new();
                for (definition, value) in definitions.iter().zip(fields) {
                    let name = definition
                        .name
                        .to_opt()
                        .ok_or(protocol("unnamed core meta field"))?
                        .data(self.db)
                        .clone();
                    if named.insert(name, freeze(*value, nodes)?).is_some() {
                        return Err(protocol("duplicate core meta field"));
                    }
                }
                Ok(FrozenMetaValue::Record {
                    item,
                    fields: named,
                })
            }
            SemConstValue::Enum {
                ty,
                variant,
                fields,
            } => {
                let item = self
                    .meta_item(*ty)
                    .ok_or(protocol("unexpected core meta variant"))?;
                let TyData::TyBase(TyBase::Adt(adt)) = ty.base_ty(self.db).data(self.db) else {
                    return Err(protocol("invalid core meta variant type"));
                };
                let AdtRef::Enum(definition) = adt.adt_ref(self.db) else {
                    return Err(protocol("expected core meta enum definition"));
                };
                let name = definition
                    .variants(self.db)
                    .nth(usize::from(variant.0))
                    .and_then(|variant| variant.name(self.db))
                    .ok_or(protocol("invalid core meta enum variant"))?
                    .data(self.db)
                    .clone();
                let fields = fields
                    .iter()
                    .map(|value| freeze(*value, nodes))
                    .collect::<Result<Vec<_>, _>>()?;
                // The evaluated discriminant already selected a variant of the
                // designated core enum. Freeze that case before source order is lost.
                let case = if item == MetaItem::DeclarationName {
                    match name.as_str() {
                        "Literal" => FrozenMetaVariantCase::DeclarationNameLiteral,
                        "FieldPrefix" => FrozenMetaVariantCase::DeclarationNameFieldPrefix,
                        _ => return Err(protocol("unexpected declaration name variant")),
                    }
                } else {
                    FrozenMetaVariantCase::Other
                };
                Ok(FrozenMetaValue::Variant {
                    item,
                    name,
                    case,
                    fields,
                })
            }
            SemConstValue::Description(_) | SemConstValue::TypeLevel { .. } => {
                Err(protocol("type-level values are not generated declarations"))
            }
        }
    }

    fn designated(&self, ty: TyId<'db>, item: MetaItem) -> bool {
        self.meta_type(item)
            .is_some_and(|expected| expected.base_ty(self.db) == ty.base_ty(self.db))
    }

    fn freeze_field_type(&self, ty: TyId<'db>, depth: usize) -> Option<FrozenQuoteFieldType> {
        if depth > 128 {
            return None;
        }
        if let TyData::TyParam(param) = ty.data(self.db) {
            return (param.owner == self.owner.scope())
                .then(|| FrozenQuoteFieldType::TargetParam(param.original_idx(self.db)));
        }
        let (base, args) = ty.decompose_ty_app(self.db);
        match base.data(self.db) {
            TyData::TyBase(TyBase::Prim(SemPrimTy::Tuple(arity))) if args.len() == *arity => args
                .iter()
                .map(|arg| self.freeze_field_type(*arg, depth + 1))
                .collect::<Option<Vec<_>>>()
                .map(FrozenQuoteFieldType::Tuple),
            TyData::TyBase(TyBase::Prim(SemPrimTy::Ptr)) if args.len() == 1 => self
                .freeze_field_type(args[0], depth + 1)
                .map(|inner| FrozenQuoteFieldType::Ptr(Box::new(inner))),
            TyData::TyBase(TyBase::Prim(primitive)) if args.is_empty() => {
                Some(FrozenQuoteFieldType::Primitive(*primitive))
            }
            _ => None,
        }
    }

    fn freeze_code(
        &self,
        root: QuotedCode<'db>,
        owner: &FrozenOwner,
        budget: &mut usize,
    ) -> Result<FrozenCode, ConstructionError> {
        let mut nodes = Vec::new();
        let mut seen = HashMap::new();
        let root = self.freeze_code_node(root, owner, budget, &mut nodes, &mut seen)?;
        Ok(FrozenCode { root, nodes })
    }

    fn freeze_code_node(
        &self,
        code: QuotedCode<'db>,
        owner: &FrozenOwner,
        budget: &mut usize,
        nodes: &mut Vec<FrozenCodeNode>,
        seen: &mut HashMap<QuotedCode<'db>, usize>,
    ) -> Result<usize, ConstructionError> {
        if let Some(index) = seen.get(&code) {
            return Ok(*index);
        }
        *budget += 1;
        if *budget > 65536 {
            return Err(limit("frozen meta nodes", *budget, 65536));
        }
        let index = nodes.len();
        seen.insert(code, index);
        nodes.push(FrozenCodeNode::empty(None));
        let mut rooted_funcs = Vec::new();
        let mut rooted_traits = Vec::new();
        let mut qualified_consts = Vec::new();
        let mut impl_trait_type_arguments = None;
        let mut locals: Vec<FrozenQuoteLocal> = Vec::new();
        let mut local_types = Vec::new();
        let mut param_uses = Vec::new();
        let mut effect_uses = Vec::new();
        let mut generic_uses = Vec::new();
        let mut self_uses = Vec::new();
        let mut self_record_inits = Vec::new();
        let mut open_uses = Vec::new();
        let mut free_self_uses = Vec::new();
        let mut function_types = Vec::new();
        let mut assoc_const_primitives = Vec::new();
        let mut assoc_type_values = Vec::new();
        let template = if let Some(site) = code.site(self.db) {
            let func = site
                .owner
                .containing_func(self.db)
                .ok_or(protocol("quote owner is not a function"))?;
            let body = func
                .body(self.db)
                .ok_or(protocol("quote owner body is missing"))?;
            if site.source == FrozenTemplateSource::CheckedBody {
                if site.owner != body
                    || site.expr != body.expr(self.db)
                    || !code.holes(self.db).is_empty()
                {
                    return Err(protocol("invalid checked-body code origin"));
                }
                let frozen = super::read_back::read_back(self.db, func)
                    .map_err(|_| protocol("checked-body code no longer reads back"))?;
                nodes[index] = frozen.nodes[frozen.root].clone();
                return Ok(index);
            }
            let Partial::Present(Expr::Quote(quote)) = &body.exprs(self.db)[site.expr] else {
                return Err(protocol("quote site no longer names a quote"));
            };
            let checked_primitive = |ty| {
                let resolved = lower_hir_ty(
                    self.db,
                    ty,
                    func.scope(),
                    PredicateListId::empty_list(self.db),
                );
                match resolved.base_ty(self.db).data(self.db) {
                    TyData::TyBase(TyBase::Prim(primitive)) => Some(*primitive),
                    _ => None,
                }
            };
            let (diagnostics, typed) = check_func_body(self.db, func);
            if !diagnostics.is_empty() {
                return Err(protocol("quote owner does not type check"));
            }
            let checked = typed
                .checked_quote(site.expr)
                .ok_or(protocol("quote has no checked name classification"))?;
            assoc_const_primitives = checked.assoc_const_primitives.clone();
            let checked_signature_type =
                |ty: TypeId<'db>,
                 signature: QuoteSignatureSite,
                 item: &QuoteFunc<'db>,
                 generic_item: u32| {
                    let mut names = checked.names.iter().filter(|use_| {
                        use_.signature == Some(signature)
                            && matches!(use_.site, QuoteNameSite::Type(_))
                    });
                    let frozen = freeze_quote_signature_type(
                        self.db,
                        ty,
                        &mut names,
                        item.generic_params.data(self.db),
                        generic_item,
                        &checked_primitive,
                    )?;
                    if frozen.is_some() && names.next().is_some() {
                        return Err(protocol("quoted signature has extra checked names"));
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
            let mut freeze_function = |item: &QuoteFunc<'db>, index: u32| {
                let generic_item = *checked
                    .function_generic_items
                    .get(index as usize)
                    .ok_or(protocol("quoted function has no checked generic owner"))?;
                let params = item
                    .params
                    .to_opt()
                    .map(|params| {
                        params
                            .data(self.db)
                            .iter()
                            .enumerate()
                            .map(|(param_index, param)| {
                                param
                                    .ty
                                    .to_opt()
                                    .map(|ty| {
                                        checked_signature_type(
                                            ty,
                                            QuoteSignatureSite::Param {
                                                func: index,
                                                index: param_index as u32,
                                            },
                                            item,
                                            generic_item,
                                        )
                                    })
                                    .transpose()
                                    .map(Option::flatten)
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
                        effect
                            .key_ty
                            .to_opt()
                            .map(|ty| {
                                checked_signature_type(
                                    ty,
                                    QuoteSignatureSite::Effect {
                                        func: index,
                                        index: effect_index as u32,
                                    },
                                    item,
                                    generic_item,
                                )
                            })
                            .transpose()
                            .map(Option::flatten)
                    })
                    .collect::<Result<Vec<_>, ConstructionError>>()?;
                let result = item
                    .ret_ty
                    .map(|ty| {
                        checked_signature_type(
                            ty,
                            QuoteSignatureSite::Result { func: index },
                            item,
                            generic_item,
                        )
                    })
                    .transpose()?
                    .flatten();
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
                        GenericParam::Type(param) => param
                            .default_ty
                            .map(|ty| {
                                checked_signature_type(
                                    ty,
                                    QuoteSignatureSite::GenericDefault {
                                        func: index,
                                        param: param_index as u32,
                                    },
                                    item,
                                    generic_item,
                                )
                            })
                            .transpose()
                            .map(Option::flatten),
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
                                QuoteWherePredicate::Hole(_) => Err(protocol(
                                    "quoted method where fragment needs checked reification",
                                )),
                                QuoteWherePredicate::Bound(_) => unreachable!(),
                            };
                        };
                        let ty = predicate
                            .ty
                            .to_opt()
                            .ok_or(protocol("quoted method where bound type is missing"))?;
                        let ty = checked_signature_type(
                            ty,
                            QuoteSignatureSite::WhereType {
                                func: index,
                                predicate: predicate_index as u32,
                            },
                            item,
                            generic_item,
                        )?
                        .ok_or(protocol(
                            "quoted method where bound type needs checked reification",
                        ))?;
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
                    .filter(|local| local.func == index)
                {
                    let ty = checked_signature_type(
                        local.ty,
                        QuoteSignatureSite::LocalType {
                            func: index,
                            stmt: local.stmt,
                        },
                        item,
                        generic_item,
                    )?
                    .ok_or(protocol("quoted local type needs checked reification"))?;
                    local_types.push(FrozenQuoteLocalType {
                        func: index,
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
                            let mut names = checked.names.iter().filter(|name| {
                                name.signature
                                    == Some(QuoteSignatureSite::AssocType {
                                        index: index as u32,
                                    })
                                    && matches!(name.site, QuoteNameSite::Type(_))
                            });
                            let frozen = freeze_quote_signature_type(
                                self.db,
                                ty,
                                &mut names,
                                &[],
                                0,
                                &checked_primitive,
                            )?;
                            if frozen.is_some() && names.next().is_some() {
                                return Err(protocol(
                                    "quoted associated type has extra checked names",
                                ));
                            }
                            Ok(frozen)
                        })
                        .collect::<Result<Vec<_>, ConstructionError>>()?;
                }
                for (index, item) in items
                    .iter()
                    .filter_map(|item| match item {
                        QuoteItem::Func(function) => Some(function),
                        _ => None,
                    })
                    .enumerate()
                {
                    freeze_function(item, index as u32)?;
                }
                if let [QuoteItem::Impl(item)] = items.as_slice() {
                    if let Some(Partial::Present(trait_ref)) = item.trait_ref
                        && let Partial::Present(path) = trait_ref.path(self.db)
                    {
                        let args = path.generic_args(self.db);
                        if args.is_given(self.db) {
                            let arguments = args
                                .data(self.db)
                                .iter()
                                .enumerate()
                                .map(|(arg_index, arg)| {
                                    let GenericArg::Type(arg) = arg else {
                                        return Err(protocol(
                                            "quoted impl trait argument needs a checked type",
                                        ));
                                    };
                                    let ty = arg.ty.to_opt().ok_or(protocol(
                                        "quoted impl trait type argument is missing",
                                    ))?;
                                    let signature = QuoteSignatureSite::ImplTraitArgument {
                                        index: arg_index as u32,
                                    };
                                    let mut names = checked.names.iter().filter(|name| {
                                        name.signature == Some(signature)
                                            && matches!(name.site, QuoteNameSite::Type(_))
                                    });
                                    let frozen = freeze_quote_signature_type(
                                        self.db,
                                        ty,
                                        &mut names,
                                        &[],
                                        u32::MAX,
                                        &checked_primitive,
                                    )?
                                    .ok_or(protocol(
                                        "quoted impl trait argument needs checked reification",
                                    ))?;
                                    if names.next().is_some() {
                                        return Err(protocol(
                                            "quoted impl trait argument has extra checked names",
                                        ));
                                    }
                                    Ok(frozen)
                                })
                                .collect::<Result<Vec<_>, ConstructionError>>()?;
                            impl_trait_type_arguments = Some(arguments);
                        }
                    }
                    let mut function_index = 0;
                    for child in &item.items {
                        if let QuoteItem::Func(method) = child {
                            freeze_function(method, function_index)?;
                            function_index += 1;
                        }
                    }
                }
            }
            for (name_index, name) in checked.names.iter().enumerate() {
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
                    return Err(protocol(
                        "quoted const where local or value use needs checked reification",
                    ));
                }
                match (name.site, &name.class) {
                    (QuoteNameSite::Pat(binder), QuoteNameClass::Binds) => {
                        if locals.iter().any(|local| local.binder == binder) {
                            return Err(protocol("quote binder is classified twice"));
                        }
                        let func = name.func.ok_or(protocol(
                            "quoted local binder outside a function needs transplantation",
                        ))?;
                        locals.push(FrozenQuoteLocal {
                            func,
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
                            .ok_or(protocol("quote local use has no binder"))?;
                        if name.func != Some(local.func) {
                            return Err(protocol("quoted local use changed function owner"));
                        }
                        local.uses.push(expr);
                    }
                    (
                        QuoteNameSite::Expr(expr),
                        QuoteNameClass::Bound(QuoteBinder::Param { func, index }),
                    ) => {
                        if name.func != Some(*func) {
                            return Err(protocol("quoted parameter use changed function owner"));
                        }
                        param_uses.push(FrozenQuoteParamUse {
                            site: expr,
                            func: *func,
                            index: *index,
                        });
                    }
                    (
                        QuoteNameSite::Expr(expr),
                        QuoteNameClass::Bound(QuoteBinder::EffectParam { func, index }),
                    ) => {
                        if name.func != Some(*func)
                            || !matches!(
                                &quote.template.exprs(self.db)[expr],
                                Partial::Present(Expr::Path(Partial::Present(path)))
                                    if path.as_ident(self.db).is_some()
                            )
                        {
                            return Err(protocol("quoted effect use needs a simple bound path"));
                        }
                        effect_uses.push(FrozenQuoteEffectUse {
                            site: expr,
                            func: *func,
                            index: *index,
                        });
                    }
                    (
                        QuoteNameSite::Expr(expr),
                        QuoteNameClass::Bound(QuoteBinder::Generic { item, index }),
                    ) => {
                        let func = name.func.ok_or(protocol(
                            "quoted generic body use outside a function needs transplantation",
                        ))?;
                        if checked.function_generic_items.get(func as usize) != Some(item)
                            || function_types
                                .get(func as usize)
                                .and_then(|types| types.generic_const_types.get(*index as usize))
                                .copied()
                                .flatten()
                                .is_none()
                        {
                            return Err(protocol(
                                "quoted generic body use needs checked reification",
                            ));
                        }
                        let simple_path = matches!(
                            &quote.template.exprs(self.db)[expr],
                            Partial::Present(Expr::Path(Partial::Present(path)))
                                if path.as_ident(self.db).is_some()
                        );
                        if !simple_path {
                            return Err(protocol("quoted generic body use needs a simple path"));
                        }
                        generic_uses.push(FrozenQuoteGenericUse {
                            site: expr,
                            func,
                            index: *index,
                            where_predicate: match name.signature {
                                Some(QuoteSignatureSite::WhereConst {
                                    func: owner,
                                    predicate,
                                }) if owner == func => Some(predicate),
                                _ => None,
                            },
                        });
                    }
                    (QuoteNameSite::Expr(expr), QuoteNameClass::SelfName) => {
                        if matches!(
                            &quote.template.exprs(self.db)[expr],
                            Partial::Present(Expr::RecordInit(Partial::Present(path), _))
                                if path.is_self_ty(self.db)
                        ) {
                            self_record_inits.push(FrozenQuoteSelfUse {
                                func: name.func.ok_or(protocol(
                                    "quoted Self construction outside a method needs transplantation",
                                ))?,
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
                            return Err(protocol("quoted Self path needs checked transplantation"));
                        }
                        if let Some(func) = name.func {
                            self_uses.push(FrozenQuoteSelfUse { func, site: expr });
                        } else if matches!(&quote.fragment, QuoteFragment::Expr) {
                            free_self_uses.push(expr);
                        } else {
                            return Err(protocol(
                                "quoted self outside a method needs transplantation",
                            ));
                        }
                    }
                    (QuoteNameSite::Expr(expr), QuoteNameClass::Open(index))
                        if name.func.is_none()
                            && matches!(&quote.fragment, QuoteFragment::Expr) =>
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
                        return Err(protocol("quoted function root has no authored function"));
                    };
                    let scope = res
                        .as_scope(self.db)
                        .ok_or(protocol("quote function scope is missing"))?;
                    let path = res
                        .pretty_path(self.db)
                        .ok_or(protocol("quote function path is missing"))?;
                    let module = scope
                        .top_mod(self.db)
                        .scope()
                        .pretty_path(self.db)
                        .ok_or(protocol("quote function module path is missing"))?;
                    let path = path
                        .strip_prefix(&format!("{module}::"))
                        .ok_or(protocol("quote function is outside its defining module"))?
                        .to_owned();
                    if name.func.is_none()
                        && name.assoc_const.is_none()
                        && !matches!(quote.fragment, QuoteFragment::Expr)
                    {
                        return Err(protocol(
                            "quoted rooted function outside a function needs transplantation",
                        ));
                    }
                    let (generics, generic_item) = if let Some(func_index) = name.func {
                        let function = match &quote.fragment {
                            QuoteFragment::Items(items) => {
                                if let [QuoteItem::Impl(implementation)] = items.as_slice() {
                                    implementation
                                        .items
                                        .iter()
                                        .filter_map(|item| match item {
                                            QuoteItem::Func(function) => Some(function),
                                            _ => None,
                                        })
                                        .nth(func_index as usize)
                                } else {
                                    items
                                        .iter()
                                        .filter_map(|item| match item {
                                            QuoteItem::Func(function) => Some(function),
                                            _ => None,
                                        })
                                        .nth(func_index as usize)
                                }
                            }
                            QuoteFragment::Expr | QuoteFragment::Where(_) => None,
                        }
                        .ok_or(protocol("quoted function generic owner is missing"))?;
                        let generic_item = *checked
                            .function_generic_items
                            .get(func_index as usize)
                            .ok_or(protocol("quoted function generic identity is missing"))?;
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
                                    .ok_or(protocol("quoted function path segment is missing"))?;
                                if segment.generic_args(self.db).is_given(self.db) {
                                    return Err(protocol(
                                        "quoted function intermediate type arguments need reification",
                                    ));
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
                                            return Err(protocol(
                                                "quoted function argument needs a type",
                                            ));
                                        };
                                        let Some(ty) = arg.ty.to_opt() else {
                                            return Err(protocol(
                                                "quoted function type argument is missing",
                                            ));
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
                                            &checked_primitive,
                                        )?
                                        .ok_or(protocol(
                                            "quoted function type argument needs checked reification",
                                        ))?;
                                        if names.next().is_some() {
                                            return Err(protocol(
                                                "quoted function type argument has extra checked names",
                                            ));
                                        }
                                        Ok(frozen)
                                    })
                                    .collect::<Result<Vec<_>, ConstructionError>>()?;
                                Some(arguments)
                            } else {
                                None
                            }
                        }
                        _ => return Err(protocol("quoted function root changed shape")),
                    };
                    rooted_funcs.push(FrozenQuoteFunction {
                        func: name.func,
                        assoc_const: name.assoc_const,
                        type_arguments,
                        site: expr,
                        where_predicate: match name.signature {
                            Some(QuoteSignatureSite::WhereConst { func, predicate })
                                if name.func == Some(func) =>
                            {
                                Some(predicate)
                            }
                            _ => None,
                        },
                        ingot: scope.ingot(self.db).base(self.db).clone(),
                        path,
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
                        return Err(protocol("quoted qualified constant has no checked trait"));
                    };
                    let (
                        QuoteNameSite::Type(trait_site),
                        QuoteNameClass::Rooted(PathRes::Trait(_)),
                    ) = (previous.site, &previous.class)
                    else {
                        return Err(protocol("quoted qualified constant needs a rooted trait"));
                    };
                    let Partial::Present(Expr::Path(Partial::Present(path))) =
                        &quote.template.exprs(self.db)[expr]
                    else {
                        return Err(protocol("quoted qualified constant has no path"));
                    };
                    if path.len(self.db) != 2
                        || !matches!(
                            path.kind(self.db),
                            PathKind::Ident { ident: Partial::Present(_), generic_args }
                                if generic_args.is_empty(self.db)
                        )
                    {
                        return Err(protocol("quoted qualified constant needs a simple member"));
                    }
                    let Some(root) = path.segment(self.db, 0) else {
                        return Err(protocol("quoted qualified constant has no root"));
                    };
                    let PathKind::QualifiedType { type_, .. } = root.kind(self.db) else {
                        return Err(protocol("quoted qualified constant has no receiver type"));
                    };
                    let Some(receiver) = checked_primitive(type_) else {
                        return Err(protocol(
                            "quoted qualified constant needs a primitive receiver",
                        ));
                    };
                    qualified_consts.push(FrozenQuoteQualifiedConst {
                        func: name.func.ok_or(protocol(
                            "quoted qualified constant outside a function needs transplantation",
                        ))?,
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
        for value in code.holes(self.db) {
            let hole = match value.value(self.db) {
                SemConstValue::Structural {
                    ty,
                    handle: StructuralHandle::Code(inner),
                } => {
                    if !self.designated(*ty, MetaItem::Code) {
                        return Err(ConstructionError::Authority);
                    }
                    FrozenCodeHole::Code(self.freeze_code_node(*inner, owner, budget, nodes, seen)?)
                }
                _ => {
                    let field_type = match value.value(self.db) {
                        SemConstValue::Structural {
                            handle:
                                StructuralHandle::FieldType {
                                    owner: field_owner,
                                    index,
                                },
                            ..
                        } if *field_owner == self.owner => self
                            .fields
                            .get(*index)
                            .and_then(|field| self.freeze_field_type(field.ty, 0)),
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
            template,
            holes,
            rooted_funcs,
            selected_methods: Vec::new(),
            rooted_traits,
            qualified_consts,
            impl_trait_type_arguments,
            locals,
            cast_primitives: Vec::new(),
            local_types,
            record_types: Vec::new(),
            param_uses,
            effect_uses,
            generic_uses,
            self_uses,
            self_record_inits,
            open_uses,
            free_self_uses,
            function_types,
            assoc_const_primitives,
            assoc_type_values,
        };
        Ok(index)
    }
}
