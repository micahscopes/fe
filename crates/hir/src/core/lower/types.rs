use parser::ast::{self, prelude::*};

use super::FileLowerCtxt;
use crate::core::hir_def::{
    AssocTypeGenericArg, Body, GenericArg, GenericArgListId, IdentId, ItemKind, Partial, PathId,
    PathKind, TraitRefId, TupleTypeId, TypeGenericArg, TypeId, TypeKind, TypeMode, VariantKind,
    params::{FuncParamMode, GenericParamListId, callable_shape_name},
    scope_graph::ScopeId,
};

impl<'db> TypeId<'db> {
    pub(super) fn lower_ast(ctxt: &mut FileLowerCtxt<'db>, ast: ast::Type) -> Self {
        let kind = match ast.kind() {
            ast::TypeKind::QuoteHole(hole) => {
                let db = ctxt.db();
                match ctxt.current_hole(hole.syntax().text_range()) {
                    Some(common::generation::FrozenHoleValue::Record(record_ref)) => {
                        let item = ctxt.resolve_record_item(&record_ref);
                        let params = match item {
                            ItemKind::Struct(s) => s.generic_params(db),
                            ItemKind::Enum(e) => e.generic_params(db),
                            _ => GenericParamListId::new(db, Vec::new()),
                        };
                        let generic_args = if params.data(db).is_empty() {
                            GenericArgListId::none(db)
                        } else {
                            let args: Vec<_> = params
                                .data(db)
                                .iter()
                                .map(|param| {
                                    let ty = param.name().to_opt().map(|ident| {
                                        TypeId::new(
                                            db,
                                            TypeKind::Path(Partial::Present(PathId::from_ident(
                                                db, ident,
                                            ))),
                                        )
                                    });
                                    GenericArg::Type(TypeGenericArg { ty: ty.into() })
                                })
                                .collect();
                            GenericArgListId::new(db, args, true)
                        };
                        let display = PathId::new(
                            db,
                            PathKind::Ident {
                                ident: item.name(db).into(),
                                generic_args,
                            },
                            None,
                        );
                        let path = PathId::new(
                            db,
                            PathKind::BoundItem {
                                item: ScopeId::Item(item),
                                display,
                            },
                            None,
                        );
                        TypeKind::Path(Partial::Present(path))
                    }
                    Some(common::generation::FrozenHoleValue::FieldType(field_ref)) => {
                        let resolved = match (
                            ctxt.resolve_record_item(&field_ref.record),
                            field_ref.record.variant_index,
                        ) {
                            (ItemKind::Struct(s), None) => s
                                .fields(db)
                                .data(db)
                                .get(field_ref.index)
                                .and_then(|f| f.type_ref.to_opt()),
                            (ItemKind::Enum(e), Some(v)) => {
                                match e.variants_list(db).data(db).get(v).map(|v| &v.kind) {
                                    Some(VariantKind::Tuple(tuple_id)) => tuple_id
                                        .data(db)
                                        .get(field_ref.index)
                                        .and_then(|t| t.to_opt()),
                                    Some(VariantKind::Record(fields_id)) => fields_id
                                        .data(db)
                                        .get(field_ref.index)
                                        .and_then(|f| f.type_ref.to_opt()),
                                    _ => None,
                                }
                            }
                            _ => None,
                        };
                        if let Some(ty) = resolved {
                            return ty;
                        }
                        TypeKind::Path(Partial::Absent)
                    }
                    _ => TypeKind::Path(Partial::Absent),
                }
            }

            ast::TypeKind::Ptr(ty) => {
                let inner = Self::lower_ast_partial(ctxt, ty.inner());
                if ty.star2().is_some() {
                    let inner = Partial::Present(TypeId::new(ctxt.db(), TypeKind::Ptr(inner)));
                    TypeKind::Ptr(inner)
                } else {
                    TypeKind::Ptr(inner)
                }
            }

            ast::TypeKind::Mode(ty) => {
                let mode = match ty.mode() {
                    Some(ast::TypeMode::Mut(_)) => TypeMode::Mut,
                    Some(ast::TypeMode::MaybeMut(_)) => TypeMode::MaybeMut,
                    Some(ast::TypeMode::Ref(_)) => TypeMode::Ref,
                    Some(ast::TypeMode::Own(_)) => TypeMode::Own,
                    None => TypeMode::Ref,
                };
                let inner = Self::lower_ast_partial(ctxt, ty.inner());
                TypeKind::Mode(mode, inner)
            }

            ast::TypeKind::Path(ty) => {
                let path = PathId::lower_ast_partial(ctxt, ty.path());
                TypeKind::Path(path)
            }

            ast::TypeKind::Tuple(ty) => TypeKind::Tuple(TupleTypeId::lower_ast(ctxt, ty)),

            ast::TypeKind::Array(ty) => {
                let elem_ty = Self::lower_ast_partial(ctxt, ty.elem_ty());
                let body = ty
                    .len()
                    .map(|ast| Body::lower_ast_nameless(ctxt, ast))
                    .into();
                TypeKind::Array(elem_ty, body)
            }

            ast::TypeKind::Never(_) => TypeKind::Never,
        };

        TypeId::new(ctxt.db(), kind)
    }

