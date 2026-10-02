//! Lowering of `quote { .. }` into a template body plus numbered splices.

use parser::ast::{self, AttrListOwner, GenericParamsOwner, WhereClauseOwner, prelude::*};

use super::{FileLowerCtxt, body::BodyCtxt};
use crate::hir_def::{
    AttrListId, BodyKind, Expr, ExprId, FuncModifiers, FuncParamListId, GenericParamListId,
    IdentId, Quote, QuoteAssocType, QuoteBlockItem, QuoteConst, QuoteFragment, QuoteFunc,
    QuoteHole, QuoteHolePosition, QuoteImpl, QuoteItem, QuoteName, QuoteWhereClause,
    QuoteWherePredicate, TrackedItemVariant, TraitRefId, TypeId, WherePredicate,
};

/// A splice found while lowering a template. Its expression is lowered into
/// the quote's owner body once the template is complete.
pub(super) struct PendingQuoteHole {
    position: QuoteHolePosition,
    expr: Option<ast::Expr>,
}

impl<'db> FileLowerCtxt<'db> {
    /// Numbers a splice of the template being lowered. `None` outside a
    /// template, where the parser has already reported the splice.
    pub(super) fn quote_hole(
        &mut self,
        position: QuoteHolePosition,
        expr: Option<ast::Expr>,
    ) -> Option<u32> {
        let holes = self.quote_holes.as_mut()?;
        holes.push(PendingQuoteHole { position, expr });
        Some(holes.len() as u32 - 1)
    }

    pub(super) fn in_quote_template(&self) -> bool {
        self.quote_holes.is_some()
    }
}

/// Lowers `quote(..) { .. }` into its template body. The splices' expressions
/// are lowered into `ctxt`, the owner body, in template order.
pub(super) fn lower_quote<'db>(ctxt: &mut BodyCtxt<'_, 'db>, ast: &ast::QuoteExpr) -> Quote<'db> {
    let open = ast
        .open_names()
        .map(|list| {
            list.names()
                .map(|name| IdentId::lower_token(ctxt.f_ctxt, name))
                .collect()
        })
        .unwrap_or_default();

    let index = ctxt.next_quote_index();
    let id = ctxt
        .f_ctxt
        .joined_id(TrackedItemVariant::QuoteTemplate(index));
    let outer_holes = ctxt.f_ctxt.quote_holes.replace(Vec::new());

    let mut template = BodyCtxt::new(ctxt.f_ctxt, id);
    template.quote_block_items = Some(Vec::new());
    let (root, fragment) = if let Some(items) = ast.items() {
        let items = items
            .entries()
            .filter_map(|entry| lower_item_entry(&mut template, entry))
            .collect();
        (template.push_missing_expr(), QuoteFragment::Items(items))
    } else if let Some(arms) = ast.arms() {
        let arms = arms
            .into_iter()
            .map(|arm| crate::hir_def::MatchArm::lower_ast(&mut template, arm))
            .collect();
        (template.push_missing_expr(), QuoteFragment::Arms(arms))
    } else if let Some(predicates) = ast.where_predicates() {
        let clause = lower_where_clause(&mut template, predicates.where_clause());
        (template.push_missing_expr(), QuoteFragment::Where(clause))
    } else {
        let body = ast
            .body()
            .and_then(|body| ast::Expr::cast(body.syntax().clone()));
        (
            Expr::push_to_body_opt(&mut template, body),
            QuoteFragment::Expr,
        )
    };
    let block_items = template.quote_block_items.take().unwrap_or_default();
    let origin = ast::Expr::cast(ast.syntax().clone());
    let template = template.build(origin.as_ref(), root, BodyKind::QuoteTemplate);

    let pending = std::mem::replace(&mut ctxt.f_ctxt.quote_holes, outer_holes).unwrap_or_default();
    let holes = pending
        .into_iter()
        .map(|hole| QuoteHole {
            position: hole.position,
            expr: Expr::push_to_body_opt(ctxt, hole.expr),
        })
        .collect();

    Quote {
        open,
        template,
        fragment,
        block_items,
        holes,
    }
}

impl<'db> BodyCtxt<'_, 'db> {
    /// An item declared in a block. In a template it is a quoted item, never
    /// a module item.
    pub(super) fn lower_block_item(&mut self, block: ExprId, item: ast::Item) {
        let Some(mut items) = self.quote_block_items.take() else {
            crate::hir_def::ItemKind::lower_ast(self.f_ctxt, item);
            return;
        };
        if let Some(item) = lower_item(self, item) {
            items.push(QuoteBlockItem { block, item });
        }
        self.quote_block_items = Some(items);
    }
}

