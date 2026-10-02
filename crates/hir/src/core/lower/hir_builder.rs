use std::{cell::Cell, collections::HashMap, rc::Rc};

use common::ingot::IngotKind;
use num_bigint::Sign;
use parser::ast;
use parser::ast::prelude::AstNode;
use parser::{SyntaxKind, SyntaxNode};
use rustc_hash::FxHashSet;

use crate::{
    HirDb,
    analysis::{
        semantic::SemConstScalar,
        ty::{adt_def::AdtRef, ty_def::PrimTy as SemPrimTy},
    },
    core::meta::{
        FrozenCode, FrozenCodeHole, FrozenMetaValue, FrozenQuoteConstructorSite,
        FrozenTemplateSource,
    },
    hir_def::{
        ArithBinOp, AssocConstDef, AssocTyDef, Attr, AttrArg, AttrListId, BinOp, Body, BodyKind,
        Cond, CondId, ConstGenericArg, ConstGenericArgValue, EffectParamListId, Expr, ExprId,
        FieldDefListId, FieldIndex, Func, FuncModifiers, FuncParam, FuncParamListId, FuncParamMode,
        FuncParamName, GenericArg, GenericArgListId, GenericParam, GenericParamListId,
        GenericParamOwner, IdentId, ImplTrait, IntegerId, ItemKind, LitKind, Mod, NormalAttr,
        Partial, Pat, PatId, PathId, PathKind, Stmt, StmtId, Struct, TopLevelMod, TrackedItemId,
        TrackedItemVariant, Trait, TraitRefId, TypeBound, TypeGenericArg, TypeGenericParam, TypeId,
        TypeKind, TypeMode, Visibility, WhereClauseId, WhereClauseOwner, WherePredicate,
        expr::CallArg,
        prim_ty::{IntTy, PrimTy as HirPrimTy, UintTy},
        scope_graph::ScopeId,
    },
    span::{DesugaredOrigin, HirOrigin},
};

use super::{FileLowerCtxt, body::BodyCtxt};

#[derive(Clone, Copy, Debug)]
pub(super) struct LibRoots<'db> {
    pub(super) core: IdentId<'db>,
    pub(super) std: IdentId<'db>,
}

impl<'db> LibRoots<'db> {
    pub(super) fn for_ctxt(ctxt: &FileLowerCtxt<'db>) -> Self {
        let db = ctxt.db();
        let ingot = ctxt.top_mod().ingot(db);

        let core = if ingot.kind(db) == IngotKind::Core {
            IdentId::make_ingot(db)
        } else {
            IdentId::make_core(db)
        };
        let std = if ingot.kind(db) == IngotKind::Std {
            IdentId::make_ingot(db)
        } else {
            IdentId::new(db, "std".to_string())
        };

        Self { core, std }
    }
}

fn rebind_owner_bound_type<'db>(
    db: &'db dyn HirDb,
    ty: TypeId<'db>,
    renames: &HashMap<IdentId<'db>, IdentId<'db>>,
) -> Result<TypeId<'db>, String> {
    let kind = match ty.data(db) {
        TypeKind::Path(Partial::Present(path)) => TypeKind::Path(Partial::Present(
            rebind_owner_bound_path(db, *path, renames)?,
        )),
        TypeKind::Mode(mode, Partial::Present(inner)) => TypeKind::Mode(
            *mode,
            Partial::Present(rebind_owner_bound_type(db, *inner, renames)?),
        ),
        TypeKind::Ptr(Partial::Present(inner)) => TypeKind::Ptr(Partial::Present(
            rebind_owner_bound_type(db, *inner, renames)?,
        )),
        TypeKind::Tuple(elements) => TypeKind::Tuple(crate::hir_def::TupleTypeId::new(
            db,
            elements
                .data(db)
                .iter()
                .map(|element| {
                    element
                        .to_opt()
                        .ok_or_else(|| "quoted target bound type is missing".to_owned())
                        .and_then(|ty| rebind_owner_bound_type(db, ty, renames))
                        .map(Partial::Present)
                })
                .collect::<Result<Vec<_>, String>>()?,
        )),
        TypeKind::Primitive(_) | TypeKind::Never => ty.data(db).clone(),
        _ => return Err("quoted target bound type needs checked reification".to_owned()),
    };
    Ok(TypeId::new(db, kind))
}

fn rebind_owner_bound_path<'db>(
    db: &'db dyn HirDb,
    path: PathId<'db>,
    renames: &HashMap<IdentId<'db>, IdentId<'db>>,
) -> Result<PathId<'db>, String> {
    let parent = path
        .parent(db)
        .map(|parent| rebind_owner_bound_path(db, parent, renames))
        .transpose()?;
    let kind = match path.kind(db) {
        PathKind::Ident {
            ident: Partial::Present(ident),
            generic_args,
        } => {
            let args = generic_args
                .data(db)
                .iter()
                .map(|arg| match arg {
                    GenericArg::Type(arg) => {
                        let ty = arg
                            .ty
                            .to_opt()
                            .ok_or_else(|| "quoted target bound argument is missing".to_owned())?;
                        Ok(GenericArg::Type(TypeGenericArg {
                            ty: Partial::Present(rebind_owner_bound_type(db, ty, renames)?),
                        }))
                    }
                    GenericArg::AssocType(arg) => {
                        let ty = arg.ty.to_opt().ok_or_else(|| {
                            "quoted target associated bound argument is missing".to_owned()
                        })?;
                        Ok(GenericArg::AssocType(crate::hir_def::AssocTypeGenericArg {
                            name: arg.name,
                            ty: Partial::Present(rebind_owner_bound_type(db, ty, renames)?),
                        }))
                    }
                    GenericArg::Const(_) => Err(
                        "quoted target const bound argument needs checked reification".to_owned(),
                    ),
                })
                .collect::<Result<Vec<_>, String>>()?;
            PathKind::Ident {
                ident: Partial::Present(if parent.is_none() {
                    renames.get(&ident).copied().unwrap_or(ident)
                } else {
                    ident
                }),
                generic_args: GenericArgListId::new(db, args, generic_args.is_given(db)),
            }
        }
        PathKind::BoundIngot { .. } => path.kind(db),
        PathKind::QualifiedType { type_, trait_ } => {
            let trait_path = trait_
                .path(db)
                .to_opt()
                .ok_or_else(|| "quoted target qualified bound trait is missing".to_owned())?;
            PathKind::QualifiedType {
                type_: rebind_owner_bound_type(db, type_, renames)?,
                trait_: TraitRefId::new(
                    db,
                    Partial::Present(rebind_owner_bound_path(db, trait_path, renames)?),
                ),
            }
        }
        _ => return Err("quoted target bound path needs checked reification".to_owned()),
    };
    Ok(PathId::new(db, kind, parent))
}

pub(super) fn rebind_owner_bound<'db>(
    db: &'db dyn HirDb,
    bound: &TypeBound<'db>,
    renames: &HashMap<IdentId<'db>, IdentId<'db>>,
) -> Result<TypeBound<'db>, String> {
    let TypeBound::Trait(trait_ref) = bound else {
        return Ok(bound.clone());
    };
    let trait_path = trait_ref
        .path(db)
        .to_opt()
        .ok_or_else(|| "quoted target where bound is missing".to_owned())?;
    if trait_path
        .root_ident(db)
        .is_some_and(|root| renames.contains_key(&root))
    {
        return Err("quoted target where bound is not a trait".to_owned());
    }
    let path = rebind_owner_bound_path(db, trait_path, renames)?;
    Ok(TypeBound::Trait(TraitRefId::new(
        db,
        Partial::Present(path),
    )))
}

pub(super) struct HirBuilder<'ctxt, 'db, O>
where
    O: Clone + Into<DesugaredOrigin>,
{
    ctxt: &'ctxt mut FileLowerCtxt<'db>,
    roots: LibRoots<'db>,
    desugared: O,
    reserved: Rc<FxHashSet<String>>,
}

/// `__fe_{name}`, or `__fe_{name}_{n}` for the first `n` that the desugared
/// source does not mention. Generated code then cannot capture a name that a
/// field type refers to, whichever namespace it is in.
fn fresh_ident<'db>(db: &'db dyn HirDb, reserved: &FxHashSet<String>, name: &str) -> IdentId<'db> {
    let mut ident = format!("__fe_{name}");
    let mut suffix = 0;
    while reserved.contains(&ident) {
        suffix += 1;
        ident = format!("__fe_{name}_{suffix}");
    }
    IdentId::new(db, ident)
}

/// Complete declaration data for a generated function with a body.
///
/// Callers that mirror an existing declaration can preserve every signature
/// component here. Narrow compiler-generated helpers use the convenience
/// wrappers below, which intentionally provide empty predicates and effects.
pub(super) struct GeneratedFunctionSpec<'db> {
    pub(super) name: IdentId<'db>,
    pub(super) attrs: AttrListId<'db>,
    pub(super) generic_params: GenericParamListId<'db>,
    pub(super) where_clause: WhereClauseId<'db>,
    pub(super) params: FuncParamListId<'db>,
    pub(super) effects: EffectParamListId<'db>,
    pub(super) ret_ty: Option<TypeId<'db>>,
    pub(super) modifiers: FuncModifiers,
}

/// Complete declaration data for a generated trait implementation.
///
/// The impl's parameters and requirements are part of its meaning. A caller
/// building an impl from a checked description must supply them explicitly.
pub(super) struct GeneratedImplSpec<'db> {
    pub(super) trait_ref: Partial<TraitRefId<'db>>,
    pub(super) ty: Partial<TypeId<'db>>,
    pub(super) selection_name: Option<IdentId<'db>>,
    pub(super) attrs: AttrListId<'db>,
    pub(super) generic_params: GenericParamListId<'db>,
    pub(super) where_clause: WhereClauseId<'db>,
    pub(super) types: Vec<AssocTyDef<'db>>,
    pub(super) consts: Vec<AssocConstDef<'db>>,
}