    pub(super) fn lower_ast_partial(
        ctxt: &mut FileLowerCtxt<'db>,
        ast: Option<ast::Type>,
    ) -> Partial<Self> {
        ast.map(|ast| Self::lower_ast(ctxt, ast)).into()
    }
}

impl<'db> TupleTypeId<'db> {
    pub(super) fn lower_ast(ctxt: &mut FileLowerCtxt<'db>, ast: ast::TupleType) -> Self {
        let mut elem_tys = Vec::new();
        for elem in ast {
            elem_tys.push(Some(TypeId::lower_ast(ctxt, elem)).into());
        }
        TupleTypeId::new(ctxt.db(), elem_tys)
    }
}

impl<'db> TraitRefId<'db> {
    pub(super) fn lower_ast(ctxt: &mut FileLowerCtxt<'db>, ast: ast::TraitRef) -> Self {
        let path = match ast.fn_shape() {
            // `Fn(own A, B) -> R` names the core trait of its shape,
            // `Fn_ov<A, B, Out = R>`; `Fn(own T) -> U` is `Fn<T, U>`.
            Some(shape) => shape.name().zip(shape.params()).map(|(name, params)| {
                let db = ctxt.db();
                let mut modes = Vec::new();
                let mut args = Vec::new();
                for param in params {
                    let ty = TypeId::lower_ast(ctxt, param);
                    let (mode, ty) = match ty.data(db) {
                        TypeKind::Mode(TypeMode::Own, inner) => (FuncParamMode::Own, *inner),
                        TypeKind::Mode(TypeMode::Mut, inner) => (FuncParamMode::Mut, *inner),
                        TypeKind::Mode(TypeMode::Ref, inner) => (FuncParamMode::View, *inner),
                        _ => (FuncParamMode::View, Partial::Present(ty)),
                    };
                    modes.push(mode);
                    args.push(GenericArg::Type(TypeGenericArg { ty }));
                }
                let ret = Partial::Present(shape.ret_ty().map_or_else(
                    || TupleTypeId::new(db, Vec::new()).to_ty(db),
                    |ty| TypeId::lower_ast(ctxt, ty),
                ));
                let trait_name = callable_shape_name(name.text() == "FnMut", &modes);
                args.push(if trait_name == "Fn" {
                    GenericArg::Type(TypeGenericArg { ty: ret })
                } else {
                    GenericArg::AssocType(AssocTypeGenericArg {
                        name: Partial::Present(IdentId::new(db, "Out".to_string())),
                        ty: ret,
                    })
                });
                ctxt.core_path().push_str(db, "functional").push_str_args(
                    db,
                    &trait_name,
                    GenericArgListId::new(db, args, true),
                )
            }),
            None => ast.path().map(|ast| PathId::lower_ast(ctxt, ast)),
        };
        Self::new(ctxt.db(), Partial::from(path))
    }

    pub(super) fn lower_ast_partial(
        ctxt: &mut FileLowerCtxt<'db>,
        ast: Option<ast::TraitRef>,
    ) -> Partial<Self> {
        ast.map(|ast| Self::lower_ast(ctxt, ast)).into()
    }
}
