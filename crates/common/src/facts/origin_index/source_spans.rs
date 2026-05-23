use super::OriginFactIndex;
use crate::facts::{FactId, SourceSpanFact};
use crate::origin::OriginExportKey;

impl<'a> OriginFactIndex<'a> {
    pub fn source_spans_for_origin(
        &self,
        id: FactId,
    ) -> impl Iterator<Item = &'a SourceSpanFact> + '_ {
        self.source_spans_by_origin
            .get(&id)
            .into_iter()
            .flat_map(|spans| spans.iter().copied())
    }

    pub fn source_spans_for_key(
        &self,
        key: &OriginExportKey,
    ) -> impl Iterator<Item = &'a SourceSpanFact> + '_ {
        self.origin_id(key)
            .into_iter()
            .flat_map(|id| self.source_spans_for_origin(id))
    }
}
