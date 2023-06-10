use std::{marker::PhantomData, mem};

use crate::{
    hir_def::{
        attr, Body, CallArg, Const, Contract, Enum, Expr, ExprId, Field, FieldDef, FieldDefListId,
        FieldIndex, Func, FuncParam, FuncParamLabel, FuncParamListId, FuncParamName, GenericArg,
        GenericArgListId, GenericParam, GenericParamListId, IdentId, Impl, ImplTrait, ItemKind,
        LitKind, MatchArm, Mod, Partial, Pat, PatId, PathId, Stmt, StmtId, Struct, TopLevelMod,
        Trait, TypeAlias, TypeBound, TypeId, TypeKind, Use, UseAlias, UsePathId, UsePathSegment,
        VariantDef, VariantDefListId, WhereClauseId, WherePredicate,
    },
    span::{
        attr::{LazyAttrListSpan, LazyAttrSpan},
        expr::{
            LazyCallArgListSpan, LazyCallArgSpan, LazyExprSpan, LazyFieldListSpan, LazyFieldSpan,
            LazyMatchArmSpan,
        },
        item::{
            LazyBodySpan, LazyConstSpan, LazyContractSpan, LazyEnumSpan, LazyFieldDefListSpan,
            LazyFieldDefSpan, LazyFuncSpan, LazyImplSpan, LazyImplTraitSpan, LazyItemSpan,
            LazyModSpan, LazyStructSpan, LazyTopModSpan, LazyTraitSpan, LazyTypeAliasSpan,
            LazyUseSpan, LazyVariantDefListSpan, LazyVariantDefSpan,
        },
        params::{
            LazyFuncParamListSpan, LazyFuncParamSpan, LazyGenericArgListSpan, LazyGenericArgSpan,
            LazyGenericParamListSpan, LazyGenericParamSpan, LazyTypeBoundListSpan,
            LazyTypeBoundSpan, LazyWhereClauseSpan, LazyWherePredicateSpan,
        },
        pat::LazyPatSpan,
        path::LazyPathSpan,
        stmt::LazyStmtSpan,
        transition::ChainRoot,
        types::LazyTySpan,
        use_tree::LazyUsePathSpan,
        DynLazySpan, LazyLitSpan, LazySpan, LazySpanAtom, SpanDowncast,
    },
    HirDb,
};

/// A visitor for traversing the HIR.
pub trait Visitor {
    fn visit_item(&mut self, ctxt: &mut VisitorCtxt<'_, LazyItemSpan>, item: ItemKind) {
        walk_item(self, ctxt, item)
    }

    fn visit_top_mod(&mut self, ctxt: &mut VisitorCtxt<'_, LazyTopModSpan>, top_mod: TopLevelMod) {
        walk_top_mod(self, ctxt, top_mod)
    }

    fn visit_mod(&mut self, ctxt: &mut VisitorCtxt<'_, LazyModSpan>, module: Mod) {
        walk_mod(self, ctxt, module)
    }

    fn visit_func(&mut self, ctxt: &mut VisitorCtxt<'_, LazyFuncSpan>, func: Func) {
        walk_func(self, ctxt, func)
    }

    fn visit_struct(&mut self, ctxt: &mut VisitorCtxt<'_, LazyStructSpan>, struct_: Struct) {
        walk_struct(self, ctxt, struct_)
    }

    fn visit_contract(&mut self, ctxt: &mut VisitorCtxt<'_, LazyContractSpan>, contract: Contract) {
        walk_contract(self, ctxt, contract)
    }

    fn visit_enum(&mut self, ctxt: &mut VisitorCtxt<'_, LazyEnumSpan>, enum_: Enum) {
        walk_enum(self, ctxt, enum_)
    }

    fn visit_type_alias(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyTypeAliasSpan>,
        alias: TypeAlias,
    ) {
        walk_type_alias(self, ctxt, alias)
    }

    fn visit_impl(&mut self, ctxt: &mut VisitorCtxt<'_, LazyImplSpan>, impl_: Impl) {
        walk_impl(self, ctxt, impl_)
    }

    fn visit_trait(&mut self, ctxt: &mut VisitorCtxt<'_, LazyTraitSpan>, trait_: Trait) {
        walk_trait(self, ctxt, trait_)
    }

    fn visit_impl_trait(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyImplTraitSpan>,
        impl_trait: ImplTrait,
    ) {
        walk_impl_trait(self, ctxt, impl_trait)
    }

    fn visit_const(&mut self, ctxt: &mut VisitorCtxt<'_, LazyConstSpan>, constant: Const) {
        walk_const(self, ctxt, constant)
    }

    fn visit_use(&mut self, ctxt: &mut VisitorCtxt<'_, LazyUseSpan>, use_: Use) {
        walk_use(self, ctxt, use_)
    }

    fn visit_body(&mut self, ctxt: &mut VisitorCtxt<'_, LazyBodySpan>, body: Body) {
        walk_body(self, ctxt, body)
    }

    fn visit_attribute_list(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyAttrListSpan>,
        attrs: AttrListId,
    ) {
        walk_attributes(self, ctxt, attrs);
    }

    fn visit_attribute(&mut self, ctxt: &mut VisitorCtxt<'_, LazyAttrSpan>, attr: &Attr) {
        walk_attribute(self, ctxt, attr);
    }

    fn visit_generic_param_list(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyGenericParamListSpan>,
        params: GenericParamListId,
    ) {
        walk_generic_param_list(self, ctxt, params);
    }

    fn visit_generic_param(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyGenericParamSpan>,
        param: &GenericParam,
    ) {
        walk_generic_param(self, ctxt, param);
    }

    fn visit_generic_arg_list(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyGenericArgListSpan>,
        args: GenericArgListId,
    ) {
        walk_generic_arg_list(self, ctxt, args);
    }

    fn visit_generic_arg(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyGenericArgSpan>,
        arg: &GenericArg,
    ) {
        walk_generic_arg(self, ctxt, arg);
    }

    fn visit_call_arg_list(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyCallArgListSpan>,
        args: &[CallArg],
    ) {
        walk_call_arg_list(self, ctxt, args);
    }

    fn visit_call_arg(&mut self, ctxt: &mut VisitorCtxt<'_, LazyCallArgSpan>, arg: CallArg) {
        walk_call_arg(self, ctxt, arg);
    }

    fn visit_type_bound_list(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyTypeBoundListSpan>,
        bounds: &[TypeBound],
    ) {
        walk_type_bound_list(self, ctxt, bounds);
    }

    fn visit_type_bound(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyTypeBoundSpan>,
        bound: &TypeBound,
    ) {
        walk_type_bound(self, ctxt, bound);
    }

    fn visit_where_clause(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyWhereClauseSpan>,
        where_clause: WhereClauseId,
    ) {
        walk_where_clause(self, ctxt, where_clause);
    }

    fn visit_where_predicate(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyWherePredicateSpan>,
        where_predicate: &WherePredicate,
    ) {
        walk_where_predicate(self, ctxt, where_predicate);
    }