fn lower_item_entry<'db>(
    ctxt: &mut BodyCtxt<'_, 'db>,
    entry: ast::QuoteItemEntry,
) -> Option<QuoteItem<'db>> {
    match entry {
        ast::QuoteItemEntry::Item(item) => lower_item(ctxt, item),
        ast::QuoteItemEntry::Hole(hole) => items_hole(ctxt.f_ctxt, hole),
    }
}

fn items_hole<'db>(ctxt: &mut FileLowerCtxt<'db>, hole: ast::QuoteHole) -> Option<QuoteItem<'db>> {
    ctxt.quote_hole(QuoteHolePosition::Items, hole.expr())
        .map(QuoteItem::Hole)
}

/// Functions, constants and impls can be quoted; the parser reports others.
fn lower_item<'db>(ctxt: &mut BodyCtxt<'_, 'db>, item: ast::Item) -> Option<QuoteItem<'db>> {
    Some(match item.kind()? {
        ast::ItemKind::Func(func) => QuoteItem::Func(lower_func(ctxt, func)),
        ast::ItemKind::Const(const_) => {
            let name = lower_name(ctxt.f_ctxt, const_.name(), const_.name_hole());
            QuoteItem::Const(QuoteConst {
                attrs: AttrListId::lower_ast_opt(ctxt.f_ctxt, const_.attr_list()),
                name,
                ty: TypeId::lower_ast_partial(ctxt.f_ctxt, const_.ty()),
                value: const_.value().map(|value| Expr::lower_ast(ctxt, value)),
                vis: super::lower_visibility(&const_),
            })
        }
        ast::ItemKind::Impl(impl_) => {
            let items = impl_
                .item_list()
                .map(|list| lower_member_list(ctxt, list.syntax()))
                .unwrap_or_default();
            QuoteItem::Impl(QuoteImpl {
                attrs: AttrListId::lower_ast_opt(ctxt.f_ctxt, impl_.attr_list()),
                generic_params: GenericParamListId::lower_ast_opt(
                    ctxt.f_ctxt,
                    impl_.generic_params(),
                ),
                trait_ref: None,
                ty: TypeId::lower_ast_partial(ctxt.f_ctxt, impl_.ty()),
                selection_name: None,
                where_clause: lower_where_clause(ctxt, impl_.where_clause()),
                items,
            })
        }
        ast::ItemKind::ImplTrait(impl_) => {
            let items = impl_
                .item_list()
                .map(|list| lower_member_list(ctxt, list.syntax()))
                .unwrap_or_default();
            QuoteItem::Impl(QuoteImpl {
                attrs: AttrListId::lower_ast_opt(ctxt.f_ctxt, impl_.attr_list()),
                generic_params: GenericParamListId::lower_ast_opt(
                    ctxt.f_ctxt,
                    impl_.generic_params(),
                ),
                trait_ref: Some(TraitRefId::lower_ast_partial(
                    ctxt.f_ctxt,
                    impl_.trait_ref(),
                )),
                ty: TypeId::lower_ast_partial(ctxt.f_ctxt, impl_.ty()),
                selection_name: IdentId::lower_token_partial(ctxt.f_ctxt, impl_.selection_name())
                    .to_opt(),
                where_clause: lower_where_clause(ctxt, impl_.where_clause()),
                items,
            })
        }
        _ => return None,
    })
}

/// The members of a quoted impl, including `${items}` splices.
fn lower_member_list<'db>(
    ctxt: &mut BodyCtxt<'_, 'db>,
    list: &parser::SyntaxNode,
) -> Vec<QuoteItem<'db>> {
    list.children()
        .filter_map(|node| {
            if let Some(hole) = ast::QuoteHole::cast(node.clone()) {
                items_hole(ctxt.f_ctxt, hole)
            } else if let Some(func) = ast::Func::cast(node.clone()) {
                Some(QuoteItem::Func(lower_func(ctxt, func)))
            } else if let Some(const_) = ast::TraitConstItem::cast(node.clone()) {
                let name = lower_name(ctxt.f_ctxt, const_.name(), const_.name_hole());
                Some(QuoteItem::Const(QuoteConst {
                    attrs: AttrListId::lower_ast_opt(ctxt.f_ctxt, const_.attr_list()),
                    name,
                    ty: TypeId::lower_ast_partial(ctxt.f_ctxt, const_.ty()),
                    value: const_.value().map(|value| Expr::lower_ast(ctxt, value)),
                    vis: super::lower_visibility(&const_),
                }))
            } else {
                ast::TraitTypeItem::cast(node).map(|ty| {
                    QuoteItem::Type(QuoteAssocType {
                        name: IdentId::lower_token_partial(ctxt.f_ctxt, ty.name()),
                        ty: ty.ty().map(|ty| TypeId::lower_ast(ctxt.f_ctxt, ty)),
                    })
                })
            }
        })
        .collect()
}

