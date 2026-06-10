//! Lowering support for `#[derive(..)]` on structs and enums.
//!
//! Each derived trait is synthesized as a real HIR `impl Trait for Item`
//! item via [`HirBuilder`], exactly like `#[event]` / `#[error]` desugaring.
//! Generated items carry [`DeriveDesugared`] origins so diagnostics and IDE
//! features can map them back to the annotated item.

use parser::ast::{self, prelude::*};
use salsa::Accumulator as _;

use super::{
    FileLowerCtxt,
    attr::{AttrForm, AttrRule, AttrTarget, has_named_attr, named_attr_specs, validate_attr_rules},
    hir_builder::{BodyBuilder, HirBuilder},
};
use crate::{
    hir_def::{
        BinOp, CompBinOp, Enum, Expr, ExprId, Field, FieldIndex, FuncModifiers, FuncParam,
        FuncParamMode, FuncParamName, GenericArg, GenericArgListId, GenericParam,
        GenericParamListId, IdentId, LogicalBinOp, MatchArm, Partial, PathId, PathKind,
        RecordPatField, Struct, TraitRefId, TypeBound, TypeGenericArg, TypeGenericParam, TypeId,
        TypeKind, VariantKind, Visibility, WhereClauseId, WherePredicate,
    },
    span::DeriveDesugared,
};

/// The targets on which a `#[default]` attribute is meaningful. Used both
/// here and by the item-level attribute validation in `item.rs`.
pub(super) const DEFAULT_ATTR_TARGETS: &str = "variants of `#[derive(Default)]` enums";

/// Derive-related errors accumulated during `#[derive(..)]` lowering /
/// validation.
#[salsa::accumulator]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeriveError {
    pub kind: DeriveErrorKind,
    pub file: common::file::File,
    /// Range of the primary span (attribute, argument, or generic params).
    pub primary_range: parser::TextRange,
    pub item_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DeriveErrorKind {
    /// `#[derive(..)]` on an item with const generic parameters is not yet
    /// supported (type generic parameters are).
    ConstGeneric { item_kind: &'static str },
    /// The derive argument is not a derivable trait (currently `Eq` and
    /// `Default`).
    UnknownTrait { name: String },
    /// The attribute has an invalid form, e.g. bare `#[derive]`,
    /// `#[derive = Eq]`, or `#[derive(Eq = 1)]`.
    InvalidForm,
    /// The same trait is requested more than once, e.g. `#[derive(Eq, Eq)]`.
    DuplicateTrait { name: String },
    /// `#[derive(..)]` cannot be combined with `#[event]` / `#[error]`.
    EventErrorStruct,
    /// `#[derive(Default)]` on an enum with no `#[default]` variant.
    MissingDefaultVariant,
    /// More than one variant is marked `#[default]`.
    MultipleDefaultVariants { first_variant_name: Option<String> },
}

/// The set of traits the compiler knows how to derive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DerivableTrait {
    Eq,
    Default,
}

pub(super) fn has_derive_attr(ast: &ast::Struct) -> bool {
    has_named_attr(ast.attr_list(), "derive")
}

pub(super) fn enum_has_derive_attr(ast: &ast::Enum) -> bool {
    has_named_attr(ast.attr_list(), "derive")
}

fn accumulate_error<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    item_name: &Option<String>,
    kind: DeriveErrorKind,
    primary_range: parser::TextRange,
) {
    let db = ctxt.db();
    DeriveError {
        kind,
        file: ctxt.top_mod().file(db),
        primary_range,
        item_name: item_name.clone(),
    }
    .accumulate(db);
}

/// Reports an error for `#[derive(..)]` combined with `#[event]` or
/// `#[error]`; derive desugaring is skipped for such structs.
pub(super) fn report_derive_on_event_or_error_struct<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    ast: &ast::Struct,
) {
    let item_name = ast.name().map(|n| n.text().to_string());
    for attr in derive_attrs(ast.attr_list()) {
        let range = attr.syntax().text_range();
        accumulate_error(ctxt, &item_name, DeriveErrorKind::EventErrorStruct, range);
    }
}

fn derive_attrs(attrs: Option<ast::AttrList>) -> Vec<ast::NormalAttr> {
    attrs
        .into_iter()
        .flat_map(|attrs| attrs.normal_attrs_named("derive").collect::<Vec<_>>())
        .collect()
}

