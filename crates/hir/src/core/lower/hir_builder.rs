use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

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
        FrozenCode, FrozenCodeHole, FrozenCodeSource, FrozenMetaValue, FrozenQuoteConstructorSite,
        FrozenTemplateSource, FrozenVariantBinding,
    },
    hir_def::{
        ArithBinOp, AssocConstDef, AssocTyDef, Attr, AttrArg, AttrListId, BinOp, Body, BodyKind,
        Cond, CondId, ConstGenericArg, ConstGenericArgValue, EffectParamListId, Expr, ExprId,
        FieldDefListId, FieldIndex, Func, FuncModifiers, FuncParam, FuncParamListId, FuncParamMode,
        FuncParamName, GenericArg, GenericArgListId, GenericParam, GenericParamListId, IdentId,
        ImplTrait, IntegerId, ItemKind, LitKind, Mod, NormalAttr, Partial, Pat, PatId, PathId,
        PathKind, Stmt, StmtId, Struct, TopLevelMod, TrackedItemId, TrackedItemVariant, TraitRefId,
        TypeBound, TypeGenericArg, TypeGenericParam, TypeId, TypeKind, TypeMode, Visibility,
        WhereClauseId, WhereClauseOwner, WherePredicate,
        expr::CallArg,
        prim_ty::{IntTy, PrimTy as HirPrimTy, UintTy},
        scope_graph::ScopeId,
    },
    span::{DesugaredOrigin, HirOrigin},
};

use super::{
    FileLowerCtxt, body::BodyCtxt, generated::QuoteConstructionError,
    quote_errors::QuoteConstructionFault,
};

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

pub(super) fn copy_kind_bound(
    kind: &crate::hir_def::KindBound,
    budget: &QuoteBudget,
    depth: usize,
) -> Result<crate::hir_def::KindBound, QuoteConstructionError> {
    use crate::hir_def::KindBound;
    budget.charge_at_depth(depth)?;
    Ok(match kind {
        KindBound::Abs(left, right) => {
            let copy = |part: &Partial<Box<KindBound>>| match part {
                Partial::Present(kind) => copy_kind_bound(kind, budget, depth + 1)
                    .map(|kind| Partial::Present(Box::new(kind))),
                Partial::Absent => Ok(Partial::Absent),
            };
            KindBound::Abs(copy(left)?, copy(right)?)
        }
        _ => kind.clone(),
    })
}

fn validate_owner_bound_type<'db>(
    db: &'db dyn HirDb,
    ty: TypeId<'db>,
    budget: &QuoteBudget,
    depth: usize,
) -> Result<(), QuoteConstructionError> {
    budget.charge_at_depth(depth)?;
    match ty.data(db) {
        TypeKind::Path(Partial::Present(path)) => {
            validate_owner_bound_path(db, *path, budget, depth + 1)
        }
        TypeKind::Mode(_, Partial::Present(inner)) | TypeKind::Ptr(Partial::Present(inner)) => {
            validate_owner_bound_type(db, *inner, budget, depth + 1)
        }
        TypeKind::Tuple(elements) => {
            for element in elements.data(db) {
                validate_owner_bound_type(
                    db,
                    element
                        .to_opt()
                        .ok_or(QuoteConstructionFault::TargetBoundTypeIsMissing)?,
                    budget,
                    depth + 1,
                )?;
            }
            Ok(())
        }
        TypeKind::Primitive(_) | TypeKind::Never => Ok(()),
        _ => Err(QuoteConstructionFault::TargetBoundTypeNeedsCheckedReification.into()),
    }
}

fn validate_owner_bound_path<'db>(
    db: &'db dyn HirDb,
    path: PathId<'db>,
    budget: &QuoteBudget,
    depth: usize,
) -> Result<(), QuoteConstructionError> {
    budget.charge_at_depth(depth)?;
    if let Some(parent) = path.parent(db) {
        validate_owner_bound_path(db, parent, budget, depth + 1)?;
    }
    match path.kind(db) {
        PathKind::Ident {
            ident: Partial::Present(_),
            generic_args,
        } => {
            budget.charge_many(1 + generic_args.data(db).len())?;
            for arg in generic_args.data(db) {
                let ty =
                    match arg {
                        GenericArg::Type(arg) => arg
                            .ty
                            .to_opt()
                            .ok_or(QuoteConstructionFault::TargetBoundArgumentIsMissing)?,
                        GenericArg::AssocType(arg) => arg.ty.to_opt().ok_or(
                            QuoteConstructionFault::TargetAssociatedBoundArgumentIsMissing,
                        )?,
                        GenericArg::Const(_) => return Err(
                            QuoteConstructionFault::TargetConstBoundArgumentNeedsCheckedReification
                                .into(),
                        ),
                    };
                validate_owner_bound_type(db, ty, budget, depth + 1)?;
            }
            Ok(())
        }
        PathKind::BoundIngot { .. } => Ok(()),
        PathKind::BoundItem { display, .. } => {
            validate_owner_bound_path(db, display, budget, depth)
        }
        PathKind::QualifiedType { type_, trait_ } => {
            budget.charge()?;
            let path = trait_
                .path(db)
                .to_opt()
                .ok_or(QuoteConstructionFault::TargetQualifiedBoundTraitIsMissing)?;
            validate_owner_bound_type(db, type_, budget, depth + 1)?;
            validate_owner_bound_path(db, path, budget, depth + 1)
        }
        _ => Err(QuoteConstructionFault::TargetBoundPathNeedsCheckedReification.into()),
    }
}