    fn visit_func_param_list(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyFuncParamListSpan>,
        params: FuncParamListId,
    ) {
        walk_func_param_list(self, ctxt, params);
    }

    fn visit_func_param(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyFuncParamSpan>,
        param: &FuncParam,
    ) {
        walk_func_param(self, ctxt, param);
    }

    fn visit_field_list(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyFieldListSpan>,
        fields: &[Field],
    ) {
        walk_field_list(self, ctxt, fields);
    }

    fn visit_field(&mut self, ctxt: &mut VisitorCtxt<'_, LazyFieldSpan>, field: Field) {
        walk_field(self, ctxt, field);
    }

    fn visit_field_def_list(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyFieldDefListSpan>,
        fields: FieldDefListId,
    ) {
        walk_field_def_list(self, ctxt, fields);
    }

    fn visit_field_def(&mut self, ctxt: &mut VisitorCtxt<'_, LazyFieldDefSpan>, field: &FieldDef) {
        walk_field_def(self, ctxt, field);
    }

    fn visit_variant_def_list(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyVariantDefListSpan>,
        variants: VariantDefListId,
    ) {
        walk_variant_def_list(self, ctxt, variants);
    }

    fn visit_variant_def(
        &mut self,
        ctxt: &mut VisitorCtxt<'_, LazyVariantDefSpan>,
        variant: &VariantDef,
    ) {
        walk_variant_def(self, ctxt, variant)
    }

    fn visit_stmt(&mut self, ctxt: &mut VisitorCtxt<'_, LazyStmtSpan>, stmt: &Stmt) {
        walk_stmt(self, ctxt, stmt)
    }

    fn visit_expr(&mut self, ctxt: &mut VisitorCtxt<'_, LazyExprSpan>, expr: &Expr) {
        walk_expr(self, ctxt, expr)
    }

    fn visit_arm(&mut self, ctxt: &mut VisitorCtxt<'_, LazyMatchArmSpan>, arm: &MatchArm) {
        walk_arm(self, ctxt, arm)
    }

    fn visit_pat(&mut self, ctxt: &mut VisitorCtxt<'_, LazyPatSpan>, pat: &Pat) {
        walk_pat(self, ctxt, pat)
    }

    fn visit_path(&mut self, ctxt: &mut VisitorCtxt<'_, LazyPathSpan>, path: PathId) {
        walk_path(self, ctxt, path)
    }

    fn visit_use_path(&mut self, ctxt: &mut VisitorCtxt<'_, LazyUsePathSpan>, use_path: UsePathId) {
        walk_use_path(self, ctxt, use_path)
    }

    fn visit_ty(&mut self, ctxt: &mut VisitorCtxt<'_, LazyTySpan>, ty: TypeId) {
        walk_ty(self, ctxt, ty)
    }

    #[allow(unused_variables)]
    fn visit_lit(&mut self, ctxt: &mut VisitorCtxt<'_, LazyLitSpan>, lit: LitKind) {}

    #[allow(unused_variables)]
    fn visit_ident(&mut self, ctxt: &mut VisitorCtxt<'_, LazySpanAtom>, ident: IdentId) {}
}

pub fn walk_item<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyItemSpan>, item: ItemKind)
where
    V: Visitor + ?Sized,
{
    match item {
        ItemKind::TopMod(top_mod) => {
            let mut new_ctxt = VisitorCtxt::with_top_mod(ctxt.db, top_mod);
            visitor.visit_top_mod(&mut new_ctxt, top_mod);
        }
        ItemKind::Mod(mod_) => {
            let mut new_ctxt = VisitorCtxt::with_mod(ctxt.db, mod_);
            visitor.visit_mod(&mut new_ctxt, mod_)
        }
        ItemKind::Func(func) => {
            let mut new_ctxt = VisitorCtxt::with_func(ctxt.db, func);
            visitor.visit_func(&mut new_ctxt, func)
        }
        ItemKind::Struct(struct_) => {
            let mut new_ctxt = VisitorCtxt::with_struct(ctxt.db, struct_);
            visitor.visit_struct(&mut new_ctxt, struct_)
        }
        ItemKind::Contract(contract) => {
            let mut new_ctxt = VisitorCtxt::with_contract(ctxt.db, contract);
            visitor.visit_contract(&mut new_ctxt, contract)
        }
        ItemKind::Enum(enum_) => {
            let mut new_ctxt = VisitorCtxt::with_enum(ctxt.db, enum_);
            visitor.visit_enum(&mut new_ctxt, enum_)
        }
        ItemKind::TypeAlias(alias) => {
            let mut new_ctxt = VisitorCtxt::with_type_alias(ctxt.db, alias);
            visitor.visit_type_alias(&mut new_ctxt, alias)
        }
        ItemKind::Impl(impl_) => {
            let mut new_ctxt = VisitorCtxt::with_impl(ctxt.db, impl_);
            visitor.visit_impl(&mut new_ctxt, impl_)
        }
        ItemKind::Trait(trait_) => {
            let mut new_ctxt = VisitorCtxt::with_trait(ctxt.db, trait_);
            visitor.visit_trait(&mut new_ctxt, trait_)
        }
        ItemKind::ImplTrait(impl_trait) => {
            let mut new_ctxt = VisitorCtxt::with_impl_trait(ctxt.db, impl_trait);
            visitor.visit_impl_trait(&mut new_ctxt, impl_trait)
        }
        ItemKind::Const(const_) => {
            let mut new_ctxt = VisitorCtxt::with_const(ctxt.db, const_);
            visitor.visit_const(&mut new_ctxt, const_)
        }
        ItemKind::Use(use_) => {
            let mut new_ctxt = VisitorCtxt::with_use(ctxt.db, use_);
            visitor.visit_use(&mut new_ctxt, use_)
        }
        ItemKind::Body(body) => {
            let mut new_ctxt = VisitorCtxt::with_body(ctxt.db, body);
            visitor.visit_body(&mut new_ctxt, body)
        }
    };
}

pub fn walk_top_mod<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyTopModSpan>,
    top_mod: TopLevelMod,
) where
    V: Visitor + ?Sized,
{
    for child in top_mod.children_non_nested(ctxt.db) {
        visitor.visit_item(&mut VisitorCtxt::with_item(ctxt.db, child), child);
    }
}

pub fn walk_mod<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyModSpan>, mod_: Mod)
where
    V: Visitor + ?Sized,
{
    if let Some(name) = mod_.name(ctxt.db).to_opt() {
        ctxt.with_new_ctxt(
            |span| span.name_moved(),
            |ctxt| {
                visitor.visit_ident(ctxt, name);
            },
        )
    };

    ctxt.with_new_ctxt(
        |span| span.attributes_moved(),
        |ctxt| {
            let id = mod_.attributes(ctxt.db);
            visitor.visit_attribute_list(ctxt, id);
        },
    );

    for child in mod_.children_non_nested(ctxt.db) {
        visitor.visit_item(&mut VisitorCtxt::with_item(ctxt.db, child), child);
    }
}

