use crate::facts::{TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName};

use super::super::TypedFactRelationIndex;

impl<'a> TypedFactRelationIndex<'a> {
    pub(in crate::facts::relation_index) fn validate_source_span_rows(
        &self,
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::SourceSpan)?;
        let origin_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::Origin,
        )?;
        let file_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::File,
        )?;
        let start_byte_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::StartByte,
        )?;
        let end_byte_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::EndByte,
        )?;
        let start_line_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::StartLine,
        )?;
        let start_col_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::StartCol,
        )?;
        let end_line_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::EndLine,
        )?;
        let end_col_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::EndCol,
        )?;

        for row in relation_table.rows() {
            let origin = &row[origin_column];
            if row[file_column].is_empty() {
                return Err(TypedFactRelationError::InvalidSourceSpanFile {
                    origin: origin.clone(),
                });
            }
            let start_byte = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::StartByte,
                &row[start_byte_column],
            )?;
            let end_byte = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::EndByte,
                &row[end_byte_column],
            )?;
            let start_line = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::StartLine,
                &row[start_line_column],
            )?;
            let start_col = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::StartCol,
                &row[start_col_column],
            )?;
            let end_line = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::EndLine,
                &row[end_line_column],
            )?;
            let end_col = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::EndCol,
                &row[end_col_column],
            )?;

            if start_byte > end_byte {
                return Err(TypedFactRelationError::InvalidSourceSpanRange {
                    origin: origin.clone(),
                    start_byte,
                    end_byte,
                });
            }
            if start_line > end_line || (start_line == end_line && start_col > end_col) {
                return Err(TypedFactRelationError::InvalidSourceSpanPosition {
                    origin: origin.clone(),
                    start_line,
                    start_col,
                    end_line,
                    end_col,
                });
            }
        }

        Ok(())
    }
}
