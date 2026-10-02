//! Builds checked generation results as ordinary HIR in the output view.
//! Provider evaluation has already finished before this module is queried.

use common::{
    ingot::Ingot,
    stdlib::{BUILTIN_CORE_BASE_URL, BUILTIN_STD_BASE_URL},
};
use parser::ast::{self, prelude::*};

use super::{
    FileLowerCtxt, base_scope_graph_impl,
    hir_builder::{BodyBuilder, GeneratedFunctionSpec, GeneratedImplSpec, HirBuilder},
    top_mod_ast,
};
use crate::{
    HirDb, LowerHirDb,
    analysis::semantic::SemConstScalar,
    core::meta::{
        FrozenGeneration, FrozenHandleKind, FrozenMetaValue, FrozenOutputShape, FrozenOwner,
        MetaItem,
    },
    hir_def::{
        ArithBinOp, AssocConstDef, BinOp, Expr, ExprId, FieldIndex, Func, FuncModifiers, FuncParam,
        FuncParamMode, FuncParamName, GenericArg, GenericArgListId, HirIngot, IdentId, IntegerId,
        ItemKind, LitKind, Partial, Pat, PathId, Stmt, Struct, TopLevelMod, TrackedItemVariant,
        TraitRefId, TypeGenericArg, TypeId, TypeKind, TypeMode, Visibility,
        prim_ty::{PrimTy, UintTy},
        scope_graph::{EdgeKind, Scope, ScopeGraph, ScopeId},
    },
    span::{GenerationDesugared, HirOrigin},
};

fn invalid(message: &'static str) -> String {
    message.to_owned()
}

fn record(
    value: &FrozenMetaValue,
    item: MetaItem,
) -> Result<&std::collections::BTreeMap<String, FrozenMetaValue>, String> {
    match value {
        FrozenMetaValue::Record {
            item: actual,
            fields,
        } if *actual == item => Ok(fields),
        _ => Err(invalid("expected a designated generation record")),
    }
}

fn field<'a>(
    fields: &'a std::collections::BTreeMap<String, FrozenMetaValue>,
    name: &str,
) -> Result<&'a FrozenMetaValue, String> {
    fields
        .get(name)
        .ok_or_else(|| invalid("missing generation field"))
}

fn variant(value: &FrozenMetaValue, item: MetaItem) -> Result<(&str, &[FrozenMetaValue]), String> {
    match value {
        FrozenMetaValue::Variant {
            item: actual,
            name,
            fields,
        } if *actual == item => Ok((name, fields)),
        _ => Err(invalid("expected a designated generation variant")),
    }
}

fn array(value: &FrozenMetaValue) -> Result<&[FrozenMetaValue], String> {
    match value {
        FrozenMetaValue::Array(values) => Ok(values),
        _ => Err(invalid("expected a generation array")),
    }
}

fn index(value: &FrozenMetaValue) -> Result<usize, String> {
    match value {
        FrozenMetaValue::Scalar {
            value: SemConstScalar::Int { value },
            ..
        } => usize::try_from(value).map_err(|_| invalid("generation index exceeds usize")),
        _ => Err(invalid("expected a generation index")),
    }
}

fn flag(value: &FrozenMetaValue) -> Result<bool, String> {
    match value {
        FrozenMetaValue::Scalar {
            value: SemConstScalar::Bool(value),
            ..
        } => Ok(*value),
        _ => Err(invalid("expected a generation boolean")),
    }
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

fn source_path<'db>(
    db: &'db dyn HirDb,
    value: &FrozenMetaValue,
    provider_ingot: Ingot<'db>,
    provider_root: IdentId<'db>,
) -> Result<PathId<'db>, String> {
    let source = text(value)?;
    let parts = source.split("::").collect::<Vec<_>>();
    if parts.len() < 2
        || !matches!(parts[0], "ingot" | "core" | "std")
        || !parts.iter().all(|part| identifier(part))
    {
        return Err(invalid(
            "expected an explicit ingot, core or std source path",
        ));
    }
    let (ingot, display) = if parts[0] == "ingot" {
        (provider_ingot, provider_root)
    } else {
        let base = match parts[0] {
            "core" => BUILTIN_CORE_BASE_URL,
            "std" => BUILTIN_STD_BASE_URL,
            _ => unreachable!(),
        };
        let url = url::Url::parse(base).map_err(|_| invalid("builtin ingot URL is invalid"))?;
        let ingot = db
            .workspace()
            .containing_ingot(db, url)
            .ok_or_else(|| invalid("builtin ingot is missing"))?;
        (ingot, IdentId::new(db, parts[0]))
    };
    let mut path = PathId::new(
        db,
        crate::hir_def::PathKind::BoundIngot { ingot, display },
        None,
    );
    for part in &parts[1..] {
        path = path.push_str(db, part);
    }
    Ok(path)
}

fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
}

struct Decoder<'db> {
    db: &'db dyn HirDb,
    owner: Struct<'db>,
    target: FrozenOwner,
    provider_ingot: Ingot<'db>,
    provider_root: IdentId<'db>,
}

impl<'db> Decoder<'db> {
    fn field_index(&self, value: &FrozenMetaValue, type_handle: bool) -> Result<usize, String> {
        let kind = match value {
            FrozenMetaValue::Structural {
                item: MetaItem::FieldType,
                owner,
                kind: FrozenHandleKind::FieldType(index),
            } if type_handle && *owner == self.target => *index,
            FrozenMetaValue::Structural {
                item: MetaItem::Field,
                owner,
                kind: FrozenHandleKind::Field(index),
            } if !type_handle && *owner == self.target => *index,
            _ => return Err(invalid("expected an authorized field handle")),
        };
        if kind >= self.owner.hir_fields(self.db).data(self.db).len() {
            return Err(invalid("field handle is out of bounds"));
        }
        Ok(kind)
    }

    fn field_name(&self, index: usize) -> Result<IdentId<'db>, String> {
        self.owner.hir_fields(self.db).data(self.db)[index]
            .name
            .to_opt()
            .ok_or_else(|| invalid("generation field name is missing"))
    }

    fn name(&self, value: &FrozenMetaValue) -> Result<IdentId<'db>, String> {
        let (case, fields) = variant(value, MetaItem::DeclarationName)?;
        let name = match (case, fields) {
            ("Literal", [name]) => text(name)?,
            ("FieldPrefix", [prefix, field]) => {
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

    fn ty(&self, value: &FrozenMetaValue, owner_type: TypeId<'db>) -> Result<TypeId<'db>, String> {
        let (case, fields) = variant(value, MetaItem::DeclarationType)?;
        let path = match (case, fields) {
            ("Unit", []) => {
                return Ok(TypeId::new(
                    self.db,
                    TypeKind::Tuple(crate::hir_def::TupleTypeId::new(self.db, vec![])),
                ));
            }
            ("Bool", []) => return Ok(TypeId::new(self.db, TypeKind::Primitive(PrimTy::Bool))),
            ("U64", []) => {
                return Ok(TypeId::new(
                    self.db,
                    TypeKind::Primitive(PrimTy::Uint(UintTy::U64)),
                ));
            }
            ("U256", []) => {
                return Ok(TypeId::new(
                    self.db,
                    TypeKind::Primitive(PrimTy::Uint(UintTy::U256)),
                ));
            }
            ("BytePointer", []) => {
                return Ok(TypeId::new(
                    self.db,
                    TypeKind::Ptr(Partial::Present(TypeId::new(
                        self.db,
                        TypeKind::Primitive(PrimTy::Uint(UintTy::U8)),
                    ))),
                ));
            }
            ("Named", [name]) => {
                source_path(self.db, name, self.provider_ingot, self.provider_root)?
            }
            ("Field", [field]) => {
                let index = self.field_index(field, true)?;
                return self.owner.hir_fields(self.db).data(self.db)[index]
                    .type_ref
                    .to_opt()
                    .ok_or_else(|| invalid("authorized field type is missing"));
            }
            (
                "Record",
                [
                    FrozenMetaValue::Structural {
                        item: MetaItem::Record,
                        owner,
                        kind: FrozenHandleKind::Record,
                    },
                ],
            ) if *owner == self.target => return Ok(owner_type),
            _ => return Err(invalid("invalid generated declaration type")),
        };
        Ok(TypeId::new(self.db, TypeKind::Path(Partial::Present(path))))
    }

    fn optional<'a>(
        &self,
        value: &'a FrozenMetaValue,
    ) -> Result<Option<&'a FrozenMetaValue>, String> {
        let (case, fields) = variant(value, MetaItem::Option)?;
        match (case, fields) {
            ("Some", [value]) => Ok(Some(value)),
            ("None", []) => Ok(None),
            _ => Err(invalid("invalid optional generation value")),
        }
    }

    fn prefix<'a>(&self, value: &'a FrozenMetaValue) -> Result<Vec<&'a FrozenMetaValue>, String> {
        let mut result = Vec::new();
        let mut ended = false;
        for slot in array(value)? {
            match self.optional(slot)? {
                Some(_) if ended => return Err(invalid("generation slots must form a prefix")),
                Some(value) => result.push(value),
                None => ended = true,
            }
        }
        Ok(result)
    }

    fn call<O>(
        &self,
        body: &mut BodyBuilder<'_, 'db, O>,
        value: &FrozenMetaValue,
        names: &[IdentId<'db>],
        owner_type: TypeId<'db>,
    ) -> Result<ExprId, String>
    where
        O: Clone + Into<crate::span::DesugaredOrigin>,
    {
        let fields = record(value, MetaItem::FunctionCall)?;
        let path = source_path(
            self.db,
            field(fields, "path")?,
            self.provider_ingot,
            self.provider_root,
        )?;
        let type_args = self
            .prefix(field(fields, "types")?)?
            .into_iter()
            .map(|value| {
                Ok(GenericArg::Type(TypeGenericArg {
                    ty: Partial::Present(self.ty(value, owner_type)?),
                }))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let path = if type_args.is_empty() {
            path
        } else {
            let parent = path.parent(self.db);
            let crate::hir_def::PathKind::Ident { ident, .. } = path.kind(self.db) else {
                return Err(invalid("call path has no final identifier"));
            };
            PathId::new(
                self.db,
                crate::hir_def::PathKind::Ident {
                    ident,
                    generic_args: GenericArgListId::given(self.db, type_args),
                },
                parent,
            )
        };
        let args = self
            .prefix(field(fields, "arguments")?)?
            .into_iter()
            .map(|argument| {
                let fields = record(argument, MetaItem::CallArgument)?;
                let index = index(field(fields, "expression")?)?;
                let name = *names
                    .get(index)
                    .ok_or_else(|| invalid("call argument must point backward"))?;
                let label = self
                    .optional(field(fields, "label")?)?
                    .map(|label| {
                        let name = text(label)?;
                        if !identifier(&name) {
                            return Err(invalid("call label must be an identifier"));
                        }
                        Ok(IdentId::new(self.db, name))
                    })
                    .transpose()?;
                Ok(crate::hir_def::expr::CallArg {
                    label,
                    expr: body.ident_expr(name),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let callee = body.path_expr(path);
        Ok(body.call_expr_with_args(callee, args))
    }

    fn expressions<O>(
        &self,
        body: &mut BodyBuilder<'_, 'db, O>,
        values: &[FrozenMetaValue],
        result: &FrozenMetaValue,
        parameters: usize,
        method: bool,
        owner_type: TypeId<'db>,
    ) -> Result<(), String>
    where
        O: Clone + Into<crate::span::DesugaredOrigin>,
    {
        let mut names = Vec::new();
        let mut ended = false;
        for (i, value) in values.iter().enumerate() {
            let (case, fields) = variant(value, MetaItem::BodyExpr)?;
            if case == "End" && fields.is_empty() {
                ended = true;
                continue;
            }
            if ended {
                return Err(invalid("body expressions after End are not allowed"));
            }
            let previous = |value: &FrozenMetaValue| -> Result<IdentId<'db>, String> {
                names
                    .get(index(value)?)
                    .copied()
                    .ok_or_else(|| invalid("body reference must point backward"))
            };
            if let ("Parameter", [parameter]) = (case, fields) {
                let index = index(parameter)?;
                if index >= parameters {
                    return Err(invalid("parameter index out of bounds"));
                }
                names.push(if method && index == 0 {
                    IdentId::make_self(self.db)
                } else {
                    IdentId::new(self.db, format!("__meta_arg_{index}"))
                });
                continue;
            }
            let expression = match (case, fields) {
                ("Field", [base, field]) => {
                    let base_name = previous(base)?;
                    let base = body.ident_expr(base_name);
                    Expr::Field(
                        base,
                        Partial::Present(FieldIndex::Ident(
                            self.field_name(self.field_index(field, false)?)?,
                        )),
                    )
                }
                ("Bool", [value]) => Expr::Lit(LitKind::Bool(flag(value)?)),
                ("U256", [value]) => {
                    let FrozenMetaValue::Scalar {
                        value: SemConstScalar::Int { value },
                        ..
                    } = value
                    else {
                        return Err(invalid("expected u256 literal"));
                    };
                    let magnitude = num_bigint::BigUint::try_from(value)
                        .map_err(|_| invalid("negative u256 literal"))?;
                    let lit =
                        body.push_expr(Expr::Lit(LitKind::Int(IntegerId::new(self.db, magnitude))));
                    Expr::Cast(
                        lit,
                        Partial::Present(TypeId::new(
                            self.db,
                            TypeKind::Primitive(PrimTy::Uint(UintTy::U256)),
                        )),
                    )
                }
                ("Add", [a, b]) => {
                    let a = body.ident_expr(previous(a)?);
                    let b = body.ident_expr(previous(b)?);
                    Expr::Bin(a, b, BinOp::Arith(ArithBinOp::Add))
                }
                ("Call", [call]) => {
                    let call = self.call(body, call, &names, owner_type)?;
                    let name = IdentId::new(self.db, format!("__meta_node_{i}"));
                    let pat = body.push_pat(Pat::Path(
                        Partial::Present(PathId::from_ident(self.db, name)),
                        false,
                    ));
                    body.emit_stmt(Stmt::Let(pat, None, Some(call)));
                    names.push(name);
                    continue;
                }
                ("Unit", []) => Expr::Tuple(vec![]),
                _ => return Err(invalid("invalid generated body expression")),
            };
            let expression = body.push_expr(expression);
            let name = IdentId::new(self.db, format!("__meta_node_{i}"));
            let pat = body.push_pat(Pat::Path(
                Partial::Present(PathId::from_ident(self.db, name)),
                false,
            ));
            body.emit_stmt(Stmt::Let(pat, None, Some(expression)));
            names.push(name);
        }
        let result = *names
            .get(index(result)?)
            .ok_or_else(|| invalid("result index is out of bounds"))?;
        let result = body.ident_expr(result);
        body.emit_expr_stmt(result);
        Ok(())
    }

    fn function(
        &self,
        ctxt: &mut FileLowerCtxt<'db>,
        value: &FrozenMetaValue,
        origin: GenerationDesugared,
        method: bool,
        inherited_owner_type: Option<TypeId<'db>>,
    ) -> Result<Func<'db>, String> {
        let fields = record(value, MetaItem::FunctionDefinition)?;
        let name = self.name(field(fields, "name")?)?;
        let public = flag(field(fields, "public")?)?;
        let constant = flag(field(fields, "const_function")?)?;
        let parameters = array(field(fields, "parameters")?)?;
        let expressions = array(field(fields, "expressions")?)?;
        if method {
            if public || parameters.is_empty() {
                return Err(invalid(
                    "trait method requires a receiver and cannot be public",
                ));
            }
            let (case, fields) = variant(&parameters[0], MetaItem::DeclarationType)?;
            if !matches!((case, fields), ("Record", [FrozenMetaValue::Structural { item: MetaItem::Record, owner, kind: FrozenHandleKind::Record }]) if *owner == self.target)
            {
                return Err(invalid("method receiver must be the authorized record"));
            }
        }
        let result = field(fields, "result")?;
        let source = top_mod_ast(self.db, ctxt.top_mod());
        let mut builder = HirBuilder::new(ctxt, origin, source.syntax());
        builder.generated_func_with_body_checked(
            name,
            |builder| {
                let owner_type = match inherited_owner_type {
                    Some(owner_type) => owner_type,
                    None => builder.generated_record_type(self.owner)?,
                };
                let params = parameters
                    .iter()
                    .enumerate()
                    .map(|(index, ty)| {
                        if method && index == 0 {
                            return Ok(FuncParam {
                                mode: FuncParamMode::Own,
                                is_mut: false,
                                has_ref_prefix: false,
                                has_own_prefix: true,
                                is_label_suppressed: false,
                                name: Partial::Present(FuncParamName::Ident(IdentId::make_self(
                                    self.db,
                                ))),
                                ty: Partial::Present(TypeId::new(
                                    self.db,
                                    TypeKind::Mode(
                                        TypeMode::Own,
                                        Partial::Present(TypeId::fallback_self_ty(self.db)),
                                    ),
                                )),
                                self_ty_fallback: true,
                            });
                        }
                        Ok(FuncParam {
                            mode: FuncParamMode::View,
                            is_mut: false,
                            has_ref_prefix: false,
                            has_own_prefix: false,
                            is_label_suppressed: true,
                            name: Partial::Present(FuncParamName::Ident(IdentId::new(
                                self.db,
                                format!("__meta_arg_{index}"),
                            ))),
                            ty: Partial::Present(self.ty(ty, owner_type)?),
                            self_ty_fallback: false,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let spec = GeneratedFunctionSpec {
                    name,
                    attrs: builder.empty_attrs(),
                    generic_params: if method {
                        builder.empty_generic_params()
                    } else {
                        builder.owner_generic_params_without_defaults(self.owner)
                    },
                    where_clause: if method {
                        builder.empty_where_clause()
                    } else {
                        builder.owner_where_clause(self.owner)
                    },
                    params: builder.params(params),
                    effects: builder.empty_effect_params(),
                    ret_ty: Some(self.ty(field(fields, "return_type")?, owner_type)?),
                    modifiers: FuncModifiers::new(
                        if public {
                            Visibility::Public
                        } else {
                            Visibility::Private
                        },
                        false,
                        constant,
                        false,
                    ),
                };
                Ok::<(GeneratedFunctionSpec<'db>, TypeId<'db>), String>((spec, owner_type))
            },
            |body, owner_type| {
                self.expressions(
                    body,
                    expressions,
                    result,
                    parameters.len(),
                    method,
                    owner_type,
                )
            },
        )
    }

    fn associated_constant<O>(
        &self,
        builder: &mut HirBuilder<'_, 'db, O>,
        value: &FrozenMetaValue,
        owner_type: TypeId<'db>,
    ) -> Result<AssocConstDef<'db>, String>
    where
        O: Clone + Into<crate::span::DesugaredOrigin>,
    {
        let (case, values) = variant(value, MetaItem::AssociatedConstant)?;
        let Some(name_value) = values.first() else {
            return Err(invalid("invalid associated constant"));
        };
        let name = self.name(name_value)?;
        let ty = match (case, values) {
            ("Bool", [_, _]) => TypeId::new(self.db, TypeKind::Primitive(PrimTy::Bool)),
            ("U256", [_, _]) => {
                TypeId::new(self.db, TypeKind::Primitive(PrimTy::Uint(UintTy::U256)))
            }
            ("Expression", [_, ty, _, _]) => self.ty(ty, owner_type)?,
            _ => return Err(invalid("invalid associated constant")),
        };
        let body =
            builder.generated_body(TrackedItemVariant::Const(Partial::Present(name)), |body| {
                match (case, values) {
                    ("Bool", [_, value]) => {
                        let value = body.push_expr(Expr::Lit(LitKind::Bool(flag(value)?)));
                        body.emit_expr_stmt(value);
                    }
                    (
                        "U256",
                        [
                            _,
                            FrozenMetaValue::Scalar {
                                value: SemConstScalar::Int { value },
                                ..
                            },
                        ],
                    ) => {
                        let value = num_bigint::BigUint::try_from(value)
                            .map_err(|_| invalid("negative u256 literal"))?;
                        let literal =
                            body.push_expr(Expr::Lit(LitKind::Int(IntegerId::new(self.db, value))));
                        let cast = body.push_expr(Expr::Cast(literal, Partial::Present(ty)));
                        body.emit_expr_stmt(cast);
                    }
                    ("Expression", [_, _, FrozenMetaValue::Array(expressions), result]) => {
                        self.expressions(body, expressions, result, 0, false, owner_type)?;
                    }
                    _ => return Err(invalid("invalid associated constant")),
                }
                Ok::<(), String>(())
            })?;
        Ok(AssocConstDef {
            attributes: builder.empty_attrs(),
            name: Partial::Present(name),
            ty: Partial::Present(ty),
            value: Partial::Present(body),
            body_check_policy: Default::default(),
            vis: Visibility::Private,
        })
    }

    fn implementation(
        &self,
        ctxt: &mut FileLowerCtxt<'db>,
        value: &FrozenMetaValue,
        origin: GenerationDesugared,
    ) -> Result<(), String> {
        let fields = record(value, MetaItem::RecordImplementation)?;
        match field(fields, "owner")? {
            FrozenMetaValue::Structural {
                item: MetaItem::Record,
                owner,
                kind: FrozenHandleKind::Record,
            } if *owner == self.target => {}
            _ => {
                return Err(invalid(
                    "implementation owner must be the authorized record",
                ));
            }
        }
        let source = top_mod_ast(self.db, ctxt.top_mod());
        let mut builder = HirBuilder::new(ctxt, origin.clone(), source.syntax());
        builder.generated_impl_with_children(
            |builder| {
                let owner_type = builder.generated_record_type(self.owner)?;
                let path = source_path(
                    self.db,
                    field(fields, "trait_path")?,
                    self.provider_ingot,
                    self.provider_root,
                )?;
                let type_args = self
                    .prefix(field(fields, "types")?)?
                    .into_iter()
                    .map(|ty| {
                        Ok(GenericArg::Type(TypeGenericArg {
                            ty: Partial::Present(self.ty(ty, owner_type)?),
                        }))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let path = if type_args.is_empty() {
                    path
                } else {
                    let parent = path.parent(self.db);
                    let crate::hir_def::PathKind::Ident { ident, .. } = path.kind(self.db) else {
                        return Err(invalid("trait path has no final identifier"));
                    };
                    PathId::new(
                        self.db,
                        crate::hir_def::PathKind::Ident {
                            ident,
                            generic_args: GenericArgListId::given(self.db, type_args),
                        },
                        parent,
                    )
                };
                let spec = GeneratedImplSpec {
                    trait_ref: Partial::Present(TraitRefId::new(self.db, Partial::Present(path))),
                    ty: Partial::Present(owner_type),
                    selection_name: None,
                    attrs: builder.empty_attrs(),
                    generic_params: builder.owner_generic_params_without_defaults(self.owner),
                    where_clause: builder.owner_where_clause(self.owner),
                    types: vec![],
                    consts: vec![],
                };
                Ok::<(GeneratedImplSpec<'db>, TypeId<'db>), String>((spec, owner_type))
            },
            |builder, owner_type| {
                let mut constants = Vec::new();
                for constant in self.prefix(field(fields, "constants")?)? {
                    constants.push(self.associated_constant(builder, constant, owner_type)?);
                }
                if let Some(method) = self.optional(field(fields, "method")?)? {
                    self.function(builder.ctxt(), method, origin, true, Some(owner_type))?;
                }
                Ok::<Vec<AssocConstDef<'db>>, String>(constants)
            },
        )?;
        Ok(())
    }
}

/// Build one request in a single expansion context. Array elements receive
/// their source-slot ordinals, independent of other generation sites.
pub(crate) fn request_fragment<'db>(
    db: &'db dyn HirDb,
    top_mod: TopLevelMod<'db>,
    request: &FrozenGeneration,
    values: &[(usize, &FrozenMetaValue)],
) -> Result<ScopeGraph<'db>, String> {
    let base = base_scope_graph_impl(db, top_mod);
    let root = top_mod_ast(db, top_mod).syntax().clone();
    let target = base
        .items_dfs(db)
        .find_map(|item| {
            let ItemKind::Struct(struct_) = item else {
                return None;
            };
            if authored_path(db, base, struct_.scope())? != request.target.path {
                return None;
            }
            let HirOrigin::Raw(ptr) = struct_.origin(db) else {
                return None;
            };
            let ast = ptr
                .syntax_node_ptr()
                .try_to_node(&root)
                .and_then(ast::Struct::cast)?;
            Some((struct_, ast))
        })
        .ok_or_else(|| invalid("generated target is absent from authored HIR"))?;
    let parent = base
        .edges(target.0.scope())
        .iter()
        .find_map(|edge| matches!(edge.kind, EdgeKind::Lex(_)).then_some(edge.dest))
        .ok_or_else(|| invalid("generated target has no lexical parent"))?;
    let origin = GenerationDesugared {
        target: parser::ast::AstPtr::new(&target.1),
        attribute_index: request.attribute_index,
    };
    let provider_file = db
        .workspace()
        .get(db, &request.provider.file)
        .ok_or_else(|| invalid("generation provider file is missing"))?;
    let provider_ingot = provider_file
        .containing_ingot(db)
        .ok_or_else(|| invalid("generation provider ingot is missing"))?;
    let target_ingot = top_mod.ingot(db);
    let provider_root = if provider_ingot == target_ingot {
        IdentId::new(db, "ingot")
    } else {
        let aliases = target_ingot
            .resolved_external_ingots(db)
            .iter()
            .filter_map(|(alias, ingot)| (*ingot == provider_ingot).then_some(*alias))
            .collect::<Vec<_>>();
        *aliases
            .first()
            .ok_or_else(|| invalid("generation provider ingot needs a direct dependency alias"))?
    };
    let decoder = Decoder {
        db,
        owner: target.0,
        target: request.target.clone(),
        provider_ingot,
        provider_root,
    };
    let mut ctxt = FileLowerCtxt::enter_expansion(db, top_mod);
    ctxt.enter_shim_scope(parent, base.scope_data(&parent).vis);
    if request.shape == FrozenOutputShape::Implementation {
        ctxt.enter_id_namespace(TrackedItemVariant::GeneratedSite(Box::new(
            crate::hir_def::GeneratedSiteKey {
                target: request.target.clone(),
                provider: request.provider.clone(),
                attribute_index: request.attribute_index,
                ordinal: 0,
            },
        )));
        decoder.implementation(&mut ctxt, &request.value, origin)?;
        ctxt.leave_id_namespace();
    } else {
        for (ordinal, value) in values {
            ctxt.enter_id_namespace(TrackedItemVariant::GeneratedSite(Box::new(
                crate::hir_def::GeneratedSiteKey {
                    target: request.target.clone(),
                    provider: request.provider.clone(),
                    attribute_index: request.attribute_index,
                    ordinal: *ordinal,
                },
            )));
            decoder.function(&mut ctxt, value, origin.clone(), false, None)?;
            ctxt.leave_id_namespace();
        }
    }
    ctxt.leave_shim_scope();
    Ok(ctxt.build())
}

fn authored_path<'db>(
    db: &'db dyn HirDb,
    graph: &ScopeGraph<'db>,
    mut scope: ScopeId<'db>,
) -> Option<String> {
    let mut names = Vec::new();
    loop {
        names.push(scope.name(db)?.data(db).clone());
        let parent = graph
            .edges(scope)
            .iter()
            .find_map(|edge| matches!(edge.kind, EdgeKind::Lex(_)).then_some(edge.dest))
            .or_else(|| {
                graph
                    .edges(scope)
                    .iter()
                    .find_map(|edge| matches!(edge.kind, EdgeKind::Super(_)).then_some(edge.dest))
            });
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
    if let Some(url) = top_mod.file(db).url(db)
        && let Some(module) = db.generated_module(&url)
    {
        for request in module.requests(db) {
            let functions = match request.shape {
                FrozenOutputShape::Function => vec![(0, &request.value)],
                FrozenOutputShape::Functions => match array(&request.value) {
                    Ok(values) => {
                        let mut functions = Vec::new();
                        for (ordinal, value) in values.iter().enumerate() {
                            match variant(value, MetaItem::Option) {
                                Ok(("Some", [function])) => functions.push((ordinal, function)),
                                Ok(("None", [])) => {}
                                _ => errors.push(GeneratedHirError {
                                    message: invalid("invalid optional generated function"),
                                    site_span: request.site_span.clone(),
                                }),
                            }
                        }
                        functions
                    }
                    Err(error) => {
                        errors.push(GeneratedHirError {
                            message: error,
                            site_span: request.site_span.clone(),
                        });
                        continue;
                    }
                },
                FrozenOutputShape::Implementation => vec![],
            };
            if functions.is_empty() && request.shape != FrozenOutputShape::Implementation {
                continue;
            }
            match request_fragment(db, top_mod, request, &functions) {
                Ok(fragment) => {
                    for (scope_id, scope) in &fragment.scopes {
                        if let Some(existing) = scopes.get_mut(scope_id) {
                            existing.edges.extend(scope.edges.iter().cloned());
                        } else {
                            scopes.insert(*scope_id, scope.clone());
                        }
                    }
                    unresolved_uses.extend(fragment.unresolved_uses);
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
    }
}

/// Output-stage construction failures point back to the generation request.
pub fn generated_hir_errors<'db>(
    db: &'db dyn LowerHirDb,
    top_mod: TopLevelMod<'db>,
) -> &'db [GeneratedHirError] {
    &generated_functions_impl(db, top_mod).errors
}
