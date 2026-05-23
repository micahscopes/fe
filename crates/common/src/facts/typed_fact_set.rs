use super::{
    DataFlowFact, OriginFactIndex, OriginLinkFact, OriginNodeFact, OwnedTypedFactSetExport,
    ShapeChildFact, ShapeEdgeFact, ShapeFieldFact, ShapeHashFact, ShapeNodeFact, SourceSpanExport,
    SourceSpanFact, SourceSpanFactError, TraceEventFact, TypedFact, TypedFactRelationSet,
    TypedFactSetExport, relation_export::typed_fact_relation_export,
    source_span::source_span_export_sort_key,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedFactSet {
    facts: Vec<TypedFact>,
}

impl TypedFactSet {
    /// Create a fact set whose internal IDs were allocated together.
    ///
    /// Do not concatenate independently exported fact sets: `FactId`s are
    /// allocation-local. Build one typed graph and export it once when a
    /// combined view needs stable cross-links.
    pub fn new(facts: Vec<TypedFact>) -> Self {
        Self { facts }
    }

    pub fn export(&self) -> TypedFactSetExport<'_> {
        TypedFactSetExport::new(&self.facts)
    }

    pub fn relation_export(&self) -> TypedFactRelationSet {
        typed_fact_relation_export(self)
    }

    pub fn to_owned_export(&self) -> OwnedTypedFactSetExport {
        OwnedTypedFactSetExport::from_facts(self.facts.clone())
    }

    pub fn facts(&self) -> &[TypedFact] {
        &self.facts
    }

    pub fn into_facts(self) -> Vec<TypedFact> {
        self.facts
    }

    pub fn origin_nodes(&self) -> impl Iterator<Item = &OriginNodeFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::OriginNode(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn origin_links(&self) -> impl Iterator<Item = &OriginLinkFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::OriginLink(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn source_spans(&self) -> impl Iterator<Item = &SourceSpanFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::SourceSpan(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn shape_nodes(&self) -> impl Iterator<Item = &ShapeNodeFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::ShapeNode(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn shape_fields(&self) -> impl Iterator<Item = &ShapeFieldFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::ShapeField(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn shape_children(&self) -> impl Iterator<Item = &ShapeChildFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::ShapeChild(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn shape_edges(&self) -> impl Iterator<Item = &ShapeEdgeFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::ShapeEdge(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn trace_events(&self) -> impl Iterator<Item = &TraceEventFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::TraceEvent(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn data_flows(&self) -> impl Iterator<Item = &DataFlowFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::DataFlow(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn shape_hashes(&self) -> impl Iterator<Item = &ShapeHashFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::ShapeHash(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn with_source_spans(
        mut self,
        spans: impl IntoIterator<Item = SourceSpanExport>,
    ) -> Result<Self, SourceSpanFactError> {
        let mut spans = spans.into_iter().collect::<Vec<_>>();
        spans.sort_by(|left, right| {
            source_span_export_sort_key(left).cmp(&source_span_export_sort_key(right))
        });
        spans.dedup();

        let span_facts = {
            let index = OriginFactIndex::new(&self).map_err(SourceSpanFactError::InvalidFacts)?;
            spans
                .into_iter()
                .map(|span| {
                    let origin = index.origin_id(span.origin_key()).ok_or_else(|| {
                        SourceSpanFactError::MissingOriginKey(span.origin_key().clone())
                    })?;
                    Ok(SourceSpanFact::from_export(origin, span))
                })
                .collect::<Result<Vec<_>, SourceSpanFactError>>()?
        };

        self.facts
            .extend(span_facts.into_iter().map(TypedFact::SourceSpan));
        Ok(self)
    }
}