pub fn walk_func<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyFuncSpan>, func: Func)
where
    V: Visitor + ?Sized,
{
    if let Some(name) = func.name(ctxt.db).to_opt() {
        ctxt.with_new_ctxt(
            |span| span.name_moved(),
            |ctxt| {
                visitor.visit_ident(ctxt, name);
            },
        )
    };

    ctxt.with_new_ctxt(
        |span| span.attributes_moved(),
        |ctxt| {
            let id = func.attributes(ctxt.db);
            visitor.visit_attribute_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.generic_params_moved(),
        |ctxt| {
            let id = func.generic_params(ctxt.db);
            visitor.visit_generic_param_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.where_clause_moved(),
        |ctxt| {
            let id = func.where_clause(ctxt.db);
            visitor.visit_where_clause(ctxt, id);
        },
    );

    if let Some(id) = func.params(ctxt.db).to_opt() {
        ctxt.with_new_ctxt(
            |span| span.params_moved(),
            |ctxt| {
                visitor.visit_func_param_list(ctxt, id);
            },
        )
    }

    if let Some(ty) = func.ret_ty(ctxt.db) {
        ctxt.with_new_ctxt(
            |span| span.ret_ty_moved(),
            |ctxt| {
                visitor.visit_ty(ctxt, ty);
            },
        )
    }

    if let Some(body) = func.body(ctxt.db) {
        visitor.visit_body(&mut VisitorCtxt::with_body(ctxt.db, body), body);
    }
}

pub fn walk_struct<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyStructSpan>, struct_: Struct)
where
    V: Visitor + ?Sized,
{
    if let Some(id) = struct_.name(ctxt.db).to_opt() {
        ctxt.with_new_ctxt(
            |span| span.name_moved(),
            |ctxt| {
                visitor.visit_ident(ctxt, id);
            },
        )
    }

    ctxt.with_new_ctxt(
        |span| span.attributes_moved(),
        |ctxt| {
            let id = struct_.attributes(ctxt.db);
            visitor.visit_attribute_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.generic_params_moved(),
        |ctxt| {
            let id = struct_.generic_params(ctxt.db);
            visitor.visit_generic_param_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.where_clause_moved(),
        |ctxt| {
            let id = struct_.where_clause(ctxt.db);
            visitor.visit_where_clause(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.fields_moved(),
        |ctxt| {
            let id = struct_.fields(ctxt.db);
            visitor.visit_field_def_list(ctxt, id);
        },
    );
}

pub fn walk_contract<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyContractSpan>,
    contract: Contract,
) where
    V: Visitor + ?Sized,
{
    if let Some(id) = contract.name(ctxt.db).to_opt() {
        ctxt.with_new_ctxt(
            |span| span.name_moved(),
            |ctxt| {
                visitor.visit_ident(ctxt, id);
            },
        )
    }

    ctxt.with_new_ctxt(
        |span| span.attributes_moved(),
        |ctxt| {
            let id = contract.attributes(ctxt.db);
            visitor.visit_attribute_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.fields_moved(),
        |ctxt| {
            let id = contract.fields(ctxt.db);
            visitor.visit_field_def_list(ctxt, id);
        },
    );
}

pub fn walk_enum<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyEnumSpan>, enum_: Enum)
where
    V: Visitor + ?Sized,
{
    if let Some(id) = enum_.name(ctxt.db).to_opt() {
        ctxt.with_new_ctxt(
            |span| span.name_moved(),
            |ctxt| {
                visitor.visit_ident(ctxt, id);
            },
        )
    }

    ctxt.with_new_ctxt(
        |span| span.attributes_moved(),
        |ctxt| {
            let id = enum_.attributes(ctxt.db);
            visitor.visit_attribute_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.generic_params_moved(),
        |ctxt| {
            let id = enum_.generic_params(ctxt.db);
            visitor.visit_generic_param_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.where_clause_moved(),
        |ctxt| {
            let id = enum_.where_clause(ctxt.db);
            visitor.visit_where_clause(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.variants_moved(),
        |ctxt| {
            let id = enum_.variants(ctxt.db);
            visitor.visit_variant_def_list(ctxt, id);
        },
    );
}

pub fn walk_type_alias<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyTypeAliasSpan>,
    alias: TypeAlias,
) where
    V: Visitor + ?Sized,
{
    if let Some(id) = alias.name(ctxt.db).to_opt() {
        ctxt.with_new_ctxt(
            |span| span.alias_moved(),
            |ctxt| {
                visitor.visit_ident(ctxt, id);
            },
        )
    }

    ctxt.with_new_ctxt(
        |span| span.attributes_moved(),
        |ctxt| {
            let id = alias.attributes(ctxt.db);
            visitor.visit_attribute_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.generic_params_moved(),
        |ctxt| {
            let id = alias.generic_params(ctxt.db);
            visitor.visit_generic_param_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.where_clause_moved(),
        |ctxt| {
            let id = alias.where_clause(ctxt.db);
            visitor.visit_where_clause(ctxt, id);
        },
    );

    if let Some(ty) = alias.ty(ctxt.db).to_opt() {
        ctxt.with_new_ctxt(
            |span| span.ty_moved(),
            |ctxt| {
                visitor.visit_ty(ctxt, ty);
            },
        )
    }
}

pub fn walk_impl<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyImplSpan>, impl_: Impl)
where
    V: Visitor + ?Sized,
{
    if let Some(ty) = impl_.ty(ctxt.db).to_opt() {
        ctxt.with_new_ctxt(
            |span| span.target_ty_moved(),
            |ctxt| {
                visitor.visit_ty(ctxt, ty);
            },
        )
    }

    ctxt.with_new_ctxt(
        |span| span.attributes_moved(),
        |ctxt| {
            let id = impl_.attributes(ctxt.db);
            visitor.visit_attribute_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.generic_params_moved(),
        |ctxt| {
            let id = impl_.generic_params(ctxt.db);
            visitor.visit_generic_param_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.where_clause_moved(),
        |ctxt| {
            let id = impl_.where_clause(ctxt.db);
            visitor.visit_where_clause(ctxt, id);
        },
    );

    for item in impl_.children_non_nested(ctxt.db) {
        visitor.visit_item(&mut VisitorCtxt::with_item(ctxt.db, item), item);
    }
}

pub fn walk_trait<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyTraitSpan>, trait_: Trait)
where
    V: Visitor + ?Sized,
{
    if let Some(name) = trait_.name(ctxt.db).to_opt() {
        ctxt.with_new_ctxt(
            |span| span.name_moved(),
            |ctxt| {
                visitor.visit_ident(ctxt, name);
            },
        )
    }

    ctxt.with_new_ctxt(
        |span| span.attributes_moved(),
        |ctxt| {
            let id = trait_.attributes(ctxt.db);
            visitor.visit_attribute_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.generic_params_moved(),
        |ctxt| {
            let id = trait_.generic_params(ctxt.db);
            visitor.visit_generic_param_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.where_clause_moved(),
        |ctxt| {
            let id = trait_.where_clause(ctxt.db);
            visitor.visit_where_clause(ctxt, id);
        },
    );

    for item in trait_.children_non_nested(ctxt.db) {
        visitor.visit_item(&mut VisitorCtxt::with_item(ctxt.db, item), item);
    }
}

pub fn walk_impl_trait<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyImplTraitSpan>,
    impl_trait: ImplTrait,
) where
    V: Visitor + ?Sized,
{
    if let Some(trait_ref) = impl_trait.trait_ref(ctxt.db).to_opt() {
        ctxt.with_new_ctxt(
            |span| span.trait_ref_moved(),
            |ctxt| {
                if let Some(path) = trait_ref.path.to_opt() {
                    ctxt.with_new_ctxt(
                        |span| span.path_moved(),
                        |ctxt| {
                            visitor.visit_path(ctxt, path);
                        },
                    )
                };

                ctxt.with_new_ctxt(
                    |span| span.generic_args_moved(),
                    |ctxt| {
                        visitor.visit_generic_arg_list(ctxt, trait_ref.generic_args);
                    },
                );
            },
        )
    }

    if let Some(ty) = impl_trait.ty(ctxt.db).to_opt() {
        ctxt.with_new_ctxt(
            |span| span.ty_moved(),
            |ctxt| {
                visitor.visit_ty(ctxt, ty);
            },
        )
    }

    ctxt.with_new_ctxt(
        |span| span.attributes_moved(),
        |ctxt| {
            let id = impl_trait.attributes(ctxt.db);
            visitor.visit_attribute_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.generic_params_moved(),
        |ctxt| {
            let id = impl_trait.generic_params(ctxt.db);
            visitor.visit_generic_param_list(ctxt, id);
        },
    );

    ctxt.with_new_ctxt(
        |span| span.where_clause_moved(),
        |ctxt| {
            let id = impl_trait.where_clause(ctxt.db);
            visitor.visit_where_clause(ctxt, id);
        },
    );
}

pub fn walk_const<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyConstSpan>, const_: Const)
where
    V: Visitor + ?Sized,
{
    if let Some(name) = const_.name(ctxt.db).to_opt() {
        ctxt.with_new_ctxt(
            |span| span.name_moved(),
            |ctxt| {
                visitor.visit_ident(ctxt, name);
            },
        )
    }

    if let Some(body) = const_.body(ctxt.db).to_opt() {
        visitor.visit_body(&mut VisitorCtxt::with_body(ctxt.db, body), body);
    }
}

pub fn walk_use<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyUseSpan>, use_: Use)
where
    V: Visitor + ?Sized,
{
    if let Some(use_path) = use_.path(ctxt.db).to_opt() {
        ctxt.with_new_ctxt(
            |span| span.path_moved(),
            |ctxt| {
                visitor.visit_use_path(ctxt, use_path);
            },
        )
    }

    if let Some(Partial::Present(UseAlias::Ident(ident))) = use_.alias(ctxt.db) {
        ctxt.with_new_ctxt(
            |span| span.alias_moved().name_moved(),
            |ctxt| {
                visitor.visit_ident(ctxt, ident);
            },
        )
    }
}

pub fn walk_body<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyBodySpan>, body: Body)
where
    V: Visitor + ?Sized,
{
    for stmt_id in body.stmts(ctxt.db).keys() {
        visit_node_in_body!(visitor, ctxt, &stmt_id, stmt);
    }
}

pub fn walk_stmt<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyStmtSpan>, stmt: &Stmt)
where
    V: Visitor + ?Sized,
{
    match stmt {
        Stmt::Let(pat_id, ty, expr_id) => {
            visit_node_in_body!(visitor, ctxt, pat_id, pat);

            if let Some(ty) = ty {
                ctxt.with_new_ctxt(
                    |span| span.into_let_stmt().ty_moved(),
                    |ctxt| {
                        visitor.visit_ty(ctxt, *ty);
                    },
                )
            };

            if let Some(expr_id) = expr_id {
                visit_node_in_body!(visitor, ctxt, expr_id, expr);
            }
        }

        Stmt::Assign(pat_id, expr_id) => {
            visit_node_in_body!(visitor, ctxt, pat_id, pat);
            visit_node_in_body!(visitor, ctxt, expr_id, expr);
        }

        Stmt::For(pat_id, cond_id, for_body_id) => {
            visit_node_in_body!(visitor, ctxt, pat_id, pat);
            visit_node_in_body!(visitor, ctxt, cond_id, expr);
            visit_node_in_body!(visitor, ctxt, for_body_id, expr);
        }

        Stmt::While(cond_id, while_body_id) => {
            visit_node_in_body!(visitor, ctxt, cond_id, expr);
            visit_node_in_body!(visitor, ctxt, while_body_id, expr);
        }

        Stmt::Return(Some(expr_id)) | Stmt::Expr(expr_id) => {
            visit_node_in_body!(visitor, ctxt, expr_id, expr);
        }

        Stmt::Return(None) | Stmt::Continue | Stmt::Break => {}
    }
}

pub fn walk_expr<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyExprSpan>, expr: &Expr)
where
    V: Visitor + ?Sized,
{
    match expr {
        Expr::Lit(lit) => ctxt.with_new_ctxt(
            |span| span.into_lit_expr().lit_moved(),
            |ctxt| {
                visitor.visit_lit(ctxt, *lit);
            },
        ),

        Expr::Block(stmts) => {
            for stmt_id in stmts {
                visit_node_in_body!(visitor, ctxt, stmt_id, stmt);
            }
        }

        Expr::Bin(lhs_id, rhs_id, _) => {
            visit_node_in_body!(visitor, ctxt, lhs_id, expr);
            visit_node_in_body!(visitor, ctxt, rhs_id, expr);
        }

        Expr::Un(expr_id, _) => {
            visit_node_in_body!(visitor, ctxt, expr_id, expr);
        }

        Expr::Call(callee_id, generic_args, call_args) => {
            visit_node_in_body!(visitor, ctxt, callee_id, expr);
            ctxt.with_new_ctxt(
                |span| span.into_call_expr(),
                |ctxt| {
                    ctxt.with_new_ctxt(
                        |span| span.generic_args_moved(),
                        |ctxt| visitor.visit_generic_arg_list(ctxt, *generic_args),
                    );

                    ctxt.with_new_ctxt(
                        |span| span.args_moved(),
                        |ctxt| {
                            visitor.visit_call_arg_list(ctxt, call_args);
                        },
                    );
                },
            );
        }

        Expr::MethodCall(receiver_id, method_name, generic_args, call_args) => {
            visit_node_in_body!(visitor, ctxt, receiver_id, expr);

            ctxt.with_new_ctxt(
                |span| span.into_method_call_expr(),
                |ctxt| {
                    if let Some(method_name) = method_name.to_opt() {
                        ctxt.with_new_ctxt(
                            |span| span.method_name_moved(),
                            |ctxt| visitor.visit_ident(ctxt, method_name),
                        );
                    }

                    ctxt.with_new_ctxt(
                        |span| span.generic_args_moved(),
                        |ctxt| visitor.visit_generic_arg_list(ctxt, *generic_args),
                    );

                    ctxt.with_new_ctxt(
                        |span| span.args_moved(),
                        |ctxt| {
                            visitor.visit_call_arg_list(ctxt, call_args);
                        },
                    );
                },
            );
        }

        Expr::Path(path) => {
            if let Some(path) = path.to_opt() {
                ctxt.with_new_ctxt(
                    |span| span.into_path_expr().path_moved(),
                    |ctxt| {
                        visitor.visit_path(ctxt, path);
                    },
                );
            }
        }

        Expr::RecordInit(path, fields) => {
            ctxt.with_new_ctxt(
                |span| span.into_record_init_expr(),
                |ctxt| {
                    if let Some(path) = path.to_opt() {
                        ctxt.with_new_ctxt(
                            |span| span.path_moved(),
                            |ctxt| {
                                visitor.visit_path(ctxt, path);
                            },
                        );
                    }

                    ctxt.with_new_ctxt(
                        |span| span.fields_moved(),
                        |ctxt| {
                            visitor.visit_field_list(ctxt, fields);
                        },
                    );
                },
            );
        }

        Expr::Field(receiver_id, field_name) => {
            visit_node_in_body!(visitor, ctxt, receiver_id, expr);

            match field_name {
                Partial::Present(FieldIndex::Ident(ident)) => {
                    ctxt.with_new_ctxt(
                        |span| span.into_field_expr().accessor_moved(),
                        |ctxt| visitor.visit_ident(ctxt, *ident),
                    );
                }

                Partial::Present(FieldIndex::Index(index)) => {
                    ctxt.with_new_ctxt(
                        |span| span.into_field_expr().accessor_moved().into_lit_span(),
                        |ctxt| visitor.visit_lit(ctxt, (*index).into()),
                    );
                }

                Partial::Absent => {}
            }
        }

        Expr::Tuple(elems) => {
            for elem_id in elems {
                visit_node_in_body!(visitor, ctxt, elem_id, expr);
            }
        }

        Expr::Index(lhs_id, rhs_id) => {
            visit_node_in_body!(visitor, ctxt, lhs_id, expr);
            visit_node_in_body!(visitor, ctxt, rhs_id, expr);
        }

        Expr::Array(elems) => {
            for elem_id in elems {
                visit_node_in_body!(visitor, ctxt, elem_id, expr);
            }
        }

        Expr::ArrayRep(val, rep) => {
            visit_node_in_body!(visitor, ctxt, val, expr);
            if let Some(body) = rep.to_opt() {
                visitor.visit_body(&mut VisitorCtxt::with_body(ctxt.db, body), body);
            }
        }

        Expr::If(cond, then, else_) => {
            visit_node_in_body!(visitor, ctxt, cond, expr);
            visit_node_in_body!(visitor, ctxt, then, expr);
            if let Some(else_) = else_ {
                visit_node_in_body!(visitor, ctxt, else_, expr);
            }
        }

        Expr::Match(scrutinee, arms) => {
            visit_node_in_body!(visitor, ctxt, scrutinee, expr);

            if let Partial::Present(arms) = arms {
                ctxt.with_new_ctxt(
                    |span| span.into_match_expr().arms_moved(),
                    |ctxt| {
                        for (i, arm) in arms.iter().enumerate() {
                            ctxt.with_new_ctxt(
                                |span| span.arm_moved(i),
                                |ctxt| {
                                    visitor.visit_arm(ctxt, arm);
                                },
                            );
                        }
                    },
                );
            }
        }
    }
}

pub fn walk_arm<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyMatchArmSpan>, arm: &MatchArm)
where
    V: Visitor + ?Sized,
{
    visit_node_in_body!(visitor, ctxt, &arm.pat, pat);
    visit_node_in_body!(visitor, ctxt, &arm.body, expr);
}

pub fn walk_pat<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyPatSpan>, pat: &Pat)
where
    V: Visitor + ?Sized,
{
    match pat {
        Pat::Lit(lit) => {
            if let Some(lit) = lit.to_opt() {
                ctxt.with_new_ctxt(
                    |span| span.into_lit_pat().lit_moved(),
                    |ctxt| {
                        visitor.visit_lit(ctxt, lit);
                    },
                )
            };
        }

        Pat::Tuple(elems) => {
            for elem in elems {
                visit_node_in_body!(visitor, ctxt, elem, pat);
            }
        }

        Pat::Path(path) => {
            if let Some(path) = path.to_opt() {
                ctxt.with_new_ctxt(
                    |span| span.into_path_pat().path_moved(),
                    |ctxt| {
                        visitor.visit_path(ctxt, path);
                    },
                )
            };
        }

        Pat::PathTuple(path, elems) => {
            if let Some(path) = path.to_opt() {
                ctxt.with_new_ctxt(
                    |span| span.into_path_pat().path_moved(),
                    |ctxt| {
                        visitor.visit_path(ctxt, path);
                    },
                )
            };

            for elem in elems {
                visit_node_in_body!(visitor, ctxt, elem, pat);
            }
        }

        Pat::Record(path, fields) => ctxt.with_new_ctxt(
            |span| span.into_record_pat(),
            |ctxt| {
                if let Some(path) = path.to_opt() {
                    ctxt.with_new_ctxt(
                        |span| span.path_moved(),
                        |ctxt| {
                            visitor.visit_path(ctxt, path);
                        },
                    );
                }

                ctxt.with_new_ctxt(
                    |span| span.fields_moved(),
                    |ctxt| {
                        for (i, field) in fields.iter().enumerate() {
                            ctxt.with_new_ctxt(
                                |span| span.field_moved(i),
                                |ctxt| {
                                    if let Some(label) = field.label.to_opt() {
                                        ctxt.with_new_ctxt(
                                            |span| span.name_moved(),
                                            |ctxt| {
                                                visitor.visit_ident(ctxt, label);
                                            },
                                        );
                                    }

                                    visit_node_in_body!(visitor, ctxt, &field.pat, pat);
                                },
                            );
                        }
                    },
                );
            },
        ),

        Pat::Or(lhs, rhs) => {
            visit_node_in_body!(visitor, ctxt, lhs, pat);
            visit_node_in_body!(visitor, ctxt, rhs, pat);
        }

        Pat::WildCard | Pat::Rest => {}
    }
}

pub fn walk_attributes<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyAttrListSpan>,
    attr: AttrListId,
) where
    V: Visitor + ?Sized,
{
    for (idx, attr) in attr.data(ctxt.db).iter().enumerate() {
        ctxt.with_new_ctxt(
            |span| span.attr_moved(idx),
            |ctxt| {
                visitor.visit_attribute(ctxt, attr);
            },
        )
    }
}

pub fn walk_attribute<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyAttrSpan>, attr: &Attr)
where
    V: Visitor + ?Sized,
{
    match attr {
        Attr::Normal(normal_attr) => {
            ctxt.with_new_ctxt(
                |span| span.into_normal_attr(),
                |ctxt| {
                    if let Some(ident) = normal_attr.name.to_opt() {
                        ctxt.with_new_ctxt(
                            |span| span.name_moved(),
                            |ctxt| {
                                visitor.visit_ident(ctxt, ident);
                            },
                        )
                    }

                    ctxt.with_new_ctxt(
                        |span| span.args_moved(),
                        |ctxt| {
                            for (i, arg) in normal_attr.args.iter().enumerate() {
                                ctxt.with_new_ctxt(
                                    |span| span.arg_moved(i),
                                    |ctxt| {
                                        if let Some(key) = arg.key.to_opt() {
                                            ctxt.with_new_ctxt(
                                                |span| span.key_moved(),
                                                |ctxt| {
                                                    visitor.visit_ident(ctxt, key);
                                                },
                                            );
                                        }
                                        if let Some(value) = arg.value.to_opt() {
                                            ctxt.with_new_ctxt(
                                                |span| span.value_moved(),
                                                |ctxt| {
                                                    visitor.visit_ident(ctxt, value);
                                                },
                                            );
                                        }
                                    },
                                );
                            }
                        },
                    );
                },
            );
        }

        Attr::DocComment(doc_comment) => ctxt.with_new_ctxt(
            |span| span.into_doc_comment_attr().doc_moved().into_lit_span(),
            |ctxt| {
                visitor.visit_lit(ctxt, doc_comment.text.into());
            },
        ),
    }
}

pub fn walk_generic_param_list<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyGenericParamListSpan>,
    params: GenericParamListId,
) where
    V: Visitor + ?Sized,
{
    for (i, param) in params.data(ctxt.db).iter().enumerate() {
        ctxt.with_new_ctxt(
            |span| span.param_moved(i),
            |ctxt| {
                visitor.visit_generic_param(ctxt, param);
            },
        )
    }
}

pub fn walk_generic_param<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyGenericParamSpan>,
    param: &GenericParam,
) where
    V: Visitor + ?Sized,
{
    match param {
        GenericParam::Type(ty_param) => ctxt.with_new_ctxt(
            |span| span.into_type_param(),
            |ctxt| {
                if let Some(name) = ty_param.name.to_opt() {
                    ctxt.with_new_ctxt(
                        |span| span.name_moved(),
                        |ctxt| {
                            visitor.visit_ident(ctxt, name);
                        },
                    );
                }

                ctxt.with_new_ctxt(
                    |span| span.bounds_moved(),
                    |ctxt| {
                        visitor.visit_type_bound_list(ctxt, &ty_param.bounds);
                    },
                );
            },
        ),

        GenericParam::Const(const_param) => ctxt.with_new_ctxt(
            |span| span.into_const_param(),
            |ctxt| {
                if let Some(name) = const_param.name.to_opt() {
                    ctxt.with_new_ctxt(
                        |span| span.name_moved(),
                        |ctxt| {
                            visitor.visit_ident(ctxt, name);
                        },
                    );
                }

                if let Some(ty) = const_param.ty.to_opt() {
                    ctxt.with_new_ctxt(
                        |span| span.ty_moved(),
                        |ctxt| {
                            visitor.visit_ty(ctxt, ty);
                        },
                    );
                }
            },
        ),
    }
}

