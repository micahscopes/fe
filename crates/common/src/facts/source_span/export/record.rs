mod deserialize;
mod sort_key;

use serde::Serialize;

use crate::origin::OriginExportKey;

use super::{SourceSpanExportError, SourceSpanKind, validated_source_span_parts};

pub(in crate::facts) use sort_key::source_span_export_sort_key;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct SourceSpanExport {
    origin_key: OriginExportKey,
    span_kind: SourceSpanKind,
    file: String,
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
}

impl SourceSpanExport {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        origin_key: OriginExportKey,
        span_kind: SourceSpanKind,
        file: impl Into<String>,
        start_byte: usize,
        end_byte: usize,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> Self {
        Self::try_new(
            origin_key, span_kind, file, start_byte, end_byte, start_line, start_col, end_line,
            end_col,
        )
        .unwrap_or_else(|err| panic!("{err}"))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        origin_key: OriginExportKey,
        span_kind: SourceSpanKind,
        file: impl Into<String>,
        start_byte: usize,
        end_byte: usize,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> Result<Self, SourceSpanExportError> {
        let file = validated_source_span_parts(
            file, start_byte, end_byte, start_line, start_col, end_line, end_col,
        )?;
        Ok(Self {
            origin_key,
            span_kind,
            file,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
        })
    }

    pub const fn origin_key(&self) -> &OriginExportKey {
        &self.origin_key
    }

    pub const fn span_kind(&self) -> SourceSpanKind {
        self.span_kind
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    pub const fn start_byte(&self) -> usize {
        self.start_byte
    }

    pub const fn end_byte(&self) -> usize {
        self.end_byte
    }

    pub const fn start_line(&self) -> usize {
        self.start_line
    }

    pub const fn start_col(&self) -> usize {
        self.start_col
    }

    pub const fn end_line(&self) -> usize {
        self.end_line
    }

    pub const fn end_col(&self) -> usize {
        self.end_col
    }

    pub(in crate::facts::source_span) fn into_parts(
        self,
    ) -> (
        OriginExportKey,
        SourceSpanKind,
        String,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
    ) {
        (
            self.origin_key,
            self.span_kind,
            self.file,
            self.start_byte,
            self.end_byte,
            self.start_line,
            self.start_col,
            self.end_line,
            self.end_col,
        )
    }
}