/// Parses the `#[derive(..)]` attributes of an item into the list of traits
/// to derive, reporting malformed arguments, unknown traits, and duplicates.
fn parse_derive_traits<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    attrs: Option<ast::AttrList>,
    item_name: &Option<String>,
) -> Vec<DerivableTrait> {
    let mut traits = Vec::new();

    for attr in derive_attrs(attrs) {
        let attr_range = attr.syntax().text_range();

        // `#[derive = ..]` or bare `#[derive]`.
        let args = match attr.args() {
            Some(args) if attr.value().is_none() => args,
            _ => {
                accumulate_error(ctxt, item_name, DeriveErrorKind::InvalidForm, attr_range);
                continue;
            }
        };

        let mut has_arg = false;
        for arg in args {
            has_arg = true;
            let arg_range = arg.syntax().text_range();
            let (Some(key), None) = (arg.key(), arg.value()) else {
                accumulate_error(ctxt, item_name, DeriveErrorKind::InvalidForm, arg_range);
                continue;
            };

            let name = key.text().to_string();
            let derived = match name.as_str() {
                "Eq" => DerivableTrait::Eq,
                "Default" => DerivableTrait::Default,
                _ => {
                    accumulate_error(
                        ctxt,
                        item_name,
                        DeriveErrorKind::UnknownTrait { name },
                        arg_range,
                    );
                    continue;
                }
            };

            if traits.contains(&derived) {
                accumulate_error(
                    ctxt,
                    item_name,
                    DeriveErrorKind::DuplicateTrait { name },
                    arg_range,
                );
                continue;
            }
            traits.push(derived);
        }

        // `#[derive()]` derives nothing; treat it as malformed to avoid
        // silently accepting a no-op attribute.
        if !has_arg {
            accumulate_error(ctxt, item_name, DeriveErrorKind::InvalidForm, attr_range);
        }
    }

    traits
}

/// Generic-parameter information for a `#[derive(..)]` item, used to put
/// real generic params, generic args, and where clauses on the synthesized
/// impls. For `struct Pair<A, B>` the derived `Eq` impl is
///
/// ```text
/// impl<A, B> Eq for Pair<A, B> where A: Eq, B: Eq { .. }
/// ```
///
/// The bound shape follows Rust's derive: one `P: DerivedTrait` predicate
/// per type *parameter* (not per field type), emitted as a real where
/// clause on the impl. The item's own inline param bounds and where-clause
/// predicates are carried over so the impl self type stays well-formed.
struct DeriveGenerics<'db> {
    /// The impl's generic param list: the item's params with default types
    /// stripped (defaults are not meaningful on an impl). Empty for
    /// non-generic items.
    impl_params: GenericParamListId<'db>,
    /// The item's own where-clause predicates, copied onto each impl.
    inherited_preds: Vec<WherePredicate<'db>>,
    /// Each type param as a path type (`A`, `B`, ..); each derived impl
    /// adds `P: DerivedTrait` for every entry to its where clause.
    param_tys: Vec<TypeId<'db>>,
    /// `<A, B>` argument list applied to the item's name in the impl self
    /// type; `GenericArgListId::none` for non-generic items.
    self_ty_args: GenericArgListId<'db>,
}

impl<'db> DeriveGenerics<'db> {
    /// The where clause of a derived impl: the item's own predicates plus
    /// `P: trait_ref` for every type param `P`.
    fn where_clause(
        &self,
        ctxt: &FileLowerCtxt<'db>,
        trait_ref: TraitRefId<'db>,
    ) -> WhereClauseId<'db> {
        let mut preds = self.inherited_preds.clone();
        preds.extend(self.param_tys.iter().map(|ty| WherePredicate {
            ty: Partial::Present(*ty),
            bounds: vec![TypeBound::Trait(trait_ref)],
        }));
        WhereClauseId::new(ctxt.db(), preds)
    }
}