pub fn walk_generic_arg_list<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyGenericArgListSpan>,
    args: GenericArgListId,
) where
    V: Visitor + ?Sized,
{
    for (i, arg) in args.data(ctxt.db).iter().enumerate() {
        ctxt.with_new_ctxt(
            |span| span.arg_moved(i),
            |ctxt| {
                visitor.visit_generic_arg(ctxt, arg);
            },
        )
    }
}

pub fn walk_generic_arg<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyGenericArgSpan>,
    arg: &GenericArg,
) where
    V: Visitor + ?Sized,
{
    match arg {
        GenericArg::Type(type_arg) => {
            if let Some(ty) = type_arg.ty.to_opt() {
                ctxt.with_new_ctxt(
                    |span| span.into_type_arg().ty_moved(),
                    |ctxt| {
                        visitor.visit_ty(ctxt, ty);
                    },
                )
            }
        }

        GenericArg::Const(const_arg) => {
            if let Some(body) = const_arg.body.to_opt() {
                visitor.visit_body(&mut VisitorCtxt::with_body(ctxt.db, body), body);
            }
        }
    }
}

pub fn walk_call_arg_list<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyCallArgListSpan>,
    args: &[CallArg],
) where
    V: Visitor + ?Sized,
{
    for (idx, arg) in args.iter().copied().enumerate() {
        ctxt.with_new_ctxt(
            |span| span.arg_moved(idx),
            |ctxt| {
                visitor.visit_call_arg(ctxt, arg);
            },
        )
    }
}

