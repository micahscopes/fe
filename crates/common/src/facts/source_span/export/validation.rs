use super::SourceSpanExportError;

pub(in crate::facts) fn validated_source_span_parts(
    file: impl Into<String>,
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
) -> Result<String, SourceSpanExportError> {
    let file = file.into();
    if file.is_empty() {
        return Err(SourceSpanExportError::EmptyFile);
    }
    if start_byte > end_byte {
        return Err(SourceSpanExportError::InvalidByteRange {
            start_byte,
            end_byte,
        });
    }
    if start_line > end_line || (start_line == end_line && start_col > end_col) {
        return Err(SourceSpanExportError::InvalidPositionRange {
            start_line,
            start_col,
            end_line,
            end_col,
        });
    }
    Ok(file)
}