/// Builds the [`DeriveGenerics`] for an item with the given generic params
/// and where clause. Returns `None` (skipping impl generation) when the item
/// has const generic params — reported with a precise diagnostic — or when a
/// param name is missing due to a parser error (reported elsewhere).
fn derive_generics<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    item_name: &Option<String>,
    item_kind: &'static str,
    generic_params: GenericParamListId<'db>,
    where_clause: WhereClauseId<'db>,
    generic_params_range: impl FnOnce() -> parser::TextRange,
) -> Option<DeriveGenerics<'db>> {
    let db = ctxt.db();

    let mut impl_params = Vec::new();
    let mut param_tys = Vec::new();
    let mut args = Vec::new();
    for param in generic_params.data(db) {
        match param {
            GenericParam::Type(ty_param) => {
                let name = ty_param.name.to_opt()?;
                let ty = TypeId::new(
                    db,
                    TypeKind::Path(Partial::Present(PathId::from_ident(db, name))),
                );
                impl_params.push(GenericParam::Type(TypeGenericParam {
                    name: ty_param.name,
                    bounds: ty_param.bounds.clone(),
                    default_ty: None,
                }));
                param_tys.push(ty);
                args.push(GenericArg::Type(TypeGenericArg {
                    ty: Partial::Present(ty),
                }));
            }
            GenericParam::Const(_) => {
                accumulate_error(
                    ctxt,
                    item_name,
                    DeriveErrorKind::ConstGeneric { item_kind },
                    generic_params_range(),
                );
                return None;
            }
        }
    }

    let self_ty_args = if args.is_empty() {
        GenericArgListId::none(db)
    } else {
        GenericArgListId::given(db, args)
    };

    Some(DeriveGenerics {
        impl_params: GenericParamListId::new(db, impl_params),
        inherited_preds: where_clause.data(db).clone(),
        param_tys,
        self_ty_args,
    })
}

/// The impl self type: the item's name applied to its own generic params,
/// e.g. `Pair<A, B>` (or just `Pair` for non-generic items).
fn derive_self_ty<'db>(
    ctxt: &FileLowerCtxt<'db>,
    name: IdentId<'db>,
    generics: &DeriveGenerics<'db>,
) -> TypeId<'db> {
    let db = ctxt.db();
    let path = PathId::new(
        db,
        PathKind::Ident {
            ident: Partial::Present(name),
            generic_args: generics.self_ty_args,
        },
        None,
    );
    TypeId::new(db, TypeKind::Path(Partial::Present(path)))
}

/// Generates `impl` items for the traits requested via `#[derive(..)]` on
/// `struct_`. Must be called after the struct item itself has been lowered
/// (i.e. its item scope has been left), so the generated impls become
/// siblings of the struct in the scope graph.
pub(super) fn lower_derive_impls<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    ast: &ast::Struct,
    struct_: Struct<'db>,
) {
    let item_name = ast.name().map(|n| n.text().to_string());
    let traits = parse_derive_traits(ctxt, ast.attr_list(), &item_name);

    let db = ctxt.db();

    let Some(generics) = derive_generics(
        ctxt,
        &item_name,
        "struct",
        struct_.generic_params(db),
        struct_.where_clause(db),
        || {
            ast.generic_params()
                .map(|g| g.syntax().text_range())
                .unwrap_or_else(|| ast.syntax().text_range())
        },
    ) else {
        return;
    };

    let Some(struct_name_ident) = struct_.name(db).to_opt() else {
        // Parser error: missing name token. Avoid panics/cascades.
        return;
    };

    // Collect `(name, type)` for every field; skip generation when the
    // struct is not well-formed enough (parser errors are reported
    // elsewhere).
    let mut fields = Vec::new();
    for field in struct_.fields(db).data(db) {
        let (Some(name), Some(ty)) = (field.name.to_opt(), field.type_ref.to_opt()) else {
            return;
        };
        fields.push((name, ty));
    }

    let self_ty = derive_self_ty(ctxt, struct_name_ident, &generics);

    let derive_desugared = DeriveDesugared::Struct(parser::ast::AstPtr::new(ast));
    let mut builder = HirBuilder::new(ctxt, derive_desugared);

    for derived in traits {
        match derived {
            DerivableTrait::Eq => lower_derived_eq_impl(&mut builder, self_ty, &generics, &fields),
            DerivableTrait::Default => {
                lower_derived_default_impl(&mut builder, self_ty, &generics, &fields)
            }
        }
    }
}

/// The payload of an enum variant, with every name/type resolved.
#[derive(Debug, Clone)]
enum VariantPayload<'db> {
    Unit,
    Tuple(Vec<TypeId<'db>>),
    Record(Vec<(IdentId<'db>, TypeId<'db>)>),
}

#[derive(Debug, Clone)]
struct VariantInfo<'db> {
    name: IdentId<'db>,
    payload: VariantPayload<'db>,
}