pub fn walk_call_arg<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyCallArgSpan>, arg: CallArg)
where
    V: Visitor + ?Sized,
{
    if let Some(label) = arg.label {
        ctxt.with_new_ctxt(
            |span| span.label_moved(),
            |ctxt| visitor.visit_ident(ctxt, label),
        );
    }

    visit_node_in_body!(visitor, ctxt, &arg.expr, expr);
}

pub fn walk_func_param_list<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyFuncParamListSpan>,
    params: FuncParamListId,
) where
    V: Visitor + ?Sized,
{
    for (idx, param) in params.data(ctxt.db).iter().enumerate() {
        ctxt.with_new_ctxt(
            |span| span.param_moved(idx),
            |ctxt| {
                visitor.visit_func_param(ctxt, param);
            },
        )
    }
}

pub fn walk_func_param<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyFuncParamSpan>,
    param: &FuncParam,
) where
    V: Visitor + ?Sized,
{
    if let Some(FuncParamLabel::Ident(ident)) = param.label {
        ctxt.with_new_ctxt(
            |span| span.label_moved(),
            |ctxt| visitor.visit_ident(ctxt, ident),
        );
    }

    if let Some(FuncParamName::Ident(ident)) = param.name.to_opt() {
        ctxt.with_new_ctxt(
            |span| span.name_moved(),
            |ctxt| visitor.visit_ident(ctxt, ident),
        );
    }

    if let Some(ty) = param.ty.to_opt() {
        ctxt.with_new_ctxt(|span| span.ty_moved(), |ctxt| visitor.visit_ty(ctxt, ty));
    }
}

