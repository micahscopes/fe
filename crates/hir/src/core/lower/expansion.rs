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

use common::indexmap::{IndexMap, IndexSet};
use parser::ast::{self, prelude::*};
use salsa::Update;

use super::{
    FileLowerCtxt, base_scope_graph_impl,
    derive::{self, DerivableTrait, DeriveErrorKind, ProviderConstruct, accumulate_error},
    top_mod_ast,
};
use crate::{
    HirDb, LowerHirDb,
    hir_def::{
        DeriveDecl, Enum, IdentId, ItemKind, PathId, Struct, TopLevelMod,
        scope_graph::{EdgeKind, ScopeGraph, ScopeId},
    },
    span::{DeriveDesugared, DesugaredOrigin, HirOrigin},
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
    /// A standalone `derive Trait for Type` declaration resolved to its
    /// struct target. The generated impl carries a [`DeriveDesugared::Decl`]
    /// origin pointing at the declaration.
    DeclStruct(Struct<'db>, ast::Struct, DerivableTrait, ast::DeriveDecl),
    /// Like [`Self::DeclStruct`], for enum targets.
    DeclEnum(Enum<'db>, ast::Enum, DerivableTrait, ast::DeriveDecl),
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
    // Every `(target item, trait)` pair scheduled for generation, used to
    // diagnose conflicts between `#[derive(..)]` attributes and standalone
    // `derive` declarations (and between identical declarations).
    let mut derived_pairs: IndexSet<(ItemKind<'db>, DerivableTrait)> = IndexSet::new();
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
                    for trait_ in derive::attr_derive_traits_silent(ast.attr_list()) {
                        derived_pairs.insert((item, trait_));
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
                for trait_ in derive::attr_derive_traits_silent(ast.attr_list()) {
                    derived_pairs.insert((item, trait_));
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

    // Second walk: standalone `derive Trait for Type` declarations
    // contribute further targets, and the provider-related forms (provider
    // definitions, `with ..` scopes, `using ..` selections) are diagnosed as
    // not yet executable — they parse and lower, but must never be silently
    // inert.
    for item in base.items_dfs(db) {
        match item {
            ItemKind::DeriveProvider(provider) => {
                let HirOrigin::Raw(ptr) = provider.origin(db) else {
                    continue;
                };
                let Some(ast) = ptr
                    .syntax_node_ptr()
                    .try_to_node(&root)
                    .and_then(ast::DeriveProvider::cast)
                else {
                    continue;
                };
                let range = ast
                    .name()
                    .map(|name| name.text_range())
                    .unwrap_or_else(|| ast.syntax().text_range());
                let item_name = provider
                    .name(db)
                    .to_opt()
                    .map(|name| name.data(db).to_string());
                accumulate_error(
                    &mut ctxt,
                    &item_name,
                    DeriveErrorKind::ProviderNotExecutable {
                        construct: ProviderConstruct::Definition,
                    },
                    range,
                );
            }
            ItemKind::DeriveProviderScope(scope) => {
                let HirOrigin::Raw(ptr) = scope.origin(db) else {
                    continue;
                };
                let Some(ast) = ptr
                    .syntax_node_ptr()
                    .try_to_node(&root)
                    .and_then(ast::DeriveProviderScope::cast)
                else {
                    continue;
                };
                let range = ast
                    .provider_path()
                    .map(|path| path.syntax().text_range())
                    .unwrap_or_else(|| ast.syntax().text_range());
                let item_name = scope
                    .provider_path(db)
                    .to_opt()
                    .map(|path| path.pretty_print(db));
                accumulate_error(
                    &mut ctxt,
                    &item_name,
                    DeriveErrorKind::ProviderNotExecutable {
                        construct: ProviderConstruct::Scope,
                    },
                    range,
                );
            }
            ItemKind::DeriveDecl(decl) => {
                expand_derive_decl(
                    db,
                    &mut ctxt,
                    base,
                    &root,
                    decl,
                    &mut groups,
                    &mut derived_pairs,
                );
            }
            _ => continue,
        }
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
                DeriveTarget::DeclStruct(struct_, ast, trait_, decl_ast) => {
                    let desugared = DeriveDesugared::Decl(parser::ast::AstPtr::new(decl_ast));
                    derive::lower_struct_derives(&mut ctxt, ast, *struct_, &[*trait_], desugared);
                }
                DeriveTarget::DeclEnum(enum_, ast, trait_, decl_ast) => {
                    let desugared = DeriveDesugared::Decl(parser::ast::AstPtr::new(decl_ast));
                    derive::lower_enum_derives(&mut ctxt, ast, *enum_, &[*trait_], desugared);
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

/// Validates one standalone `derive Trait for Type` declaration and, when it
/// is a plain compiler-internal derive (no provider involved), schedules its
/// target for impl generation in `groups`.
///
/// Declarations that select a provider — explicitly via `using Provider` or
/// implicitly through an enclosing `with Provider { .. }` scope — generate
/// nothing yet: the explicit form is diagnosed here on its `using` path, the
/// scoped form inherits the not-yet-executable diagnostic reported on the
/// enclosing scope itself.
fn expand_derive_decl<'db>(
    db: &'db dyn HirDb,
    ctxt: &mut FileLowerCtxt<'db>,
    base: &ScopeGraph<'db>,
    root: &parser::SyntaxNode,
    decl: DeriveDecl<'db>,
    groups: &mut IndexMap<ScopeId<'db>, Vec<DeriveTarget<'db>>>,
    derived_pairs: &mut IndexSet<(ItemKind<'db>, DerivableTrait)>,
) {
    let HirOrigin::Raw(ptr) = decl.origin(db) else {
        return;
    };
    let Some(decl_ast) = ptr
        .syntax_node_ptr()
        .try_to_node(root)
        .and_then(ast::DeriveDecl::cast)
    else {
        return;
    };
    let item_name = decl
        .target_path(db)
        .to_opt()
        .map(|path| path.pretty_print(db));

    if decl.selected_provider_path(db).is_some() || decl.provider_scope(db).is_some() {
        if !decl.selected_provider_from_scope(db)
            && let Some(using_path) = decl_ast.provider_path()
        {
            accumulate_error(
                ctxt,
                &item_name,
                DeriveErrorKind::ProviderNotExecutable {
                    construct: ProviderConstruct::NamedSelection,
                },
                using_path.syntax().text_range(),
            );
        }
        return;
    }

    // Trait: must be one of the compiler-derivable traits.
    let Some(head_path) = decl.head_path(db).to_opt() else {
        // Parser error: missing trait path, already reported.
        return;
    };
    if path_has_missing_segment(db, head_path) {
        // Parser error inside the path, already reported; avoid cascading
        // a bogus unknown-trait diagnostic on top of it.
        return;
    }
    let Some(trait_) = head_path
        .as_ident(db)
        .and_then(|ident| DerivableTrait::from_name(ident.data(db)))
    else {
        let range = decl_ast
            .head_path()
            .map(|path| path.syntax().text_range())
            .unwrap_or_else(|| decl_ast.syntax().text_range());
        accumulate_error(
            ctxt,
            &item_name,
            DeriveErrorKind::UnknownTrait {
                name: head_path.pretty_print(db),
            },
            range,
        );
        return;
    };

    // Target: resolve against the base scope graph from the declaration's
    // lexical scope.
    let Some(target_path) = decl.target_path(db).to_opt() else {
        // Parser error: missing target path, already reported.
        return;
    };
    if path_has_missing_segment(db, target_path) {
        // Parser error inside the path, already reported; avoid cascading
        // a bogus unresolved-target diagnostic on top of it.
        return;
    }
    let Some(decl_parent) = lex_parent(base, ItemKind::DeriveDecl(decl)) else {
        return;
    };
    let target_pretty = target_path.pretty_print(db);
    let target_range = decl_ast
        .target_path()
        .map(|path| path.syntax().text_range())
        .unwrap_or_else(|| decl_ast.syntax().text_range());
    let Some(target_item) = resolve_decl_target(db, base, decl_parent, target_path) else {
        accumulate_error(
            ctxt,
            &item_name,
            DeriveErrorKind::UnresolvedDeclTarget {
                path: target_pretty,
            },
            target_range,
        );
        return;
    };

    let target = match target_item {
        ItemKind::Struct(struct_) => match struct_.origin(db) {
            HirOrigin::Raw(ptr) => {
                let Some(ast) = ptr
                    .syntax_node_ptr()
                    .try_to_node(root)
                    .and_then(ast::Struct::cast)
                else {
                    return;
                };
                DeriveTarget::DeclStruct(struct_, ast, trait_, decl_ast.clone())
            }
            HirOrigin::Desugared(DesugaredOrigin::Event(_) | DesugaredOrigin::Error(_)) => {
                accumulate_error(
                    ctxt,
                    &item_name,
                    DeriveErrorKind::InvalidDeclTarget {
                        path: target_pretty,
                        actual: "`#[event]`/`#[error]` struct",
                    },
                    target_range,
                );
                return;
            }
            _ => return,
        },
        ItemKind::Enum(enum_) => {
            let HirOrigin::Raw(ptr) = enum_.origin(db) else {
                return;
            };
            let Some(ast) = ptr
                .syntax_node_ptr()
                .try_to_node(root)
                .and_then(ast::Enum::cast)
            else {
                return;
            };
            DeriveTarget::DeclEnum(enum_, ast, trait_, decl_ast.clone())
        }
        other => {
            accumulate_error(
                ctxt,
                &item_name,
                DeriveErrorKind::InvalidDeclTarget {
                    path: target_pretty,
                    actual: other.kind_name(),
                },
                target_range,
            );
            return;
        }
    };

    if !derived_pairs.insert((target_item, trait_)) {
        accumulate_error(
            ctxt,
            &item_name,
            DeriveErrorKind::ConflictingDerive {
                trait_name: trait_.name().to_string(),
                target: target_pretty,
            },
            decl_ast.syntax().text_range(),
        );
        return;
    }

    // Generated impls become siblings of the *target* (not of the
    // declaration), exactly like attribute derives: the synthesized impl
    // self type names the target unqualified, so it must resolve in the
    // target's own lexical context.
    let Some(parent) = lex_parent(base, target_item) else {
        return;
    };
    groups.entry(parent).or_default().push(target);
}

/// Resolves the target path of a standalone `derive` declaration against the
/// *base* scope graph. The full path resolver cannot be used here: it
/// queries the merged scope graph, which would cycle back into this stage.
///
/// Leading `self`/`super` segments navigate the module chain explicitly
/// (modules are not lexically transparent in Fe). Otherwise the first
/// segment is looked up along the lexical scope chain of the declaration;
/// the remaining segments descend through named module/type edges. This
/// covers targets declared anywhere in the same file (including nested
/// modules), but not imported or cross-file names — those are diagnosed as
/// unresolved by the caller.
fn resolve_decl_target<'db>(
    db: &'db dyn HirDb,
    graph: &ScopeGraph<'db>,
    start: ScopeId<'db>,
    path: PathId<'db>,
) -> Option<ItemKind<'db>> {
    // Flatten the path into its segment identifiers, root first. Generic
    // arguments are irrelevant for target lookup (the generated impl always
    // covers the target's own parameters) and qualified-type paths are not
    // supported.
    let mut segments = Vec::new();
    let mut cursor = Some(path);
    while let Some(p) = cursor {
        segments.push(p.ident(db).to_opt()?);
        cursor = p.parent(db);
    }
    segments.reverse();

    let mut segments = segments.into_iter().peekable();

    // Leading `self` / `super..` segments: navigate the module chain. The
    // declaration's lexical parent is already its enclosing module (decls
    // are item-level), so `self` resolves to `start`.
    let mut module_relative = false;
    let mut scope = start;
    if segments.peek().is_some_and(|seg| seg.is_self(db)) {
        segments.next();
        module_relative = true;
    } else {
        while segments.peek().is_some_and(|seg| seg.is_super(db)) {
            segments.next();
            module_relative = true;
            scope = super_dest(graph, scope)?;
        }
    }

    let mut resolved = if module_relative {
        scope
    } else {
        // First segment: search the lexical scope chain.
        let first = segments.next()?;
        loop {
            if let Some(dest) = named_edge_dest(graph, scope, first) {
                break dest;
            }
            scope = lex_parent_scope(graph, scope)?;
        }
    };

    // Remaining segments: descend through named edges.
    for segment in segments {
        resolved = named_edge_dest(graph, resolved, segment)?;
    }

    match resolved {
        // An edge may leave the file (e.g. `super` from the file's own top
        // module); only same-file items can be expanded by this stage.
        ScopeId::Item(item) if item.top_mod(db) == graph.top_mod => Some(item),
        _ => None,
    }
}

/// Whether any segment of `path` lost its identifier to a parser error.
fn path_has_missing_segment<'db>(db: &'db dyn HirDb, path: PathId<'db>) -> bool {
    let mut cursor = Some(path);
    while let Some(p) = cursor {
        if !p.ident(db).is_present() {
            return true;
        }
        cursor = p.parent(db);
    }
    false
}

/// The edges of `scope`, or `None` when `scope` does not belong to `graph`
/// (e.g. a destination in another file reached through a `super` edge).
/// Never panics, unlike [`ScopeGraph::edges`].
fn scope_edges<'db, 'a>(
    graph: &'a ScopeGraph<'db>,
    scope: ScopeId<'db>,
) -> impl Iterator<Item = &'a crate::hir_def::scope_graph::ScopeEdge<'db>> {
    graph
        .scopes
        .get(&scope)
        .into_iter()
        .flat_map(|data| data.edges.iter())
}

/// The destination of a named (module/type/trait/value) edge of `scope`
/// matching `ident`, if any. Trait and value edges are included so that a
/// declaration targeting e.g. a function is reported as an *invalid* target
/// (with its actual kind) rather than an unresolved one.
fn named_edge_dest<'db>(
    graph: &ScopeGraph<'db>,
    scope: ScopeId<'db>,
    ident: IdentId<'db>,
) -> Option<ScopeId<'db>> {
    scope_edges(graph, scope).find_map(|edge| match edge.kind {
        EdgeKind::Mod(m) if m.0 == ident => Some(edge.dest),
        EdgeKind::Type(t) if t.0 == ident => Some(edge.dest),
        EdgeKind::Trait(t) if t.0 == ident => Some(edge.dest),
        EdgeKind::Value(v) if v.0 == ident => Some(edge.dest),
        _ => None,
    })
}

/// The lexical parent scope of `item` in `graph`.
fn lex_parent<'db>(graph: &ScopeGraph<'db>, item: ItemKind<'db>) -> Option<ScopeId<'db>> {
    lex_parent_scope(graph, ScopeId::Item(item))
}

/// The lexical parent of `scope` in `graph`.
fn lex_parent_scope<'db>(graph: &ScopeGraph<'db>, scope: ScopeId<'db>) -> Option<ScopeId<'db>> {
    scope_edges(graph, scope)
        .find_map(|edge| matches!(edge.kind, EdgeKind::Lex(_)).then_some(edge.dest))
}

/// The destination of the `super` edge of `scope`, if any.
fn super_dest<'db>(graph: &ScopeGraph<'db>, scope: ScopeId<'db>) -> Option<ScopeId<'db>> {
    scope_edges(graph, scope)
        .find_map(|edge| matches!(edge.kind, EdgeKind::Super(_)).then_some(edge.dest))
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

    /// Standalone `derive Trait for Type` declarations expand through the
    /// same stage as attribute derives: the generated impls become siblings
    /// of the *target* (not the declaration), including for targets declared
    /// in nested modules of the same file.
    #[test]
    fn derive_decls_expand_like_attr_derives() {
        let mut db = TestDb::default();
        let text = r#"
            struct It {
                x: u256,
            }

            derive Eq for It
            derive Default for It

            mod inner {
                pub struct Boxed {
                    pub v: u256,
                }

                derive Eq for Boxed
            }

            derive Eq for inner::Sealed
            mod inner2 {
                pub struct Sealed {
                    pub v: u256,
                }
            }
            derive Eq for inner2::Sealed
        "#;
        let file = db.standalone_file(text);
        let top_mod = map_file_to_mod(&db, file);

        // Base lowering produces the decls as items but no impls.
        let base = base_scope_graph_impl(&db, top_mod);
        assert_eq!(
            base.items_dfs(&db)
                .filter(|item| matches!(item, ItemKind::DeriveDecl(_)))
                .count(),
            5
        );
        assert!(
            base.items_dfs(&db)
                .all(|item| !matches!(item, ItemKind::ImplTrait(_)))
        );

        // One impl per *resolvable* declaration: `inner::Sealed` does not
        // exist (diagnosed, not expanded), `inner2::Sealed` does.
        let generated = generated_hir_items(&db, top_mod);
        assert_eq!(generated.len(), 4);
        assert!(
            generated
                .iter()
                .all(|item| matches!(item, ItemKind::ImplTrait(_)))
        );

        // Merged graph exposes the generated impls and their methods
        // (`eq`, `default`, `eq`, `eq`) like ordinary items.
        let merged = scope_graph(&db, top_mod);
        let funcs = merged
            .items_dfs(&db)
            .filter(|item| matches!(item, ItemKind::Func(_)))
            .count();
        assert_eq!(funcs, 4);
    }
}
