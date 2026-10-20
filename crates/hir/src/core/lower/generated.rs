//! Output HIR is built from the original checked quote syntax, never from an
//! emitted source string. Provider evaluation has completed before this query.
use super::FileLowerCtxt;
use crate::hir_def::{Func, Impl, ImplTrait, ItemKind, TopLevelMod};
use common::generation::{FrozenCode, FrozenCodeKind, FrozenHoleValue};
use parser::ast::{self, prelude::*};

#[salsa::accumulator]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenerationLoweringError {
    pub file: common::file::File,
    pub primary_range: parser::TextRange,
    pub message: String,
}

pub(crate) struct ActiveQuoteFrame<'db> {
    pub(super) quote_top_mod: TopLevelMod<'db>,
    pub(super) holes: Vec<(parser::TextRange, FrozenHoleValue)>,
}

pub(super) fn append_functions<'db>(ctxt: &mut FileLowerCtxt<'db>) {
    let db = ctxt.db();
    let top_mod = ctxt.top_mod();
    let Some(url) = top_mod.file(db).url(db) else {
        return;
    };
    let plans = db
        .generation_inputs()
        .functions(db)
        .iter()
        .filter(|plan| plan.target_file == url)
        .cloned()
        .collect::<Vec<_>>();
    let root = super::top_mod_ast(db, top_mod);
    for plan in plans {
        let quote = root
            .syntax()
            .descendants()
            .filter_map(ast::QuoteExpr::cast)
            .find(|quote| quote.syntax().text_range() == plan.quote_range)
            .expect("validated frozen quote belongs to unchanged input source");
        let holes = crate::core::meta::quote_holes(&quote)
            .expect("checked frozen quote retains valid bounded syntax")
            .into_iter()
            .map(|hole| hole.syntax().text_range())
            .zip(plan.holes.into_iter().map(FrozenHoleValue::Scalar))
            .collect();
        ctxt.quote_frames.push(ActiveQuoteFrame {
            quote_top_mod: top_mod,
            holes,
        });
        let function = if let Some(items) = quote.item_list() {
            match items.into_iter().next().unwrap().kind().unwrap() {
                ast::ItemKind::Func(function) => function,
                _ => unreachable!("validated one-function template"),
            }
        } else {
            let ast::ItemKind::Func(function) = quote
                .body()
                .unwrap()
                .items()
                .next()
                .unwrap()
                .kind()
                .unwrap()
            else {
                unreachable!("validated one-function template")
            };
            function
        };
        let generated = Func::lower_ast(ctxt, function);
        ctxt.quote_frames.pop();
        ctxt.builder.set_quoted_function_origin(
            generated,
            plan.producer_module,
            plan.producer_block,
        );
    }

    let requests = db
        .generation_inputs()
        .requests(db)
        .iter()
        .filter(|request| request.target_file == url)
        .cloned()
        .collect::<Vec<_>>();
    for request in requests {
        lower_generated_item_code(ctxt, &request.code);
    }
}

fn lower_quoted_ast_item<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    item: ast::Item,
) -> Option<ItemKind<'db>> {
    match item.kind()? {
        ast::ItemKind::Func(func) => Some(Func::lower_ast(ctxt, func).into()),
        ast::ItemKind::Impl(impl_) => Some(Impl::lower_ast(ctxt, impl_).into()),
        ast::ItemKind::ImplTrait(impl_trait) => Some(ImplTrait::lower_ast(ctxt, impl_trait).into()),
        _ => None,
    }
}

pub(super) fn lower_generated_item_code<'db>(ctxt: &mut FileLowerCtxt<'db>, code: &FrozenCode) {
    match code {
        FrozenCode::Empty { .. } => {}
        FrozenCode::Template {
            kind: FrozenCodeKind::Item,
            quote_file,
            quote_range,
            producer_module,
            producer_block,
            holes,
        } => {
            let (quote, frame) =
                ctxt.resolve_quote_template(quote_file, *quote_range, holes.clone());
            let quote_top_mod = frame.quote_top_mod;
            ctxt.quote_frames.push(frame);
            if let Some(item_list) = quote.item_list() {
                for child in item_list.syntax().children() {
                    if let Some(item) = ast::Item::cast(child.clone()) {
                        if let Some(lowered) = lower_quoted_ast_item(ctxt, item) {
                            ctxt.builder.set_quoted_item_origin(
                                lowered,
                                quote_top_mod,
                                *producer_module,
                                *producer_block,
                            );
                        }
                    } else if let Some(hole) = ast::QuoteHole::cast(child)
                        && let Some(FrozenHoleValue::Code(nested)) =
                            ctxt.current_hole(hole.syntax().text_range())
                    {
                        lower_generated_item_code(ctxt, &nested);
                    }
                }
            } else if let Some(block) = quote.body() {
                for item in block.items() {
                    if let Some(lowered) = lower_quoted_ast_item(ctxt, item) {
                        ctxt.builder.set_quoted_item_origin(
                            lowered,
                            quote_top_mod,
                            *producer_module,
                            *producer_block,
                        );
                    }
                }
                for stmt in block.stmts() {
                    if let ast::StmtKind::Expr(expr_stmt) = stmt.kind()
                        && let Some(expr) = expr_stmt.expr()
                        && let ast::ExprKind::QuoteHole(hole) = expr.kind()
                        && let Some(FrozenHoleValue::Code(nested)) =
                            ctxt.current_hole(hole.syntax().text_range())
                    {
                        lower_generated_item_code(ctxt, &nested);
                    }
                }
            }
            ctxt.quote_frames.pop();
        }
        _ => {}
    }
}
