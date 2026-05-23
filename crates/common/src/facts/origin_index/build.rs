use std::collections::{BTreeMap, BTreeSet};

use super::OriginFactIndex;
use crate::facts::index_error::require_fact_namespace;
use crate::facts::{
    FactId, FactIndexError, FactNamespace, OriginLinkFact, OriginNodeFact, SourceSpanFact,
    TypedFactSet,
};
use crate::origin::OriginExportKey;

impl<'a> OriginFactIndex<'a> {
    pub fn new(facts: &'a TypedFactSet) -> Result<Self, FactIndexError> {
        let (nodes_by_id, ids_by_key) = indexed_origin_nodes(facts)?;
        let outgoing = indexed_origin_links(facts, &nodes_by_id)?;
        let source_spans_by_origin = indexed_source_spans(facts, &nodes_by_id)?;

        Ok(Self {
            nodes_by_id,
            ids_by_key,
            outgoing,
            source_spans_by_origin,
        })
    }
}

fn indexed_origin_nodes<'a>(
    facts: &'a TypedFactSet,
) -> Result<
    (
        BTreeMap<FactId, &'a OriginNodeFact>,
        BTreeMap<OriginExportKey, FactId>,
    ),
    FactIndexError,
> {
    let mut nodes_by_id = BTreeMap::new();
    let mut ids_by_key = BTreeMap::new();

    for node in facts.origin_nodes() {
        require_fact_namespace(node.id(), FactNamespace::OriginNode)?;
        if nodes_by_id.insert(node.id(), node).is_some() {
            return Err(FactIndexError::DuplicateOriginId);
        }
        if ids_by_key.insert(node.key().clone(), node.id()).is_some() {
            return Err(FactIndexError::DuplicateOriginKey);
        }
    }

    Ok((nodes_by_id, ids_by_key))
}

fn indexed_origin_links<'a>(
    facts: &'a TypedFactSet,
    nodes_by_id: &BTreeMap<FactId, &'a OriginNodeFact>,
) -> Result<BTreeMap<FactId, Vec<&'a OriginLinkFact>>, FactIndexError> {
    let mut seen_links = BTreeSet::new();
    let mut outgoing: BTreeMap<FactId, Vec<&OriginLinkFact>> = BTreeMap::new();
    for link in facts.origin_links() {
        require_fact_namespace(link.from(), FactNamespace::OriginNode)?;
        require_fact_namespace(link.to(), FactNamespace::OriginNode)?;
        if !nodes_by_id.contains_key(&link.from()) {
            return Err(FactIndexError::OriginLinkMissingEndpoint {
                endpoint: link.from(),
            });
        }
        if !nodes_by_id.contains_key(&link.to()) {
            return Err(FactIndexError::OriginLinkMissingEndpoint {
                endpoint: link.to(),
            });
        }
        let link_key = (link.from(), link.to(), link.kind());
        if !seen_links.insert(link_key) {
            return Err(FactIndexError::DuplicateOriginLink {
                from: link.from(),
                to: link.to(),
                kind: link.kind(),
            });
        }
        outgoing.entry(link.from()).or_default().push(link);
    }
    for links in outgoing.values_mut() {
        links.sort_by_key(|link| (link.to(), link.kind()));
    }

    Ok(outgoing)
}

fn indexed_source_spans<'a>(
    facts: &'a TypedFactSet,
    nodes_by_id: &BTreeMap<FactId, &'a OriginNodeFact>,
) -> Result<BTreeMap<FactId, Vec<&'a SourceSpanFact>>, FactIndexError> {
    let mut source_spans_by_origin: BTreeMap<FactId, Vec<&SourceSpanFact>> = BTreeMap::new();
    for span in facts.source_spans() {
        require_fact_namespace(span.origin(), FactNamespace::OriginNode)?;
        if !nodes_by_id.contains_key(&span.origin()) {
            return Err(FactIndexError::SourceSpanMissingOrigin {
                origin: span.origin(),
            });
        }
        if span.file().is_empty() {
            return Err(FactIndexError::InvalidSourceSpanFile {
                origin: span.origin(),
            });
        }
        if span.start_byte() > span.end_byte() {
            return Err(FactIndexError::InvalidSourceSpanRange {
                origin: span.origin(),
                start_byte: span.start_byte(),
                end_byte: span.end_byte(),
            });
        }
        if span.start_line() > span.end_line()
            || (span.start_line() == span.end_line() && span.start_col() > span.end_col())
        {
            return Err(FactIndexError::InvalidSourceSpanPosition {
                origin: span.origin(),
                start_line: span.start_line(),
                start_col: span.start_col(),
                end_line: span.end_line(),
                end_col: span.end_col(),
            });
        }
        source_spans_by_origin
            .entry(span.origin())
            .or_default()
            .push(span);
    }
    for spans in source_spans_by_origin.values_mut() {
        spans.sort_by_key(|span| {
            (
                span.file(),
                span.start_byte(),
                span.end_byte(),
                span.start_line(),
                span.start_col(),
                span.end_line(),
                span.end_col(),
                span.span_kind(),
            )
        });
    }

    Ok(source_spans_by_origin)
}
