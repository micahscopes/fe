use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceSpanExportError {
    EmptyFile,
    InvalidByteRange {
        start_byte: usize,
        end_byte: usize,
    },
    InvalidPositionRange {
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    },
}

impl fmt::Display for SourceSpanExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyFile => write!(f, "source span file must not be empty"),
            Self::InvalidByteRange {
                start_byte,
                end_byte,
            } => write!(
                f,
                "source span byte range must be ordered: {start_byte}..{end_byte}"
            ),
            Self::InvalidPositionRange {
                start_line,
                start_col,
                end_line,
                end_col,
            } => write!(
                f,
                "source span line/column range must be ordered: {start_line}:{start_col}..{end_line}:{end_col}"
            ),
        }
    }
}

impl std::error::Error for SourceSpanExportError {}