pub fn walk_field_list<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyFieldListSpan>,
    fields: &[Field],
) where
    V: Visitor + ?Sized,
{
    for (idx, field) in fields.iter().copied().enumerate() {
        ctxt.with_new_ctxt(
            |span| span.field_moved(idx),
            |ctxt| {
                visitor.visit_field(ctxt, field);
            },
        )
    }
}

pub fn walk_field<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyFieldSpan>, field: Field)
where
    V: Visitor + ?Sized,
{
    if let Some(name) = field.label {
        ctxt.with_new_ctxt(
            |span| span.label_moved(),
            |ctxt| visitor.visit_ident(ctxt, name),
        );
    }

    visit_node_in_body!(visitor, ctxt, &field.expr, expr);
}

pub fn walk_field_def_list<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyFieldDefListSpan>,
    fields: FieldDefListId,
) where
    V: Visitor + ?Sized,
{
    for (idx, field) in fields.data(ctxt.db).iter().enumerate() {
        ctxt.with_new_ctxt(
            |span| span.field_moved(idx),
            |ctxt| {
                visitor.visit_field_def(ctxt, field);
            },
        )
    }
}

pub fn walk_field_def<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyFieldDefSpan>,
    field: &FieldDef,
) where
    V: Visitor + ?Sized,
{
    if let Some(name) = field.name.to_opt() {
        ctxt.with_new_ctxt(
            |span| span.name_moved(),
            |ctxt| {
                visitor.visit_ident(ctxt, name);
            },
        )
    }

    if let Some(ty) = field.ty.to_opt() {
        ctxt.with_new_ctxt(
            |span| span.ty_moved(),
            |ctxt| {
                visitor.visit_ty(ctxt, ty);
            },
        )
    }
}

