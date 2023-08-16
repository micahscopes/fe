use fxhash::FxHashMap;
use hir::{
    hir_def::{scope_graph::ScopeId, PathId, TopLevelMod, ItemKind},
    visitor::{prelude::LazyPathSpan, Visitor, VisitorCtxt},
    HirDb,
};
use hir_analysis::name_resolution::EarlyResolvedPath;

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
            if range_size < smallest_range_size.unwrap() {
                smallest_enclosing_path = Some(*enclosing_path);
                smallest_range_size = Some(range_size);
            }
        }
    }

    return smallest_enclosing_path;
}

pub fn goto_enclosing_path(db: &mut LanguageServerDatabase, top_mod: TopLevelMod, cursor: Cursor) -> Option<EarlyResolvedPath> {
    // Find the innermost item enclosing the cursor.
    let item: ItemKind = db.find_enclosing_item(top_mod, cursor)?;

    let mut visitor_ctxt = VisitorCtxt::with_item(db.as_hir_db(), item);
    let mut path_collector = PathSpanCollector::new(&db);
    path_collector.visit_item(&mut visitor_ctxt, item);

    let path_map = path_collector.path_map;

    // Find the path that encloses the cursor.
    let goto_path = smallest_enclosing_path(cursor, &path_map)?;

    let (path_id, scope) = goto_path;

    // Resolve path.
    let resolved_path = hir_analysis::name_resolution::resolve_path_early(db, path_id, scope);

    Some(resolved_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fe_compiler_test_utils::snap_test;
    use dir_test::{dir_test, Fixture};
    use std::path::Path;

    fn extract_multiple_cursor_positions_from_comments(content: &str) -> Vec<rowan::TextSize> {
        let lines: Vec<&str> = content.lines().collect();
        let mut cursor_positions = Vec::new();

        // Find the indices of the lines with the cursor marker
        let cursor_line_indices: Vec<usize> = lines.iter().enumerate()
            .filter_map(|(i, &line)| if line.trim() == "^" { Some(i) } else { None })
            .collect();

        for &cursor_line_index in cursor_line_indices.iter() {
            let actual_line = lines[cursor_line_index - 1];
            let cursor_index = lines[cursor_line_index].chars().position(|ch| ch == '^').unwrap();
            cursor_positions.push(rowan::TextSize::from((actual_line.len() + cursor_index) as u32));
        }

        cursor_positions
    }

    
    
    #[dir_test(
        dir: "$CARGO_MANIFEST_DIR/test_files",
        glob: "goto*.fe"
    )]
    fn test_goto_enclosing_path(fixture: Fixture<&str>) {
        let mut db = LanguageServerDatabase::default();
        let path = Path::new(fixture.path());
        let top_mod = db.top_mod_from_file(path, fixture.content());

        let cursor = extract_multiple_cursor_positions_from_comments(fixture.content());
        
        let result = cursor.iter().map(|cursor| {
            let resolved_path = goto_enclosing_path(&mut db, top_mod, *cursor);

            let res = match resolved_path {
                Some(path) => match path {
                    EarlyResolvedPath::Full(bucket) => {
                        bucket.iter().map(|x| x.pretty_path(&db).unwrap()).collect::<Vec<_>>()
                        .join("\n")
                    },
                    EarlyResolvedPath::Partial { res, unresolved_from } => {
                        res.pretty_path(&db).unwrap()
                    },
                }
                None => String::from("No path found"),
            };
            res
        }).collect::<Vec<_>>().join("\n");

        snap_test!(result, fixture.path());
    }

    #[dir_test(
        dir: "$CARGO_MANIFEST_DIR/test_files",
        glob: "smallest_enclosing*.fe"
    )]
    fn test_smallest_enclosing_path(fixture: Fixture<&str>) {
        let mut db = LanguageServerDatabase::default();
        let path = Path::new(fixture.path());
        let top_mod = db.top_mod_from_file(path, fixture.content());

        let cursors = extract_multiple_cursor_positions_from_comments(fixture.content());

        let result = cursors.iter().map(|cursor| {
            let mut visitor_ctxt = VisitorCtxt::with_top_mod(db.as_hir_db(), top_mod);
            let mut path_collector = PathSpanCollector::new(&db);
            path_collector.visit_top_mod(&mut visitor_ctxt, top_mod);

            let path_map = path_collector.path_map;
            let enclosing_path = smallest_enclosing_path(*cursor, &path_map);

            let res = match enclosing_path {
                Some(path) => format!("{:?}", path),
                None => String::from("No path found"),
            };
            res
        }).collect::<Vec<_>>().join("\n");

        snap_test!(result, fixture.path());
    }
}
