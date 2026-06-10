//! Post-lowering expansion stage.
//!
//! Base lowering ([`base_scope_graph_impl`]) turns the AST into HIR items and
//! the base scope graph. This module runs *after* that, synthesizing
//! additional HIR items from the lowered ones — currently the
//! compiler-internal `#[derive(Eq, Default)]` impls — and producing a partial
//! scope graph that [`scope_graph_impl`](super::scope_graph_impl) merges into
//! the base graph. Downstream consumers (name resolution, `all_items`,
//! type checking, MIR) therefore see generated items exactly like
//! hand-written ones, with no special casing.
//!
//! Stratification: this stage may only depend on the *base* scope graph (and
//! analyses over base items); it must never read the merged
//! [`scope_graph_impl`](super::scope_graph_impl) query, which would form a
//! cycle. Generated items never feed back into generation: derive targets
//! are discovered only among base items.

use common::indexmap::IndexMap;
use parser::ast::{self, prelude::*};
use salsa::Update;

use super::{FileLowerCtxt, base_scope_graph_impl, derive, top_mod_ast};
use crate::{
    HirDb, LowerHirDb,
    hir_def::{
        Enum, ItemKind, Struct, TopLevelMod,
        scope_graph::{EdgeKind, ScopeGraph, ScopeId},
    },
    span::{DesugaredOrigin, HirOrigin},
};

/// The result of the post-lowering expansion stage for one top-level module.
#[derive(Debug, Clone, PartialEq, Eq, Update)]
pub struct ExpandedItems<'db> {
    /// The generated top-level items (currently `impl Trait` items derived
    /// via `#[derive(..)]`), in deterministic base-DFS order.
    pub items: Vec<ItemKind<'db>>,
    /// Partial scope graph holding the generated items' scopes. Each
    /// generated item hangs off a shim node that mirrors an existing scope
    /// of the base graph (the lexical parent of the item it was derived
    /// from); merging unions the shim's edges into the base scope.
    pub(crate) graph: ScopeGraph<'db>,
}

/// Returns the HIR items synthesized for `top_mod` by the post-lowering
/// expansion stage, in deterministic order.
pub fn generated_hir_items<'db>(
    db: &'db dyn LowerHirDb,
    top_mod: TopLevelMod<'db>,
) -> &'db [ItemKind<'db>] {
    &expanded_items_impl(db, top_mod).items
}

/// A `#[derive(..)]`-annotated base item to expand, paired with its original
/// AST node (recovered through the item's `HirOrigin`).
enum DeriveTarget<'db> {
    Struct(Struct<'db>, ast::Struct),
    Enum(Enum<'db>, ast::Enum),
}

/// Runs the expansion stage for `top_mod`: walks the *base* scope graph for
/// `#[derive(..)]` targets and synthesizes their trait impls as real HIR
/// items. Derive diagnostics ([`DeriveError`](super::DeriveError) and
/// `#[default]` attribute misuse) are accumulated during this query's
/// execution.
#[salsa::tracked(return_ref)]
pub(crate) fn expanded_items_impl<'db>(
    db: &'db dyn HirDb,
    top_mod: TopLevelMod<'db>,
) -> ExpandedItems<'db> {
    let base = base_scope_graph_impl(db, top_mod);
    let root = top_mod_ast(db, top_mod).syntax().clone();

    let mut ctxt = FileLowerCtxt::enter_expansion(db, top_mod);

    // Collect derive targets in base-DFS order, grouped by the lexical parent
    // scope of the annotated item so the generated impls become siblings of
    // it (resolving unqualified references to the item and matching its
    // visibility context), exactly as when they were generated in place
    // during lowering.
    let mut groups: IndexMap<ScopeId<'db>, Vec<DeriveTarget<'db>>> = IndexMap::new();
    for item in base.items_dfs(db) {
        let target = match item {
            ItemKind::Struct(struct_) => match struct_.origin(db) {
                HirOrigin::Raw(ptr) => {
                    let Some(ast) = ptr
                        .syntax_node_ptr()
                        .try_to_node(&root)
                        .and_then(ast::Struct::cast)
                    else {
                        continue;
                    };
                    if !derive::has_derive_attr(&ast) {
                        continue;
                    }
                    DeriveTarget::Struct(struct_, ast)
                }
                // `#[event]` / `#[error]` structs cannot be derive targets;
                // report `#[derive(..)]` on them instead of expanding.
                HirOrigin::Desugared(
                    DesugaredOrigin::Event(_) | DesugaredOrigin::Error(_),
                ) => {
                    report_derive_on_desugared_struct(&mut ctxt, struct_, &root);
                    continue;
                }
                _ => continue,
            },
            ItemKind::Enum(enum_) => {
                let HirOrigin::Raw(ptr) = enum_.origin(db) else {
                    continue;
                };
                let Some(ast) = ptr
                    .syntax_node_ptr()
                    .try_to_node(&root)
                    .and_then(ast::Enum::cast)
                else {
                    continue;
                };
                if !derive::enum_has_derive_attr(&ast) {
                    continue;
                }
                DeriveTarget::Enum(enum_, ast)
            }
            _ => continue,
        };

        let Some(parent) = lex_parent(base, item) else {
            continue;
        };
        groups.entry(parent).or_default().push(target);
    }

    let mut items = Vec::new();
    if groups.is_empty() {
        // Avoid building (and later merging) an empty graph.
        let graph = ctxt.build();
        debug_assert!(graph.scopes.is_empty());
        return ExpandedItems { items, graph };
    }

    for (&parent, targets) in &groups {
        let vis = base.scope_data(&parent).vis;
        ctxt.enter_shim_scope(parent, vis);
        for target in targets {
            match target {
                DeriveTarget::Struct(struct_, ast) => {
                    derive::lower_derive_impls(&mut ctxt, ast, *struct_);
                }
                DeriveTarget::Enum(enum_, ast) => {
                    derive::lower_enum_derive_impls(&mut ctxt, ast, *enum_);
                }
            }
        }
        ctxt.leave_shim_scope();
    }

    let graph = ctxt.build();
    for parent in groups.keys() {
        items.extend(graph.child_items(*parent));
    }

    ExpandedItems { items, graph }
}