/// Validates `#[default]` attributes on the variants of `ast`:
/// * on a `#[derive(Default)]` enum the attribute must be bare and unique
///   per variant;
/// * on any other enum the attribute is reported as misplaced.
fn validate_variant_default_attrs<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    ast: &ast::Enum,
    derives_default: bool,
) {
    let Some(variants) = ast.variants() else {
        return;
    };
    let rule = if derives_default {
        AttrRule::supported("default", AttrForm::Bare, "`#[default]`")
    } else {
        AttrRule::unsupported("default", DEFAULT_ATTR_TARGETS)
    };
    for variant in variants {
        let target = AttrTarget::new("variant", variant.name().map(|n| n.text().to_string()));
        validate_attr_rules(ctxt, variant.attr_list(), target, &[rule]);
    }
}

/// Reports `#[default]` markers on the variants of an enum without any
/// `#[derive(..)]` attribute; the marker only means something on
/// `#[derive(Default)]` enums.
pub(super) fn report_misplaced_default_attrs<'db>(ctxt: &mut FileLowerCtxt<'db>, ast: &ast::Enum) {
    validate_variant_default_attrs(ctxt, ast, false);
}

/// Finds the variant marked `#[default]`, reporting a missing marker and any
/// extra markers. Returns the index of the first marked variant so partial
/// code can still be generated alongside the diagnostics.
fn resolve_default_variant<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    ast: &ast::Enum,
    item_name: &Option<String>,
) -> Option<usize> {
    let mut first: Option<(usize, Option<String>)> = None;

    for (idx, variant) in ast.variants().into_iter().flatten().enumerate() {
        let specs = named_attr_specs(variant.attr_list(), "default");
        let Some(spec) = specs.first() else {
            continue;
        };
        if let Some((_, first_variant_name)) = &first {
            accumulate_error(
                ctxt,
                item_name,
                DeriveErrorKind::MultipleDefaultVariants {
                    first_variant_name: first_variant_name.clone(),
                },
                spec.range,
            );
        } else {
            first = Some((idx, variant.name().map(|n| n.text().to_string())));
        }
    }

    if first.is_none() {
        let range = derive_attrs(ast.attr_list())
            .first()
            .map(|attr| attr.syntax().text_range())
            .unwrap_or_else(|| ast.syntax().text_range());
        accumulate_error(
            ctxt,
            item_name,
            DeriveErrorKind::MissingDefaultVariant,
            range,
        );
    }

    first.map(|(idx, _)| idx)
}

/// Generates `impl` items for the traits requested via `#[derive(..)]` on
/// `enum_`. Must be called after the enum item itself has been lowered
/// (i.e. its item scope has been left), so the generated impls become
/// siblings of the enum in the scope graph.
pub(super) fn lower_enum_derive_impls<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    ast: &ast::Enum,
    enum_: Enum<'db>,
) {
    let item_name = ast.name().map(|n| n.text().to_string());
    let traits = parse_derive_traits(ctxt, ast.attr_list(), &item_name);

    let derives_default = traits.contains(&DerivableTrait::Default);
    validate_variant_default_attrs(ctxt, ast, derives_default);

    let db = ctxt.db();

    let Some(generics) = derive_generics(
        ctxt,
        &item_name,
        "enum",
        enum_.generic_params(db),
        enum_.where_clause(db),
        || {
            ast.generic_params()
                .map(|g| g.syntax().text_range())
                .unwrap_or_else(|| ast.syntax().text_range())
        },
    ) else {
        return;
    };

    let Some(enum_name_ident) = enum_.name(db).to_opt() else {
        // Parser error: missing name token. Avoid panics/cascades.
        return;
    };

    // Collect the name and payload of every variant; skip generation when
    // the enum is not well-formed enough (parser errors are reported
    // elsewhere).
    let mut variants = Vec::new();
    for variant in enum_.variants_list(db).data(db) {
        let Some(name) = variant.name.to_opt() else {
            return;
        };
        let payload = match variant.kind {
            VariantKind::Unit => VariantPayload::Unit,
            VariantKind::Tuple(tup) => {
                let mut elems = Vec::new();
                for ty in tup.data(db) {
                    let Some(ty) = ty.to_opt() else {
                        return;
                    };
                    elems.push(ty);
                }
                VariantPayload::Tuple(elems)
            }
            VariantKind::Record(fields) => {
                let mut record_fields = Vec::new();
                for field in fields.data(db) {
                    let (Some(name), Some(ty)) = (field.name.to_opt(), field.type_ref.to_opt())
                    else {
                        return;
                    };
                    record_fields.push((name, ty));
                }
                VariantPayload::Record(record_fields)
            }
        };
        variants.push(VariantInfo { name, payload });
    }

    // Resolve the `#[default]` variant up front so its diagnostics are
    // reported even when generation of other traits proceeds.
    let default_variant = derives_default
        .then(|| resolve_default_variant(ctxt, ast, &item_name))
        .flatten();

    let self_ty = derive_self_ty(ctxt, enum_name_ident, &generics);

    let derive_desugared = DeriveDesugared::Enum(parser::ast::AstPtr::new(ast));
    let mut builder = HirBuilder::new(ctxt, derive_desugared);

    for derived in traits {
        match derived {
            DerivableTrait::Eq => lower_derived_enum_eq_impl(
                &mut builder,
                self_ty,
                &generics,
                enum_name_ident,
                &variants,
            ),
            DerivableTrait::Default => {
                if let Some(idx) = default_variant {
                    lower_derived_enum_default_impl(
                        &mut builder,
                        self_ty,
                        &generics,
                        enum_name_ident,
                        &variants[idx],
                    );
                }
            }
        }
    }
}

