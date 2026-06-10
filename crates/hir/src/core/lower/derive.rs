//! Lowering support for `#[derive(..)]` on structs.
//!
//! Each derived trait is synthesized as a real HIR `impl Trait for Struct`
//! item via [`HirBuilder`], exactly like `#[event]` / `#[error]` desugaring.
//! Generated items carry [`DeriveDesugared`] origins so diagnostics and IDE
//! features can map them back to the annotated struct.

use parser::ast::{self, prelude::*};
use salsa::Accumulator as _;

use super::{
    FileLowerCtxt,
    attr::has_named_attr,
    hir_builder::{BodyBuilder, HirBuilder},
};
use crate::{
    hir_def::{
        BinOp, CompBinOp, Expr, Field, FieldIndex, FuncModifiers, FuncParam, FuncParamMode,
        FuncParamName, IdentId, LitKind, LogicalBinOp, Partial, PathId, PathKind, Struct,
        TraitRefId, TypeId, TypeKind, Visibility,
    },
    span::DeriveDesugared,
};

/// Derive-related errors accumulated during `#[derive(..)]` lowering /
/// validation.
#[salsa::accumulator]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeriveError {
    pub kind: DeriveErrorKind,
    pub file: common::file::File,
    /// Range of the primary span (attribute, argument, or generic params).
    pub primary_range: parser::TextRange,
    pub struct_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DeriveErrorKind {
    /// `#[derive(..)]` on a generic struct is not yet supported.
    GenericStruct,
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

fn accumulate_error<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    ast: &ast::Struct,
    kind: DeriveErrorKind,
    primary_range: parser::TextRange,
) {
    let db = ctxt.db();
    DeriveError {
        kind,
        file: ctxt.top_mod().file(db),
        primary_range,
        struct_name: ast.name().map(|n| n.text().to_string()),
    }
    .accumulate(db);
}

/// Reports an error for `#[derive(..)]` combined with `#[event]` or
/// `#[error]`; derive desugaring is skipped for such structs.
pub(super) fn report_derive_on_event_or_error_struct<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    ast: &ast::Struct,
) {
    for attr in derive_attrs(ast) {
        let range = attr.syntax().text_range();
        accumulate_error(ctxt, ast, DeriveErrorKind::EventErrorStruct, range);
    }
}

fn derive_attrs(ast: &ast::Struct) -> Vec<ast::NormalAttr> {
    ast.attr_list()
        .into_iter()
        .flat_map(|attrs| attrs.normal_attrs_named("derive").collect::<Vec<_>>())
        .collect()
}

/// Parses the `#[derive(..)]` attributes of a struct into the list of traits
/// to derive, reporting malformed arguments, unknown traits, and duplicates.
fn parse_derive_traits<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    ast: &ast::Struct,
) -> Vec<DerivableTrait> {
    let mut traits = Vec::new();

    for attr in derive_attrs(ast) {
        let attr_range = attr.syntax().text_range();

        // `#[derive = ..]` or bare `#[derive]`.
        let args = match attr.args() {
            Some(args) if attr.value().is_none() => args,
            _ => {
                accumulate_error(ctxt, ast, DeriveErrorKind::InvalidForm, attr_range);
                continue;
            }
        };

        let mut has_arg = false;
        for arg in args {
            has_arg = true;
            let arg_range = arg.syntax().text_range();
            let (Some(key), None) = (arg.key(), arg.value()) else {
                accumulate_error(ctxt, ast, DeriveErrorKind::InvalidForm, arg_range);
                continue;
            };

            let name = key.text().to_string();
            let derived = match name.as_str() {
                "Eq" => DerivableTrait::Eq,
                "Default" => DerivableTrait::Default,
                _ => {
                    accumulate_error(
                        ctxt,
                        ast,
                        DeriveErrorKind::UnknownTrait { name },
                        arg_range,
                    );
                    continue;
                }
            };

            if traits.contains(&derived) {
                accumulate_error(
                    ctxt,
                    ast,
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
            accumulate_error(ctxt, ast, DeriveErrorKind::InvalidForm, attr_range);
        }
    }

    traits
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
    let traits = parse_derive_traits(ctxt, ast);

    let db = ctxt.db();

    // Derives on generic structs are not yet supported.
    if !struct_.generic_params(db).data(db).is_empty() {
        let range = ast
            .generic_params()
            .map(|g| g.syntax().text_range())
            .unwrap_or_else(|| ast.syntax().text_range());
        accumulate_error(ctxt, ast, DeriveErrorKind::GenericStruct, range);
        return;
    }

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

    let self_ty = TypeId::new(
        db,
        TypeKind::Path(Partial::Present(PathId::from_ident(db, struct_name_ident))),
    );

    let derive_desugared = DeriveDesugared {
        derive_struct: parser::ast::AstPtr::new(ast),
    };
    let mut builder = HirBuilder::new(ctxt, derive_desugared);

    for derived in traits {
        match derived {
            DerivableTrait::Eq => lower_derived_eq_impl(&mut builder, self_ty, &fields),
            DerivableTrait::Default => lower_derived_default_impl(&mut builder, self_ty, &fields),
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

/// Generates:
///
/// ```text
/// impl core::ops::Eq for Struct {
///     fn eq(self, _ other: Struct) -> bool {
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
    fields: &[(IdentId<'db>, TypeId<'db>)],
) {
    let db = builder.db();
    let trait_path = builder.path_from_root(builder.roots().core, &["ops", "Eq"]);
    let trait_ref = TraitRefId::new(db, Partial::Present(trait_path));

    let other_ident = builder.ident("other");
    let other_param = builder.param_underscore_named(other_ident, self_ty);
    let params = builder.params([param_view_self(builder), other_param]);
    let bool_ty = builder.ty_ident(builder.ident("bool"));

    let fields = fields.to_vec();
    builder.impl_trait(trait_ref, self_ty, |builder| {
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

                let mut result = None;
                for (name, _ty) in fields.iter().copied() {
                    let lhs = field_expr(body, self_expr, name);
                    let rhs = field_expr(body, other_expr, name);
                    let cmp = body.push_expr(Expr::Bin(lhs, rhs, BinOp::Comp(CompBinOp::Eq)));
                    result = Some(match result {
                        None => cmp,
                        Some(acc) => body.push_expr(Expr::Bin(
                            acc,
                            cmp,
                            BinOp::Logical(LogicalBinOp::And),
                        )),
                    });
                }
                let result = result
                    .unwrap_or_else(|| body.push_expr(Expr::Lit(LitKind::Bool(true))));
                body.emit_return(Some(result));
            },
        );
    });
}

/// Generates:
///
/// ```text
/// impl core::default::Default for Struct {
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
    fields: &[(IdentId<'db>, TypeId<'db>)],
) {
    let db = builder.db();
    let trait_path = builder.path_from_root(builder.roots().core, &["default", "Default"]);
    let trait_ref = TraitRefId::new(db, Partial::Present(trait_path));

    let params = builder.params([]);
    let ret_ty = builder.self_ty();

    let fields = fields.to_vec();
    builder.impl_trait(trait_ref, self_ty, |builder| {
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
                        // `<FieldTy as core::default::Default>::default()`,
                        // qualified so resolution does not depend on imports
                        // at the derive site.
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
                        let expr = body.call_expr(callee, vec![]);
                        Field {
                            label: Some(name),
                            expr,
                        }
                    })
                    .collect();

                let self_path = Partial::Present(PathId::from_ident(db, IdentId::make_self_ty(db)));
                let record = body.push_expr(Expr::RecordInit(self_path, field_inits));
                body.emit_return(Some(record));
            },
        );
    });
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
