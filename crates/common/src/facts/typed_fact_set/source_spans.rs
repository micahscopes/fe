use crate::facts::{
    OriginFactIndex, SourceSpanExport, SourceSpanFact, SourceSpanFactError, TypedFact,
    TypedFactSet, source_span::source_span_export_sort_key,
};

impl TypedFactSet {
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
