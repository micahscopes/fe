use std::collections::BTreeMap;

use crate::facts::{
    SourceSpanFileCount, TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName,
};

use super::super::TypedFactRelationIndex;

impl<'a> TypedFactRelationIndex<'a> {
    pub fn source_span_file_counts(
        &self,
    ) -> Result<Vec<SourceSpanFileCount>, TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::SourceSpan)?;
        let file_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::File,
        )?;
        let mut counts = BTreeMap::<String, usize>::new();

        for row in relation_table.rows() {
            *counts.entry(row[file_column].clone()).or_default() += 1;
        }

        Ok(counts
            .into_iter()
            .map(|(file, spans)| SourceSpanFileCount::new(file, spans))
            .collect())
    }
}