/// Plain `self` receiver (view mode), matching the `self` parameter of
/// `core::ops::Eq::eq`.
fn param_view_self<'db>(builder: &HirBuilder<'_, 'db, DeriveDesugared>) -> FuncParam<'db> {
    let db = builder.db();
    FuncParam {
        mode: FuncParamMode::View,
        is_mut: false,
        has_ref_prefix: false,
        has_own_prefix: false,
        is_label_suppressed: false,
        name: Partial::Present(FuncParamName::Ident(IdentId::make_self(db))),
        ty: Partial::Present(builder.self_ty()),
        self_ty_fallback: true,
    }
}

/// `core::ops::Eq` trait reference.
fn eq_trait_ref<'db>(builder: &HirBuilder<'_, 'db, DeriveDesugared>) -> TraitRefId<'db> {
    let trait_path = builder.path_from_root(builder.roots().core, &["ops", "Eq"]);
    TraitRefId::new(builder.db(), Partial::Present(trait_path))
}

/// `core::default::Default` trait reference.
fn default_trait_ref<'db>(builder: &HirBuilder<'_, 'db, DeriveDesugared>) -> TraitRefId<'db> {
    let trait_path = builder.path_from_root(builder.roots().core, &["default", "Default"]);
    TraitRefId::new(builder.db(), Partial::Present(trait_path))
}

/// Folds `exprs` into a `&&` conjunction; an empty list is `true`.
fn fold_and<'db>(
    body: &mut BodyBuilder<'_, 'db, DeriveDesugared>,
    exprs: impl IntoIterator<Item = ExprId>,
) -> ExprId {
    let mut result = None;
    for expr in exprs {
        result = Some(match result {
            None => expr,
            Some(acc) => body.push_expr(Expr::Bin(acc, expr, BinOp::Logical(LogicalBinOp::And))),
        });
    }
    result.unwrap_or_else(|| body.bool_lit_expr(true))
}

/// `<Ty as core::default::Default>::default()`, qualified so resolution does
/// not depend on imports at the derive site.
fn default_call_expr<'db>(
    body: &mut BodyBuilder<'_, 'db, DeriveDesugared>,
    trait_ref: TraitRefId<'db>,
    ty: TypeId<'db>,
) -> ExprId {
    let db = body.db();
    let qualified = PathId::new(
        db,
        PathKind::QualifiedType {
            type_: ty,
            trait_: trait_ref,
        },
        None,
    )
    .push_str(db, "default");
    let callee = body.path_expr(qualified);
    body.call_expr(callee, vec![])
}

