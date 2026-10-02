//! Checking `quote { .. }` in its provider.
//!
//! The splices are ordinary provider expressions, checked here against the
//! values their positions accept. The template itself is not type-checked
//! where it is written: its names are classified in the provider's
//! environment, where the provider's locals and generic parameters are live,
//! and the result is kept in the typed body for generation to read.

use rustc_hash::FxHashMap;

use super::{TyChecker, env::ExprProp, expr::resolve_ident_expr, path::ResolvedPathInBody};
use crate::{
    analysis::{
        name_resolution::{PathRes, resolve_path},
        ty::{
            const_ty::{BodyHoleSite, HoleAnchor, LoweringContext},
            diagnostics::BodyDiag,
            ty_def::{PrimTy, TyBase, TyData, TyId, TyVarSort},
            ty_lower::lower_hir_ty,
        },
    },
    core::meta::MetaItem,
    hir_def::{
        Body, CallableDef, Cond, CondId, Expr, ExprId, FuncParamName, GenericArg, GenericArgListId,
        GenericParam, GenericParamListId, IdentId, Partial, Pat, PatId, PathId, PathKind, Quote,
        QuoteHolePosition, QuoteItem, QuoteKind, QuoteWhereClause, QuoteWherePredicate, Stmt,
        StmtId, TypeBound, TypeId as HirTyId, TypeKind,
    },
    span::DynLazySpan,
};

/// A checked quote: its kind, the position each splice stands for, and how
/// each name in its template was classified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedQuote<'db> {
    pub kind: QuoteKind,
    /// Parallel to the quote's holes. A body made only of splices takes the
    /// position of its kind (`Items` or `Where`) here.
    pub positions: Vec<QuoteHolePosition>,
    /// Every name in the template, in the order [`walk`] visits them.
    pub names: Vec<QuoteNameUse<'db>>,
    /// Generic item index for each quoted function, in function visiting order.
    pub function_generic_items: Vec<u32>,
    /// Explicit local annotations in quoted functions and standalone fragments.
    pub local_types: Vec<QuoteLocalType<'db>>,
    /// Checked primitive types for associated constants in a quoted impl.
    pub assoc_const_primitives: Vec<Option<PrimTy>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuoteLocalType<'db> {
    pub func: Option<u32>,
    pub stmt: StmtId,
    pub ty: HirTyId<'db>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteNameUse<'db> {
    pub site: QuoteNameSite,
    pub class: QuoteNameClass<'db>,
    /// Quoted function currently being visited, if any.
    pub func: Option<u32>,
    /// Quoted associated constant currently being visited, if any.
    pub assoc_const: Option<u32>,
    /// Signature slot being checked, when this name occurs in a function type.
    pub signature: Option<QuoteSignatureSite>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteSignatureSite {
    Param {
        func: u32,
        index: u32,
    },
    Result {
        func: u32,
    },
    Effect {
        func: u32,
        index: u32,
    },
    GenericBound {
        func: u32,
        param: u32,
        bound: u32,
    },
    GenericDefault {
        func: u32,
        param: u32,
    },
    ImplTraitArgument {
        index: u32,
    },
    AssocType {
        index: u32,
    },
    FragmentWhereType {
        predicate: u32,
    },
    FragmentWhereBound {
        predicate: u32,
        bound: u32,
    },
    WhereType {
        func: u32,
        predicate: u32,
    },
    WhereBound {
        func: u32,
        predicate: u32,
        bound: u32,
    },
    WhereConst {
        func: u32,
        predicate: u32,
    },
    LocalType {
        func: Option<u32>,
        stmt: StmtId,
    },
    CallTypeArgument {
        expr: ExprId,
        index: u32,
    },
}

/// Where a name stands in the template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteNameSite {
    Expr(ExprId),
    Pat(PatId),
    /// The `n`th type or trait path in visiting order; types are interned, so
    /// they have no node of their own.
    Type(u32),
}