pub(super) fn copy_owner_bound<'db>(
    db: &'db dyn HirDb,
    bound: &TypeBound<'db>,
    names: &HashSet<IdentId<'db>>,
    budget: &QuoteBudget,
    depth: usize,
) -> Result<TypeBound<'db>, QuoteConstructionError> {
    budget.charge_at_depth(depth)?;
    let TypeBound::Trait(trait_ref) = bound else {
        return Ok(match bound {
            TypeBound::Kind(Partial::Present(kind)) => {
                TypeBound::Kind(Partial::Present(copy_kind_bound(kind, budget, depth + 1)?))
            }
            _ => bound.clone(),
        });
    };
    let trait_path = trait_ref
        .path(db)
        .to_opt()
        .ok_or(QuoteConstructionFault::TargetWhereBoundIsMissing)?;
    if trait_path
        .root_ident(db)
        .is_some_and(|root| names.contains(&root))
    {
        return Err(QuoteConstructionFault::TargetWhereBoundIsNotATrait.into());
    }
    validate_owner_bound_path(db, trait_path, budget, depth + 1)?;
    Ok(TypeBound::Trait(*trait_ref))
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
/// The output name of a quoted local binder: the name it was written with,
/// marked with `'` so that no item can have it. Fragments are closed and
/// compiler-issued pattern binders have their own names, so a use resolves by
/// spelling to the binder the template meant, and no destination constant or
/// unit variant can turn the binder into a pattern that matches it.
pub(super) fn authored_binder_name<'db>(
    db: &'db dyn HirDb,
    template: Body<'db>,
    binder: PatId,
) -> Result<IdentId<'db>, QuoteConstructionFault> {
    match &template.pats(db)[binder] {
        Partial::Present(Pat::Path(Partial::Present(path), _)) => path
            .as_ident(db)
            .map(|name| IdentId::new(db, format!("{}'", name.data(db))))
            .ok_or(QuoteConstructionFault::PatternNeedsACheckedBinder),
        _ => Err(QuoteConstructionFault::PatternNeedsACheckedBinder),
    }
}

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
    ) -> Result<WhereClauseId<'db>, QuoteConstructionError> {
        let db = self.db();
        let params = owner.generic_owner().params_list(db);
        if params.data(db).len() != names.len() {
            return Err(QuoteConstructionFault::TargetParameterCountChanged.into());
        }
        self.ctxt
            .quote_budget
            .charge_many(1 + params.data(db).len())?;
        for (param, name) in params.data(db).iter().zip(names) {
            if param.name().to_opt() != Some(*name) {
                return Err(QuoteConstructionFault::TargetWhereTypeIsNotAnOwnerBinder.into());
            }
        }
        let names = names.iter().copied().collect::<HashSet<_>>();
        let source = match owner {
            crate::analysis::ty::adt_def::AdtRef::Struct(owner) => WhereClauseOwner::Struct(owner),
            crate::analysis::ty::adt_def::AdtRef::Enum(owner) => WhereClauseOwner::Enum(owner),
        }
        .where_clause(db);

        let predicates = source
            .type_predicates(db)
            .map(|(_, predicate)| {
                self.ctxt.quote_budget.charge_many(3)?;
                let ty = predicate
                    .ty
                    .to_opt()
                    .ok_or(QuoteConstructionFault::TargetWhereTypeIsMissing)?;
                let TypeKind::Path(Partial::Present(path)) = ty.data(db) else {
                    return Err(
                        QuoteConstructionFault::TargetWhereTypeNeedsCheckedReification.into(),
                    );
                };
                if path.parent(db).is_some() || !path.generic_args(db).is_empty(db) {
                    return Err(
                        QuoteConstructionFault::TargetWhereTypeNeedsCheckedReification.into(),
                    );
                }
                let original = path
                    .ident(db)
                    .to_opt()
                    .ok_or(QuoteConstructionFault::TargetWhereBinderIsMissing)?;
                if !names.contains(&original) {
                    return Err(QuoteConstructionFault::TargetWhereTypeIsNotAnOwnerBinder.into());
                }
                let bounds = predicate
                    .bounds
                    .iter()
                    .map(|bound| copy_owner_bound(db, bound, &names, &self.ctxt.quote_budget, 0))
                    .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
                Ok(WherePredicate {
                    ty: Partial::Present(ty),
                    bounds,
                })
            })
            .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
        let mut const_predicates = Vec::new();
        for (index, predicate) in source.const_predicates(db).iter().enumerate() {
            self.ctxt.quote_budget.charge_many(
                1 + 3 * predicate.exprs(db).len()
                    + predicate.stmts(db).len()
                    + predicate.pats(db).len()
                    + predicate.conds(db).len(),
            )?;
            let index = u32::try_from(index)
                .map_err(|_| QuoteConstructionFault::TargetConstPredicateCountExceedsU32)?;
            if !predicate.stmts(db).is_empty()
                || !predicate.pats(db).is_empty()
                || !predicate.conds(db).is_empty()
            {
                return Err(
                    QuoteConstructionFault::TargetConstPredicateNeedsCheckedReification.into(),
                );
            }
            let id = self
                .ctxt
                .joined_id(TrackedItemVariant::WhereConstPredicate(index));
            let mut body = BodyCtxt::new(self.ctxt, id);
            body.exprs = predicate.exprs(db).clone();
            body.source_map = predicate.source_map(db).clone();
            for (_, expr) in body.exprs.iter_mut() {
                match expr {
                    Partial::Present(Expr::Path(Partial::Present(path))) => {
                        if path.parent(db).is_some() || !path.generic_args(db).is_empty(db) {
                            return Err(
                                QuoteConstructionFault::TargetConstPathNeedsCheckedReification
                                    .into(),
                            );
                        }
                        let original = path
                            .ident(db)
                            .to_opt()
                            .ok_or(QuoteConstructionFault::TargetConstBinderIsMissing)?;
                        if !names.contains(&original) {
                            return Err(
                                QuoteConstructionFault::TargetConstPathNeedsCheckedReification
                                    .into(),
                            );
                        }
                    }
                    Partial::Present(Expr::Lit(_) | Expr::Bin(..) | Expr::Un(..)) => {}
                    _ => {
                        return Err(
                            QuoteConstructionFault::TargetConstPredicateNeedsCheckedReification
                                .into(),
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
    ) -> Result<TypeId<'db>, QuoteConstructionFault> {
        let db = self.db();
        let name = owner
            .name(db)
            .ok_or(QuoteConstructionFault::AuthorizedRecordNameIsMissing)?;
        let params = owner.generic_owner().params_list(db);
        if params.data(db).len() != names.len() {
            return Err(QuoteConstructionFault::RecordGenericParameterCountChanged);
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
        bindings: QuoteBodyBindings<'_, 'db>,
    ) -> Result<(Body<'db>, Vec<(ExprId, IdentId<'db>)>), QuoteConstructionError> {
        let mut body = BodyBuilder::new(
            self.ctxt,
            self.roots,
            self.desugared.clone(),
            self.reserved.clone(),
            TrackedItemVariant::WhereConstPredicate(index),
        );
        let budget = body.body.f_ctxt.quote_budget.clone();
        let (root, copied) =
            body.copy_closed_quote_body(template, root, provider_scope, budget, bindings)?;
        Ok((
            body.body.build(None, root, BodyKind::Anonymous),
            copied.generics,
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
        match self.generated_func_with_body_checked(
            spec.name,
            |_| Ok((spec, ())),
            |body, ()| {
                build_body(body);
                Ok::<_, std::convert::Infallible>(())
            },
        ) {
            Ok(function) => function,
            Err(never) => match never {},
        }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, salsa::Update)]
pub(super) enum QuoteDestinationBinding {
    Param(usize),
    Local(PatId),
}

struct QuoteCopyMap<'a, 'db> {
    holes: &'a [FrozenCodeHole],
    groups: Rc<HashMap<FrozenVariantBinding, Vec<(IdentId<'db>, PatId)>>>,
    group_locals: Rc<RefCell<HashMap<PatId, Vec<ExprId>>>>,
    group_names: Rc<Cell<usize>>,
    fragments: Option<(&'a FrozenCode, &'a [Option<QuoteFragmentSource<'db>>])>,
    fragment_depth: usize,
    provider_scope: Option<ScopeId<'db>>,
    budget: QuoteBudget,
    open_bindings: &'a [(IdentId<'db>, usize)],
    visible_locals: Vec<(IdentId<'db>, IdentId<'db>, PatId)>,
    owner_generic_bindings: &'a [IdentId<'db>],
    generic_bindings: &'a [IdentId<'db>],
    allow_self: bool,
    self_record_inits: Vec<ExprId>,
    constructor_paths: HashMap<FrozenQuoteConstructorSite, PathId<'db>>,
    paths: HashMap<ExprId, PathId<'db>>,
    members: HashMap<u32, QuoteMemberHole<'db>>,
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
    fragment_path_scopes: Vec<(ExprId, ScopeId<'db>)>,
    fragment_bindings: Vec<(ExprId, QuoteDestinationBinding)>,
    fragment_generics: Vec<(ExprId, IdentId<'db>)>,
}

fn map_group_field<'db>(
    map: &QuoteCopyMap<'_, 'db>,
    group: &FrozenVariantBinding,
    field: usize,
) -> Result<(IdentId<'db>, PatId), QuoteConstructionFault> {
    map.groups
        .get(group)
        .ok_or(QuoteConstructionFault::BinderGroupIsNotBoundInThisScope)?
        .get(field)
        .copied()
        .ok_or(QuoteConstructionFault::BinderFieldIndexIsOutOfBounds)
}

pub(super) enum QuoteFragmentRoot {
    Expr {
        root: ExprId,
        splices: Option<Vec<u32>>,
    },
    Arms(Vec<crate::hir_def::MatchArm>),
}

#[derive(Clone, Copy)]
pub(super) struct QuoteMemberHole<'db> {
    pub(super) slot: u32,
    pub(super) label: IdentId<'db>,
    pub(super) field: crate::hir_def::QuoteFieldGrant<'db>,
}

pub(super) struct QuoteFragmentSource<'db> {
    pub(super) template: Body<'db>,
    pub(super) root: QuoteFragmentRoot,
    pub(super) open_names: Vec<IdentId<'db>>,
    pub(super) member_holes: Vec<QuoteMemberHole<'db>>,
    pub(super) rooted_funcs: Vec<(ExprId, PathId<'db>)>,
    pub(super) qualified_paths: Vec<(ExprId, PathId<'db>)>,
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

/// Inputs for copying one quoted body, retaining each expected identity beside
/// its authored site throughout transport.
#[derive(Default)]
pub(super) struct QuoteBodyBindings<'a, 'db> {
    pub(super) holes: &'a [FrozenCodeHole],
    pub(super) cast_types: &'a [(ExprId, TypeId<'db>)],
    pub(super) fragments: Option<(&'a FrozenCode, &'a [Option<QuoteFragmentSource<'db>>])>,
    pub(super) open_bindings: &'a [(IdentId<'db>, usize)],
    pub(super) owner_generic_bindings: &'a [IdentId<'db>],
    pub(super) generic_bindings: &'a [IdentId<'db>],
    pub(super) allow_self: bool,
    pub(super) rooted_funcs: &'a [(ExprId, PathId<'db>)],
    pub(super) qualified_paths: &'a [(ExprId, PathId<'db>)],
    pub(super) constructor_paths: &'a [(
        FrozenQuoteConstructorSite,
        PathId<'db>,
        super::generated::QuoteConstructorIdentity<'db>,
    )],
    pub(super) self_record_inits: &'a [ExprId],
    pub(super) locals: &'a [(PatId, IdentId<'db>, Vec<ExprId>)],
    pub(super) local_types: &'a [(StmtId, TypeId<'db>)],
    pub(super) param_uses: &'a [(ExprId, IdentId<'db>, usize)],
    pub(super) effect_uses: &'a [(ExprId, IdentId<'db>, usize)],
    pub(super) generic_uses: &'a [(ExprId, IdentId<'db>)],
    pub(super) member_holes: &'a [QuoteMemberHole<'db>],
}