/// Generates:
///
/// ```text
/// impl<A, ..> core::ops::Eq for Struct<A, ..> where A: Eq, .. {
///     fn eq(self, _ other: Struct<A, ..>) -> bool {
///         return self.f0 == other.f0 && self.f1 == other.f1 && ...
///     }
/// }
/// ```
///
/// An empty struct compares as `true`. Per-field `==` routes through
/// `core::ops::Eq`, so nested derived structs compose.
fn lower_derived_eq_impl<'db>(
    builder: &mut HirBuilder<'_, 'db, DeriveDesugared>,
    self_ty: TypeId<'db>,
    generics: &DeriveGenerics<'db>,
    fields: &[(IdentId<'db>, TypeId<'db>)],
) {
    let trait_ref = eq_trait_ref(builder);
    let where_clause = generics.where_clause(builder.ctxt(), trait_ref);

    let other_ident = builder.ident("other");
    let other_param = builder.param_underscore_named(other_ident, self_ty);
    let params = builder.params([param_view_self(builder), other_param]);
    let bool_ty = builder.ty_ident(builder.ident("bool"));

    let fields = fields.to_vec();
    builder.impl_trait_generic(
        trait_ref,
        self_ty,
        generics.impl_params,
        where_clause,
        |builder| {
            let eq_ident = builder.ident("eq");
            builder.func_with_body_inline_always(
                eq_ident,
                builder.empty_generic_params(),
                params,
                Some(bool_ty),
                FuncModifiers::new(Visibility::Private, false, false, false),
                move |body| {
                    let db = body.db();
                    let self_expr = body.path_expr(PathId::from_ident(db, IdentId::make_self(db)));
                    let other_expr = body.ident_expr(other_ident);

                    let cmps: Vec<_> = fields
                        .iter()
                        .copied()
                        .map(|(name, _ty)| {
                            let lhs = field_expr(body, self_expr, name);
                            let rhs = field_expr(body, other_expr, name);
                            body.push_expr(Expr::Bin(lhs, rhs, BinOp::Comp(CompBinOp::Eq)))
                        })
                        .collect();
                    let result = fold_and(body, cmps);
                    body.emit_return(Some(result));
                },
            );
        },
    );
}

/// Generates:
///
/// ```text
/// impl<A, ..> core::default::Default for Struct<A, ..> where A: Default, .. {
///     fn default() -> Self {
///         return Self {
///             f0: <F0Ty as core::default::Default>::default(),
///             ...
///         }
///     }
/// }
/// ```
fn lower_derived_default_impl<'db>(
    builder: &mut HirBuilder<'_, 'db, DeriveDesugared>,
    self_ty: TypeId<'db>,
    generics: &DeriveGenerics<'db>,
    fields: &[(IdentId<'db>, TypeId<'db>)],
) {
    let trait_ref = default_trait_ref(builder);
    let where_clause = generics.where_clause(builder.ctxt(), trait_ref);

    let params = builder.params([]);
    let ret_ty = builder.self_ty();

    let fields = fields.to_vec();
    builder.impl_trait_generic(
        trait_ref,
        self_ty,
        generics.impl_params,
        where_clause,
        |builder| {
            let default_ident = builder.ident("default");
            builder.func_with_body_inline_always(
                default_ident,
                builder.empty_generic_params(),
                params,
                Some(ret_ty),
                FuncModifiers::new(Visibility::Private, false, false, false),
                move |body| {
                    let db = body.db();
                    let field_inits = fields
                        .iter()
                        .copied()
                        .map(|(name, ty)| {
                            let expr = default_call_expr(body, trait_ref, ty);
                            Field {
                                label: Some(name),
                                expr,
                            }
                        })
                        .collect();

                    let self_path =
                        Partial::Present(PathId::from_ident(db, IdentId::make_self_ty(db)));
                    let record = body.push_expr(Expr::RecordInit(self_path, field_inits));
                    body.emit_return(Some(record));
                },
            );
        },
    );
}

