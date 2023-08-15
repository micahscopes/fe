use fxhash::FxHashMap;
use hir::{
    hir_def::{scope_graph::ScopeId, PathId, TopLevelMod, ItemKind},
    visitor::{prelude::LazyPathSpan, Visitor, VisitorCtxt},
    HirDb,
};
// use hir_analysis::{name_resolution::EarlyResolvedPath, HirAnalysisDb};

use crate::db::{LanguageServerDatabase, LanguageServerDb};
use common::diagnostics::Span;
use hir::span::LazySpan;

pub(crate) type GotoEnclosingPath = (PathId, ScopeId);
pub(crate) type GotoPathMap = FxHashMap<Span, GotoEnclosingPath>;

pub struct PathSpanCollector<'db> {
    // You don't need to collect scope id basically.
    path_map: GotoPathMap,
    db: &'db dyn LanguageServerDb,
}

impl<'db> PathSpanCollector<'db> {
    pub fn new(db: &'db LanguageServerDatabase) -> Self {
        Self {
            path_map: FxHashMap::default(),
            db,
        }
    }
}

pub(crate) type Cursor = rowan::TextSize;

impl<'db> Visitor for PathSpanCollector<'db> {
    fn visit_path(&mut self, ctxt: &mut VisitorCtxt<'_, LazyPathSpan>, path: PathId) {
        let Some(span) = ctxt
            .span()
            .map(|lazy_span| lazy_span.resolve(
                self.db.as_spanned_hir_db()
            ))
            .flatten()
        else {
            return;
        };

        let scope = ctxt.scope();
        self.path_map.insert(span, (path, scope));
    }
}

fn smallest_enclosing_path(cursor: Cursor, path_map: &GotoPathMap) -> Option<GotoEnclosingPath>{
    let mut smallest_enclosing_path = None;
    let mut smallest_range_size = None;

    for (span, enclosing_path) in path_map {
        // print the span and enclosing path
        if span.range.contains(cursor) {
            let range_size = span.range.end() - span.range.start();
            if smallest_range_size.is_none() || range_size < smallest_range_size.unwrap() {
                smallest_enclosing_path = Some(*enclosing_path);
                smallest_range_size = Some(range_size);
            }
        }
    }

    return smallest_enclosing_path;
}

pub fn goto_enclosing_path(db: &mut LanguageServerDatabase, top_mod: TopLevelMod, cursor: Cursor) -> Option<GotoEnclosingPath> {
    // Find the innermost item enclosing the cursor.
    let item: ItemKind = db.find_enclosing_item(top_mod, cursor)?;

    let mut visitor_ctxt = VisitorCtxt::with_item(db.as_hir_db(), item);
    let mut path_collector = PathSpanCollector::new(&db);
    path_collector.visit_item(&mut visitor_ctxt, item);

    let path_map = path_collector.path_map;

    // Find the path that encloses the cursor.
    let goto_path = smallest_enclosing_path(cursor, &path_map)?;
    Some(goto_path)

    // let (path_id, scope) = goto_path;

    // Resolve path.
    // let resolved_path = hir_analysis::name_resolution::resolve_path_early(db, path_id, scope);

}