impl<'ctxt, 'db, O> HirBuilder<'ctxt, 'db, O>
where
    O: Clone + Into<DesugaredOrigin>,
{
    /// A builder for the items desugared from `source`, whose identifiers
    /// generated names avoid.
    pub(super) fn new(
        ctxt: &'ctxt mut FileLowerCtxt<'db>,
        desugared: O,
        source: &SyntaxNode,
    ) -> Self {
        let roots = LibRoots::for_ctxt(ctxt);
        let reserved = source
            .descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() == SyntaxKind::Ident)
            .map(|token| token.text().to_string())
            .collect();
        Self {
            ctxt,
            roots,
            desugared,
            reserved: Rc::new(reserved),
        }
    }

    pub(super) fn db(&self) -> &'db dyn HirDb {
        self.ctxt.db()
    }

    pub(super) fn ctxt(&mut self) -> &mut FileLowerCtxt<'db> {
        self.ctxt
    }

    pub(super) fn top_mod(&self) -> TopLevelMod<'db> {
        self.ctxt.top_mod()
    }

    pub(super) fn ident(&self, name: &str) -> IdentId<'db> {
        IdentId::new(self.db(), name.to_string())
    }

    pub(super) fn generated_ident(&self, name: &str) -> IdentId<'db> {
        fresh_ident(self.db(), &self.reserved, name)
    }

    pub(super) fn roots(&self) -> LibRoots<'db> {
        self.roots
    }

    pub(super) fn origin<T>(&self) -> HirOrigin<T>
    where
        T: AstNode<Language = parser::FeLang>,
    {
        HirOrigin::desugared(self.desugared.clone())
    }

    pub(super) fn with_desugared<O2>(&mut self, desugared: O2) -> HirBuilder<'_, 'db, O2>
    where
        O2: Clone + Into<DesugaredOrigin>,
    {
        HirBuilder {
            ctxt: self.ctxt,
            roots: self.roots,
            desugared,
            reserved: self.reserved.clone(),
        }
    }

    pub(super) fn empty_attrs(&self) -> AttrListId<'db> {
        AttrListId::new(self.db(), vec![])
    }

    pub(super) fn inline_always_attrs(&self) -> AttrListId<'db> {
        AttrListId::new(self.db(), vec![self.flag_attr("inline", "always")])
    }

    /// `#[inline(always)]` and `#[arithmetic(unchecked)]`.
    pub(super) fn inline_always_unchecked_attrs(&self) -> AttrListId<'db> {
        AttrListId::new(
            self.db(),
            vec![
                self.flag_attr("inline", "always"),
                self.flag_attr("arithmetic", "unchecked"),
            ],
        )
    }

    /// `#[name(arg)]`
    fn flag_attr(&self, name: &str, arg: &str) -> Attr<'db> {
        let db = self.db();
        Attr::Normal(NormalAttr {
            path: Partial::Present(PathId::from_ident(db, self.ident(name))),
            value: None,
            has_value: false,
            has_args: true,
            args: vec![AttrArg {
                key: Partial::Present(PathId::from_ident(db, self.ident(arg))),
                value: None,
                has_value: false,
            }],
        })
    }

    pub(super) fn empty_generic_params(&self) -> GenericParamListId<'db> {
        GenericParamListId::new(self.db(), vec![])
    }

    pub(super) fn empty_where_clause(&self) -> WhereClauseId<'db> {
        WhereClauseId::new(self.db(), vec![])
    }

    pub(super) fn empty_effect_params(&self) -> EffectParamListId<'db> {
        EffectParamListId::new(self.db(), vec![])
    }

    pub(super) fn self_ty(&self) -> TypeId<'db> {
        TypeId::fallback_self_ty(self.db())
    }

    pub(super) fn path_from_root(&self, root: IdentId<'db>, segments: &[&str]) -> PathId<'db> {
        let db = self.db();
        let mut path = PathId::from_ident(db, root);
        for seg in segments {
            path = path.push_str(db, seg);
        }
        path
    }

    pub(super) fn ty_path(&self, path: PathId<'db>) -> TypeId<'db> {
        TypeId::new(self.db(), TypeKind::Path(Partial::Present(path)))
    }

    pub(super) fn ty_ident(&self, ident: IdentId<'db>) -> TypeId<'db> {
        self.ty_path(PathId::from_ident(self.db(), ident))
    }

    pub(super) fn ty_ptr(&self, pointee: TypeId<'db>) -> TypeId<'db> {
        TypeId::new(self.db(), TypeKind::Ptr(Partial::Present(pointee)))
    }

    pub(super) fn sol_ty(&self) -> TypeId<'db> {
        let path = self.path_from_root(self.roots.std, &["abi", "Sol"]);
        self.ty_path(path)
    }

    pub(super) fn sol_args(&self) -> GenericArgListId<'db> {
        GenericArgListId::given1_type(self.db(), self.sol_ty())
    }

    pub(super) fn abi_record_args(&mut self, field_count: usize) -> GenericArgListId<'db> {
        let id = self.ctxt.joined_id(TrackedItemVariant::NamelessBody);
        let origin = self.origin();
        let mut body = BodyCtxt::new(self.ctxt, id);
        let db = body.f_ctxt.db();
        let value = body.push_expr(
            Expr::Lit(LitKind::Int(IntegerId::from_usize(db, field_count))),
            origin.clone(),
        );
        let body = body.build_with_origin(origin, value, BodyKind::Anonymous);
        GenericArgListId::given(
            db,
            vec![GenericArg::Const(ConstGenericArg {
                value: ConstGenericArgValue::Expr(Partial::Present(body)),
            })],
        )
    }

    pub(super) fn abi_record_trait_ref(&mut self, field_count: usize) -> TraitRefId<'db> {
        let db = self.db();
        let args = self.abi_record_args(field_count);
        let path = PathId::from_ident(db, self.roots.core)
            .push_str(db, "abi")
            .push_str_args(db, "AbiRecord", args);
        TraitRefId::new(db, Partial::Present(path))
    }

    pub(super) fn core_abi_trait_ref_sol(&self, name: &str) -> TraitRefId<'db> {
        let db = self.db();
        let path = PathId::from_ident(db, self.roots.core)
            .push_str(db, "abi")
            .push_str_args(db, name, self.sol_args());
        TraitRefId::new(db, Partial::Present(path))
    }

    pub(super) fn core_abi_trait_ref(&self, name: &str) -> TraitRefId<'db> {
        let db = self.db();
        let path = PathId::from_ident(db, self.roots.core)
            .push_str(db, "abi")
            .push_str(db, name);
        TraitRefId::new(db, Partial::Present(path))
    }

    /// A generated generic parameter, named so that it cannot shadow a user
    /// type spelled `name` in the generated signature or body.
    pub(super) fn type_param_with_trait_bound(
        &self,
        name: &str,
        bound: TraitRefId<'db>,
    ) -> (GenericParamListId<'db>, TypeId<'db>) {
        let db = self.db();
        let ident = self.generated_ident(name);
        let params = GenericParamListId::new(
            db,
            vec![GenericParam::Type(TypeGenericParam {
                name: Partial::Present(ident),
                bounds: vec![TypeBound::Trait(bound)],
                default_ty: None,
            })],
        );
        (params, self.ty_ident(ident))
    }

    pub(super) fn param_own_self(&self) -> FuncParam<'db> {
        let db = self.db();
        FuncParam {
            mode: FuncParamMode::Own,
            is_mut: false,
            has_ref_prefix: false,
            has_own_prefix: false,
            is_label_suppressed: false,
            name: Partial::Present(FuncParamName::Ident(IdentId::make_self(db))),
            ty: Partial::Present(self.self_ty()),
            self_ty_fallback: true,
        }
    }

    pub(super) fn param_mut_underscore_named(
        &self,
        name: IdentId<'db>,
        ty: TypeId<'db>,
    ) -> FuncParam<'db> {
        FuncParam {
            mode: FuncParamMode::View,
            is_mut: false,
            has_ref_prefix: false,
            has_own_prefix: false,
            is_label_suppressed: true,
            name: Partial::Present(FuncParamName::Ident(name)),
            ty: Partial::Present(TypeId::new(
                self.db(),
                TypeKind::Mode(TypeMode::Mut, Partial::Present(ty)),
            )),
            self_ty_fallback: false,
        }
    }

    pub(super) fn param_underscore_named(
        &self,
        name: IdentId<'db>,
        ty: TypeId<'db>,
    ) -> FuncParam<'db> {
        FuncParam {
            mode: FuncParamMode::View,
            is_mut: false,
            has_ref_prefix: false,
            has_own_prefix: false,
            is_label_suppressed: true,
            name: Partial::Present(FuncParamName::Ident(name)),
            ty: Partial::Present(ty),
            self_ty_fallback: false,
        }
    }

    pub(super) fn params(
        &self,
        params: impl IntoIterator<Item = FuncParam<'db>>,
    ) -> FuncParamListId<'db> {
        FuncParamListId::new(self.db(), params.into_iter().collect::<Vec<_>>())
    }

    pub(super) fn assoc_ty(&self, name: &str, type_ref: Partial<TypeId<'db>>) -> AssocTyDef<'db> {
        AssocTyDef {
            generic_params: GenericParamListId::new(self.db(), Vec::new()),
            attributes: self.empty_attrs(),
            name: Partial::Present(self.ident(name)),
            type_ref,
        }
    }

    pub(super) fn with_mod_scope<R>(
        &mut self,
        id: TrackedItemVariant<'db>,
        build: impl FnOnce(&mut Self, TrackedItemId<'db>) -> R,
    ) -> R {
        let id = self.ctxt.joined_id(id);
        self.ctxt.enter_item_scope(id, true);
        build(self, id)
    }

    pub(super) fn with_item_scope<I>(
        &mut self,
        id: TrackedItemVariant<'db>,
        build: impl FnOnce(&mut Self, TrackedItemId<'db>) -> I,
    ) -> I
    where
        I: Into<ItemKind<'db>> + Copy,
    {
        let id = self.ctxt.joined_id(id);
        self.ctxt.enter_item_scope(id, false);
        let item = build(self, id);
        self.ctxt.leave_item_scope(item);
        item
    }

    pub(super) fn finish_mod(
        &mut self,
        id: TrackedItemId<'db>,
        name: Partial<IdentId<'db>>,
        attributes: AttrListId<'db>,
        vis: Visibility,
        origin: HirOrigin<ast::Mod>,
    ) -> Mod<'db> {
        let mod_ = Mod::new(self.db(), id, name, attributes, vis, self.top_mod(), origin);
        self.ctxt.leave_item_scope(mod_)
    }

    pub(super) fn desugared_mod(
        &mut self,
        name: Partial<IdentId<'db>>,
        attributes: AttrListId<'db>,
        vis: Visibility,
        build: impl FnOnce(&mut Self),
    ) -> Mod<'db> {
        self.with_mod_scope(TrackedItemVariant::Mod(name), |this, id| {
            this.ctxt.insert_synthetic_prelude_use();
            this.ctxt.insert_synthetic_super_use();
            build(this);
            this.finish_mod(id, name, attributes, vis, this.origin())
        })
    }

    pub(super) fn struct_item(
        &mut self,
        name: Partial<IdentId<'db>>,
        attributes: AttrListId<'db>,
        vis: Visibility,
        generic_params: GenericParamListId<'db>,
        where_clause: WhereClauseId<'db>,
        fields: FieldDefListId<'db>,
    ) -> Struct<'db> {
        self.with_item_scope(TrackedItemVariant::Struct(name), |this, id| {
            Struct::new(
                this.db(),
                id,
                name,
                attributes,
                vis,
                generic_params,
                where_clause,
                fields,
                this.top_mod(),
                this.origin(),
            )
        })
    }

    pub(super) fn struct_simple(
        &mut self,
        name: Partial<IdentId<'db>>,
        attributes: AttrListId<'db>,
        vis: Visibility,
        fields: FieldDefListId<'db>,
    ) -> Struct<'db> {
        self.struct_item(
            name,
            attributes,
            vis,
            self.empty_generic_params(),
            self.empty_where_clause(),
            fields,
        )
    }

    pub(super) fn pub_struct(
        &mut self,
        name: Partial<IdentId<'db>>,
        attributes: AttrListId<'db>,
        fields: FieldDefListId<'db>,
    ) -> Struct<'db> {
        self.struct_simple(name, attributes, Visibility::Public, fields)
    }

    pub(super) fn new_impl_trait_complete(
        &mut self,
        id: TrackedItemId<'db>,
        spec: GeneratedImplSpec<'db>,
        origin: HirOrigin<ast::ImplTrait>,
    ) -> ImplTrait<'db> {
        ImplTrait::new(
            self.db(),
            id,
            spec.trait_ref,
            spec.ty,
            spec.selection_name,
            spec.attrs,
            spec.generic_params,
            spec.where_clause,
            spec.types,
            spec.consts,
            self.top_mod(),
            origin,
        )
    }

    pub(super) fn generated_impl_with_children<E, T>(
        &mut self,
        build_spec: impl FnOnce(&mut Self) -> Result<(GeneratedImplSpec<'db>, T), E>,
        build_children: impl FnOnce(&mut Self, T) -> Result<Vec<AssocConstDef<'db>>, E>,
    ) -> Result<ImplTrait<'db>, E> {
        let index = self.ctxt.next_impl_trait_idx();
        let id = self.ctxt.joined_id(TrackedItemVariant::ImplTrait(index));
        self.ctxt.enter_item_scope(id, false);
        let (mut spec, state) = build_spec(self)?;
        spec.consts = build_children(self, state)?;
        let impl_trait = self.new_impl_trait_complete(id, spec, self.origin());
        Ok(self.ctxt.leave_item_scope(impl_trait))
    }

    pub(super) fn quoted_owner_where_clause(
        &mut self,
        owner: crate::analysis::ty::adt_def::AdtRef<'db>,
        names: &[IdentId<'db>],
    ) -> Result<WhereClauseId<'db>, String> {
        let db = self.db();
        let params = match owner {
            crate::analysis::ty::adt_def::AdtRef::Struct(owner) => GenericParamOwner::Struct(owner),
            crate::analysis::ty::adt_def::AdtRef::Enum(owner) => GenericParamOwner::Enum(owner),
        }
        .params_list(db);
        if params.data(db).len() != names.len() {
            return Err("quoted target parameter count changed".to_owned());
        }
        let renames = params
            .data(db)
            .iter()
            .zip(names)
            .map(|(param, name)| {
                Ok((
                    param
                        .name()
                        .to_opt()
                        .ok_or_else(|| "quoted target parameter has no name".to_owned())?,
                    *name,
                ))
            })
            .collect::<Result<HashMap<_, _>, String>>()?;
        let source = match owner {
            crate::analysis::ty::adt_def::AdtRef::Struct(owner) => WhereClauseOwner::Struct(owner),
            crate::analysis::ty::adt_def::AdtRef::Enum(owner) => WhereClauseOwner::Enum(owner),
        }
        .where_clause(db);
        let predicates = source
            .type_predicates(db)
            .map(|(_, predicate)| {
                let ty = predicate
                    .ty
                    .to_opt()
                    .ok_or_else(|| "quoted target where type is missing".to_owned())?;
                let TypeKind::Path(Partial::Present(path)) = ty.data(db) else {
                    return Err("quoted target where type needs checked reification".to_owned());
                };
                if path.parent(db).is_some() || !path.generic_args(db).is_empty(db) {
                    return Err("quoted target where type needs checked reification".to_owned());
                }
                let original = path
                    .ident(db)
                    .to_opt()
                    .ok_or_else(|| "quoted target where binder is missing".to_owned())?;
                let name = renames
                    .get(&original)
                    .ok_or_else(|| "quoted target where type is not an owner binder".to_owned())?;
                let bounds = predicate
                    .bounds
                    .iter()
                    .map(|bound| rebind_owner_bound(db, bound, &renames))
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(WherePredicate {
                    ty: Partial::Present(TypeId::new(
                        db,
                        TypeKind::Path(Partial::Present(PathId::from_ident(db, *name))),
                    )),
                    bounds,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut const_predicates = Vec::new();
        for (index, predicate) in source.const_predicates(db).iter().enumerate() {
            let index = u32::try_from(index)
                .map_err(|_| "quoted target const predicate count exceeds u32".to_owned())?;
            if !predicate.stmts(db).is_empty()
                || !predicate.pats(db).is_empty()
                || !predicate.conds(db).is_empty()
            {
                return Err("quoted target const predicate needs checked reification".to_owned());
            }
            let id = self
                .ctxt
                .joined_id(TrackedItemVariant::WhereConstPredicate(index));
            let mut body = BodyCtxt::new(self.ctxt, id);
            body.stmts = predicate.stmts(db).clone();
            body.exprs = predicate.exprs(db).clone();
            body.conds = predicate.conds(db).clone();
            body.pats = predicate.pats(db).clone();
            body.source_map = predicate.source_map(db).clone();
            for (_, expr) in body.exprs.iter_mut() {
                match expr {
                    Partial::Present(Expr::Path(Partial::Present(path))) => {
                        if path.parent(db).is_some() || !path.generic_args(db).is_empty(db) {
                            return Err(
                                "quoted target const path needs checked reification".to_owned()
                            );
                        }
                        let original = path
                            .ident(db)
                            .to_opt()
                            .ok_or_else(|| "quoted target const binder is missing".to_owned())?;
                        let name = renames.get(&original).ok_or_else(|| {
                            "quoted target const path needs checked reification".to_owned()
                        })?;
                        *path = PathId::from_ident(db, *name);
                    }
                    Partial::Present(Expr::Lit(_) | Expr::Bin(..) | Expr::Un(..)) => {}
                    _ => {
                        return Err(
                            "quoted target const predicate needs checked reification".to_owned()
                        );
                    }
                }
            }
            const_predicates.push(body.build(None, predicate.expr(db), BodyKind::Anonymous));
        }
        let mut generated_types = predicates.into_iter();
        let mut generated = const_predicates.into_iter();
        let predicates = source
            .predicates(db)
            .iter()
            .map(|predicate| match predicate {
                crate::hir_def::WhereClausePredicate::Type(_) => {
                    crate::hir_def::WhereClausePredicate::Type(
                        generated_types.next().expect("generated type predicate"),
                    )
                }
                crate::hir_def::WhereClausePredicate::Const(_) => {
                    crate::hir_def::WhereClausePredicate::Const(
                        generated.next().expect("generated predicate body"),
                    )
                }
            })
            .collect::<Vec<_>>();
        Ok(WhereClauseId::new(db, predicates))
    }

    pub(super) fn generated_record_type_with_names(
        &mut self,
        owner: crate::analysis::ty::adt_def::AdtRef<'db>,
        names: &[IdentId<'db>],
    ) -> Result<TypeId<'db>, String> {
        let db = self.db();
        let name = match owner {
            crate::analysis::ty::adt_def::AdtRef::Struct(owner) => owner.name(db).to_opt(),
            crate::analysis::ty::adt_def::AdtRef::Enum(owner) => owner.name(db).to_opt(),
        }
        .ok_or_else(|| "authorized record name is missing".to_owned())?;
        let params = match owner {
            crate::analysis::ty::adt_def::AdtRef::Struct(owner) => GenericParamOwner::Struct(owner),
            crate::analysis::ty::adt_def::AdtRef::Enum(owner) => GenericParamOwner::Enum(owner),
        }
        .params_list(db);
        if params.data(db).len() != names.len() {
            return Err("record generic parameter count changed".to_owned());
        }
        let mut args = Vec::new();
        for (param, param_name) in params.data(db).iter().zip(names) {
            let argument = match param {
                GenericParam::Type(_) => GenericArg::Type(TypeGenericArg {
                    ty: Partial::Present(TypeId::new(
                        db,
                        TypeKind::Path(Partial::Present(PathId::from_ident(db, *param_name))),
                    )),
                }),
                GenericParam::Const(_) => {
                    self.ctxt
                        .enter_id_namespace(TrackedItemVariant::Const(Partial::Present(
                            *param_name,
                        )));
                    let id = self.ctxt.joined_id(TrackedItemVariant::NamelessBody);
                    let origin = self.origin();
                    let mut body = BodyCtxt::new(self.ctxt, id);
                    let expr = body.push_expr(
                        Expr::Path(Partial::Present(PathId::from_ident(db, *param_name))),
                        origin,
                    );
                    let body = body.build(None, expr, BodyKind::Anonymous);
                    self.ctxt.leave_id_namespace();
                    GenericArg::Const(ConstGenericArg {
                        value: ConstGenericArgValue::Expr(Partial::Present(body)),
                    })
                }
            };
            args.push(argument);
        }
        let path = if args.is_empty() {
            PathId::from_ident(db, name)
        } else {
            PathId::new(
                db,
                PathKind::Ident {
                    ident: Partial::Present(name),
                    generic_args: GenericArgListId::given(db, args),
                },
                None,
            )
        };
        Ok(TypeId::new(db, TypeKind::Path(Partial::Present(path))))
    }

    pub(super) fn generated_func_with_body_checked<E, T>(
        &mut self,
        name: IdentId<'db>,
        build_spec: impl FnOnce(&mut Self) -> Result<(GeneratedFunctionSpec<'db>, T), E>,
        build_body: impl FnOnce(&mut BodyBuilder<'_, 'db, O>, T) -> Result<(), E>,
    ) -> Result<Func<'db>, E> {
        let id = self
            .ctxt
            .joined_id(TrackedItemVariant::Func(Partial::Present(name)));
        self.ctxt.enter_item_scope(id, false);
        let (spec, state) = build_spec(self)?;
        let roots = self.roots;
        let origin = self.desugared.clone();
        let mut body = BodyBuilder::new(
            self.ctxt,
            roots,
            origin,
            self.reserved.clone(),
            TrackedItemVariant::FuncBody,
        );
        build_body(&mut body, state)?;
        let body = body.finish();
        let function = Func::new(
            self.db(),
            id,
            Partial::Present(spec.name),
            spec.attrs,
            spec.generic_params,
            spec.where_clause,
            Partial::Present(spec.params),
            spec.effects,
            spec.ret_ty,
            spec.modifiers,
            Some(body),
            self.top_mod(),
            self.origin(),
        );
        Ok(self.ctxt.leave_item_scope(function))
    }

    pub(super) fn generated_body<E>(
        &mut self,
        id: TrackedItemVariant<'db>,
        build_body: impl FnOnce(&mut BodyBuilder<'_, 'db, O>) -> Result<(), E>,
    ) -> Result<Body<'db>, E> {
        let mut body = BodyBuilder::new(
            self.ctxt,
            self.roots,
            self.desugared.clone(),
            self.reserved.clone(),
            id,
        );
        build_body(&mut body)?;
        Ok(body.finish())
    }

    pub(super) fn generated_quote_const_predicate(
        &mut self,
        index: u32,
        template: Body<'db>,
        root: ExprId,
        provider_scope: ScopeId<'db>,
        holes: &[FrozenCodeHole],
        rooted_funcs: &[(ExprId, PathId<'db>)],
        generic_uses: &[(ExprId, IdentId<'db>)],
    ) -> Result<(Body<'db>, Vec<ExprId>, Vec<ExprId>), String> {
        let db = self.db();
        let mut body = BodyBuilder::new(
            self.ctxt,
            self.roots,
            self.desugared.clone(),
            self.reserved.clone(),
            TrackedItemVariant::WhereConstPredicate(index),
        );
        let mut map = QuoteCopyMap::new(db, holes, rooted_funcs, &[], &[], generic_uses, &[])?;
        let root = body.copy_quote_expr(template, root, &mut map)?;
        for site in &map.method_calls {
            body.body.quote_method_scopes.push((*site, provider_scope));
        }
        let copied = |sites: Vec<ExprId>| {
            sites
                .into_iter()
                .map(|site| {
                    map.exprs
                        .get(&site)
                        .copied()
                        .ok_or_else(|| "quoted const predicate name was not copied".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()
        };
        let rooted = copied(rooted_funcs.iter().map(|(site, _)| *site).collect())?;
        body.body
            .quote_path_scopes
            .extend(rooted.iter().map(|site| (*site, provider_scope)));
        let generics = copied(generic_uses.iter().map(|(site, _)| *site).collect())?;
        Ok((
            body.body.build(None, root, BodyKind::Anonymous),
            rooted,
            generics,
        ))
    }

    pub(super) fn new_impl_trait(
        &mut self,
        id: TrackedItemId<'db>,
        trait_ref: Partial<TraitRefId<'db>>,
        ty: Partial<TypeId<'db>>,
        types: Vec<AssocTyDef<'db>>,
        consts: Vec<AssocConstDef<'db>>,
        origin: HirOrigin<ast::ImplTrait>,
    ) -> ImplTrait<'db> {
        let spec = self.impl_spec(trait_ref, ty, types, consts);
        self.new_impl_trait_complete(id, spec, origin)
    }

    pub(super) fn impl_trait(
        &mut self,
        trait_ref: TraitRefId<'db>,
        ty: TypeId<'db>,
        build_children: impl FnOnce(&mut Self),
    ) -> ImplTrait<'db> {
        self.impl_trait_with_children(trait_ref, ty, vec![], vec![], build_children)
    }

    pub(super) fn impl_trait_assocs_build(
        &mut self,
        trait_ref: TraitRefId<'db>,
        ty: TypeId<'db>,
        build_assocs: impl FnOnce(&mut Self) -> (Vec<AssocTyDef<'db>>, Vec<AssocConstDef<'db>>),
    ) -> ImplTrait<'db> {
        let trait_ref = Partial::Present(trait_ref);
        let ty = Partial::Present(ty);

        let idx = self.ctxt.next_impl_trait_idx();
        self.with_item_scope(TrackedItemVariant::ImplTrait(idx), |this, id| {
            let (types, consts) = build_assocs(this);
            let spec = this.impl_spec(trait_ref, ty, types, consts);
            this.new_impl_trait_complete(id, spec, this.origin())
        })
    }

    pub(super) fn impl_trait_with_children(
        &mut self,
        trait_ref: TraitRefId<'db>,
        ty: TypeId<'db>,
        types: Vec<AssocTyDef<'db>>,
        consts: Vec<AssocConstDef<'db>>,
        build_children: impl FnOnce(&mut Self),
    ) -> ImplTrait<'db> {
        let trait_ref = Partial::Present(trait_ref);
        let ty = Partial::Present(ty);

        let idx = self.ctxt.next_impl_trait_idx();
        self.with_item_scope(TrackedItemVariant::ImplTrait(idx), |this, id| {
            let spec = this.impl_spec(trait_ref, ty, types, consts);
            let impl_trait = this.new_impl_trait_complete(id, spec, this.origin());
            build_children(this);
            impl_trait
        })
    }

    fn impl_spec(
        &self,
        trait_ref: Partial<TraitRefId<'db>>,
        ty: Partial<TypeId<'db>>,
        types: Vec<AssocTyDef<'db>>,
        consts: Vec<AssocConstDef<'db>>,
    ) -> GeneratedImplSpec<'db> {
        GeneratedImplSpec {
            trait_ref,
            ty,
            selection_name: None,
            attrs: self.empty_attrs(),
            generic_params: self.empty_generic_params(),
            where_clause: self.empty_where_clause(),
            types,
            consts,
        }
    }

    pub(super) fn func_with_body_inline_always(
        &mut self,
        name: IdentId<'db>,
        generic_params: GenericParamListId<'db>,
        params: FuncParamListId<'db>,
        ret_ty: Option<TypeId<'db>>,
        modifiers: FuncModifiers,
        build_body: impl FnOnce(&mut BodyBuilder<'_, 'db, O>),
    ) -> Func<'db> {
        self.generated_func_with_body(
            GeneratedFunctionSpec {
                name,
                attrs: self.inline_always_attrs(),
                generic_params,
                where_clause: self.empty_where_clause(),
                params,
                effects: self.empty_effect_params(),
                ret_ty,
                modifiers,
            },
            build_body,
        )
    }

    /// Builds a function without filling in any declaration defaults.
    pub(super) fn generated_func_with_body(
        &mut self,
        spec: GeneratedFunctionSpec<'db>,
        build_body: impl FnOnce(&mut BodyBuilder<'_, 'db, O>),
    ) -> Func<'db> {
        self.with_item_scope(
            TrackedItemVariant::Func(Partial::Present(spec.name)),
            |this, id| {
                let mut body_builder = BodyBuilder::new(
                    this.ctxt,
                    this.roots,
                    this.desugared.clone(),
                    this.reserved.clone(),
                    TrackedItemVariant::FuncBody,
                );
                build_body(&mut body_builder);
                let body = body_builder.finish();

                Func::new(
                    this.db(),
                    id,
                    Partial::Present(spec.name),
                    spec.attrs,
                    spec.generic_params,
                    spec.where_clause,
                    Partial::Present(spec.params),
                    spec.effects,
                    spec.ret_ty,
                    spec.modifiers,
                    Some(body),
                    this.top_mod(),
                    this.origin(),
                )
            },
        )
    }

    pub(super) fn func_generic_inline_always(
        &mut self,
        name: &str,
        generic_params: GenericParamListId<'db>,
        params: FuncParamListId<'db>,
        ret_ty: Option<TypeId<'db>>,
        modifiers: FuncModifiers,
        build_body: impl FnOnce(&mut BodyBuilder<'_, 'db, O>),
    ) -> Func<'db> {
        self.generated_func_with_body(
            GeneratedFunctionSpec {
                name: self.ident(name),
                attrs: self.inline_always_attrs(),
                generic_params,
                where_clause: self.empty_where_clause(),
                params,
                effects: self.empty_effect_params(),
                ret_ty,
                modifiers,
            },
            build_body,
        )
    }
}

pub(super) struct BodyBuilder<'ctxt, 'db, O>
where
    O: Clone + Into<DesugaredOrigin>,
{
    body: BodyCtxt<'ctxt, 'db>,
    roots: LibRoots<'db>,
    desugared: O,
    reserved: Rc<FxHashSet<String>>,
    stmts: Vec<StmtId>,
}

#[derive(Clone, Copy)]
pub(super) struct DecodeInputBindings<'db> {
    pub(super) input_ident: IdentId<'db>,
    pub(super) input_ty: TypeId<'db>,
    pub(super) base_ident: IdentId<'db>,
    pub(super) input_len_ident: IdentId<'db>,
}

