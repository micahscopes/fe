//! Output HIR is built from the original checked quote syntax, never from an
//! emitted source string. Provider evaluation has completed before this query.
use super::FileLowerCtxt;
use crate::{
    hir_def::{
        AttrListId, Func, GenericArgListId, GenericParamListId, IdentId, ImplTrait,
        ImplTraitModifiers, Partial, PathId, PathKind, TrackedItemVariant, TraitRefId, TypeId,
        TypeKind, Visibility, WhereClauseId,
    },
    span::HirOrigin,
};
use parser::ast::{self, prelude::*};

fn path_from_segments<'db>(db: &'db dyn crate::HirDb, segments: &[String]) -> PathId<'db> {
    let mut path: Option<PathId<'db>> = None;
    for segment in segments {
        let ident = IdentId::new(db, segment.clone());
        path = Some(match path {
            Some(prev) => prev.push_ident(db, ident),
            None => PathId::from_ident(db, ident),
        });
    }
    path.expect("scoped impl path must have at least one segment")
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
        .filter(|plan| plan.target_file == url && plan.request_impl.is_none())
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
            .zip(plan.holes)
            .collect();
        ctxt.generation_holes = holes;
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
        if let Some(scoped) = plan.scoped_impl {
            let trait_path = path_from_segments(db, &scoped.trait_path);
            let trait_ref = TraitRefId::new(db, Partial::Present(trait_path));
            let target_path = path_from_segments(db, &scoped.target_path);
            let ty = TypeId::new(
                db,
                TypeKind::Path(Partial::Present(PathId::new(
                    db,
                    PathKind::Ident {
                        ident: Partial::Present(target_path.ident(db).unwrap()),
                        generic_args: GenericArgListId::none(db),
                    },
                    target_path.parent(db),
                ))),
            );
            let selection_name = Some(IdentId::new(db, scoped.selection_name));
            let idx = ctxt.next_impl_trait_idx();
            let id = ctxt.joined_id(TrackedItemVariant::ImplTrait(idx));
            ctxt.enter_item_scope(id, false);
            super::item::validate_func_attrs(
                ctxt,
                function.attr_list(),
                "fn",
                function.sig().name().map(|name| name.text().to_string()),
                true,
            );
            let generated = Func::lower_ast(ctxt, function);
            ctxt.generation_holes.clear();
            let impl_trait = ImplTrait::new(
                db,
                id,
                Partial::Present(trait_ref),
                Partial::Present(ty),
                AttrListId::new(db, vec![]),
                GenericParamListId::new(db, vec![]),
                WhereClauseId::new(db, vec![]),
                vec![],
                vec![],
                vec![],
                ImplTraitModifiers {
                    is_unsafe: false,
                    selection_name,
                    vis: Visibility::Public,
                },
                ctxt.top_mod(),
                HirOrigin::synthetic(),
            );
            ctxt.leave_item_scope(impl_trait);
            ctxt.builder.set_quoted_function_origin(
                generated,
                plan.producer_module,
                plan.producer_block,
            );
        } else {
            super::item::validate_func_attrs(
                ctxt,
                function.attr_list(),
                "fn",
                function.sig().name().map(|name| name.text().to_string()),
                true,
            );
            let generated = Func::lower_ast(ctxt, function);
            ctxt.generation_holes.clear();
            ctxt.builder.set_quoted_function_origin(
                generated,
                plan.producer_module,
                plan.producer_block,
            );
        }
    }
}