/// One outer `eq` match arm for `variant`:
///
/// ```text
/// Enum::V(__lhs_..) => match other {
///     Enum::V(__rhs_..) => __lhs_0 == __rhs_0 && ..
///     _ => false
/// }
/// ```
///
/// The inner catch-all is skipped for single-variant enums where the variant
/// arm is already exhaustive (a trailing `_` would be an unreachable
/// pattern).
fn enum_eq_match_arm<'db>(
    body: &mut BodyBuilder<'_, 'db, DeriveDesugared>,
    enum_name: IdentId<'db>,
    variant: &VariantInfo<'db>,
    other_ident: IdentId<'db>,
    multi_variant: bool,
) -> MatchArm {
    let db = body.db();
    let variant_path = PathId::from_ident(db, enum_name).push_ident(db, variant.name);

    let (lhs_pat, rhs_pat, cmps) = match &variant.payload {
        // `(Enum::V, Enum::V) => true`
        VariantPayload::Unit => (
            body.path_pat(variant_path),
            body.path_pat(variant_path),
            vec![],
        ),
        // `(Enum::V(__lhs_0, ..), Enum::V(__rhs_0, ..)) => __lhs_0 == __rhs_0 && ..`
        VariantPayload::Tuple(elems) => {
            let binders: Vec<_> = (0..elems.len())
                .map(|idx| {
                    (
                        IdentId::new(db, format!("__lhs_{idx}")),
                        IdentId::new(db, format!("__rhs_{idx}")),
                    )
                })
                .collect();
            let lhs_elems = binders.iter().map(|(lhs, _)| body.bind_pat(*lhs)).collect();
            let rhs_elems = binders.iter().map(|(_, rhs)| body.bind_pat(*rhs)).collect();
            (
                body.path_tuple_pat(variant_path, lhs_elems),
                body.path_tuple_pat(variant_path, rhs_elems),
                binder_cmps(body, &binders),
            )
        }
        // `(Enum::V { f: __lhs_f, .. }, Enum::V { f: __rhs_f, .. })
        //      => __lhs_f == __rhs_f && ..`
        VariantPayload::Record(fields) => {
            let binders: Vec<_> = fields
                .iter()
                .map(|(name, _ty)| {
                    (
                        IdentId::new(db, format!("__lhs_{}", name.data(db))),
                        IdentId::new(db, format!("__rhs_{}", name.data(db))),
                    )
                })
                .collect();
            let lhs_fields = fields
                .iter()
                .zip(&binders)
                .map(|((name, _ty), (lhs, _))| RecordPatField {
                    label: Partial::Present(*name),
                    pat: body.bind_pat(*lhs),
                })
                .collect();
            let rhs_fields = fields
                .iter()
                .zip(&binders)
                .map(|((name, _ty), (_, rhs))| RecordPatField {
                    label: Partial::Present(*name),
                    pat: body.bind_pat(*rhs),
                })
                .collect();
            (
                body.record_pat(variant_path, lhs_fields),
                body.record_pat(variant_path, rhs_fields),
                binder_cmps(body, &binders),
            )
        }
    };

    let cmp_expr = fold_and(body, cmps);
    let mut inner_arms = vec![MatchArm {
        pat: rhs_pat,
        body: cmp_expr,
    }];
    if multi_variant {
        let pat = body.wildcard_pat();
        let arm_body = body.bool_lit_expr(false);
        inner_arms.push(MatchArm {
            pat,
            body: arm_body,
        });
    }
    let inner_scrutinee = body.ident_expr(other_ident);
    let inner_match = body.match_expr(inner_scrutinee, inner_arms);
    MatchArm {
        pat: lhs_pat,
        body: inner_match,
    }
}

/// `__lhs == __rhs` comparisons for a list of payload binder pairs.
fn binder_cmps<'db>(
    body: &mut BodyBuilder<'_, 'db, DeriveDesugared>,
    binders: &[(IdentId<'db>, IdentId<'db>)],
) -> Vec<ExprId> {
    binders
        .iter()
        .copied()
        .map(|(lhs, rhs)| {
            let lhs = body.ident_expr(lhs);
            let rhs = body.ident_expr(rhs);
            body.push_expr(Expr::Bin(lhs, rhs, BinOp::Comp(CompBinOp::Eq)))
        })
        .collect()
}