struct QuoteCopyMap<'a, 'db> {
    holes: &'a [FrozenCodeHole],
    code: Option<&'a FrozenCode>,
    fragments: Option<&'a [Option<QuoteExprSource<'db>>]>,
    fragment_depth: usize,
    budget: Rc<Cell<usize>>,
    open_bindings: Vec<(IdentId<'db>, usize)>,
    owner_generic_bindings: Vec<IdentId<'db>>,
    generic_bindings: Vec<IdentId<'db>>,
    allow_self: bool,
    self_record_inits: Vec<ExprId>,
    constructor_paths: HashMap<FrozenQuoteConstructorSite, PathId<'db>>,
    paths: HashMap<ExprId, PathId<'db>>,
    members: HashMap<u32, IdentId<'db>>,
    local_binders: HashMap<PatId, IdentId<'db>>,
    local_types: HashMap<StmtId, TypeId<'db>>,
    cast_types: HashMap<ExprId, TypeId<'db>>,
    record_paths: HashMap<ExprId, PathId<'db>>,
    exprs: HashMap<ExprId, ExprId>,
    stmts: HashMap<StmtId, StmtId>,
    conds: HashMap<CondId, CondId>,
    pats: HashMap<PatId, PatId>,
    method_calls: Vec<ExprId>,
    fragment_method_calls: Vec<QuoteMethodCall<'db>>,
    fragment_rooted_paths: Vec<(ExprId, Func<'db>, ScopeId<'db>)>,
    fragment_constructors: Vec<(
        FrozenQuoteConstructorSite,
        super::generated::QuoteConstructorIdentity<'db>,
    )>,
    fragment_nominal_types: Vec<(TypeId<'db>, AdtRef<'db>)>,
    fragment_params: Vec<(ExprId, usize)>,
    fragment_generics: Vec<(ExprId, IdentId<'db>)>,
    fragment_qualified_paths: Vec<(ExprId, Trait<'db>, ScopeId<'db>)>,
}

pub(super) struct QuoteExprSource<'db> {
    pub(super) template: Body<'db>,
    pub(super) root: ExprId,
    pub(super) open_names: Vec<IdentId<'db>>,
    pub(super) member_holes: Vec<(u32, IdentId<'db>)>,
    pub(super) rooted_funcs: Vec<(ExprId, PathId<'db>, Func<'db>)>,
    pub(super) qualified_paths: Vec<(ExprId, PathId<'db>, Trait<'db>)>,
    pub(super) selected_methods: Vec<(ExprId, Func<'db>)>,
    pub(super) provider_scope: ScopeId<'db>,
    pub(super) has_block_items: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, salsa::Update)]
pub(crate) struct QuoteMethodCall<'db> {
    pub(super) site: ExprId,
    pub(super) scope: ScopeId<'db>,
    /// Read-back already selected a declaration; authored quotes select after substitution.
    pub(super) selected: Option<Func<'db>>,
}