pub fn walk_variant_def_list<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyVariantDefListSpan>,
    variants: VariantDefListId,
) where
    V: Visitor + ?Sized,
{
    for (idx, variant) in variants.data(ctxt.db).iter().enumerate() {
        ctxt.with_new_ctxt(
            |span| span.variant_moved(idx),
            |ctxt| {
                visitor.visit_variant_def(ctxt, variant);
            },
        )
    }
}

pub fn walk_variant_def<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyVariantDefSpan>,
    variant: &VariantDef,
) where
    V: Visitor + ?Sized,
{
    if let Some(name) = variant.name.to_opt() {
        ctxt.with_new_ctxt(
            |span| span.name_moved(),
            |ctxt| {
                visitor.visit_ident(ctxt, name);
            },
        )
    }

    if let Some(ty) = variant.ty {
        ctxt.with_new_ctxt(
            |span| span.ty_moved(),
            |ctxt| {
                visitor.visit_ty(ctxt, ty);
            },
        )
    }
}

pub fn walk_path<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyPathSpan>, path: PathId)
where
    V: Visitor + ?Sized,
{
    for (idx, segment) in path.data(ctxt.db).iter().enumerate() {
        if let Some(ident) = segment.to_opt() {
            ctxt.with_new_ctxt(
                |span| span.segment_moved(idx).into_atom(),
                |ctxt| {
                    visitor.visit_ident(ctxt, ident);
                },
            )
        }
    }
}

pub fn walk_use_path<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyUsePathSpan>,
    path: UsePathId,
) where
    V: Visitor + ?Sized,
{
    for (i, segment) in path.data(ctxt.db).iter().enumerate() {
        if let Some(UsePathSegment::Ident(ident)) = segment.to_opt() {
            ctxt.with_new_ctxt(
                |span| span.segment_moved(i).into_atom(),
                |ctxt| {
                    visitor.visit_ident(ctxt, ident);
                },
            )
        }
    }
}

pub fn walk_ty<V>(visitor: &mut V, ctxt: &mut VisitorCtxt<'_, LazyTySpan>, ty: TypeId)
where
    V: Visitor + ?Sized,
{
    match ty.data(ctxt.db) {
        TypeKind::Ptr(ty) => {
            if let Some(ty) = ty.to_opt() {
                ctxt.with_new_ctxt(
                    |ctxt| ctxt.into_ptr_type().ty(),
                    |ctxt| {
                        visitor.visit_ty(ctxt, ty);
                    },
                )
            }
        }

        TypeKind::Path(path, generic_args) => ctxt.with_new_ctxt(
            |span| span.into_path_type(),
            |ctxt| {
                if let Some(path) = path.to_opt() {
                    ctxt.with_new_ctxt(
                        |span| span.path_moved(),
                        |ctxt| visitor.visit_path(ctxt, path),
                    );
                }
                ctxt.with_new_ctxt(
                    |span| span.generic_args_moved(),
                    |ctxt| {
                        visitor.visit_generic_arg_list(ctxt, generic_args);
                    },
                );
            },
        ),

        TypeKind::Tuple(elems) => ctxt.with_new_ctxt(
            |span| span.into_tuple_type(),
            |ctxt| {
                for (i, elem) in elems.iter().enumerate() {
                    let Some(elem) = elem.to_opt() else {
                        continue;
                    };
                    ctxt.with_new_ctxt(
                        |span| span.elem_ty_moved(i),
                        |ctxt| {
                            visitor.visit_ty(ctxt, elem);
                        },
                    )
                }
            },
        ),

        TypeKind::Array(elem, body) => ctxt.with_new_ctxt(
            |span| span.into_array_type(),
            |ctxt| {
                if let Some(elem) = elem.to_opt() {
                    ctxt.with_new_ctxt(
                        |span| span.elem_moved(),
                        |ctxt| {
                            visitor.visit_ty(ctxt, elem);
                        },
                    )
                }
                if let Some(body) = body.to_opt() {
                    visitor.visit_body(&mut VisitorCtxt::with_body(ctxt.db, body), body);
                }
            },
        ),

        TypeKind::SelfType => {}
    }
}

pub fn walk_type_bound_list<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyTypeBoundListSpan>,
    bounds: &[TypeBound],
) where
    V: Visitor + ?Sized,
{
    for (idx, bound) in bounds.iter().enumerate() {
        ctxt.with_new_ctxt(
            |span| span.bound_moved(idx),
            |ctxt| {
                visitor.visit_type_bound(ctxt, bound);
            },
        )
    }
}

pub fn walk_type_bound<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyTypeBoundSpan>,
    bound: &TypeBound,
) where
    V: Visitor + ?Sized,
{
    if let Some(path) = bound.path.to_opt() {
        ctxt.with_new_ctxt(
            |span| span.path_moved(),
            |ctxt| {
                visitor.visit_path(ctxt, path);
            },
        )
    }

    if let Some(args) = bound.generic_args {
        ctxt.with_new_ctxt(
            |span| span.generic_args_moved(),
            |ctxt| {
                visitor.visit_generic_arg_list(ctxt, args);
            },
        )
    }
}

pub fn walk_where_clause<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyWhereClauseSpan>,
    predicates: WhereClauseId,
) where
    V: Visitor + ?Sized,
{
    for (idx, predicate) in predicates.data(ctxt.db).iter().enumerate() {
        ctxt.with_new_ctxt(
            |span| span.predicate_moved(idx),
            |ctxt| {
                visitor.visit_where_predicate(ctxt, predicate);
            },
        )
    }
}

pub fn walk_where_predicate<V>(
    visitor: &mut V,
    ctxt: &mut VisitorCtxt<'_, LazyWherePredicateSpan>,
    predicate: &WherePredicate,
) where
    V: Visitor + ?Sized,
{
    if let Some(ty) = predicate.ty.to_opt() {
        ctxt.with_new_ctxt(
            |span| span.ty_moved(),
            |ctxt| {
                visitor.visit_ty(ctxt, ty);
            },
        )
    }

    ctxt.with_new_ctxt(
        |span| span.bounds_moved(),
        |ctxt| {
            visitor.visit_type_bound_list(ctxt, &predicate.bounds);
        },
    )
}