/// What a name in a template means. Binders are identified by the node that
/// introduces them, never by spelling, so emission can keep them binders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuoteNameClass<'db> {
    /// A pattern identifier that introduces a binder of the fragment.
    Binds,
    /// A use of a binder introduced inside the fragment.
    Bound(QuoteBinder),
    /// The `n`th name of the quote's open list; the destination binds it.
    Open(u32),
    /// `self` or `Self`, bound by the destination method or impl.
    SelfName,
    /// Resolved where the quote is written.
    Rooted(PathRes<'db>),
    /// `<T as Tr>::X`: its type and trait are classified as their own sites,
    /// and `X` is looked up through them at the destination.
    Qualified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuoteBinder {
    /// A pattern binder in the template.
    Pat(PatId),
    /// Parameter `index` of the `func`th quoted function in visiting order.
    Param { func: u32, index: u32 },
    /// Effect binding `index` of the `func`th quoted function.
    EffectParam { func: u32, index: u32 },
    /// Generic parameter `index` of the `item`th quoted item in visiting order.
    Generic { item: u32, index: u32 },
}

impl<'db> TyChecker<'db> {
    pub(super) fn in_const_context(&self) -> bool {
        self.env.owner().is_const_context(self.db)
    }

    pub(super) fn callable_contains_quote(&self, callable: CallableDef<'db>) -> bool {
        let CallableDef::Func(func) = callable else {
            return false;
        };
        func.body(self.db).is_some_and(|body| {
            body.exprs(self.db)
                .iter()
                .any(|(_, expr)| matches!(expr, Partial::Present(Expr::Quote(_))))
        })
    }

    pub(super) fn is_code_type(&self, ty: TyId<'db>) -> bool {
        MetaItem::Code.resolve(self.db, self.env.scope()) == Some(ty.base_ty(self.db))
    }

    pub(super) fn check_quote(&mut self, expr: ExprId, quote: &Quote<'db>) -> ExprProp<'db> {
        let db = self.db;
        let quote_span: DynLazySpan<'db> = expr.span(self.body()).into();
        let in_const_context = self.in_const_context();
        if !in_const_context {
            self.push_diag(BodyDiag::QuoteOutsideConstContext(quote_span.clone()));
        }

        let holes: Vec<TyId<'db>> = quote
            .holes
            .iter()
            .map(|hole| {
                let ty = self.check_expr_unknown(hole.expr).ty;
                self.normalize_ty(ty)
            })
            .collect();

        let meta = match QuoteTypes::resolve(self) {
            Ok(meta) => meta,
            Err(item) => {
                self.push_diag(BodyDiag::MissingMetaItem {
                    primary: quote_span,
                    item,
                });
                return ExprProp::invalid(db);
            }
        };

        let splices = quote.splice_only_body(db);
        let (kind, positions) = match quote.syntactic_kind(db) {
            Some(kind) => (kind, quote.holes.iter().map(|hole| hole.position).collect()),
            None => {
                let mut kinds = holes.iter().filter_map(|ty| meta.code_kind(db, *ty));
                let first = kinds.next();
                if kinds.any(|kind| Some(kind) != first) {
                    self.push_diag(BodyDiag::QuoteSpliceKinds(quote_span.clone()));
                    return ExprProp::invalid(db);
                }
                let kind = first.unwrap_or(QuoteKind::Expr);
                (kind, vec![QuoteHolePosition::Code(kind); holes.len()])
            }
        };

        for ((hole, ty), position) in quote.holes.iter().zip(holes).zip(&positions) {
            self.check_hole_value(&meta, hole.expr, ty, *position);
        }

        let (names, function_generic_items, local_types) = if splices.is_some() {
            (Vec::new(), Vec::new(), Vec::new())
        } else {
            let mut walk = TemplateWalk::new(self, expr, quote);
            walk.walk_quote();
            (walk.names, walk.function_generic_items, walk.local_types)
        };
        let assoc_const_primitives = match &quote.fragment {
            crate::hir_def::QuoteFragment::Items(items)
                if matches!(items.as_slice(), [QuoteItem::Impl(_)]) =>
            {
                let [QuoteItem::Impl(item)] = items.as_slice() else {
                    unreachable!()
                };
                item.items
                    .iter()
                    .filter_map(|child| {
                        let QuoteItem::Const(constant) = child else {
                            return None;
                        };
                        Some(constant.ty.to_opt().and_then(|ty| {
                            let resolved =
                                lower_hir_ty(db, ty, self.env.scope(), self.env.assumptions());
                            match resolved.base_ty(db).data(db) {
                                TyData::TyBase(TyBase::Prim(primitive)) => Some(*primitive),
                                _ => None,
                            }
                        }))
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        self.env.register_quote(
            expr,
            CheckedQuote {
                kind,
                positions,
                names,
                function_generic_items,
                local_types,
                assoc_const_primitives,
            },
        );

        if !in_const_context {
            return ExprProp::invalid(db);
        }
        ExprProp::new(meta.code(db, kind), true)
    }

    fn check_hole_value(
        &mut self,
        meta: &QuoteTypes<'db>,
        hole: ExprId,
        ty: TyId<'db>,
        position: QuoteHolePosition,
    ) {
        let db = self.db;
        let ty = if let TyData::TyVar(var) = ty.data(db)
            && let TyVarSort::String { min_len, .. } = var.sort
        {
            let string_ty = TyId::string_with_len(db, min_len);
            self.equate_ty(ty, string_ty, hole.span(self.body()).into());
            string_ty
        } else {
            ty
        };
        if ty.has_invalid(db) {
            return;
        }
        let accepted = match position {
            QuoteHolePosition::Code(kind) => {
                meta.code_kind(db, ty) == Some(kind)
                    || (kind == QuoteKind::Expr
                        && (ty.is_bool(db) || ty.is_integral(db) || ty.is_string(db)))
            }
            QuoteHolePosition::Member | QuoteHolePosition::FieldLabel => {
                ty == meta.get(MetaItem::Field)
            }
            QuoteHolePosition::Type => {
                ty == meta.get(MetaItem::FieldType) || ty == meta.get(MetaItem::Record)
            }
            QuoteHolePosition::Name => {
                ty == meta.get(MetaItem::DeclarationName) || ty == meta.get(MetaItem::Field)
            }
        };
        if accepted {
            return;
        }
        let span: DynLazySpan<'db> = hole.span(self.body()).into();
        if matches!(ty.data(db), TyData::TyVar(_)) && !ty.is_integral_var(db) {
            // Nothing decided the splice's type; the position's main value
            // type decides it.
            let expected = match position {
                QuoteHolePosition::Code(kind) => meta.code(db, kind),
                QuoteHolePosition::Member | QuoteHolePosition::FieldLabel => {
                    meta.get(MetaItem::Field)
                }
                QuoteHolePosition::Type => meta.get(MetaItem::FieldType),
                QuoteHolePosition::Name => meta.get(MetaItem::DeclarationName),
            };
            self.equate_ty(ty, expected, span);
            return;
        }
        self.push_diag(BodyDiag::QuoteHoleValue {
            primary: span,
            position,
            found: ty,
        });
    }
}

/// The designated `core::meta` types a quote's splices are checked against.
struct QuoteTypes<'db>([TyId<'db>; MetaItem::ALL.len()]);

impl<'db> QuoteTypes<'db> {
    fn resolve(tc: &TyChecker<'db>) -> Result<Self, MetaItem> {
        let resolved = MetaItem::ALL.map(|item| item.resolve(tc.db, tc.env.scope()).ok_or(item));
        for result in &resolved {
            (*result)?;
        }
        Ok(Self(resolved.map(|result| {
            result.expect("all designated items resolved")
        })))
    }

    fn get(&self, item: MetaItem) -> TyId<'db> {
        self.0[item as usize]
    }

    fn code(&self, db: &'db dyn crate::analysis::HirAnalysisDb, kind: QuoteKind) -> TyId<'db> {
        TyId::app(db, self.get(MetaItem::Code), self.get(kind.meta_item()))
    }

    /// `K` when `ty` is `Code<K>`.
    fn code_kind(
        &self,
        db: &'db dyn crate::analysis::HirAnalysisDb,
        ty: TyId<'db>,
    ) -> Option<QuoteKind> {
        let (base, args) = ty.decompose_ty_app(db);
        if base != self.get(MetaItem::Code) {
            return None;
        }
        let [argument] = args else {
            return None;
        };
        QuoteKind::ALL
            .into_iter()
            .find(|kind| self.get(kind.meta_item()) == *argument)
    }
}

/// One lexical pass over a template: fragment binders are tracked in scopes
/// as Fe scopes them; every other name is resolved in the provider's
/// environment at the quote.
struct TemplateWalk<'a, 'db> {
    tc: &'a mut TyChecker<'db>,
    quote_expr: ExprId,
    template: Body<'db>,
    quote: &'a Quote<'db>,
    open: &'a [IdentId<'db>],
    /// Value binder scopes, innermost last.
    values: Vec<FxHashMap<IdentId<'db>, QuoteBinder>>,
    /// Generic parameter scopes of quoted items, innermost last.
    types: Vec<FxHashMap<IdentId<'db>, QuoteBinder>>,
    funcs: u32,
    assoc_consts: u32,
    assoc_types: u32,
    active_func: Option<u32>,
    active_assoc_const: Option<u32>,
    signature_site: Option<QuoteSignatureSite>,
    function_generic_items: Vec<u32>,
    local_types: Vec<QuoteLocalType<'db>>,
    items: u32,
    type_sites: u32,
    /// The template node whose span a type path reports at.
    anchor: Option<ExprId>,
    names: Vec<QuoteNameUse<'db>>,
}

impl<'a, 'db> TemplateWalk<'a, 'db> {
    fn new(tc: &'a mut TyChecker<'db>, quote_expr: ExprId, quote: &'a Quote<'db>) -> Self {
        Self {
            tc,
            quote_expr,
            template: quote.template,
            quote,
            open: &quote.open,
            values: vec![FxHashMap::default()],
            types: vec![FxHashMap::default()],
            funcs: 0,
            assoc_consts: 0,
            assoc_types: 0,
            active_func: None,
            active_assoc_const: None,
            signature_site: None,
            function_generic_items: Vec::new(),
            local_types: Vec::new(),
            items: 0,
            type_sites: 0,
            anchor: None,
            names: Vec::new(),
        }
    }

    fn db(&self) -> &'db dyn crate::analysis::HirAnalysisDb {
        self.tc.db
    }

    fn walk_quote(&mut self) {
        let db = self.db();
        let quote = self.quote;
        match &quote.fragment {
            crate::hir_def::QuoteFragment::Arms(arms) => self.walk_arms(arms),
            crate::hir_def::QuoteFragment::Expr => self.walk_expr(self.template.expr(db)),
            crate::hir_def::QuoteFragment::Items(items) => {
                items.iter().for_each(|item| self.walk_item(item))
            }
            crate::hir_def::QuoteFragment::Where(clause) => self.walk_where(clause),
        }
        for item in &quote.block_items {
            self.anchor = Some(item.block);
            self.walk_item(&item.item);
        }
    }

    fn walk_arms(&mut self, arms: &[crate::hir_def::MatchArm]) {
        for arm in arms {
            if let Some((pat, body)) = arm.parts() {
                self.values.push(FxHashMap::default());
                self.walk_pat(pat);
                self.walk_expr(body);
                self.values.pop();
            }
        }
    }

    fn walk_item(&mut self, item: &QuoteItem<'db>) {
        let db = self.db();
        match item {
            QuoteItem::Func(func) => {
                let func_index = self.funcs;
                self.funcs += 1;
                let previous_func = self.active_func.replace(func_index);
                self.function_generic_items.push(self.items);
                self.enter_generics(func.generic_params);
                if let Partial::Present(params) = func.params {
                    for (index, param) in params.data(db).iter().enumerate() {
                        let previous = self.signature_site.replace(QuoteSignatureSite::Param {
                            func: func_index,
                            index: index as u32,
                        });
                        self.walk_ty_opt(param.ty);
                        self.signature_site = previous;
                    }
                }
                for (index, effect) in func.effects.data(db).iter().enumerate() {
                    let previous = self.signature_site.replace(QuoteSignatureSite::Effect {
                        func: func_index,
                        index: index as u32,
                    });
                    self.walk_ty_opt(effect.key_ty);
                    self.signature_site = previous;
                }
                if let Some(ty) = func.ret_ty {
                    let previous = self
                        .signature_site
                        .replace(QuoteSignatureSite::Result { func: func_index });
                    self.walk_ty(ty);
                    self.signature_site = previous;
                }
                self.walk_where(&func.where_clause);
                let mut params_scope = FxHashMap::default();
                if let Partial::Present(params) = func.params {
                    for (index, param) in params.data(db).iter().enumerate() {
                        if let Partial::Present(FuncParamName::Ident(name)) = param.name
                            && !name.is_self(db)
                        {
                            params_scope.insert(
                                name,
                                QuoteBinder::Param {
                                    func: func_index,
                                    index: index as u32,
                                },
                            );
                        }
                    }
                }
                for (index, effect) in func.effects.data(db).iter().enumerate() {
                    if let Some(name) = effect.name {
                        params_scope.insert(
                            name,
                            QuoteBinder::EffectParam {
                                func: func_index,
                                index: index as u32,
                            },
                        );
                    }
                }
                self.values.push(params_scope);
                if let Some(body) = func.body {
                    self.walk_expr(body);
                }
                self.values.pop();
                self.types.pop();
                self.active_func = previous_func;
            }
            QuoteItem::Const(const_) => {
                let index = self.assoc_consts;
                self.assoc_consts += 1;
                self.walk_ty_opt(const_.ty);
                let previous = self.active_assoc_const.replace(index);
                if let Some(value) = const_.value {
                    self.walk_expr(value);
                }
                self.active_assoc_const = previous;
            }
            QuoteItem::Type(ty) => {
                let index = self.assoc_types;
                self.assoc_types += 1;
                if let Some(ty) = ty.ty {
                    let previous = self
                        .signature_site
                        .replace(QuoteSignatureSite::AssocType { index });
                    self.walk_ty(ty);
                    self.signature_site = previous;
                }
            }
            QuoteItem::Impl(impl_) => {
                self.enter_generics(impl_.generic_params);
                if let Some(Partial::Present(trait_ref)) = impl_.trait_ref
                    && let Partial::Present(path) = trait_ref.path(db)
                {
                    self.impl_trait_path(path);
                }
                self.walk_ty_opt(impl_.ty);
                self.walk_where(&impl_.where_clause);
                for item in &impl_.items {
                    self.walk_item(item);
                }
                self.types.pop();
            }
            QuoteItem::Hole(_) => {}
        }
    }

    /// Opens a generic scope for a quoted item and walks its bounds.
    fn enter_generics(&mut self, params: GenericParamListId<'db>) {
        let db = self.db();
        let item = self.items;
        self.items += 1;
        let mut scope = FxHashMap::default();
        for (index, param) in params.data(db).iter().enumerate() {
            if let Partial::Present(name) = param.name() {
                scope.insert(
                    name,
                    QuoteBinder::Generic {
                        item,
                        index: index as u32,
                    },
                );
            }
        }
        self.types.push(scope);
        for (param_index, param) in params.data(db).iter().enumerate() {
            match param {
                GenericParam::Type(param) => {
                    for (bound_index, bound) in param.bounds.iter().enumerate() {
                        let previous = self.signature_site;
                        self.signature_site =
                            self.active_func
                                .map(|func| QuoteSignatureSite::GenericBound {
                                    func,
                                    param: param_index as u32,
                                    bound: bound_index as u32,
                                });
                        self.walk_bound(bound);
                        self.signature_site = previous;
                    }
                    if let Some(ty) = param.default_ty {
                        let previous = self.signature_site;
                        self.signature_site =
                            self.active_func
                                .map(|func| QuoteSignatureSite::GenericDefault {
                                    func,
                                    param: param_index as u32,
                                });
                        self.walk_ty(ty);
                        self.signature_site = previous;
                    }
                }
                GenericParam::Const(param) => self.walk_ty_opt(param.ty),
            }
        }
    }

    fn walk_where(&mut self, clause: &QuoteWhereClause<'db>) {
        for (predicate_index, predicate) in clause.predicates.iter().enumerate() {
            match predicate {
                QuoteWherePredicate::Bound(predicate) => {
                    let previous = self.signature_site;
                    self.signature_site = self
                        .active_func
                        .map(|func| QuoteSignatureSite::WhereType {
                            func,
                            predicate: predicate_index as u32,
                        })
                        .or(Some(QuoteSignatureSite::FragmentWhereType {
                            predicate: predicate_index as u32,
                        }));
                    self.walk_ty_opt(predicate.ty);
                    self.signature_site = previous;
                    for (bound_index, bound) in predicate.bounds.iter().enumerate() {
                        let previous = self.signature_site;
                        self.signature_site = self
                            .active_func
                            .map(|func| QuoteSignatureSite::WhereBound {
                                func,
                                predicate: predicate_index as u32,
                                bound: bound_index as u32,
                            })
                            .or(Some(QuoteSignatureSite::FragmentWhereBound {
                                predicate: predicate_index as u32,
                                bound: bound_index as u32,
                            }));
                        self.walk_bound(bound);
                        self.signature_site = previous;
                    }
                }
                QuoteWherePredicate::Const(expr) => {
                    let previous = self.signature_site;
                    self.signature_site =
                        self.active_func.map(|func| QuoteSignatureSite::WhereConst {
                            func,
                            predicate: predicate_index as u32,
                        });
                    self.walk_expr(*expr);
                    self.signature_site = previous;
                }
                QuoteWherePredicate::Hole(_) => {}
            }
        }
    }

    fn walk_bound(&mut self, bound: &TypeBound<'db>) {
        if let TypeBound::Trait(trait_ref) = bound
            && let Partial::Present(path) = trait_ref.path(self.db())
        {
            self.type_path(path);
        }
    }

    fn walk_ty_opt(&mut self, ty: Partial<HirTyId<'db>>) {
        if let Partial::Present(ty) = ty {
            self.walk_ty(ty);
        }
    }

    fn walk_ty(&mut self, ty: HirTyId<'db>) {
        let db = self.db();
        match ty.data(db) {
            TypeKind::Ptr(inner) | TypeKind::Mode(_, inner) => self.walk_ty_opt(*inner),
            TypeKind::Path(Partial::Present(path)) => self.type_path(*path),
            TypeKind::Tuple(elems) => {
                for elem in elems.data(db) {
                    self.walk_ty_opt(*elem);
                }
            }
            TypeKind::Array(elem, _) => self.walk_ty_opt(*elem),
            TypeKind::Path(Partial::Absent)
            | TypeKind::Primitive(_)
            | TypeKind::Never
            | TypeKind::Hole(_) => {}
        }
    }

    fn walk_generic_args(&mut self, args: GenericArgListId<'db>) {
        for arg in args.data(self.db()) {
            match arg {
                GenericArg::Type(arg) => self.walk_ty_opt(arg.ty),
                GenericArg::AssocType(arg) => self.walk_ty_opt(arg.ty),
                GenericArg::Const(_) => {}
            }
        }
    }

    fn walk_path_args(&mut self, path: PathId<'db>) {
        let db = self.db();
        for index in 0..path.len(db) {
            if let Some(segment) = path.segment(db, index) {
                self.walk_generic_args(segment.generic_args(db));
            }
        }
    }

    fn walk_expr(&mut self, expr: ExprId) {
        let db = self.db();
        let Partial::Present(data) = &self.template.exprs(db)[expr] else {
            return;
        };
        let outer_anchor = self.anchor.replace(expr);
        match data {
            Expr::Lit(_) | Expr::Hole(_) | Expr::UnsupportedMacroCall | Expr::Quote(_) => {}
            Expr::Block(stmts) => {
                self.values.push(FxHashMap::default());
                for stmt in stmts {
                    self.walk_stmt(*stmt);
                }
                self.values.pop();
            }
            Expr::Bin(lhs, rhs, _) | Expr::Assign(lhs, rhs) | Expr::AugAssign(lhs, rhs, _) => {
                self.walk_expr(*lhs);
                self.walk_expr(*rhs);
            }
            Expr::Un(inner, _) => self.walk_expr(*inner),
            Expr::Cast(inner, ty) => {
                self.walk_expr(*inner);
                self.walk_ty_opt(*ty);
            }
            Expr::Call(callee, args) => {
                self.walk_expr(*callee);
                args.iter().for_each(|arg| self.walk_expr(arg.expr));
            }
            Expr::Assert(args) => args.iter().for_each(|arg| self.walk_expr(arg.expr)),
            Expr::MethodCall(receiver, _, generic_args, args) => {
                self.walk_expr(*receiver);
                self.walk_generic_args(*generic_args);
                args.iter().for_each(|arg| self.walk_expr(arg.expr));
            }
            Expr::Path(Partial::Present(path)) => self.value_path(expr, *path),
            Expr::Path(Partial::Absent) => {}
            Expr::RecordInit(path, fields) => {
                if let Partial::Present(path) = path {
                    self.record_path(QuoteNameSite::Expr(expr), *path);
                }
                fields.iter().for_each(|field| self.walk_expr(field.expr));
            }
            // A member splice names a field of the destination's record.
            Expr::Field(receiver, _) => self.walk_expr(*receiver),
            Expr::Tuple(elems) | Expr::Array(elems) => {
                elems.iter().for_each(|elem| self.walk_expr(*elem))
            }
            Expr::ArrayRep(value, _) => self.walk_expr(*value),
            Expr::If(cond, then, else_) => {
                self.values.push(FxHashMap::default());
                self.walk_cond(*cond);
                self.walk_expr(*then);
                self.values.pop();
                if let Some(else_) = else_ {
                    self.walk_expr(*else_);
                }
            }
            Expr::Match(scrutinee, arms) => {
                self.walk_expr(*scrutinee);
                if let Partial::Present(arms) = arms {
                    self.walk_arms(arms);
                }
            }
            Expr::With(bindings, body) => {
                for binding in bindings {
                    if let Some(Partial::Present(path)) = binding.key_path {
                        self.type_path(path);
                    }
                    self.walk_expr(binding.value);
                }
                self.walk_expr(*body);
            }
        }
        self.anchor = outer_anchor;
    }

    fn walk_stmt(&mut self, stmt: crate::hir_def::StmtId) {
        let db = self.db();
        let Partial::Present(data) = &self.template.stmts(db)[stmt] else {
            return;
        };
        match data {
            Stmt::Let(pat, ty, init) => {
                if let Some(ty) = ty {
                    let func = self.active_func;
                    self.local_types.push(QuoteLocalType {
                        func,
                        stmt,
                        ty: *ty,
                    });
                    let previous = self
                        .signature_site
                        .replace(QuoteSignatureSite::LocalType { func, stmt });
                    self.walk_ty(*ty);
                    self.signature_site = previous;
                }
                if let Some(init) = init {
                    self.walk_expr(*init);
                }
                self.walk_pat(*pat);
            }
            Stmt::For(pat, iter, body, _) => {
                self.walk_expr(*iter);
                self.values.push(FxHashMap::default());
                self.walk_pat(*pat);
                self.walk_expr(*body);
                self.values.pop();
            }
            Stmt::While(cond, body) => {
                self.values.push(FxHashMap::default());
                self.walk_cond(*cond);
                self.walk_expr(*body);
                self.values.pop();
            }
            Stmt::Return(Some(expr)) | Stmt::Expr(expr) => self.walk_expr(*expr),
            Stmt::Return(None) | Stmt::Continue | Stmt::Break => {}
        }
    }

    fn walk_cond(&mut self, cond: CondId) {
        let db = self.db();
        let Partial::Present(data) = &self.template.conds(db)[cond] else {
            return;
        };
        match data {
            Cond::Expr(expr) => self.walk_expr(*expr),
            Cond::Let(pat, expr) => {
                self.walk_expr(*expr);
                self.walk_pat(*pat);
            }
            Cond::Bin(lhs, rhs, _) => {
                self.walk_cond(*lhs);
                self.walk_cond(*rhs);
            }
        }
    }

    /// A bare identifier binds unless it names a type, variant or constant
    /// where the quote is written, the rule Fe's checker applies to patterns.
    fn walk_pat(&mut self, pat: PatId) {
        let db = self.db();
        let Partial::Present(data) = &self.template.pats(db)[pat] else {
            return;
        };
        match data {
            Pat::Hole(_) => {}
            Pat::WildCard | Pat::Rest | Pat::Lit(_) => {}
            Pat::Tuple(pats) => pats.iter().for_each(|pat| self.walk_pat(*pat)),
            Pat::Path(Partial::Present(path), _) => {
                if let Some(name) = path.as_ident(db)
                    && !self.names_constant(*path)
                {
                    self.values
                        .last_mut()
                        .expect("a value scope is always open")
                        .insert(name, QuoteBinder::Pat(pat));
                    self.push(QuoteNameSite::Pat(pat), QuoteNameClass::Binds);
                } else {
                    self.record_path(QuoteNameSite::Pat(pat), *path);
                }
            }
            Pat::Path(Partial::Absent, _) => {}
            Pat::PathTuple(path, pats) => {
                if let Partial::Present(path) = path {
                    self.record_path(QuoteNameSite::Pat(pat), *path);
                }
                pats.iter().for_each(|pat| self.walk_pat(*pat));
            }
            Pat::Record(path, fields) => {
                if let Partial::Present(path) = path {
                    self.record_path(QuoteNameSite::Pat(pat), *path);
                }
                fields.iter().for_each(|field| self.walk_pat(field.pat));
            }
            Pat::Or(lhs, rhs) => {
                self.walk_pat(*lhs);
                self.walk_pat(*rhs);
            }
        }
    }

    fn names_constant(&self, path: PathId<'db>) -> bool {
        matches!(
            self.resolve_in_provider(path, true),
            Some(
                PathRes::EnumVariant(_)
                    | PathRes::Const(..)
                    | PathRes::Ty(_)
                    | PathRes::TyAlias(..)
            )
        )
    }

    fn lookup_value(&self, name: IdentId<'db>) -> Option<QuoteBinder> {
        self.values
            .iter()
            .rev()
            .find_map(|scope| scope.get(&name).copied())
    }

    fn lookup_type(&self, name: IdentId<'db>) -> Option<QuoteBinder> {
        self.types
            .iter()
            .rev()
            .find_map(|scope| scope.get(&name).copied())
    }

    fn open_index(&self, name: IdentId<'db>) -> Option<u32> {
        self.open
            .iter()
            .position(|open| *open == name)
            .map(|index| index as u32)
    }

    fn push(&mut self, site: QuoteNameSite, class: QuoteNameClass<'db>) {
        self.names.push(QuoteNameUse {
            site,
            class,
            func: self.active_func,
            assoc_const: self.active_assoc_const,
            signature: self.signature_site,
        });
    }

    fn resolve_in_provider(&self, path: PathId<'db>, tail_as_value: bool) -> Option<PathRes<'db>> {
        let db = self.db();
        resolve_path(
            db,
            path,
            self.tc.env.scope(),
            self.tc.env.assumptions(),
            tail_as_value,
        )
        .or_else(|_| {
            resolve_path(
                db,
                path.strip_generic_args(db),
                self.tc.env.scope(),
                self.tc.env.assumptions(),
                tail_as_value,
            )
        })
        .ok()
    }

    fn span_at(&self, site: QuoteNameSite) -> DynLazySpan<'db> {
        match site {
            QuoteNameSite::Expr(expr) => expr.span(self.template).into(),
            QuoteNameSite::Pat(pat) => pat.span(self.template).into(),
            QuoteNameSite::Type(_) => match self.anchor {
                Some(expr) => expr.span(self.template).into(),
                None => self.quote_expr.span(self.tc.body()).into(),
            },
        }
    }

    /// A path in expression position.
    fn value_path(&mut self, expr: ExprId, path: PathId<'db>) {
        let db = self.db();
        let site = QuoteNameSite::Expr(expr);
        for index in 0..path.len(db) {
            let Some(segment) = path.segment(db, index) else {
                continue;
            };
            let args = segment.generic_args(db);
            if index + 1 != path.len(db) {
                self.walk_generic_args(args);
                continue;
            }
            for (arg_index, arg) in args.data(db).iter().enumerate() {
                let previous = self
                    .signature_site
                    .replace(QuoteSignatureSite::CallTypeArgument {
                        expr,
                        index: arg_index as u32,
                    });
                match arg {
                    GenericArg::Type(arg) => self.walk_ty_opt(arg.ty),
                    GenericArg::AssocType(arg) => self.walk_ty_opt(arg.ty),
                    GenericArg::Const(_) => {}
                }
                self.signature_site = previous;
            }
        }
        if let Some(name) = path.as_ident(db) {
            if name.is_self(db) {
                return self.push(site, QuoteNameClass::SelfName);
            }
            if let Some(binder) = self.lookup_value(name) {
                return self.push(site, QuoteNameClass::Bound(binder));
            }
            if let Some(index) = self.open_index(name) {
                return self.push(site, QuoteNameClass::Open(index));
            }
            // Provider locals live only in the checker's environment.
            let span = self.span_at(site);
            let minter = LoweringContext::new(HoleAnchor::BodySyntax {
                body: self.tc.body(),
                site: BodyHoleSite::Expr(self.quote_expr),
            });
            match resolve_ident_expr(db, &self.tc.env, path, span.clone(), &minter) {
                ResolvedPathInBody::Binding(_) => {
                    return self.tc.push_diag(BodyDiag::QuoteProviderLocal {
                        primary: span,
                        name,
                    });
                }
                ResolvedPathInBody::Reso(PathRes::FuncParam(..)) => {
                    return self.tc.push_diag(BodyDiag::QuoteProviderLocal {
                        primary: span,
                        name,
                    });
                }
                _ => {}
            }
        }
        self.record_path(site, path);
    }

    /// A type or trait path; its site is its position in visiting order.
    fn type_path(&mut self, path: PathId<'db>) {
        let site = QuoteNameSite::Type(self.type_sites);
        self.type_sites += 1;
        self.walk_path_args(path);
        self.record_path(site, path);
    }

    fn impl_trait_path(&mut self, path: PathId<'db>) {
        let db = self.db();
        let site = QuoteNameSite::Type(self.type_sites);
        self.type_sites += 1;
        for index in 0..path.len(db) {
            if let Some(segment) = path.segment(db, index) {
                if index + 1 == path.len(db) {
                    for (arg_index, arg) in segment.generic_args(db).data(db).iter().enumerate() {
                        if let GenericArg::Type(arg) = arg {
                            let previous = self.signature_site;
                            self.signature_site = Some(QuoteSignatureSite::ImplTraitArgument {
                                index: arg_index as u32,
                            });
                            self.walk_ty_opt(arg.ty);
                            self.signature_site = previous;
                        }
                    }
                } else {
                    self.walk_generic_args(segment.generic_args(db));
                }
            }
        }
        self.record_path(site, path);
    }

    /// Classifies a path whose root is not a value binder of the fragment.
    fn record_path(&mut self, site: QuoteNameSite, path: PathId<'db>) {
        let db = self.db();
        let root = path.segment(db, 0).unwrap_or(path);
        let root_name = match root.kind(db) {
            PathKind::Ident { ident, .. } => ident.to_opt(),
            PathKind::QualifiedType { type_, trait_ } => {
                self.walk_ty(type_);
                if let Partial::Present(trait_path) = trait_.path(db) {
                    self.type_path(trait_path);
                }
                return self.push(site, QuoteNameClass::Qualified);
            }
            PathKind::BoundIngot { .. } => None,
        };
        if let Some(name) = root_name {
            if name.is_self(db) || name.is_self_ty(db) {
                return self.push(site, QuoteNameClass::SelfName);
            }
            if let Some(binder) = self.lookup_type(name) {
                return self.push(site, QuoteNameClass::Bound(binder));
            }
            if path.len(db) == 1 {
                if let Some(binder) = self.lookup_value(name) {
                    return self.push(site, QuoteNameClass::Bound(binder));
                }
                if let Some(index) = self.open_index(name) {
                    return self.push(site, QuoteNameClass::Open(index));
                }
            }
        }
        let tail_as_value = !matches!(site, QuoteNameSite::Type(_));
        if let Some(name) = root_name
            && let Some(root_res) =
                self.resolve_in_provider(root, path.len(db) == 1 && tail_as_value)
            && is_generic_param(db, &root_res)
        {
            let span = self.span_at(site);
            return self.tc.push_diag(BodyDiag::QuoteProviderGeneric {
                primary: span,
                name: name.data(db).clone(),
            });
        }
        match self.resolve_in_provider(path, tail_as_value) {
            Some(res) => self.push(site, QuoteNameClass::Rooted(res)),
            None => {
                let span = self.span_at(site);
                self.tc.push_diag(BodyDiag::QuoteUnresolvedName {
                    primary: span,
                    name: path.strip_generic_args(db).pretty_print(db),
                });
            }
        }
    }
}

fn is_generic_param<'db>(db: &'db dyn crate::analysis::HirAnalysisDb, res: &PathRes<'db>) -> bool {
    match res {
        PathRes::Ty(ty) | PathRes::TyAlias(_, ty) => {
            let (base, _) = ty.decompose_ty_app(db);
            match base.data(db) {
                TyData::TyParam(_) => true,
                TyData::ConstTy(const_ty) => matches!(
                    const_ty.data(db),
                    crate::analysis::ty::const_ty::ConstTyData::TyParam(..)
                ),
                _ => false,
            }
        }
        _ => false,
    }
}