/// Generates:
///
/// ```text
/// impl<A, ..> core::ops::Eq for Enum<A, ..> where A: Eq, .. {
///     fn eq(self, _ other: Enum<A, ..>) -> bool {
///         return match self {
///             Enum::Unit => match other {
///                 Enum::Unit => true
///                 _ => false
///             }
///             Enum::Tup(__lhs_0) => match other {
///                 Enum::Tup(__rhs_0) => __lhs_0 == __rhs_0
///                 _ => false
///             }
///         }
///     }
/// }
/// ```
///
/// Payload `==` routes through `core::ops::Eq`, so nested derived types
/// compose. A nested match (rather than `match (self, other)`) is used
/// because a tuple-of-enums scrutinee currently miscompiles during MIR ->
/// Sonatina emission (`enum.tag operand must have enum type`).
fn lower_derived_enum_eq_impl<'db>(
    builder: &mut HirBuilder<'_, 'db, DeriveDesugared>,
    self_ty: TypeId<'db>,
    generics: &DeriveGenerics<'db>,
    enum_name: IdentId<'db>,
    variants: &[VariantInfo<'db>],
) {
    let trait_ref = eq_trait_ref(builder);
    let where_clause = generics.where_clause(builder.ctxt(), trait_ref);

    let other_ident = builder.ident("other");
    let other_param = builder.param_underscore_named(other_ident, self_ty);
    let params = builder.params([param_view_self(builder), other_param]);
    let bool_ty = builder.ty_ident(builder.ident("bool"));

    let variants = variants.to_vec();
    builder.impl_trait_generic(
        trait_ref,
        self_ty,
        generics.impl_params,
        where_clause,
        |builder| {
            let eq_ident = builder.ident("eq");
            builder.func_with_body_inline_always(
                eq_ident,
                builder.empty_generic_params(),
                params,
                Some(bool_ty),
                FuncModifiers::new(Visibility::Private, false, false, false),
                move |body| {
                    let db = body.db();

                    // An uninhabited enum has no values to compare; `eq` is
                    // trivially `true` (and can never be called).
                    if variants.is_empty() {
                        let result = body.bool_lit_expr(true);
                        body.emit_return(Some(result));
                        return;
                    }

                    let multi_variant = variants.len() > 1;
                    let arms: Vec<_> = variants
                        .iter()
                        .map(|variant| {
                            enum_eq_match_arm(body, enum_name, variant, other_ident, multi_variant)
                        })
                        .collect();

                    let scrutinee = body.path_expr(PathId::from_ident(db, IdentId::make_self(db)));
                    let result = body.match_expr(scrutinee, arms);
                    body.emit_return(Some(result));
                },
            );
        },
    );
}

/// Generates:
///
/// ```text
/// impl<A, ..> core::default::Default for Enum<A, ..> where A: Default, .. {
///     fn default() -> Self {
///         return Enum::DefaultVariant     // unit variant
///         return Enum::DefaultVariant(<T0 as Default>::default(), ..)
///         return Enum::DefaultVariant { f: <F as Default>::default(), .. }
///     }
/// }
/// ```
///
/// where `DefaultVariant` is the variant marked `#[default]`.
fn lower_derived_enum_default_impl<'db>(
    builder: &mut HirBuilder<'_, 'db, DeriveDesugared>,
    self_ty: TypeId<'db>,
    generics: &DeriveGenerics<'db>,
    enum_name: IdentId<'db>,
    variant: &VariantInfo<'db>,
) {
    let trait_ref = default_trait_ref(builder);
    let where_clause = generics.where_clause(builder.ctxt(), trait_ref);

    let params = builder.params([]);
    let ret_ty = builder.self_ty();

    let variant = variant.clone();
    builder.impl_trait_generic(
        trait_ref,
        self_ty,
        generics.impl_params,
        where_clause,
        |builder| {
            let default_ident = builder.ident("default");
            builder.func_with_body_inline_always(
                default_ident,
                builder.empty_generic_params(),
                params,
                Some(ret_ty),
                FuncModifiers::new(Visibility::Private, false, false, false),
                move |body| {
                    let db = body.db();
                    let variant_path =
                        PathId::from_ident(db, enum_name).push_ident(db, variant.name);

                    let value = match &variant.payload {
                        VariantPayload::Unit => body.path_expr(variant_path),
                        VariantPayload::Tuple(elems) => {
                            let args = elems
                                .iter()
                                .copied()
                                .map(|ty| default_call_expr(body, trait_ref, ty))
                                .collect();
                            let callee = body.path_expr(variant_path);
                            body.call_expr(callee, args)
                        }
                        VariantPayload::Record(fields) => {
                            let field_inits = fields
                                .iter()
                                .copied()
                                .map(|(name, ty)| {
                                    let expr = default_call_expr(body, trait_ref, ty);
                                    Field {
                                        label: Some(name),
                                        expr,
                                    }
                                })
                                .collect();
                            body.push_expr(Expr::RecordInit(
                                Partial::Present(variant_path),
                                field_inits,
                            ))
                        }
                    };
                    body.emit_return(Some(value));
                },
            );
        },
    );
}

fn field_expr<'db>(
    body: &mut BodyBuilder<'_, 'db, DeriveDesugared>,
    receiver: crate::hir_def::ExprId,
    field: IdentId<'db>,
) -> crate::hir_def::ExprId {
    body.push_expr(Expr::Field(
        receiver,
        Partial::Present(FieldIndex::Ident(field)),
    ))
}