pub(super) struct QuoteCopySummary<'db> {
    pub(super) constructor_sites: Vec<FrozenQuoteConstructorSite>,
    pub(super) rooted_paths: Vec<ExprId>,
    pub(super) qualified_paths: Vec<ExprId>,
    pub(super) locals: Vec<(PatId, Vec<ExprId>)>,
    pub(super) params: Vec<ExprId>,
    pub(super) generics: Vec<ExprId>,
    pub(super) method_calls: Vec<ExprId>,
    pub(super) fragment_method_calls: Vec<QuoteMethodCall<'db>>,
    pub(super) fragment_rooted_paths: Vec<(ExprId, Func<'db>)>,
    pub(super) fragment_constructors: Vec<(
        FrozenQuoteConstructorSite,
        super::generated::QuoteConstructorIdentity<'db>,
    )>,
    pub(super) fragment_nominal_types: Vec<(TypeId<'db>, AdtRef<'db>)>,
    pub(super) fragment_params: Vec<(ExprId, usize)>,
    pub(super) fragment_generics: Vec<(ExprId, IdentId<'db>)>,
    pub(super) fragment_qualified_paths: Vec<(ExprId, Trait<'db>)>,
}

impl<'a, 'db> QuoteCopyMap<'a, 'db> {
    const MAX_TRANSPLANTED_NODES: usize = 1_000;

    fn charge_node(&mut self) -> Result<(), String> {
        let used = self.budget.get();
        if used >= Self::MAX_TRANSPLANTED_NODES {
            return Err(format!(
                "quoted function exceeds {} transplanted HIR nodes",
                Self::MAX_TRANSPLANTED_NODES
            ));
        }
        self.budget.set(used + 1);
        Ok(())
    }

    fn new(
        db: &'db dyn HirDb,
        holes: &'a [FrozenCodeHole],
        rooted_funcs: &[(ExprId, PathId<'db>)],
        locals: &[(PatId, IdentId<'db>, Vec<ExprId>)],
        param_uses: &[(ExprId, IdentId<'db>)],
        generic_uses: &[(ExprId, IdentId<'db>)],
        member_holes: &[(u32, IdentId<'db>)],
    ) -> Result<Self, String> {
        let mut paths = HashMap::new();
        let mut local_binders = HashMap::new();
        for (site, path) in rooted_funcs {
            if paths.insert(*site, *path).is_some() {
                return Err("quoted path is classified twice".into());
            }
        }
        for (binder, name, uses) in locals {
            if local_binders.insert(*binder, *name).is_some() {
                return Err("quoted binder is classified twice".into());
            }
            for site in uses {
                if paths.insert(*site, PathId::from_ident(db, *name)).is_some() {
                    return Err("quoted path is classified twice".into());
                }
            }
        }
        for (site, name) in param_uses {
            if paths.insert(*site, PathId::from_ident(db, *name)).is_some() {
                return Err("quoted path is classified twice".into());
            }
        }
        for (site, name) in generic_uses {
            if paths.insert(*site, PathId::from_ident(db, *name)).is_some() {
                return Err("quoted path is classified twice".into());
            }
        }
        let mut members = HashMap::new();
        for (slot, name) in member_holes {
            if members.insert(*slot, *name).is_some() {
                return Err("quoted member hole is classified twice".into());
            }
        }
        Ok(Self {
            holes,
            code: None,
            fragments: None,
            fragment_depth: 0,
            budget: Rc::new(Cell::new(0)),
            open_bindings: Vec::new(),
            owner_generic_bindings: Vec::new(),
            generic_bindings: Vec::new(),
            allow_self: false,
            self_record_inits: Vec::new(),
            paths,
            members,
            local_binders,
            local_types: HashMap::new(),
            cast_types: HashMap::new(),
            constructor_paths: HashMap::new(),
            record_paths: HashMap::new(),
            exprs: HashMap::new(),
            stmts: HashMap::new(),
            conds: HashMap::new(),
            pats: HashMap::new(),
            method_calls: Vec::new(),
            fragment_method_calls: Vec::new(),
            fragment_rooted_paths: Vec::new(),
            fragment_constructors: Vec::new(),
            fragment_nominal_types: Vec::new(),
            fragment_params: Vec::new(),
            fragment_generics: Vec::new(),
            fragment_qualified_paths: Vec::new(),
        })
    }
}

pub(super) fn quote_hir_primitive(primitive: SemPrimTy) -> Option<HirPrimTy> {
    Some(match primitive {
        SemPrimTy::Bool => HirPrimTy::Bool,
        SemPrimTy::U8 => HirPrimTy::Uint(UintTy::U8),
        SemPrimTy::U16 => HirPrimTy::Uint(UintTy::U16),
        SemPrimTy::U32 => HirPrimTy::Uint(UintTy::U32),
        SemPrimTy::U64 => HirPrimTy::Uint(UintTy::U64),
        SemPrimTy::U128 => HirPrimTy::Uint(UintTy::U128),
        SemPrimTy::U256 => HirPrimTy::Uint(UintTy::U256),
        SemPrimTy::Usize => HirPrimTy::Uint(UintTy::Usize),
        SemPrimTy::I8 => HirPrimTy::Int(IntTy::I8),
        SemPrimTy::I16 => HirPrimTy::Int(IntTy::I16),
        SemPrimTy::I32 => HirPrimTy::Int(IntTy::I32),
        SemPrimTy::I64 => HirPrimTy::Int(IntTy::I64),
        SemPrimTy::I128 => HirPrimTy::Int(IntTy::I128),
        SemPrimTy::I256 => HirPrimTy::Int(IntTy::I256),
        SemPrimTy::Isize => HirPrimTy::Int(IntTy::Isize),
        _ => return None,
    })
}

impl<'ctxt, 'db, O> BodyBuilder<'ctxt, 'db, O>
where
    O: Clone + Into<DesugaredOrigin>,
{
    /// Copy only reachable quote nodes into the generated function's arena.
    /// The template may contain other items and placeholder nodes, none of
    /// which may enter this function. New nodes receive generation origins.
    pub(super) fn adopt_closed_quote_body(
        &mut self,
        template: Body<'db>,
        root: ExprId,
        provider_scope: ScopeId<'db>,
        holes: &[FrozenCodeHole],
        fragments: Option<(&FrozenCode, &[Option<QuoteExprSource<'db>>])>,
        open_bindings: &[(IdentId<'db>, usize)],
        owner_generic_bindings: &[IdentId<'db>],
        generic_bindings: &[IdentId<'db>],
        allow_self: bool,
        rooted_funcs: &[(ExprId, PathId<'db>)],
        qualified_paths: &[(ExprId, PathId<'db>)],
        constructor_paths: &[(FrozenQuoteConstructorSite, PathId<'db>)],
        self_record_inits: &[ExprId],
        locals: &[(PatId, IdentId<'db>, Vec<ExprId>)],
        local_types: &[(StmtId, TypeId<'db>)],
        param_uses: &[(ExprId, IdentId<'db>)],
        generic_uses: &[(ExprId, IdentId<'db>)],
        member_holes: &[(u32, IdentId<'db>)],
    ) -> Result<QuoteCopySummary<'db>, String> {
        let mut all_paths = rooted_funcs.to_vec();
        all_paths.extend_from_slice(qualified_paths);
        let mut map = QuoteCopyMap::new(
            self.db(),
            holes,
            &all_paths,
            locals,
            param_uses,
            generic_uses,
            member_holes,
        )?;
        if let Some((code, sources)) = fragments {
            map.code = Some(code);
            map.fragments = Some(sources);
        }
        map.open_bindings = open_bindings.to_vec();
        map.owner_generic_bindings = owner_generic_bindings.to_vec();
        map.generic_bindings = generic_bindings.to_vec();
        map.constructor_paths = constructor_paths.iter().copied().collect();
        if map.constructor_paths.len() != constructor_paths.len() {
            return Err("quoted variant site is classified twice".into());
        }
        for (site, path) in constructor_paths {
            if let FrozenQuoteConstructorSite::Expr(site) = site {
                map.paths.insert(*site, *path);
                map.record_paths.entry(*site).or_insert(*path);
            }
        }
        map.allow_self = allow_self;
        map.self_record_inits = self_record_inits.to_vec();
        for (stmt, ty) in local_types {
            if map.local_types.insert(*stmt, *ty).is_some() {
                return Err("quoted local type is classified twice".into());
            }
        }
        let root = self.copy_quote_expr(template, root, &mut map)?;
        self.emit_expr_stmt(root);
        for site in &map.method_calls {
            self.body.quote_method_scopes.push((*site, provider_scope));
        }
        self.body.quote_method_scopes.extend(
            map.fragment_method_calls
                .iter()
                .map(|call| (call.site, call.scope)),
        );
        self.body.quote_path_scopes.extend(
            map.fragment_rooted_paths
                .iter()
                .map(|(site, _, scope)| (*site, *scope)),
        );
        self.body.quote_path_scopes.extend(
            map.fragment_qualified_paths
                .iter()
                .map(|(site, _, scope)| (*site, *scope)),
        );
        let rooted_paths = rooted_funcs
            .iter()
            .map(|(site, _)| {
                map.exprs
                    .get(site)
                    .copied()
                    .ok_or_else(|| "quoted rooted path was not copied".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.body
            .quote_path_scopes
            .extend(rooted_paths.iter().map(|site| (*site, provider_scope)));
        let locals = locals
            .iter()
            .map(|(binder, _, uses)| {
                let binder = map
                    .pats
                    .get(binder)
                    .copied()
                    .ok_or_else(|| "quoted local binder was not copied".to_owned())?;
                let uses = uses
                    .iter()
                    .map(|site| {
                        map.exprs
                            .get(site)
                            .copied()
                            .ok_or_else(|| "quoted local use was not copied".to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((binder, uses))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let qualified_paths = qualified_paths
            .iter()
            .map(|(site, _)| {
                map.exprs
                    .get(site)
                    .copied()
                    .ok_or_else(|| "quoted qualified constant was not copied".to_owned())
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.body
            .quote_path_scopes
            .extend(qualified_paths.iter().map(|site| (*site, provider_scope)));
        let params = param_uses
            .iter()
            .map(|(site, _)| {
                map.exprs
                    .get(site)
                    .copied()
                    .ok_or_else(|| "quoted parameter path was not copied".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let generics = generic_uses
            .iter()
            .map(|(site, _)| {
                map.exprs
                    .get(site)
                    .copied()
                    .ok_or_else(|| "quoted generic path was not copied".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let constructor_sites = constructor_paths
            .iter()
            .map(|(site, _)| match site {
                FrozenQuoteConstructorSite::Expr(site) => map
                    .exprs
                    .get(site)
                    .copied()
                    .map(FrozenQuoteConstructorSite::Expr),
                FrozenQuoteConstructorSite::Pat(site) => map
                    .pats
                    .get(site)
                    .copied()
                    .map(FrozenQuoteConstructorSite::Pat),
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| "quoted variant site was not copied".to_owned())?;
        Ok(QuoteCopySummary {
            constructor_sites,
            rooted_paths,
            qualified_paths,
            locals,
            params,
            generics,
            method_calls: map.method_calls,
            fragment_method_calls: map.fragment_method_calls,
            fragment_rooted_paths: map
                .fragment_rooted_paths
                .into_iter()
                .map(|(site, expected, _)| (site, expected))
                .collect(),
            fragment_params: map.fragment_params,
            fragment_generics: map.fragment_generics,
            fragment_qualified_paths: map
                .fragment_qualified_paths
                .into_iter()
                .map(|(site, expected, _)| (site, expected))
                .collect(),
            fragment_constructors: map.fragment_constructors,
            fragment_nominal_types: map.fragment_nominal_types,
        })
    }

    fn quote_scalar_expr(&mut self, hole: &FrozenCodeHole) -> Result<ExprId, String> {
        let FrozenCodeHole::Value(value, _) = hole else {
            return Err("quoted code splice requires fragment transplantation".into());
        };
        let FrozenMetaValue::Scalar { value, primitive } = value.as_ref() else {
            return Err("quoted splice must be a scalar value".into());
        };
        let primitive = primitive
            .and_then(quote_hir_primitive)
            .ok_or_else(|| "quoted scalar has no supported primitive type".to_owned())?;
        let db = self.db();
        let literal = match value {
            SemConstScalar::Bool(value) if primitive == HirPrimTy::Bool => LitKind::Bool(*value),
            SemConstScalar::Int { value } if primitive != HirPrimTy::Bool => {
                if value.sign() == Sign::Minus {
                    return Err("negative quoted scalar requires signed literal lowering".into());
                }
                LitKind::Int(IntegerId::new(db, value.magnitude().clone()))
            }
            _ => return Err("quoted scalar does not match its primitive type".into()),
        };
        let literal = self.push_expr(Expr::Lit(literal));
        if primitive == HirPrimTy::Bool {
            Ok(literal)
        } else {
            let ty = TypeId::new(db, TypeKind::Primitive(primitive));
            Ok(self.push_expr(Expr::Cast(literal, Partial::Present(ty))))
        }
    }

    fn copy_quote_expr(
        &mut self,
        template: Body<'db>,
        old: ExprId,
        map: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<ExprId, String> {
        if let Some(id) = map.exprs.get(&old) {
            return Ok(*id);
        }
        map.charge_node()?;
        let db = self.db();
        let Partial::Present(expr) = &template.exprs(db)[old] else {
            return Err("quoted function contains an invalid expression".into());
        };
        if let Expr::Hole(slot) = expr {
            let hole = map
                .holes
                .get(*slot as usize)
                .ok_or_else(|| "quoted splice slot is missing".to_owned())?;
            let id = match hole {
                FrozenCodeHole::Code(child) => self.copy_quote_fragment(*child, map)?,
                FrozenCodeHole::Value(..) => self.quote_scalar_expr(hole)?,
            };
            map.exprs.insert(old, id);
            return Ok(id);
        }
        let expr = match expr {
            Expr::Lit(value) => Expr::Lit(*value),
            Expr::Path(Partial::Present(source)) => {
                let path = map.paths.get(&old).ok_or_else(|| {
                    "quoted function path has no checked root or binder".to_owned()
                })?;
                if source.strip_generic_args(db) != *source && path.strip_generic_args(db) == *path
                {
                    return Err("quoted function path needs generic argument rewriting".into());
                }
                Expr::Path(Partial::Present(*path))
            }
            Expr::RecordInit(Partial::Present(path), fields) => {
                let path = if let Some(path) = map.record_paths.get(&old) {
                    *path
                } else if map.allow_self
                    && map.self_record_inits.contains(&old)
                    && path.is_self_ty(db)
                {
                    PathId::self_ty(db, GenericArgListId::none(db))
                } else {
                    return Err("quoted record construction has no checked nominal owner".into());
                };
                let fields = fields
                    .iter()
                    .map(|field| {
                        map.charge_node()?;
                        let label = match field.label_hole {
                            Some(slot) => Some(*map.members.get(&slot).ok_or_else(|| {
                                "quoted field label has no checked field".to_owned()
                            })?),
                            None => field.label_eagerly(db, template),
                        };
                        Ok(crate::hir_def::Field {
                            label,
                            label_hole: None,
                            expr: self.copy_quote_expr(template, field.expr, map)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Expr::RecordInit(Partial::Present(path), fields)
            }
            Expr::Call(callee, args) => Expr::Call(
                self.copy_quote_expr(template, *callee, map)?,
                args.iter()
                    .map(|arg| {
                        // Keep a shorthand label before a hygienic binder rename changes its name.
                        Ok(CallArg {
                            label: arg.label_eagerly(db, template),
                            expr: self.copy_quote_expr(template, arg.expr, map)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            ),
            Expr::MethodCall(receiver, Partial::Present(name), generic_args, args) => {
                if generic_args.is_given(db) {
                    return Err(
                        "quoted method call needs checked generic argument rewriting".into(),
                    );
                }
                Expr::MethodCall(
                    self.copy_quote_expr(template, *receiver, map)?,
                    Partial::Present(*name),
                    GenericArgListId::none(db),
                    args.iter()
                        .map(|arg| {
                            Ok(CallArg {
                                label: arg.label_eagerly(db, template),
                                expr: self.copy_quote_expr(template, arg.expr, map)?,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                )
            }
            Expr::Field(receiver, Partial::Present(FieldIndex::Hole(slot))) => {
                let receiver = self.copy_quote_expr(template, *receiver, map)?;
                let name = map
                    .members
                    .get(slot)
                    .ok_or_else(|| "quoted member hole has no checked field".to_owned())?;
                Expr::Field(receiver, Partial::Present(FieldIndex::Ident(*name)))
            }
            Expr::Field(
                receiver,
                Partial::Present(index @ (FieldIndex::Ident(_) | FieldIndex::Index(_))),
            ) => Expr::Field(
                self.copy_quote_expr(template, *receiver, map)?,
                Partial::Present(*index),
            ),
            Expr::Bin(lhs, rhs, op) => Expr::Bin(
                self.copy_quote_expr(template, *lhs, map)?,
                self.copy_quote_expr(template, *rhs, map)?,
                *op,
            ),
            Expr::Un(value, op) => Expr::Un(self.copy_quote_expr(template, *value, map)?, *op),
            Expr::Cast(value, Partial::Present(ty)) => {
                let checked = map
                    .cast_types
                    .get(&old)
                    .copied()
                    .or_else(|| matches!(ty.data(db), TypeKind::Primitive(_)).then_some(*ty));
                let checked = checked.ok_or_else(|| {
                    "quoted cast needs a checked type before transplantation".to_owned()
                })?;
                Expr::Cast(
                    self.copy_quote_expr(template, *value, map)?,
                    Partial::Present(checked),
                )
            }
            Expr::Tuple(values) => Expr::Tuple(
                values
                    .iter()
                    .map(|value| self.copy_quote_expr(template, *value, map))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Expr::Array(values) => Expr::Array(
                values
                    .iter()
                    .map(|value| self.copy_quote_expr(template, *value, map))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Expr::AugAssign(lhs, rhs, op) => Expr::AugAssign(
                self.copy_quote_expr(template, *lhs, map)?,
                self.copy_quote_expr(template, *rhs, map)?,
                *op,
            ),
            Expr::Assign(lhs, rhs) => Expr::Assign(
                self.copy_quote_expr(template, *lhs, map)?,
                self.copy_quote_expr(template, *rhs, map)?,
            ),
            Expr::If(cond, then, otherwise) => Expr::If(
                self.copy_quote_cond(template, *cond, map)?,
                self.copy_quote_expr(template, *then, map)?,
                otherwise
                    .map(|value| self.copy_quote_expr(template, value, map))
                    .transpose()?,
            ),
            Expr::Match(scrutinee, Partial::Present(arms)) => Expr::Match(
                self.copy_quote_expr(template, *scrutinee, map)?,
                Partial::Present(
                    arms.iter()
                        .map(|arm| {
                            map.charge_node()?;
                            Ok(crate::hir_def::MatchArm {
                                pat: self.copy_quote_pat(template, arm.pat, map)?,
                                body: self.copy_quote_expr(template, arm.body, map)?,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                ),
            ),
            Expr::Block(stmts) => {
                self.body.f_ctxt.enter_block_scope();
                let stmts = stmts
                    .iter()
                    .map(|stmt| self.copy_quote_stmt(template, *stmt, map))
                    .collect::<Result<Vec<_>, _>>()?;
                Expr::Block(stmts)
            }
            _ => return Err("quoted function body requires a splice or name rewrite".into()),
        };
        let is_block = matches!(expr, Expr::Block(_));
        let is_method_call = matches!(expr, Expr::MethodCall(..));
        let id = self.push_expr(expr);
        if is_method_call {
            map.method_calls.push(id);
        }
        if is_block {
            self.body.f_ctxt.leave_block_scope(id);
        }
        map.exprs.insert(old, id);
        Ok(id)
    }

    fn copy_quote_fragment(
        &mut self,
        child: usize,
        parent: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<ExprId, String> {
        if parent.fragment_depth >= 64 {
            return Err("quoted expression fragments exceed nesting limit".into());
        }
        let code = parent
            .code
            .ok_or_else(|| "quoted code splice requires fragment transplantation".to_owned())?;
        let sources = parent
            .fragments
            .ok_or_else(|| "quoted code splice has no checked source".to_owned())?;
        let node = code
            .nodes
            .get(child)
            .ok_or_else(|| "quoted code splice has no frozen node".to_owned())?;
        let source = sources
            .get(child)
            .and_then(Option::as_ref)
            .ok_or_else(|| "quoted expression splice needs expression code".to_owned())?;
        if source.has_block_items
            || node
                .rooted_funcs
                .iter()
                .any(|root| root.func.is_some() || root.assoc_const.is_some())
            || node.rooted_traits.iter().any(|root| {
                !node
                    .qualified_consts
                    .iter()
                    .any(|use_| use_.trait_site == root.site)
            })
            || !node.param_uses.is_empty()
            || !node.effect_uses.is_empty()
            || !node.self_uses.is_empty()
        {
            return Err("quoted expression fragment needs checked name transplantation".into());
        }
        let mut param_uses = Vec::new();
        let mut bound_sites = Vec::new();
        let is_checked_body = node
            .template
            .as_ref()
            .is_some_and(|key| key.source == FrozenTemplateSource::CheckedBody);
        let generic_uses = node
            .generic_uses
            .iter()
            .map(|use_| {
                if !is_checked_body || use_.func != 0 || use_.where_predicate.is_some() {
                    return Err("quoted fragment generic use needs a checked body".to_owned());
                }
                let name = parent
                    .generic_bindings
                    .get(use_.index as usize)
                    .ok_or_else(|| "read-back generic has no destination binding".to_owned())?;
                Ok((use_.site, *name))
            })
            .collect::<Result<Vec<_>, String>>()?;
        for use_ in &node.open_uses {
            let open_name = source
                .open_names
                .get(use_.index as usize)
                .ok_or_else(|| "quoted open name has no declaration".to_owned())?;
            let (name, index) = parent
                .open_bindings
                .iter()
                .find(|(name, index)| {
                    if is_checked_body {
                        *index == use_.index as usize
                    } else {
                        name == open_name
                    }
                })
                .ok_or_else(|| {
                    if is_checked_body {
                        "read-back parameter has no destination binding".to_owned()
                    } else {
                        "quoted open name has no destination parameter".to_owned()
                    }
                })?;
            param_uses.push((use_.site, *name));
            bound_sites.push((use_.site, *index));
        }
        if !node.free_self_uses.is_empty() && !parent.allow_self {
            return Err("quoted self needs a destination method".into());
        }
        for site in &node.free_self_uses {
            param_uses.push((*site, IdentId::make_self(self.db())));
            bound_sites.push((*site, 0));
        }
        let types = super::generated::reify_quote_expr_types(
            self.db(),
            node,
            source,
            &parent.owner_generic_bindings,
            &parent.generic_bindings,
        )?;
        let mut rooted_paths = types.rooted_paths;
        rooted_paths.extend(
            source
                .qualified_paths
                .iter()
                .map(|(site, path, _)| (*site, *path)),
        );
        let locals = node
            .locals
            .iter()
            .enumerate()
            .map(|(index, local)| {
                (
                    local.binder,
                    IdentId::new(self.db(), format!("<quote-fragment-local-{child}-{index}>")),
                    local.uses.clone(),
                )
            })
            .collect::<Vec<_>>();
        let mut map = QuoteCopyMap::new(
            self.db(),
            &node.holes,
            &rooted_paths,
            &locals,
            &param_uses,
            &generic_uses,
            &source.member_holes,
        )?;
        map.code = Some(code);
        map.fragments = Some(sources);
        map.fragment_depth = parent.fragment_depth + 1;
        map.budget = parent.budget.clone();
        map.open_bindings = parent.open_bindings.clone();
        map.owner_generic_bindings = parent.owner_generic_bindings.clone();
        map.generic_bindings = parent.generic_bindings.clone();
        map.allow_self = parent.allow_self;
        map.cast_types = types.cast_types.into_iter().collect();
        map.local_types = types.local_types.into_iter().collect();
        map.record_paths = types.record_paths.into_iter().collect();
        for (site, path, _) in &types.variants {
            if map.constructor_paths.insert(*site, *path).is_some() {
                return Err("quoted fragment variant is classified twice".into());
            }
            if let FrozenQuoteConstructorSite::Expr(site) = site {
                map.paths.insert(*site, *path);
                map.record_paths.entry(*site).or_insert(*path);
            }
        }
        let copied = self.copy_quote_expr(source.template, source.root, &mut map)?;
        for (site, index) in bound_sites {
            let copied_site = map
                .exprs
                .get(&site)
                .copied()
                .ok_or_else(|| "quoted open name was not copied".to_owned())?;
            parent.fragment_params.push((copied_site, index));
        }
        parent.fragment_params.extend(map.fragment_params);
        for (site, name) in generic_uses {
            let copied_site = map
                .exprs
                .get(&site)
                .copied()
                .ok_or_else(|| "read-back generic was not copied".to_owned())?;
            parent.fragment_generics.push((copied_site, name));
        }
        parent.fragment_generics.extend(map.fragment_generics);
        for (site, _, expected) in &source.qualified_paths {
            let copied_site = map
                .exprs
                .get(site)
                .copied()
                .ok_or_else(|| "read-back qualified constant was not copied".to_owned())?;
            parent
                .fragment_qualified_paths
                .push((copied_site, *expected, source.provider_scope));
        }
        parent
            .fragment_qualified_paths
            .extend(map.fragment_qualified_paths);
        let mut selected_methods = HashMap::new();
        for (old, selected) in &source.selected_methods {
            let site = map
                .exprs
                .get(old)
                .copied()
                .ok_or_else(|| "read-back selected method was not copied".to_owned())?;
            if selected_methods.insert(site, *selected).is_some() {
                return Err("read-back method is classified twice".into());
            }
        }
        if is_checked_body && selected_methods.len() != map.method_calls.len() {
            return Err("read-back method has no checked selection".into());
        }
        parent
            .fragment_method_calls
            .extend(map.method_calls.into_iter().map(|site| QuoteMethodCall {
                site,
                scope: source.provider_scope,
                selected: selected_methods.get(&site).copied(),
            }));
        parent
            .fragment_method_calls
            .extend(map.fragment_method_calls);
        for (site, _, expected) in &source.rooted_funcs {
            let copied_site = map
                .exprs
                .get(site)
                .copied()
                .ok_or_else(|| "quoted rooted function was not copied".to_owned())?;
            parent
                .fragment_rooted_paths
                .push((copied_site, *expected, source.provider_scope));
        }
        parent
            .fragment_rooted_paths
            .extend(map.fragment_rooted_paths);
        for (site, _, variant) in types.variants {
            let copied_site = match site {
                FrozenQuoteConstructorSite::Expr(site) => map
                    .exprs
                    .get(&site)
                    .copied()
                    .map(FrozenQuoteConstructorSite::Expr),
                FrozenQuoteConstructorSite::Pat(site) => map
                    .pats
                    .get(&site)
                    .copied()
                    .map(FrozenQuoteConstructorSite::Pat),
            }
            .ok_or_else(|| "quoted fragment variant was not copied".to_owned())?;
            parent.fragment_constructors.push((copied_site, variant));
        }
        parent
            .fragment_constructors
            .extend(map.fragment_constructors);
        parent.fragment_nominal_types.extend(types.nominal_types);
        parent
            .fragment_nominal_types
            .extend(map.fragment_nominal_types);
        Ok(copied)
    }

    fn copy_quote_cond(
        &mut self,
        template: Body<'db>,
        old: CondId,
        map: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<CondId, String> {
        if let Some(id) = map.conds.get(&old) {
            return Ok(*id);
        }
        map.charge_node()?;
        let Partial::Present(cond) = &template.conds(self.db())[old] else {
            return Err("quoted function contains an invalid condition".into());
        };
        let cond = match cond {
            Cond::Expr(value) => Cond::Expr(self.copy_quote_expr(template, *value, map)?),
            Cond::Bin(lhs, rhs, op) => Cond::Bin(
                self.copy_quote_cond(template, *lhs, map)?,
                self.copy_quote_cond(template, *rhs, map)?,
                *op,
            ),
            Cond::Let(..) => {
                return Err("quoted let condition needs pattern transplantation".into());
            }
        };
        let id = self.body.push_cond(cond);
        map.conds.insert(old, id);
        Ok(id)
    }

    fn copy_quote_stmt(
        &mut self,
        template: Body<'db>,
        old: StmtId,
        map: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<StmtId, String> {
        if let Some(id) = map.stmts.get(&old) {
            return Ok(*id);
        }
        map.charge_node()?;
        let db = self.db();
        let Partial::Present(stmt) = &template.stmts(db)[old] else {
            return Err("quoted function contains an invalid statement".into());
        };
        let stmt = match stmt {
            Stmt::Let(pat, ty, init) => {
                let ty = ty
                    .map(|_| {
                        map.local_types
                            .get(&old)
                            .copied()
                            .ok_or_else(|| "quoted let type has no checked root".to_owned())
                    })
                    .transpose()?;
                let init = init
                    .map(|value| self.copy_quote_expr(template, value, map))
                    .transpose()?;
                Stmt::Let(self.copy_quote_pat(template, *pat, map)?, ty, init)
            }
            Stmt::Expr(value) => Stmt::Expr(self.copy_quote_expr(template, *value, map)?),
            Stmt::Return(value) => Stmt::Return(
                value
                    .map(|value| self.copy_quote_expr(template, value, map))
                    .transpose()?,
            ),
            Stmt::While(cond, body) => Stmt::While(
                self.copy_quote_cond(template, *cond, map)?,
                self.copy_quote_expr(template, *body, map)?,
            ),
            Stmt::For(pat, iterable, body, unroll_hint) => {
                let iterable = self.copy_quote_expr(template, *iterable, map)?;
                Stmt::For(
                    self.copy_quote_pat(template, *pat, map)?,
                    iterable,
                    self.copy_quote_expr(template, *body, map)?,
                    *unroll_hint,
                )
            }
            Stmt::Break => Stmt::Break,
            Stmt::Continue => Stmt::Continue,
        };
        let id = self.push_stmt_raw(stmt);
        map.stmts.insert(old, id);
        Ok(id)
    }

    fn copy_quote_pat(
        &mut self,
        template: Body<'db>,
        old: PatId,
        map: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<PatId, String> {
        if let Some(id) = map.pats.get(&old) {
            return Ok(*id);
        }
        map.charge_node()?;
        let db = self.db();
        let Partial::Present(pat) = &template.pats(db)[old] else {
            return Err("quoted function contains an invalid pattern".into());
        };
        let pat = match pat {
            Pat::Path(Partial::Present(_), mutable)
                if map
                    .constructor_paths
                    .contains_key(&FrozenQuoteConstructorSite::Pat(old)) =>
            {
                Pat::Path(
                    Partial::Present(map.constructor_paths[&FrozenQuoteConstructorSite::Pat(old)]),
                    *mutable,
                )
            }
            Pat::PathTuple(_, parts) => {
                let path = *map
                    .constructor_paths
                    .get(&FrozenQuoteConstructorSite::Pat(old))
                    .ok_or_else(|| {
                        "quoted tuple pattern has no checked nominal constructor".to_owned()
                    })?;
                Pat::PathTuple(
                    Partial::Present(path),
                    parts
                        .iter()
                        .map(|part| self.copy_quote_pat(template, *part, map))
                        .collect::<Result<Vec<_>, _>>()?,
                )
            }
            Pat::Record(_, fields) => {
                let path = *map
                    .constructor_paths
                    .get(&FrozenQuoteConstructorSite::Pat(old))
                    .ok_or_else(|| {
                        "quoted record pattern has no checked nominal constructor".to_owned()
                    })?;
                let mut fields = fields.clone();
                for field in &mut fields {
                    if let Some(label) = field.label(db, template) {
                        field.label = Partial::Present(label);
                    }
                    field.pat = self.copy_quote_pat(template, field.pat, map)?;
                }
                Pat::Record(Partial::Present(path), fields)
            }
            Pat::Path(Partial::Present(path), mutable) => {
                if path.as_ident(db).is_none() {
                    return Err("quoted pattern needs a checked binder".into());
                }
                let name = map
                    .local_binders
                    .get(&old)
                    .ok_or_else(|| "quoted pattern is not a checked binder".to_owned())?;
                Pat::Path(Partial::Present(PathId::from_ident(db, *name)), *mutable)
            }
            Pat::Tuple(parts) => Pat::Tuple(
                parts
                    .iter()
                    .map(|part| self.copy_quote_pat(template, *part, map))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Pat::WildCard => Pat::WildCard,
            Pat::Rest => Pat::Rest,
            Pat::Lit(Partial::Present(literal)) => Pat::Lit(Partial::Present(*literal)),
            Pat::Or(lhs, rhs) => Pat::Or(
                self.copy_quote_pat(template, *lhs, map)?,
                self.copy_quote_pat(template, *rhs, map)?,
            ),
            _ => return Err("quoted pattern needs classification before transplantation".into()),
        };
        let id = self.push_pat(pat);
        map.pats.insert(old, id);
        Ok(id)
    }

    fn new(
        ctxt: &'ctxt mut FileLowerCtxt<'db>,
        roots: LibRoots<'db>,
        desugared: O,
        reserved: Rc<FxHashSet<String>>,
        id: TrackedItemVariant<'db>,
    ) -> Self {
        let id = ctxt.joined_id(id);
        Self {
            body: BodyCtxt::new(ctxt, id),
            roots,
            desugared,
            reserved,
            stmts: Vec::new(),
        }
    }

    pub(super) fn db(&self) -> &'db dyn HirDb {
        self.body.f_ctxt.db()
    }

    pub(super) fn generated_ident(&self, name: &str) -> IdentId<'db> {
        fresh_ident(self.db(), &self.reserved, name)
    }

    pub(super) fn roots(&self) -> LibRoots<'db> {
        self.roots
    }

    pub(super) fn sol_ty(&self) -> TypeId<'db> {
        let path = PathId::from_ident(self.db(), self.roots.std)
            .push_str(self.db(), "abi")
            .push_str(self.db(), "Sol");
        TypeId::new(self.db(), TypeKind::Path(Partial::Present(path)))
    }

    fn abi_size_trait_ref(&self) -> TraitRefId<'db> {
        let path = PathId::from_ident(self.db(), self.roots.core)
            .push_str(self.db(), "abi")
            .push_str(self.db(), "AbiSize");
        TraitRefId::new(self.db(), Partial::Present(path))
    }

    fn expr_origin(&self) -> HirOrigin<ast::Expr> {
        HirOrigin::desugared(self.desugared.clone())
    }

    fn stmt_origin(&self) -> HirOrigin<ast::Stmt> {
        HirOrigin::desugared(self.desugared.clone())
    }

    fn pat_origin(&self) -> HirOrigin<ast::Pat> {
        HirOrigin::desugared(self.desugared.clone())
    }

    pub(super) fn push_expr(&mut self, expr: Expr<'db>) -> ExprId {
        self.body.push_expr(expr, self.expr_origin())
    }

    pub(super) fn push_pat(&mut self, pat: Pat<'db>) -> PatId {
        self.body.push_pat(pat, self.pat_origin())
    }

    pub(super) fn push_stmt_raw(&mut self, stmt: Stmt<'db>) -> StmtId {
        self.body.push_stmt(stmt, self.stmt_origin())
    }

    pub(super) fn emit_stmt(&mut self, stmt: Stmt<'db>) -> StmtId {
        let stmt_id = self.push_stmt_raw(stmt);
        self.stmts.push(stmt_id);
        stmt_id
    }

    pub(super) fn emit_expr_stmt(&mut self, expr: ExprId) -> StmtId {
        self.emit_stmt(Stmt::Expr(expr))
    }

    pub(super) fn emit_return(&mut self, expr: Option<ExprId>) -> StmtId {
        self.emit_stmt(Stmt::Return(expr))
    }

    pub(super) fn ident_expr(&mut self, ident: IdentId<'db>) -> ExprId {
        self.push_expr(Expr::Path(Partial::Present(PathId::from_ident(
            self.db(),
            ident,
        ))))
    }

    pub(super) fn path_expr(&mut self, path: PathId<'db>) -> ExprId {
        self.push_expr(Expr::Path(Partial::Present(path)))
    }

    pub(super) fn abi_size_assoc_expr(&mut self, ty: TypeId<'db>, assoc_name: &str) -> ExprId {
        let qualified = PathId::new(
            self.db(),
            PathKind::QualifiedType {
                type_: ty,
                trait_: self.abi_size_trait_ref(),
            },
            None,
        );
        self.path_expr(qualified.push_str(self.db(), assoc_name))
    }

    pub(super) fn abi_record_layout_expr(&mut self, trait_ref: TraitRefId<'db>) -> ExprId {
        let db = self.db();
        let qualified = PathId::new(
            db,
            PathKind::QualifiedType {
                type_: TypeId::fallback_self_ty(db),
                trait_: trait_ref,
            },
            None,
        );
        self.path_expr(qualified.push_str(db, super::generated_abi_const::LAYOUT))
    }

    pub(super) fn abi_record_layout_field_expr(
        &mut self,
        trait_ref: TraitRefId<'db>,
        field: &str,
    ) -> ExprId {
        let layout = self.abi_record_layout_expr(trait_ref);
        let field = IdentId::new(self.db(), field.to_owned());
        self.push_expr(Expr::Field(
            layout,
            Partial::Present(FieldIndex::Ident(field)),
        ))
    }

    pub(super) fn abi_record_offset_expr(
        &mut self,
        trait_ref: TraitRefId<'db>,
        index: usize,
    ) -> ExprId {
        let offsets = self.abi_record_layout_field_expr(trait_ref, "offsets");
        let index = self.push_expr(Expr::Lit(LitKind::Int(IntegerId::from_usize(
            self.db(),
            index,
        ))));
        self.push_expr(Expr::Bin(offsets, index, BinOp::Index))
    }

    pub(super) fn call_expr(&mut self, callee: ExprId, args: Vec<ExprId>) -> ExprId {
        self.call_expr_with_args(
            callee,
            args.into_iter()
                .map(|expr| CallArg { label: None, expr })
                .collect(),
        )
    }

    pub(super) fn call_expr_with_args(
        &mut self,
        callee: ExprId,
        args: Vec<CallArg<'db>>,
    ) -> ExprId {
        self.push_expr(Expr::Call(callee, args))
    }

    pub(super) fn method_call_expr(
        &mut self,
        receiver: ExprId,
        name: IdentId<'db>,
        args: Vec<ExprId>,
    ) -> ExprId {
        self.method_call_expr_with_args(
            receiver,
            name,
            args.into_iter()
                .map(|expr| CallArg { label: None, expr })
                .collect(),
        )
    }

    pub(super) fn method_call_expr_with_args(
        &mut self,
        receiver: ExprId,
        name: IdentId<'db>,
        args: Vec<CallArg<'db>>,
    ) -> ExprId {
        self.push_expr(Expr::MethodCall(
            receiver,
            Partial::Present(name),
            GenericArgListId::none(self.db()),
            args,
        ))
    }

    /// `let <ident> = <value>`
    pub(super) fn emit_let(&mut self, ident: IdentId<'db>, value: ExprId) -> StmtId {
        let pat = self.push_pat(Pat::Path(
            Partial::Present(PathId::from_ident(self.db(), ident)),
            false,
        ));
        self.emit_stmt(Stmt::Let(pat, None, Some(value)))
    }

    /// `self.f0, self.f1, ...`
    pub(super) fn self_field_exprs(
        &mut self,
        fields: &[(IdentId<'db>, TypeId<'db>)],
    ) -> Vec<ExprId> {
        if fields.is_empty() {
            return Vec::new();
        }
        let db = self.db();
        let self_expr = self.path_expr(PathId::from_ident(db, IdentId::make_self(db)));
        fields
            .iter()
            .map(|(field, _)| {
                self.push_expr(Expr::Field(
                    self_expr,
                    Partial::Present(FieldIndex::Ident(*field)),
                ))
            })
            .collect()
    }

    /// `head_size + core::abi::dynamic_payload_size(v0) + ...`, the exact
    /// payload size of a record whose field values are `values`.
    pub(super) fn record_payload_size_expr(
        &mut self,
        head_size: ExprId,
        values: Vec<ExprId>,
    ) -> ExprId {
        let db = self.db();
        let dynamic_payload_size = PathId::from_ident(db, self.roots.core)
            .push_str(db, "abi")
            .push_str(db, "dynamic_payload_size");
        let mut expr = head_size;
        for value in values {
            let callee = self.path_expr(dynamic_payload_size);
            let field_size = self.call_expr(callee, vec![value]);
            expr = self.push_expr(Expr::Bin(expr, field_size, BinOp::Arith(ArithBinOp::Add)));
        }
        expr
    }

    /// `let __field_{i} = self.<field>` for each field, in order. The locals
    /// are the ones `encode_bound_fields` reads.
    pub(super) fn bind_self_fields(
        &mut self,
        fields: &[(IdentId<'db>, TypeId<'db>)],
    ) -> Vec<IdentId<'db>> {
        let values = self.self_field_exprs(fields);
        values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let local = self.field_local(index);
                self.emit_let(local, value);
                local
            })
            .collect()
    }

    fn field_local(&self, index: usize) -> IdentId<'db> {
        self.generated_ident(&format!("field_{index}"))
    }

    /// Encode `self`'s fields as an ABI record at `ptr`, moving each field
    /// into a local just before encoding it.
    pub(super) fn encode_fields(
        &mut self,
        fields: &[(IdentId<'db>, TypeId<'db>)],
        ptr_ident: IdentId<'db>,
        abi_record_trait: TraitRefId<'db>,
    ) {
        self.encode_record(fields, ptr_ident, abi_record_trait, true);
    }

    /// Like `encode_fields`, for fields already moved into locals by
    /// `bind_self_fields`.
    pub(super) fn encode_bound_fields(
        &mut self,
        fields: &[(IdentId<'db>, TypeId<'db>)],
        ptr_ident: IdentId<'db>,
        abi_record_trait: TraitRefId<'db>,
    ) {
        self.encode_record(fields, ptr_ident, abi_record_trait, false);
    }

    fn encode_record(
        &mut self,
        fields: &[(IdentId<'db>, TypeId<'db>)],
        ptr_ident: IdentId<'db>,
        abi_record_trait: TraitRefId<'db>,
        bind_fields: bool,
    ) {
        if fields.is_empty() {
            return;
        }

        let db = self.db();
        let self_expr =
            bind_fields.then(|| self.path_expr(PathId::from_ident(db, IdentId::make_self(db))));
        let tail_ident = self.generated_ident("tail");
        let head_size = self.abi_record_layout_field_expr(abi_record_trait, "head_size");
        let tail_pat = self.push_pat(Pat::Path(
            Partial::Present(PathId::from_ident(db, tail_ident)),
            true,
        ));
        self.emit_stmt(Stmt::Let(tail_pat, None, Some(head_size)));
        for (index, (field, field_ty)) in fields.iter().copied().enumerate() {
            let field_ident = self.field_local(index);
            if let Some(self_expr) = self_expr {
                let receiver = self.push_expr(Expr::Field(
                    self_expr,
                    Partial::Present(FieldIndex::Ident(field)),
                ));
                self.emit_let(field_ident, receiver);
            }
            let encode_field_args = GenericArgListId::given(
                db,
                vec![
                    GenericArg::Type(TypeGenericArg {
                        ty: Partial::Present(self.sol_ty()),
                    }),
                    GenericArg::Type(TypeGenericArg {
                        ty: Partial::Present(field_ty),
                    }),
                ],
            );
            let encode_field_path = PathId::from_ident(db, self.roots.core)
                .push_str(db, "abi")
                .push_str_args(db, "encode_field_at", encode_field_args);
            let encode_field_callee = self.path_expr(encode_field_path);
            let field_value = self.ident_expr(field_ident);
            let ptr_arg = self.ident_expr(ptr_ident);
            let head_pos_arg = self.abi_record_offset_expr(abi_record_trait, index);
            let tail_arg = self.ident_expr(tail_ident);
            let call = self.call_expr_with_args(
                encode_field_callee,
                vec![
                    CallArg {
                        label: None,
                        expr: field_value,
                    },
                    CallArg {
                        label: None,
                        expr: ptr_arg,
                    },
                    CallArg {
                        label: Some(IdentId::new(db, "head_pos".to_string())),
                        expr: head_pos_arg,
                    },
                    CallArg {
                        label: Some(IdentId::new(db, "tail".to_string())),
                        expr: tail_arg,
                    },
                ],
            );
            if index + 1 == fields.len() {
                self.emit_expr_stmt(call);
                continue;
            }

            let tail_place = self.ident_expr(tail_ident);
            let assign_tail = self.push_expr(Expr::Assign(tail_place, call));
            self.emit_expr_stmt(assign_tail);
        }
    }

    pub(super) fn decode_into(
        &mut self,
        target_ident: IdentId<'db>,
        ty: TypeId<'db>,
        decoder_ident: IdentId<'db>,
        decoder_ty: TypeId<'db>,
    ) {
        let db = self.db();
        let decode_args = GenericArgListId::given(
            db,
            vec![
                GenericArg::Type(TypeGenericArg {
                    ty: Partial::Present(self.sol_ty()),
                }),
                GenericArg::Type(TypeGenericArg {
                    ty: Partial::Present(ty),
                }),
                GenericArg::Type(TypeGenericArg {
                    ty: Partial::Present(decoder_ty),
                }),
            ],
        );
        let decode_path = PathId::from_ident(db, self.roots.core)
            .push_str(db, "abi")
            .push_str_args(db, "decode_field", decode_args);
        let decode_callee = self.path_expr(decode_path);
        let decoder_expr = self.ident_expr(decoder_ident);
        let decode_call = self.call_expr(decode_callee, vec![decoder_expr]);

        let bind_pat = self.push_pat(Pat::Path(
            Partial::Present(PathId::from_ident(db, target_ident)),
            false,
        ));
        self.emit_stmt(Stmt::Let(bind_pat, Some(ty), Some(decode_call)));
    }

    pub(super) fn decode_from_into(
        &mut self,
        target_ident: IdentId<'db>,
        ty: TypeId<'db>,
        input: DecodeInputBindings<'db>,
        head_pos: ExprId,
    ) {
        self.decode_field_into("decode_msg_field_from", target_ident, ty, input, head_pos);
    }

    /// `let target: ty = core::abi::<decode_fn><Sol, ty, I>(input, base:,
    /// head_pos:, input_len:)`.
    pub(super) fn decode_field_into(
        &mut self,
        decode_fn: &str,
        target_ident: IdentId<'db>,
        ty: TypeId<'db>,
        input: DecodeInputBindings<'db>,
        head_pos: ExprId,
    ) {
        let db = self.db();
        let decode_args = GenericArgListId::given_types(db, [self.sol_ty(), ty, input.input_ty]);
        let decode_path = PathId::from_ident(db, self.roots.core)
            .push_str(db, "abi")
            .push_str_args(db, decode_fn, decode_args);
        let decode_callee = self.path_expr(decode_path);
        let input_expr = self.ident_expr(input.input_ident);
        let base_expr = self.ident_expr(input.base_ident);
        let args = vec![
            CallArg {
                label: None,
                expr: input_expr,
            },
            CallArg {
                label: Some(IdentId::new(db, "base".to_string())),
                expr: base_expr,
            },
            CallArg {
                label: Some(IdentId::new(db, "head_pos".to_string())),
                expr: head_pos,
            },
            CallArg {
                label: Some(IdentId::new(db, "input_len".to_string())),
                expr: self.ident_expr(input.input_len_ident),
            },
        ];
        let decode_call = self.call_expr_with_args(decode_callee, args);

        let bind_pat = self.push_pat(Pat::Path(
            Partial::Present(PathId::from_ident(db, target_ident)),
            false,
        ));
        self.emit_stmt(Stmt::Let(bind_pat, Some(ty), Some(decode_call)));
    }

    pub(super) fn bind_input_len(&mut self, target_ident: IdentId<'db>, input_ident: IdentId<'db>) {
        let db = self.db();
        let input_expr = self.ident_expr(input_ident);
        let len_call =
            self.method_call_expr(input_expr, IdentId::new(db, "len".to_string()), vec![]);
        let bind_pat = self.push_pat(Pat::Path(
            Partial::Present(PathId::from_ident(db, target_ident)),
            false,
        ));
        let u256_ty = TypeId::new(
            db,
            TypeKind::Path(Partial::Present(PathId::from_ident(
                db,
                IdentId::new(db, "u256".to_string()),
            ))),
        );
        self.emit_stmt(Stmt::Let(bind_pat, Some(u256_ty), Some(len_call)));
    }

    pub(super) fn return_record_self(&mut self, fields: &[IdentId<'db>]) {
        let db = self.db();
        let self_path = Partial::Present(PathId::from_ident(db, IdentId::make_self_ty(db)));
        let fields = fields
            .iter()
            .copied()
            .map(|field_name| crate::hir_def::Field {
                label: Some(field_name),
                label_hole: None,
                expr: self.ident_expr(field_name),
            })
            .collect();
        let record_expr = self.push_expr(Expr::RecordInit(self_path, fields));
        self.emit_return(Some(record_expr));
    }

    fn finish(mut self) -> Body<'db> {
        self.body.f_ctxt.enter_block_scope();
        let stmts = std::mem::take(&mut self.stmts);
        let root_expr = self.push_expr(Expr::Block(stmts));
        self.body.f_ctxt.leave_block_scope(root_expr);
        self.body.build(None, root_expr, BodyKind::FuncBody)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        HirDb,
        hir_def::{
            EffectParam, EffectParamListId, Func, FuncModifiers, FuncParam, FuncParamMode,
            FuncParamName, InlineHint, Partial, Stmt, TopLevelMod, TraitRefId, TypeBound, TypeMode,
            Visibility, WhereClauseId, WhereClausePredicate, WherePredicate,
        },
        span::{EventDesugared, HirOrigin},
        test_db::{HirAnalysisTestDb, TestDb},
    };
    use parser::ast::prelude::AstNode as _;

    use super::{FileLowerCtxt, GeneratedFunctionSpec, HirBuilder};

    #[salsa::tracked]
    fn build_complete_test_function<'db>(
        db: &'db dyn HirDb,
        top_mod: TopLevelMod<'db>,
    ) -> Func<'db> {
        let source_struct = top_mod
            .all_structs(db)
            .first()
            .expect("test source has an origin struct");
        let HirOrigin::Raw(event_struct) = source_struct.origin(db).clone() else {
            panic!("test origin struct was not raw")
        };
        let mut ctxt = FileLowerCtxt::enter_top_mod(db, top_mod);
        ctxt.insert_synthetic_prelude_use();
        let source_ast = crate::lower::top_mod_ast(db, top_mod);
        let mut builder = HirBuilder::new(
            &mut ctxt,
            EventDesugared { event_struct },
            source_ast.syntax(),
        );

        let copy_path = builder.path_from_root(builder.roots().core, &["marker", "Copy"]);
        let copy_trait = TraitRefId::new(db, Partial::Present(copy_path));
        let (generic_params, value_ty) = builder.type_param_with_trait_bound("T", copy_trait);
        let where_clause = WhereClauseId::new(
            db,
            vec![WhereClausePredicate::Type(WherePredicate {
                ty: Partial::Present(value_ty),
                bounds: vec![TypeBound::Trait(copy_trait)],
            })],
        );
        let effect_name = builder.ident("capability");
        let effects = EffectParamListId::new(
            db,
            vec![EffectParam {
                name: Some(effect_name),
                key_ty: Partial::Present(value_ty),
                is_mut: true,
            }],
        );
        let value_name = builder.ident("value");
        let borrowed_name = builder.ident("borrowed");
        let borrowed_ty = builder.ty_path(crate::hir_def::PathId::from_ident(
            db,
            builder.ident("u256"),
        ));
        let borrowed_ty = crate::hir_def::TypeId::new(
            db,
            crate::hir_def::TypeKind::Mode(TypeMode::Ref, Partial::Present(borrowed_ty)),
        );
        let params = builder.params([
            FuncParam {
                mode: FuncParamMode::Own,
                is_mut: false,
                has_ref_prefix: false,
                has_own_prefix: true,
                is_label_suppressed: false,
                name: Partial::Present(FuncParamName::Ident(value_name)),
                ty: Partial::Present(value_ty),
                self_ty_fallback: false,
            },
            FuncParam {
                mode: FuncParamMode::View,
                is_mut: true,
                has_ref_prefix: true,
                has_own_prefix: false,
                is_label_suppressed: true,
                name: Partial::Present(FuncParamName::Ident(borrowed_name)),
                ty: Partial::Present(borrowed_ty),
                self_ty_fallback: false,
            },
        ]);
        let spec = GeneratedFunctionSpec {
            name: builder.ident("generated_identity"),
            attrs: builder.inline_always_attrs(),
            generic_params,
            where_clause,
            params,
            effects,
            ret_ty: Some(value_ty),
            modifiers: FuncModifiers::new(Visibility::Public, true, false, false),
        };
        let func = builder.generated_func_with_body(spec, |body| {
            let value = body.ident_expr(value_name);
            body.emit_return(Some(value));
        });
        ctxt.leave_item_scope(top_mod);
        let _graph = ctxt.build();
        func
    }

    #[test]
    fn complete_generated_function_preserves_signature_and_body_fields() {
        let mut db = TestDb::default();
        let file = db.standalone_file("struct Origin {}\n");
        let top_mod = crate::lower::map_file_to_mod(&db, file);
        let func = build_complete_test_function(&db, top_mod);

        let generic_params = func.generic_params(&db);
        assert_eq!(generic_params.data(&db).len(), 1);
        let where_clause = func.where_clause(&db);
        assert_eq!(where_clause.predicates(&db).len(), 1);
        let effects = func.effects(&db);
        let [effect] = effects.data(&db).as_slice() else {
            panic!("expected one generated effect parameter")
        };
        assert_eq!(effect.name.unwrap().data(&db), "capability");
        assert!(effect.is_mut);

        let params = func.params_list(&db).to_opt().unwrap();
        let [owned, borrowed] = params.data(&db).as_slice() else {
            panic!("expected two generated parameters")
        };
        assert_eq!(owned.mode, FuncParamMode::Own);
        assert!(owned.has_own_prefix);
        assert!(!owned.is_label_suppressed);
        assert_eq!(func.param_label(&db, 0).unwrap().data(&db), "value");
        assert_eq!(borrowed.mode, FuncParamMode::View);
        assert!(borrowed.is_mut);
        assert!(borrowed.has_ref_prefix);
        assert!(borrowed.is_label_suppressed);
        assert!(matches!(
            borrowed.ty.to_opt().unwrap().data(&db),
            crate::hir_def::TypeKind::Mode(TypeMode::Ref, _)
        ));

        assert!(func.ret_type_ref(&db).is_some());
        assert_eq!(
            func.modifiers(&db),
            FuncModifiers::new(Visibility::Public, true, false, false)
        );
        assert_eq!(func.inline_hint(&db), Some(InlineHint::Always));
        let body = func.body(&db).expect("generated function has a body");
        assert!(
            body.stmts(&db)
                .iter()
                .any(|(_, stmt)| matches!(stmt, Partial::Present(Stmt::Return(Some(_)))))
        );
    }

    #[test]
    fn existing_event_error_and_msg_generated_functions_still_type_check() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "complete_generated_function_consumers.fe".into(),
            r#"
use core::abi::AbiSize
use std::evm::{Address, Log}

#[event]
struct Transfer {
    #[indexed]
    from: Address,
    amount: u256,
}

#[error]
struct Failure {}

msg Calls {
    #[selector = 1]
    Ping { value: u256 } -> bool,
}

fn error_payload(value: own Failure) -> u256 { value.payload_size() }
fn msg_payload(value: own Calls::Ping) -> u256 { value.payload_size() }
fn emit_event<L: Log>(value: own Transfer, log: mut L) { value.emit(log) }
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        db.assert_no_diags(top_mod);

        let generated = top_mod
            .all_funcs(&db)
            .iter()
            .copied()
            .filter(|func| matches!(func.origin(&db), HirOrigin::Desugared(_)))
            .collect::<Vec<_>>();
        assert!(
            !generated.is_empty(),
            "fixture produced no generated functions"
        );
        assert!(generated.iter().all(|func| {
            func.where_clause(&db).predicates(&db).is_empty()
                && func.effects(&db).data(&db).is_empty()
        }));
    }
}