use attr::{Attr, AttrListId};

pub struct VisitorCtxt<'db, T>
where
    T: LazySpan,
{
    db: &'db dyn HirDb,
    span: DynLazySpan,
    _t: PhantomData<T>,
}

impl<'db, T> VisitorCtxt<'db, T>
where
    T: LazySpan,
{
    pub fn span(&self) -> Option<T>
    where
        T: SpanDowncast,
    {
        let dyn_span: DynLazySpan = self.span.clone();
        T::downcast(dyn_span)
    }

    fn with_new_ctxt<F1, F2, U>(&mut self, f1: F1, f2: F2)
    where
        T: SpanDowncast,
        F1: FnOnce(T) -> U,
        F2: FnOnce(&mut VisitorCtxt<U>),
        U: LazySpan + SpanDowncast + Into<DynLazySpan>,
    {
        let mut new_ctxt = self.transition(f1);
        f2(&mut new_ctxt);
        *self = new_ctxt.pop();
    }

    fn transition<F, U>(&mut self, f: F) -> VisitorCtxt<'db, U>
    where
        T: SpanDowncast,
        F: FnOnce(T) -> U,
        U: LazySpan + SpanDowncast + Into<DynLazySpan>,
    {
        let dyn_span = mem::replace(&mut self.span, DynLazySpan::invalid());
        let span = T::downcast(dyn_span).unwrap();
        let u = f(span);

        Self {
            db: self.db,
            span: u.into(),
            _t: PhantomData,
        }
        .cast()
    }

    fn pop<U>(mut self) -> VisitorCtxt<'db, U>
    where
        U: LazySpan,
    {
        self.span.0.as_mut().unwrap().pop_transition();

        Self {
            db: self.db,
            span: self.span,
            _t: PhantomData,
        }
        .cast()
    }

    fn cast<U: LazySpan>(self) -> VisitorCtxt<'db, U> {
        VisitorCtxt {
            db: self.db,
            span: self.span,
            _t: PhantomData,
        }
    }

    fn body(&self) -> Body {
        match self.span.0.as_ref().unwrap().root {
            ChainRoot::Body(body) => body,
            ChainRoot::Expr(expr) => expr.body,
            ChainRoot::Stmt(stmt) => stmt.body,
            ChainRoot::Pat(pat) => pat.body,
            _ => panic!(),
        }
    }
}

macro_rules! define_ctxt_ctor {
    ($((
        $span_ty:ty,
        $ctor:ident($($ctor_name:ident: $ctor_ty:ty),*)),)*) => {
        $(impl<'db> VisitorCtxt<'db, $span_ty> {
            pub fn $ctor(db: &'db dyn HirDb, $($ctor_name: $ctor_ty,)*) -> Self {
                Self {
                    db,
                    span: <$span_ty>::new($($ctor_name),*).into(),
                    _t: PhantomData,
                }
            }
        })*
    };
}

define_ctxt_ctor! {
    (LazyItemSpan, with_item(item: ItemKind)),
    (LazyTopModSpan, with_top_mod(top_mod: TopLevelMod)),
    (LazyModSpan, with_mod(mod_: Mod)),
    (LazyFuncSpan, with_func(func: Func)),
    (LazyStructSpan, with_struct(struct_: Struct)),
    (LazyContractSpan, with_contract(contract: Contract)),
    (LazyEnumSpan, with_enum(enum_: Enum)),
    (LazyTypeAliasSpan, with_type_alias(type_alias: TypeAlias)),
    (LazyImplSpan, with_impl(impl_: Impl)),
    (LazyTraitSpan, with_trait(trait_: Trait)),
    (LazyImplTraitSpan, with_impl_trait(impl_trait: ImplTrait)),
    (LazyConstSpan, with_const(const_: Const)),
    (LazyUseSpan, with_use(use_: Use)),
    (LazyBodySpan, with_body(body: Body)),
    (LazyExprSpan, with_expr(body: Body, expr: ExprId)),
    (LazyStmtSpan, with_stmt(body: Body, stmt: StmtId)),
    (LazyPatSpan, with_pat(body: Body, pat: PatId)),

}

macro_rules! visit_node_in_body {
    ($visitor:expr,  $ctxt:expr,  $id:expr, $inner:ident) => {
        if let Partial::Present(data) = $id.data($ctxt.db, $ctxt.body()) {
            paste::paste! {
                $visitor.[<visit_ $inner>](&mut VisitorCtxt::[<with_ $inner>]($ctxt.db, $ctxt.body(), *$id), data);

            }
        }
    }
}
use visit_node_in_body;

#[cfg(test)]
mod tests {

    use crate::test_db::TestDb;

    use super::*;
    struct MyVisitor {
        generic_param_list: Option<LazyGenericParamListSpan>,
        attributes: Vec<LazyAttrSpan>,
        lit_ints: Vec<LazyLitSpan>,
    }

    impl Visitor for MyVisitor {
        fn visit_attribute(&mut self, ctxt: &mut VisitorCtxt<LazyAttrSpan>, _attrs: &Attr) {
            self.attributes.push(ctxt.span().unwrap());
        }

        fn visit_generic_param_list(
            &mut self,
            ctxt: &mut VisitorCtxt<LazyGenericParamListSpan>,
            _params: GenericParamListId,
        ) {
            self.generic_param_list = Some(ctxt.span().unwrap());
        }

        fn visit_lit(&mut self, ctxt: &mut VisitorCtxt<LazyLitSpan>, lit: LitKind) {
            if let LitKind::Int(_) = lit {
                self.lit_ints.push(ctxt.span().unwrap());
            }
        }
    }

    #[test]
    fn visitor() {
        let mut db = TestDb::default();
        let text = r#"
            #[attr1]
            #[attr2]
            fn foo<T: 'static, V: Add>() {
                1
                "foo"
                42
            }"#;

        let func = db.expect_item::<Func>(text);
        let top_mod = func.top_mod(&db);

        let mut visitor = MyVisitor {
            generic_param_list: None,
            attributes: Vec::new(),
            lit_ints: Vec::new(),
        };

        let mut ctxt = VisitorCtxt::with_func(&db, func);
        visitor.visit_func(&mut ctxt, func);

        assert_eq!(
            "<T: 'static, V: Add>",
            db.text_at(top_mod, &visitor.generic_param_list.unwrap())
        );

        assert_eq!(visitor.attributes.len(), 2);
        assert_eq!("#[attr1]", db.text_at(top_mod, &visitor.attributes[0]));
        assert_eq!("#[attr2]", db.text_at(top_mod, &visitor.attributes[1]));

        assert_eq!(visitor.lit_ints.len(), 2);
        assert_eq!("1", db.text_at(top_mod, &visitor.lit_ints[0]));
        assert_eq!("42", db.text_at(top_mod, &visitor.lit_ints[1]));
    }
}