pub(super) struct QuoteCopySummary<'db> {
    pub(super) constructor_sites: Vec<(
        FrozenQuoteConstructorSite,
        super::generated::QuoteConstructorIdentity<'db>,
    )>,
    pub(super) locals: Vec<(PatId, Vec<ExprId>)>,
    pub(super) bindings: Vec<(ExprId, QuoteDestinationBinding)>,
    pub(super) effects: Vec<(ExprId, usize)>,
    pub(super) generics: Vec<(ExprId, IdentId<'db>)>,
    pub(super) method_calls: Vec<QuoteMethodCall<'db>>,
}

pub(super) const MAX_QUOTED_NODES: usize = 65_536;
const MAX_QUOTED_FUNCTION_NODES: usize = 8_192;
const MAX_QUOTED_FRAGMENT_DEPTH: usize = 256;

/// All quoted copying within one expansion context shares this counter.
#[derive(Clone, Default)]
pub(super) struct QuoteBudget(Rc<Cell<usize>>, Option<Rc<Cell<usize>>>);
impl QuoteBudget {
    fn charge_at_depth(&self, depth: usize) -> Result<(), QuoteConstructionError> {
        if depth > MAX_QUOTED_FRAGMENT_DEPTH {
            return Err(QuoteConstructionError::type_depth_limit(
                depth,
                MAX_QUOTED_FRAGMENT_DEPTH,
            ));
        }
        self.charge()
    }

    pub(super) fn charge_emitted_type(
        &self,
        ty: &crate::core::meta::FrozenQuoteSignatureType,
        depth: usize,
    ) -> Result<(), QuoteConstructionError> {
        self.charge_type_nodes(ty.reification_nodes(), depth)
    }
    pub(super) fn charge_type_nodes(
        &self,
        nodes: usize,
        depth: usize,
    ) -> Result<(), QuoteConstructionError> {
        let request = Self(self.0.clone(), None);
        request.charge_at_depth(depth)?;
        request.charge_many(nodes.saturating_sub(1))
    }

    fn for_body(&self) -> Self {
        Self(self.0.clone(), Some(Rc::new(Cell::new(0))))
    }

    pub(super) fn charge_expression_paths(
        &self,
        node: &crate::core::meta::FrozenCodeNode,
        function: Option<u32>,
    ) -> Result<(), QuoteConstructionError> {
        self.charge_auxiliary(
            node.rooted_path_nodes(function)
                + if function.is_none() {
                    node.cast_primitives.len()
                } else {
                    0
                },
        )
    }

    pub(super) fn charge_auxiliary(&self, nodes: usize) -> Result<(), QuoteConstructionError> {
        Self(self.0.clone(), None).charge_many(nodes)
    }

    pub(super) fn charge(&self) -> Result<(), QuoteConstructionError> {
        self.charge_many(1)
    }
    pub(super) fn charge_many(&self, nodes: usize) -> Result<(), QuoteConstructionError> {
        let used = self.0.get().saturating_add(nodes);
        if used > MAX_QUOTED_NODES {
            return Err(QuoteConstructionError::request_node_limit(
                used,
                MAX_QUOTED_NODES,
            ));
        }
        if let Some(function) = &self.1 {
            let function_used = function.get().saturating_add(nodes);
            if function_used > MAX_QUOTED_FUNCTION_NODES {
                return Err(QuoteConstructionError::function_node_limit(
                    function_used,
                    MAX_QUOTED_FUNCTION_NODES,
                ));
            }
            function.set(function_used);
        }
        self.0.set(used);
        Ok(())
    }
}

fn classify<K: Eq + std::hash::Hash, V>(
    map: &mut HashMap<K, V>,
    key: K,
    value: V,
    error: QuoteConstructionFault,
) -> Result<(), QuoteConstructionFault> {
    match map.entry(key) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(value);
            Ok(())
        }
        std::collections::hash_map::Entry::Occupied(_) => Err(error),
    }
}

impl<'a, 'db> QuoteCopyMap<'a, 'db> {
    fn copied_expr(
        &self,
        old: ExprId,
        error: QuoteConstructionFault,
    ) -> Result<ExprId, QuoteConstructionFault> {
        self.exprs.get(&old).copied().ok_or(error)
    }

    fn copied_pat(
        &self,
        old: PatId,
        error: QuoteConstructionFault,
    ) -> Result<PatId, QuoteConstructionFault> {
        self.pats.get(&old).copied().ok_or(error)
    }

    fn copied_site(&self, old: FrozenQuoteConstructorSite) -> Option<FrozenQuoteConstructorSite> {
        match old {
            FrozenQuoteConstructorSite::Expr(site) => self
                .exprs
                .get(&site)
                .copied()
                .map(FrozenQuoteConstructorSite::Expr),
            FrozenQuoteConstructorSite::Pat(site) => self
                .pats
                .get(&site)
                .copied()
                .map(FrozenQuoteConstructorSite::Pat),
        }
    }

    fn hole_source(&self, slot: u32) -> Option<crate::core::meta::FrozenQuoteSpan> {
        match self.holes.get(slot as usize) {
            Some(FrozenCodeHole::Code(_, source)) => source.clone(),
            _ => None,
        }
    }

    fn add_constructor_path(
        &mut self,
        site: FrozenQuoteConstructorSite,
        path: PathId<'db>,
        error: QuoteConstructionFault,
    ) -> Result<(), QuoteConstructionFault> {
        classify(&mut self.constructor_paths, site, path, error)?;
        if let FrozenQuoteConstructorSite::Expr(site) = site {
            // Constructor roots override the classified expression path, but
            // retain any explicit record type already present.
            self.paths.insert(site, path);
            self.record_paths.entry(site).or_insert(path);
        }
        Ok(())
    }

    fn prepare_groups_mutation(&self) -> Result<(), QuoteConstructionError> {
        if Rc::strong_count(&self.groups) > 1 {
            self.budget
                .charge_auxiliary(self.groups.values().map(|fields| 1 + fields.len()).sum())?;
        }
        Ok(())
    }

    fn charge_node(&mut self) -> Result<(), QuoteConstructionError> {
        self.budget.charge()
    }

