mod deserialize;

use serde::Serialize;

use crate::facts::{FactId, FactNamespace, ids::validated_fact_namespace};

use super::super::{SourceSpanExport, SourceSpanKind, validated_source_span_parts};
use super::SourceSpanFactBuildError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceSpanFact {
    origin: FactId,
    span_kind: SourceSpanKind,
    file: String,
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
}

impl SourceSpanFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        origin: FactId,
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
            origin, span_kind, file, start_byte, end_byte, start_line, start_col, end_line, end_col,
        )
        .unwrap_or_else(|err| panic!("{err}"))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        origin: FactId,
        span_kind: SourceSpanKind,
        file: impl Into<String>,
        start_byte: usize,
        end_byte: usize,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> Result<Self, SourceSpanFactBuildError> {
        let origin = validated_fact_namespace(origin, FactNamespace::OriginNode)?;
        let file = validated_source_span_parts(
            file, start_byte, end_byte, start_line, start_col, end_line, end_col,
        )?;
        Ok(Self {
            origin,
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

    pub(in crate::facts) fn from_export(origin: FactId, span: SourceSpanExport) -> Self {
        let (
            _origin_key,
            span_kind,
            file,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
        ) = span.into_parts();
        Self::new(
            origin, span_kind, file, start_byte, end_byte, start_line, start_col, end_line, end_col,
        )
    }

    pub const fn origin(&self) -> FactId {
        self.origin
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
}