/// Reports the `#[derive(..)]` attributes of an `#[event]` / `#[error]`
/// struct (which cannot be derive targets). The original AST is recovered
/// from the struct's desugared origin.
fn report_derive_on_desugared_struct<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    struct_: Struct<'db>,
    root: &parser::SyntaxNode,
) {
    let (HirOrigin::Desugared(DesugaredOrigin::Event(crate::span::EventDesugared {
        event_struct: ptr,
    }))
    | HirOrigin::Desugared(DesugaredOrigin::Error(crate::span::ErrorDesugared {
        error_struct: ptr,
    }))) = struct_.origin(ctxt.db())
    else {
        return;
    };
    let Some(ast) = ptr
        .syntax_node_ptr()
        .try_to_node(root)
        .and_then(ast::Struct::cast)
    else {
        return;
    };
    if derive::has_derive_attr(&ast) {
        derive::report_derive_on_event_or_error_struct(ctxt, &ast);
    }
}

/// The lexical parent scope of `item` in `graph`.
fn lex_parent<'db>(graph: &ScopeGraph<'db>, item: ItemKind<'db>) -> Option<ScopeId<'db>> {
    graph
        .edges(ScopeId::Item(item))
        .iter()
        .find_map(|edge| matches!(edge.kind, EdgeKind::Lex(_)).then_some(edge.dest))
}

#[cfg(test)]
mod tests {
    use crate::{
        hir_def::ItemKind,
        lower::{base_scope_graph_impl, generated_hir_items, map_file_to_mod, scope_graph},
        test_db::TestDb,
    };

    /// `#[derive(..)]` impls are synthesized by the post-lowering expansion
    /// stage: absent from the base scope graph, listed by
    /// [`generated_hir_items`], and present in the merged scope graph like
    /// ordinary items — including for items nested in inner modules.
    #[test]
    fn derive_impls_synthesized_in_expansion_stage() {
        let mut db = TestDb::default();
        let text = r#"
            #[derive(Eq, Default)]
            struct Point {
                x: u256,
                y: u256,
            }

            mod inner {
                #[derive(Eq)]
                pub enum Mode {
                    On,
                    Off,
                }
            }
        "#;
        let file = db.standalone_file(text);
        let top_mod = map_file_to_mod(&db, file);

        // Base lowering no longer synthesizes any derive impls.
        let base = base_scope_graph_impl(&db, top_mod);
        assert!(
            base.items_dfs(&db)
                .all(|item| !matches!(item, ItemKind::ImplTrait(_)))
        );

        // The expansion stage generates one impl per derived trait.
        let generated = generated_hir_items(&db, top_mod);
        assert_eq!(generated.len(), 3);
        assert!(
            generated
                .iter()
                .all(|item| matches!(item, ItemKind::ImplTrait(_)))
        );

        // The merged scope graph exposes the generated impls and their
        // methods (`eq`, `eq`, `default`) like ordinary items.
        let merged = scope_graph(&db, top_mod);
        let impls = merged
            .items_dfs(&db)
            .filter(|item| matches!(item, ItemKind::ImplTrait(_)))
            .count();
        assert_eq!(impls, 3);
        let funcs = merged
            .items_dfs(&db)
            .filter(|item| matches!(item, ItemKind::Func(_)))
            .count();
        assert_eq!(funcs, 3);
    }
}