    fn new(
        db: &'db dyn HirDb,
        holes: &'a [FrozenCodeHole],
        rooted_funcs: &[(ExprId, PathId<'db>)],
        locals: &[(PatId, IdentId<'db>, Vec<ExprId>)],
        param_uses: &[(ExprId, IdentId<'db>)],
        generic_uses: &[(ExprId, IdentId<'db>)],
        member_holes: &[QuoteMemberHole<'db>],
    ) -> Result<Self, QuoteConstructionFault> {
        let mut paths = HashMap::new();
        let mut local_binders = HashMap::new();
        for (site, path) in rooted_funcs {
            classify(
                &mut paths,
                *site,
                *path,
                QuoteConstructionFault::PathIsClassifiedTwice,
            )?;
        }
        for (binder, name, _) in locals {
            classify(
                &mut local_binders,
                *binder,
                *name,
                QuoteConstructionFault::BinderIsClassifiedTwice,
            )?;
        }
        let uses = locals
            .iter()
            .flat_map(|(_, name, uses)| uses.iter().map(move |site| (site, name)))
            .chain(param_uses.iter().map(|(site, name)| (site, name)))
            .chain(generic_uses.iter().map(|(site, name)| (site, name)));
        for (site, name) in uses {
            classify(
                &mut paths,
                *site,
                PathId::from_ident(db, *name),
                QuoteConstructionFault::PathIsClassifiedTwice,
            )?;
        }
        let mut members = HashMap::new();
        for member in member_holes {
            classify(
                &mut members,
                member.slot,
                *member,
                QuoteConstructionFault::MemberHoleIsClassifiedTwice,
            )?;
        }
        Ok(Self {
            holes,
            groups: Rc::new(HashMap::new()),
            group_locals: Rc::new(RefCell::new(HashMap::new())),
            group_names: Rc::new(Cell::new(0)),
            fragments: None,
            fragment_depth: 0,
            provider_scope: None,
            budget: QuoteBudget::default(),
            open_bindings: &[],
            visible_locals: Vec::new(),
            owner_generic_bindings: &[],
            generic_bindings: &[],
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
            fragment_path_scopes: Vec::new(),
            fragment_bindings: Vec::new(),
            fragment_generics: Vec::new(),
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
        bindings: QuoteBodyBindings<'_, 'db>,
    ) -> Result<QuoteCopySummary<'db>, QuoteConstructionError> {
        let budget = self.body.f_ctxt.quote_budget.for_body();
        let (root, copied) =
            self.copy_closed_quote_body(template, root, provider_scope, budget, bindings)?;
        self.emit_expr_stmt(root);
        Ok(copied)
    }

    fn copy_closed_quote_body(
        &mut self,
        template: Body<'db>,
        root: ExprId,
        provider_scope: ScopeId<'db>,
        budget: QuoteBudget,
        bindings: QuoteBodyBindings<'_, 'db>,
    ) -> Result<(ExprId, QuoteCopySummary<'db>), QuoteConstructionError> {
        let QuoteBodyBindings {
            holes,
            cast_types,
            fragments,
            open_bindings,
            owner_generic_bindings,
            generic_bindings,
            allow_self,
            rooted_funcs,
            qualified_paths,
            constructor_paths,
            self_record_inits,
            locals,
            local_types,
            param_uses,
            effect_uses,
            generic_uses,
            member_holes,
        } = bindings;
        let mut all_paths = rooted_funcs
            .iter()
            .map(|(site, path)| (*site, *path))
            .collect::<Vec<_>>();
        all_paths.extend(qualified_paths.iter().map(|(site, path)| (*site, *path)));
        let use_paths = param_uses
            .iter()
            .chain(effect_uses)
            .map(|(site, name, _)| (*site, *name))
            .collect::<Vec<_>>();
        let mut map = QuoteCopyMap::new(
            self.db(),
            holes,
            &all_paths,
            locals,
            &use_paths,
            generic_uses,
            member_holes,
        )?;
        map.budget = budget;
        map.fragments = fragments;
        map.open_bindings = open_bindings;
        map.owner_generic_bindings = owner_generic_bindings;
        map.generic_bindings = generic_bindings;
        for (site, path, _) in constructor_paths {
            map.add_constructor_path(
                *site,
                *path,
                QuoteConstructionFault::VariantSiteIsClassifiedTwice,
            )?;
        }
        map.allow_self = allow_self;
        map.self_record_inits = self_record_inits.to_vec();
        map.cast_types = cast_types.iter().copied().collect();
        for (stmt, ty) in local_types {
            classify(
                &mut map.local_types,
                *stmt,
                *ty,
                QuoteConstructionFault::LocalTypeIsClassifiedTwice,
            )?;
        }
        map.provider_scope = Some(provider_scope);
        let root = self.copy_quote_expr(template, root, &mut map)?;
        for site in &map.method_calls {
            self.body
                .quote_scopes
                .push(((*site).into(), provider_scope.into()));
        }
        self.body.quote_scopes.extend(
            map.fragment_method_calls
                .iter()
                .map(|call| (call.site.into(), call.scope.into())),
        );
        self.body.quote_scopes.extend(
            map.fragment_path_scopes
                .iter()
                .map(|(site, scope)| ((*site).into(), (*scope).into())),
        );
        let rooted_paths = rooted_funcs
            .iter()
            .map(|(site, _)| map.copied_expr(*site, QuoteConstructionFault::RootedPathWasNotCopied))
            .collect::<Result<Vec<_>, QuoteConstructionFault>>()?;
        self.body.quote_scopes.extend(
            rooted_paths
                .iter()
                .map(|site| ((*site).into(), provider_scope.into())),
        );
        let mut locals = locals
            .iter()
            .map(|(binder, _, uses)| {
                let binder =
                    map.copied_pat(*binder, QuoteConstructionFault::LocalBinderWasNotCopied)?;
                let uses = uses
                    .iter()
                    .map(|site| {
                        map.copied_expr(*site, QuoteConstructionFault::LocalUseWasNotCopied)
                    })
                    .collect::<Result<Vec<_>, QuoteConstructionFault>>()?;
                Ok((binder, uses))
            })
            .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
        for (site, _) in qualified_paths {
            let copied =
                map.copied_expr(*site, QuoteConstructionFault::QualifiedConstantWasNotCopied)?;
            self.body
                .quote_scopes
                .push((copied.into(), provider_scope.into()));
        }
        let bindings = param_uses
            .iter()
            .map(|(site, _, index)| {
                Ok((
                    map.copied_expr(*site, QuoteConstructionFault::ParameterPathWasNotCopied)?,
                    QuoteDestinationBinding::Param(*index),
                ))
            })
            .collect::<Result<Vec<_>, QuoteConstructionFault>>()?;
        let effects = effect_uses
            .iter()
            .map(|(site, _, index)| {
                Ok((
                    map.copied_expr(*site, QuoteConstructionFault::ParameterPathWasNotCopied)?,
                    *index,
                ))
            })
            .collect::<Result<Vec<_>, QuoteConstructionFault>>()?;
        let generics = generic_uses
            .iter()
            .map(|(site, name)| {
                Ok((
                    map.copied_expr(*site, QuoteConstructionFault::GenericPathWasNotCopied)?,
                    *name,
                ))
            })
            .collect::<Result<Vec<_>, QuoteConstructionFault>>()?;
        let constructor_sites = constructor_paths
            .iter()
            .filter(|(_, path, _)| {
                path.segment(self.db(), 0)
                    .is_some_and(|root| root.is_self_ty(self.db()))
            })
            .map(|(site, _, expected)| map.copied_site(*site).map(|site| (site, *expected)))
            .collect::<Option<Vec<_>>>()
            .ok_or(QuoteConstructionFault::VariantSiteWasNotCopied)?;
        map.budget.charge_auxiliary(
            map.group_locals
                .borrow()
                .values()
                .map(|uses| 1 + uses.len())
                .sum(),
        )?;
        let mut group_locals = map
            .group_locals
            .borrow()
            .iter()
            .map(|(binder, uses)| (*binder, uses.clone()))
            .collect::<Vec<_>>();
        group_locals.sort_by_key(|(binder, _)| binder.as_u32());
        locals.extend(group_locals);
        let mut generics = generics;
        generics.extend(map.fragment_generics);
        let mut bindings = bindings;
        bindings.extend(map.fragment_bindings);
        let mut method_calls = map
            .method_calls
            .into_iter()
            .map(|site| QuoteMethodCall {
                site,
                scope: provider_scope,
                selected: None,
            })
            .collect::<Vec<_>>();
        method_calls.extend(map.fragment_method_calls);
        Ok((
            root,
            QuoteCopySummary {
                constructor_sites,
                locals,
                bindings,
                effects,
                generics,
                method_calls,
            },
        ))
    }

    fn quote_scalar_expr(
        &mut self,
        hole: &FrozenCodeHole,
        map: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<ExprId, QuoteConstructionError> {
        let FrozenCodeHole::Value(value, _) = hole else {
            return Err(QuoteConstructionFault::CodeSpliceRequiresFragmentTransplantation.into());
        };
        let FrozenMetaValue::Scalar { value, primitive } = value.as_ref() else {
            return Err(QuoteConstructionFault::SpliceMustBeAScalarValue.into());
        };
        let primitive = primitive
            .and_then(quote_hir_primitive)
            .ok_or(QuoteConstructionFault::ScalarHasNoSupportedPrimitiveType)?;
        let db = self.db();
        let literal = match value {
            SemConstScalar::Bool(value) if primitive == HirPrimTy::Bool => LitKind::Bool(*value),
            SemConstScalar::Int { value } if primitive != HirPrimTy::Bool => {
                if value.sign() == Sign::Minus {
                    return Err(
                        QuoteConstructionFault::NegativeQuotedScalarRequiresSignedLiteralLowering
                            .into(),
                    );
                }
                map.budget.charge_auxiliary(1)?;
                LitKind::Int(IntegerId::new(db, value.magnitude().clone()))
            }
            _ => return Err(QuoteConstructionFault::ScalarDoesNotMatchItsPrimitiveType.into()),
        };
        let literal = self.push_expr(Expr::Lit(literal));
        if primitive == HirPrimTy::Bool {
            Ok(literal)
        } else {
            map.charge_node()?;
            map.budget.charge_auxiliary(1)?;
            let ty = TypeId::new(db, TypeKind::Primitive(primitive));
            Ok(self.push_expr(Expr::Cast(literal, Partial::Present(ty))))
        }
    }

    fn copy_quote_expr(
        &mut self,
        template: Body<'db>,
        old: ExprId,
        map: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<ExprId, QuoteConstructionError> {
        self.copy_quote_expr_inner(template, old, map)
            .map_err(|error| {
                let source = match &template.exprs(self.db())[old] {
                    Partial::Present(Expr::Hole(slot)) => map.hole_source(*slot),
                    _ => None,
                };
                error.at(source)
            })
    }

    fn copy_quote_expr_inner(
        &mut self,
        template: Body<'db>,
        old: ExprId,
        map: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<ExprId, QuoteConstructionError> {
        if let Some(id) = map.exprs.get(&old) {
            return Ok(*id);
        }
        map.charge_node()?;
        let db = self.db();
        let Partial::Present(expr) = &template.exprs(db)[old] else {
            return Err(QuoteConstructionFault::FunctionContainsAnInvalidExpression.into());
        };
        if let Expr::Hole(slot) = expr {
            let hole = map
                .holes
                .get(*slot as usize)
                .ok_or(QuoteConstructionFault::SpliceSlotIsMissing)?;
            let id = match hole {
                FrozenCodeHole::Code(child, ..) => self.copy_quote_fragment(*child, map)?,
                FrozenCodeHole::Value(..) => self.quote_scalar_expr(hole, map)?,
            };
            let issued_field_reference = match hole {
                FrozenCodeHole::Code(child, ..) => map
                    .fragments
                    .and_then(|(code, _)| code.nodes.get(*child))
                    .is_some_and(|node| matches!(node.source, FrozenCodeSource::Field(..))),
                FrozenCodeHole::Value(..) => false,
            };
            if matches!(hole, FrozenCodeHole::Value(..)) || issued_field_reference {
                let origin = self.quote_origin(
                    template,
                    template.source_map(db).expr_map.node_to_source(old),
                );
                self.body.source_map.expr_map.insert(id, origin.clone());
                if let Partial::Present(Expr::Cast(inner, _)) = self.body.exprs[id] {
                    self.body.source_map.expr_map.insert(inner, origin);
                }
            }
            map.exprs.insert(old, id);
            return Ok(id);
        }
        let expr = match expr {
            Expr::Lit(value) => Expr::Lit(*value),
            Expr::Path(Partial::Present(source)) => {
                let path = map
                    .paths
                    .get(&old)
                    .ok_or(QuoteConstructionFault::FunctionPathHasNoCheckedRootOrBinder)?;
                if source.strip_generic_args(db) != *source && path.strip_generic_args(db) == *path
                {
                    return Err(
                        QuoteConstructionFault::FunctionPathNeedsGenericArgumentRewriting.into(),
                    );
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
                    return Err(
                        QuoteConstructionFault::RecordConstructionHasNoCheckedNominalOwner.into(),
                    );
                };
                let fields = fields
                    .iter()
                    .map(|field| {
                        map.charge_node()?;
                        let label = match field.label {
                            Some(crate::hir_def::FieldLabel::Hole(slot)) => Some(
                                map.members
                                    .get(&slot)
                                    .ok_or(QuoteConstructionFault::FieldLabelHasNoCheckedField)?
                                    .label,
                            ),
                            _ => field.label_eagerly(db, template),
                        };
                        Ok(crate::hir_def::Field {
                            label: label.map(crate::hir_def::FieldLabel::Ident),
                            expr: self.copy_quote_expr(template, field.expr, map)?,
                        })
                    })
                    .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
                Expr::RecordInit(Partial::Present(path), fields)
            }
            Expr::Call(callee, args) => Expr::Call(
                self.copy_quote_expr(template, *callee, map)?,
                self.copy_quote_call_args(template, args, map)?,
            ),
            Expr::Assert(args) => Expr::Assert(self.copy_quote_call_args(template, args, map)?),
            Expr::MethodCall(receiver, Partial::Present(name), generic_args, args) => {
                if generic_args.is_given(db) {
                    return Err(
                        QuoteConstructionFault::MethodCallNeedsCheckedGenericArgumentRewriting
                            .into(),
                    );
                }
                Expr::MethodCall(
                    self.copy_quote_expr(template, *receiver, map)?,
                    Partial::Present(*name),
                    GenericArgListId::none(db),
                    self.copy_quote_call_args(template, args, map)?,
                )
            }
            Expr::Field(receiver, Partial::Present(FieldIndex::Hole(slot))) => {
                let receiver = self.copy_quote_expr(template, *receiver, map)?;
                let name = map
                    .members
                    .get(slot)
                    .ok_or(QuoteConstructionFault::MemberHoleHasNoCheckedField)?;
                Expr::Field(receiver, Partial::Present(FieldIndex::Ident(name.label)))
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
            Expr::Cast(value, Partial::Present(_)) => {
                let checked = map
                    .cast_types
                    .get(&old)
                    .copied()
                    .ok_or(QuoteConstructionFault::CastHasNoFrozenType)?;
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
                Partial::Present(self.copy_quote_arms(template, arms, map)?),
            ),
            Expr::Block(stmts) => {
                let enclosing = map.groups.clone();
                let visible = map.visible_locals.len();
                self.body.f_ctxt.enter_block_scope();
                let stmts = stmts
                    .iter()
                    .map(|stmt| self.copy_quote_stmt(template, *stmt, map))
                    .collect::<Result<Vec<_>, _>>()?;
                map.groups = enclosing;
                map.visible_locals.truncate(visible);
                Expr::Block(stmts)
            }
            _ => {
                return Err(
                    QuoteConstructionFault::FunctionBodyRequiresASpliceOrNameRewrite.into(),
                );
            }
        };
        let is_block = matches!(expr, Expr::Block(_));
        let is_method_call = matches!(expr, Expr::MethodCall(..));
        let retains_scope = matches!(expr, Expr::Field(..) | Expr::RecordInit(..))
            || map
                .constructor_paths
                .contains_key(&FrozenQuoteConstructorSite::Expr(old));
        let origin = self.quote_origin(
            template,
            template.source_map(db).expr_map.node_to_source(old),
        );
        let id = self.body.push_expr(expr, origin);
        if retains_scope && let Some(scope) = map.provider_scope {
            let fields = match &template.exprs(db)[old] {
                Partial::Present(Expr::Field(_, Partial::Present(FieldIndex::Hole(slot)))) => map
                    .members
                    .get(slot)
                    .map(|member| vec![member.field])
                    .unwrap_or_default(),
                Partial::Present(Expr::RecordInit(_, fields)) => fields
                    .iter()
                    .filter_map(|field| {
                        let Some(crate::hir_def::FieldLabel::Hole(slot)) = field.label else {
                            return None;
                        };
                        map.members.get(&slot).map(|member| member.field)
                    })
                    .collect(),
                _ => Vec::new(),
            };
            self.body.quote_scopes.push((
                id.into(),
                crate::hir_def::QuoteScope {
                    scope,
                    source: Some((template, old.into())),
                    fields,
                },
            ));
        }
        if is_method_call {
            map.method_calls.push(id);
        }
        if is_block {
            self.body.f_ctxt.leave_block_scope(id);
        }
        map.exprs.insert(old, id);
        Ok(id)
    }

    fn copy_quote_arm(
        &mut self,
        template: Body<'db>,
        arm: &crate::hir_def::MatchArm,
        map: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<crate::hir_def::MatchArm, QuoteConstructionError> {
        let Some((pat, body)) = arm.parts() else {
            return Err(QuoteConstructionFault::ArmListSpliceNeedsAFragmentSource.into());
        };
        map.charge_node()?;
        let enclosing = map.groups.clone();
        let visible = map.visible_locals.len();
        let result = (|| {
            Ok(crate::hir_def::MatchArm::Arm {
                pat: self.copy_quote_pat(template, pat, map)?,
                body: self.copy_quote_expr(template, body, map)?,
            })
        })();
        map.groups = enclosing;
        map.visible_locals.truncate(visible);
        result
    }

    fn copy_quote_call_args(
        &mut self,
        template: Body<'db>,
        args: &[CallArg<'db>],
        map: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<Vec<CallArg<'db>>, QuoteConstructionError> {
        let db = self.db();
        args.iter()
            .map(|arg| {
                // Keep a shorthand label before a hygienic binder rename changes its name.
                Ok(CallArg {
                    label: arg.label_eagerly(db, template),
                    expr: self.copy_quote_expr(template, arg.expr, map)?,
                })
            })
            .collect()
    }

    fn copy_quote_arms(
        &mut self,
        template: Body<'db>,
        arms: &[crate::hir_def::MatchArm],
        map: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<Vec<crate::hir_def::MatchArm>, QuoteConstructionError> {
        let mut copied = Vec::new();
        for arm in arms {
            match arm {
                crate::hir_def::MatchArm::Arm { .. } => {
                    copied.push(self.copy_quote_arm(template, arm, map)?)
                }
                crate::hir_def::MatchArm::Hole(slot) => {
                    let Some(FrozenCodeHole::Code(child, source)) = map.holes.get(*slot as usize)
                    else {
                        return Err(QuoteConstructionFault::ArmListSpliceNeedsCodeArm.into());
                    };
                    copied.extend(
                        self.copy_quote_arm_fragment(*child, map)
                            .map_err(|error| error.at(source.clone()))?,
                    );
                }
            }
        }
        Ok(copied)
    }

    fn copy_quote_arm_fragment(
        &mut self,
        child: usize,
        parent: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<Vec<crate::hir_def::MatchArm>, QuoteConstructionError> {
        // Flatten concatenation iteratively. The existing node budget bounds
        // graph traversal; expression nesting limits do not bound arm counts.
        enum Work {
            Fragment(usize, Option<crate::core::meta::FrozenQuoteSpan>),
            Arm(crate::hir_def::MatchArm),
        }
        let (code, sources) = parent
            .fragments
            .ok_or(QuoteConstructionFault::ArmSpliceHasNoFrozenCode)?;
        let mut pending = vec![Work::Fragment(child, None)];
        let mut copied = Vec::new();
        while let Some(work) = pending.pop() {
            let Work::Fragment(child, site) = work else {
                if let Work::Arm(arm) = work {
                    copied.push(arm);
                }
                continue;
            };
            let result = (|| {
                parent.charge_node()?;
                let node = code
                    .nodes
                    .get(child)
                    .ok_or(QuoteConstructionFault::ArmSpliceHasNoFrozenNode)?;
                if node == &crate::core::meta::FrozenCodeNode::empty(None) {
                    return Ok(());
                }
                let source = sources
                    .get(child)
                    .and_then(Option::as_ref)
                    .ok_or(QuoteConstructionFault::ArmSpliceNeedsAQuotedArmList)?;
                let arms = match &source.root {
                    QuoteFragmentRoot::Arms(arms) => {
                        self.copy_quote_fragment_with(child, parent, |this, source, map| {
                            arms.iter()
                                .map(|arm| match arm {
                                    crate::hir_def::MatchArm::Hole(slot) => {
                                        Ok(crate::hir_def::MatchArm::Hole(*slot))
                                    }
                                    _ => this.copy_quote_arm(source.template, arm, map),
                                })
                                .collect::<Result<Vec<_>, QuoteConstructionError>>()
                        })?
                    }
                    QuoteFragmentRoot::Expr {
                        splices: Some(slots),
                        ..
                    } => slots
                        .iter()
                        .copied()
                        .map(crate::hir_def::MatchArm::Hole)
                        .collect(),
                    _ => return Err(QuoteConstructionFault::ArmSpliceNeedsCodeArm.into()),
                };
                for arm in arms.into_iter().rev() {
                    match arm {
                        crate::hir_def::MatchArm::Hole(slot) => {
                            let Some(FrozenCodeHole::Code(next, source)) =
                                node.holes.get(slot as usize)
                            else {
                                return Err(
                                    QuoteConstructionFault::ArmListSpliceNeedsCodeArm.into()
                                );
                            };
                            pending.push(Work::Fragment(
                                *next,
                                source.clone().or_else(|| site.clone()),
                            ));
                        }
                        _ => pending.push(Work::Arm(arm)),
                    }
                }
                Ok(())
            })();
            result.map_err(|error: QuoteConstructionError| error.at(site))?;
        }
        Ok(copied)
    }

    fn copy_quote_fragment(
        &mut self,
        child: usize,
        parent: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<ExprId, QuoteConstructionError> {
        if parent.fragment_depth >= MAX_QUOTED_FRAGMENT_DEPTH {
            return Err(QuoteConstructionError::fragment_depth_limit(
                parent.fragment_depth + 1,
                MAX_QUOTED_FRAGMENT_DEPTH,
            ));
        }
        let (code, _) = parent
            .fragments
            .ok_or(QuoteConstructionFault::CodeSpliceRequiresFragmentTransplantation)?;
        let node = code
            .nodes
            .get(child)
            .ok_or(QuoteConstructionFault::CodeSpliceHasNoFrozenNode)?;
        if matches!(node.source, FrozenCodeSource::Pattern(_)) {
            return Err(QuoteConstructionFault::PatternCodeCannotStandForAnExpression.into());
        }
        if let FrozenCodeSource::Field(group, field) = &node.source {
            let (name, binder) = map_group_field(parent, group, *field)?;
            let expr = self.push_expr(Expr::Path(Partial::Present(PathId::from_ident(
                self.db(),
                name,
            ))));
            parent
                .group_locals
                .borrow_mut()
                .get_mut(&binder)
                .ok_or(QuoteConstructionFault::BinderGroupHasNoHygieneRecord)?
                .push(expr);
            return Ok(expr);
        }
        self.copy_quote_fragment_with(child, parent, |this, source, map| {
            let QuoteFragmentRoot::Expr { root, .. } = &source.root else {
                return Err(QuoteConstructionFault::ExpressionSpliceNeedsExpressionCode.into());
            };
            this.copy_quote_expr(source.template, *root, map)
        })
    }

    fn copy_quote_fragment_with<T>(
        &mut self,
        child: usize,
        parent: &mut QuoteCopyMap<'_, 'db>,
        copy: impl FnOnce(
            &mut Self,
            &QuoteFragmentSource<'db>,
            &mut QuoteCopyMap<'_, 'db>,
        ) -> Result<T, QuoteConstructionError>,
    ) -> Result<T, QuoteConstructionError> {
        let (code, sources) = parent
            .fragments
            .ok_or(QuoteConstructionFault::CodeSpliceRequiresFragmentTransplantation)?;
        let node = code
            .nodes
            .get(child)
            .ok_or(QuoteConstructionFault::CodeSpliceHasNoFrozenNode)?;
        let source = sources
            .get(child)
            .and_then(Option::as_ref)
            .ok_or(QuoteConstructionFault::SpliceHasNoCheckedFragmentSource)?;
        if source.has_block_items
            || node.rooted_funcs.iter().any(|root| {
                !matches!(
                    root.member,
                    crate::core::meta::QuoteMember::Fragment
                        | crate::core::meta::QuoteMember::CheckedBody
                )
            })
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
            return Err(
                QuoteConstructionFault::ExpressionFragmentNeedsCheckedNameTransplantation.into(),
            );
        }
        let mut param_uses = Vec::new();
        let mut bound_sites = Vec::new();
        let is_checked_body = node
            .template()
            .is_some_and(|key| key.source == FrozenTemplateSource::CheckedBody);
        let generic_uses = node
            .generic_uses
            .iter()
            .map(|use_| {
                if !is_checked_body || use_.member != crate::core::meta::QuoteMember::CheckedBody {
                    return Err(QuoteConstructionFault::FragmentGenericUseNeedsACheckedBody.into());
                }
                let name = parent
                    .generic_bindings
                    .get(use_.index as usize)
                    .ok_or(QuoteConstructionFault::ReadBackGenericHasNoDestinationBinding)?;
                Ok((use_.site, *name))
            })
            .collect::<Result<Vec<_>, QuoteConstructionError>>()?;
        for use_ in &node.open_uses {
            let open_name = source
                .open_names
                .get(use_.index as usize)
                .ok_or(QuoteConstructionFault::OpenNameHasNoDeclaration)?;
            // Checked bodies retain formal parameter identity. Authored open names
            // instead capture the nearest binder visible at this splice.
            let local = (!is_checked_body)
                .then(|| {
                    parent
                        .visible_locals
                        .iter()
                        .rev()
                        .find(|(name, _, _)| name == open_name)
                })
                .flatten();
            let (name, binding) = if let Some((_, name, pat)) = local {
                (*name, QuoteDestinationBinding::Local(*pat))
            } else {
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
                            QuoteConstructionError::from(
                                QuoteConstructionFault::ReadBackParameterHasNoDestinationBinding,
                            )
                        } else {
                            QuoteConstructionError::unbound_open_name(
                                open_name.data(self.db()).to_string(),
                            )
                        }
                    })?;
                (*name, QuoteDestinationBinding::Param(*index))
            };
            param_uses.push((use_.site, name));
            bound_sites.push((use_.site, binding));
        }
        if !node.free_self_uses.is_empty() && !parent.allow_self {
            return Err(QuoteConstructionFault::SelfNeedsADestinationMethod.into());
        }
        for site in &node.free_self_uses {
            param_uses.push((*site, IdentId::make_self(self.db())));
            bound_sites.push((*site, QuoteDestinationBinding::Param(0)));
        }
        parent.budget.charge_expression_paths(node, None)?;
        let types = super::generated::reify_quote_fragment_types(
            super::generated::QuotePathContext::new(
                self.db(),
                self.body.f_ctxt.top_mod().ingot(self.db()),
            ),
            &parent.budget,
            node,
            source,
            parent.owner_generic_bindings,
            parent.generic_bindings,
        )?;
        let mut rooted_paths = types.rooted_paths;
        rooted_paths.extend(
            source
                .qualified_paths
                .iter()
                .map(|(site, path)| (*site, *path)),
        );
        parent.budget.charge_auxiliary(node.metadata_nodes())?;
        parent
            .budget
            .charge_auxiliary(parent.visible_locals.len())?;
        let locals = node
            .locals
            .iter()
            .map(|local| {
                Ok((
                    local.binder,
                    authored_binder_name(self.db(), source.template, local.binder)?,
                    local.uses.clone(),
                ))
            })
            .collect::<Result<Vec<_>, QuoteConstructionFault>>()?;
        let mut map = QuoteCopyMap::new(
            self.db(),
            &node.holes,
            &rooted_paths,
            &locals,
            &param_uses,
            &generic_uses,
            &source.member_holes,
        )?;
        map.provider_scope = Some(source.provider_scope);
        map.fragments = Some((code, sources));
        map.fragment_depth = parent.fragment_depth
            + usize::from(matches!(source.root, QuoteFragmentRoot::Expr { .. }));
        map.budget = parent.budget.clone();
        map.open_bindings = parent.open_bindings;
        map.visible_locals = parent.visible_locals.clone();
        map.owner_generic_bindings = parent.owner_generic_bindings;
        map.generic_bindings = parent.generic_bindings;
        map.allow_self = parent.allow_self;
        map.groups = parent.groups.clone();
        map.group_locals = parent.group_locals.clone();
        map.group_names = parent.group_names.clone();
        map.cast_types = types.cast_types.into_iter().collect();
        map.local_types = types.local_types.into_iter().collect();
        map.record_paths = types.record_paths.into_iter().collect();
        for (site, path, _) in &types.variants {
            map.add_constructor_path(
                *site,
                *path,
                QuoteConstructionFault::FragmentVariantIsClassifiedTwice,
            )?;
        }

        let copied = copy(self, source, &mut map)?;
        for (site, index) in bound_sites {
            let copied_site =
                map.copied_expr(site, QuoteConstructionFault::OpenNameWasNotCopied)?;
            parent.fragment_bindings.push((copied_site, index));
        }
        for (site, name) in generic_uses {
            let copied_site =
                map.copied_expr(site, QuoteConstructionFault::ReadBackGenericWasNotCopied)?;
            parent.fragment_generics.push((copied_site, name));
        }
        for (site, _) in &source.qualified_paths {
            let copied_site = map.copied_expr(
                *site,
                QuoteConstructionFault::ReadBackQualifiedConstantWasNotCopied,
            )?;
            parent
                .fragment_path_scopes
                .push((copied_site, source.provider_scope));
        }
        let mut selected_methods = HashMap::new();
        for (old, selected) in &source.selected_methods {
            let site = map.copied_expr(
                *old,
                QuoteConstructionFault::ReadBackSelectedMethodWasNotCopied,
            )?;
            classify(
                &mut selected_methods,
                site,
                *selected,
                QuoteConstructionFault::ReadBackMethodIsClassifiedTwice,
            )?;
        }
        if is_checked_body && selected_methods.len() != map.method_calls.len() {
            return Err(QuoteConstructionFault::ReadBackMethodHasNoCheckedSelection.into());
        }
        for (site, _) in &source.rooted_funcs {
            let copied_site =
                map.copied_expr(*site, QuoteConstructionFault::RootedFunctionWasNotCopied)?;
            parent
                .fragment_path_scopes
                .push((copied_site, source.provider_scope));
        }
        parent.fragment_bindings.extend(map.fragment_bindings);
        parent.fragment_generics.extend(map.fragment_generics);
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
        parent.fragment_path_scopes.extend(map.fragment_path_scopes);
        Ok(copied)
    }

    fn copy_quote_cond(
        &mut self,
        template: Body<'db>,
        old: CondId,
        map: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<CondId, QuoteConstructionError> {
        if let Some(id) = map.conds.get(&old) {
            return Ok(*id);
        }
        map.charge_node()?;
        let Partial::Present(cond) = &template.conds(self.db())[old] else {
            return Err(QuoteConstructionFault::FunctionContainsAnInvalidCondition.into());
        };
        let cond = match cond {
            Cond::Expr(value) => Cond::Expr(self.copy_quote_expr(template, *value, map)?),
            Cond::Bin(lhs, rhs, op) => Cond::Bin(
                self.copy_quote_cond(template, *lhs, map)?,
                self.copy_quote_cond(template, *rhs, map)?,
                *op,
            ),
            Cond::Let(..) => {
                return Err(QuoteConstructionFault::LetConditionNeedsPatternTransplantation.into());
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
    ) -> Result<StmtId, QuoteConstructionError> {
        if let Some(id) = map.stmts.get(&old) {
            return Ok(*id);
        }
        map.charge_node()?;
        let db = self.db();
        let Partial::Present(stmt) = &template.stmts(db)[old] else {
            return Err(QuoteConstructionFault::FunctionContainsAnInvalidStatement.into());
        };
        let stmt = match stmt {
            Stmt::Let(pat, ty, init) => {
                let ty = ty
                    .map(|_| {
                        map.local_types
                            .get(&old)
                            .copied()
                            .ok_or(QuoteConstructionFault::LetTypeHasNoCheckedRoot)
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
                let visible = map.visible_locals.len();
                let pat = self.copy_quote_pat(template, *pat, map)?;
                let body = self.copy_quote_expr(template, *body, map)?;
                map.visible_locals.truncate(visible);
                Stmt::For(pat, iterable, body, *unroll_hint)
            }
            Stmt::Break => Stmt::Break,
            Stmt::Continue => Stmt::Continue,
        };
        let origin = self.quote_origin(
            template,
            template.source_map(db).stmt_map.node_to_source(old),
        );
        let id = self.body.push_stmt(stmt, origin);
        map.stmts.insert(old, id);
        Ok(id)
    }

    fn copy_quote_pat(
        &mut self,
        template: Body<'db>,
        old: PatId,
        map: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<PatId, QuoteConstructionError> {
        self.copy_quote_pat_inner(template, old, map)
            .map_err(|error| {
                let source = match &template.pats(self.db())[old] {
                    Partial::Present(Pat::Hole(slot)) => map.hole_source(*slot),
                    _ => None,
                };
                error.at(source)
            })
    }

    fn copy_quote_pat_inner(
        &mut self,
        template: Body<'db>,
        old: PatId,
        map: &mut QuoteCopyMap<'_, 'db>,
    ) -> Result<PatId, QuoteConstructionError> {
        if let Some(id) = map.pats.get(&old) {
            return Ok(*id);
        }
        map.charge_node()?;
        let db = self.db();
        let Partial::Present(pat) = &template.pats(db)[old] else {
            return Err(QuoteConstructionFault::FunctionContainsAnInvalidPattern.into());
        };
        if let Pat::Hole(slot) = pat {
            let FrozenCodeHole::Code(child, ..) = map
                .holes
                .get(*slot as usize)
                .ok_or(QuoteConstructionFault::PatternSpliceSlotIsMissing)?
            else {
                return Err(QuoteConstructionFault::PatternSpliceRequiresCodePat.into());
            };
            let source = &map
                .fragments
                .and_then(|(code, _)| code.nodes.get(*child))
                .ok_or(QuoteConstructionFault::PatternSpliceNeedsACompilerIssuedBinder)?
                .source;
            let binding = match source {
                FrozenCodeSource::Pattern(binding) => binding.clone(),
                FrozenCodeSource::Field(..) => {
                    return Err(
                        QuoteConstructionFault::FieldReferenceCodeCannotStandForAPattern.into(),
                    );
                }
                _ => {
                    return Err(
                        QuoteConstructionFault::PatternSpliceNeedsACompilerIssuedBinder.into(),
                    );
                }
            };
            if map.groups.contains_key(&binding) {
                return Err(
                    QuoteConstructionFault::BinderGroupAlreadyBoundInAnEnclosingPattern.into(),
                );
            }
            map.budget.charge_auxiliary(
                crate::core::meta::authored_path_nodes(&binding.owner.owner.path) + 1,
            )?;
            let (path, identity) = super::generated::reify_quote_nominal_constructor(
                super::generated::QuotePathContext::new(db, self.body.f_ctxt.top_mod().ingot(db)),
                &binding.owner,
                Some(binding.variant_index),
            )?;
            let AdtRef::Enum(owner) = identity.owner else {
                return Err(QuoteConstructionFault::VariantBinderNeedsAnEnumOwner.into());
            };
            let variant =
                crate::hir_def::EnumVariant::new(owner, usize::from(binding.variant_index));
            let count = match variant.kind(db) {
                crate::hir_def::VariantKind::Unit => 0,
                crate::hir_def::VariantKind::Tuple(types) => types.data(db).len(),
                crate::hir_def::VariantKind::Record(fields) => fields.data(db).len(),
            };
            map.budget.charge_auxiliary(2 * count)?;
            let labels = match variant.kind(db) {
                crate::hir_def::VariantKind::Unit => Vec::new(),
                crate::hir_def::VariantKind::Tuple(types) => vec![None; types.data(db).len()],
                crate::hir_def::VariantKind::Record(fields) => fields
                    .data(db)
                    .iter()
                    .map(|field| field.name.to_opt())
                    .collect(),
            };
            let mut fields = Vec::new();
            for _ in &labels {
                map.charge_node()?;
                let fresh = map.group_names.get();
                map.group_names.set(fresh + 1);
                let name = IdentId::new(db, format!("<quote-group-local-{fresh}>"));
                let pat = self.push_pat(Pat::Path(
                    Partial::Present(PathId::from_ident(db, name)),
                    false,
                ));
                map.group_locals.borrow_mut().insert(pat, Vec::new());
                fields.push((name, pat));
            }
            let pattern = match variant.kind(db) {
                crate::hir_def::VariantKind::Unit => Pat::Path(Partial::Present(path), false),
                crate::hir_def::VariantKind::Tuple(_) => Pat::PathTuple(
                    Partial::Present(path),
                    fields.iter().map(|(_, pat)| *pat).collect(),
                ),
                crate::hir_def::VariantKind::Record(_) => Pat::Record(
                    Partial::Present(path),
                    labels
                        .into_iter()
                        .zip(&fields)
                        .map(|(label, (_, pat))| crate::hir_def::RecordPatField {
                            label: Partial::from(label),
                            pat: *pat,
                        })
                        .collect(),
                ),
            };
            let origin = self.quote_origin(
                template,
                template.source_map(db).pat_map.node_to_source(old),
            );
            let id = self.body.push_pat(pattern, origin);
            map.prepare_groups_mutation()?;
            Rc::make_mut(&mut map.groups).insert(binding, fields);
            map.pats.insert(old, id);
            return Ok(id);
        }
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
                    .ok_or(QuoteConstructionFault::TuplePatternHasNoCheckedNominalConstructor)?;
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
                    .ok_or(QuoteConstructionFault::RecordPatternHasNoCheckedNominalConstructor)?;
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
                    return Err(QuoteConstructionFault::PatternNeedsACheckedBinder.into());
                }
                let name = map
                    .local_binders
                    .get(&old)
                    .ok_or(QuoteConstructionFault::PatternIsNotACheckedBinder)?;
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
            _ => {
                return Err(
                    QuoteConstructionFault::PatternNeedsClassificationBeforeTransplantation.into(),
                );
            }
        };
        let retains_scope = matches!(pat, Pat::Record(..))
            || map
                .constructor_paths
                .contains_key(&FrozenQuoteConstructorSite::Pat(old));
        let origin = self.quote_origin(
            template,
            template.source_map(db).pat_map.node_to_source(old),
        );
        let id = self.body.push_pat(pat, origin);
        if retains_scope && let Some(scope) = map.provider_scope {
            self.body.quote_scopes.push((
                id.into(),
                crate::hir_def::QuoteScope {
                    scope,
                    source: Some((template, old.into())),
                    fields: Vec::new(),
                },
            ));
        }
        if let Some(actual_name) = map.local_binders.get(&old)
            && let Partial::Present(Pat::Path(Partial::Present(path), _)) =
                &template.pats(self.db())[old]
            && let Some(source_name) = path.as_ident(self.db())
        {
            map.visible_locals.push((source_name, *actual_name, id));
        }
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

    fn quote_origin<T>(&self, template: Body<'db>, source: &HirOrigin<T>) -> HirOrigin<T>
    where
        T: AstNode<Language = parser::FeLang>,
    {
        let mut origin: DesugaredOrigin = self.desugared.clone().into();
        if let DesugaredOrigin::Generation(generation) = &mut origin {
            generation.source = match source {
                HirOrigin::Raw(pointer) => Some((
                    template.top_mod(self.db()).file(self.db()),
                    pointer.syntax_node_ptr(),
                )),
                HirOrigin::Desugared(DesugaredOrigin::Generation(source)) => source.source,
                _ => None,
            };
        }
        HirOrigin::desugared(origin)
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
                label: Some(crate::hir_def::FieldLabel::Ident(field_name)),
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