fn lower_func<'db>(ctxt: &mut BodyCtxt<'_, 'db>, func: ast::Func) -> QuoteFunc<'db> {
    let sig = func.sig();
    let name = lower_name(ctxt.f_ctxt, sig.name(), sig.name_hole());
    let attrs = AttrListId::lower_ast_opt(ctxt.f_ctxt, func.attr_list());
    let generic_params = GenericParamListId::lower_ast_opt(ctxt.f_ctxt, sig.generic_params());
    let where_clause = lower_where_clause(ctxt, sig.where_clause());
    let params = sig
        .params()
        .map(|params| FuncParamListId::lower_ast(ctxt.f_ctxt, params))
        .into();
    let effects = super::item::lower_uses_clause_opt(ctxt.f_ctxt, sig.uses_clause());
    let ret_ty = sig.ret_ty().map(|ty| TypeId::lower_ast(ctxt.f_ctxt, ty));
    let modifiers = FuncModifiers::new(
        super::lower_visibility(&func),
        super::lower_is_unsafe(&func),
        func.const_kw().is_some(),
        false,
    );
    let body = func
        .body()
        .and_then(|body| ast::Expr::cast(body.syntax().clone()))
        .map(|body| Expr::lower_ast(ctxt, body));
    QuoteFunc {
        name,
        attrs,
        generic_params,
        where_clause,
        params,
        effects,
        ret_ty,
        modifiers,
        body,
    }
}

fn lower_name<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    name: Option<parser::SyntaxToken>,
    hole: Option<ast::QuoteHole>,
) -> QuoteName<'db> {
    if let Some(hole) = hole
        && let Some(slot) = ctxt.quote_hole(QuoteHolePosition::Name, hole.expr())
    {
        return QuoteName::Hole(slot);
    }
    QuoteName::Ident(IdentId::lower_token_partial(ctxt, name))
}

fn lower_where_clause<'db>(
    ctxt: &mut BodyCtxt<'_, 'db>,
    clause: Option<ast::WhereClause>,
) -> QuoteWhereClause<'db> {
    let Some(clause) = clause else {
        return QuoteWhereClause::default();
    };
    let predicates = clause
        .syntax()
        .children()
        .filter_map(|node| {
            if let Some(predicate) = ast::WherePredicate::cast(node.clone()) {
                Some(QuoteWherePredicate::Bound(WherePredicate::lower_ast(
                    ctxt.f_ctxt,
                    predicate,
                )))
            } else if let Some(predicate) = ast::WhereConstPredicate::cast(node.clone()) {
                Some(QuoteWherePredicate::Const(Expr::push_to_body_opt(
                    ctxt,
                    predicate.expr(),
                )))
            } else {
                let hole = ast::QuoteHole::cast(node)?;
                ctxt.f_ctxt
                    .quote_hole(QuoteHolePosition::Where, hole.expr())
                    .map(QuoteWherePredicate::Hole)
            }
        })
        .collect();
    QuoteWhereClause { predicates }
}

/// `${e}` in expression position.
pub(super) fn lower_expr_hole(
    ctxt: &mut BodyCtxt<'_, '_>,
    hole: &ast::QuoteHoleExpr,
) -> Option<u32> {
    ctxt.f_ctxt.quote_hole(QuoteHolePosition::Expr, hole.expr())
}

/// `x.${f}`.
pub(super) fn lower_member_hole(ctxt: &mut BodyCtxt<'_, '_>, hole: &ast::QuoteHole) -> Option<u32> {
    ctxt.f_ctxt
        .quote_hole(QuoteHolePosition::Member, hole.expr())
}

/// `Record { ${f}: value }`.
pub(super) fn lower_field_label_hole(
    ctxt: &mut BodyCtxt<'_, '_>,
    hole: &ast::QuoteHole,
) -> Option<u32> {
    ctxt.f_ctxt
        .quote_hole(QuoteHolePosition::FieldLabel, hole.expr())
}

/// `${t}` in type position.
pub(super) fn lower_type_hole(
    ctxt: &mut FileLowerCtxt<'_>,
    hole: &ast::QuoteHoleType,
) -> Option<u32> {
    ctxt.quote_hole(QuoteHolePosition::Type, hole.expr())
}
