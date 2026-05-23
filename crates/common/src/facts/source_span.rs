use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::origin::OriginExportKey;

use super::{FactId, FactNamespace, FactNamespaceError, ids::validated_fact_namespace};

crate::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum SourceSpanKind {
        Original => "original",
        Expanded => "expanded",
        NotFound => "not_found",
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceSpanFileCount {
    file: String,
    spans: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceSpanFileCountError {
    EmptyFile,
    ZeroSpans,
}

impl fmt::Display for SourceSpanFileCountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyFile => write!(f, "source span file count file must not be empty"),
            Self::ZeroSpans => write!(f, "source span file count spans must be greater than zero"),
        }
    }
}

impl std::error::Error for SourceSpanFileCountError {}

impl SourceSpanFileCount {
    pub fn new(file: impl Into<String>, spans: usize) -> Self {
        Self::try_new(file, spans).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        file: impl Into<String>,
        spans: usize,
    ) -> Result<Self, SourceSpanFileCountError> {
        let file = file.into();
        if file.is_empty() {
            return Err(SourceSpanFileCountError::EmptyFile);
        }
        if spans == 0 {
            return Err(SourceSpanFileCountError::ZeroSpans);
        }
        Ok(Self { file, spans })
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    pub const fn spans(&self) -> usize {
        self.spans
    }
}

impl<'de> Deserialize<'de> for SourceSpanFileCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCount {
            file: String,
            spans: usize,
        }

        let raw = RawCount::deserialize(deserializer)?;
        Self::try_new(raw.file, raw.spans).map_err(de::Error::custom)
    }
}

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

fn validated_source_span_parts(
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

impl SourceSpanExport {
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
}

impl<'de> Deserialize<'de> for SourceSpanExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawSourceSpan {
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

        let raw = RawSourceSpan::deserialize(deserializer)?;
        Self::try_new(
            raw.origin_key,
            raw.span_kind,
            raw.file,
            raw.start_byte,
            raw.end_byte,
            raw.start_line,
            raw.start_col,
            raw.end_line,
            raw.end_col,
        )
        .map_err(de::Error::custom)
    }
}

pub(super) fn source_span_export_sort_key(span: &SourceSpanExport) -> impl Ord + '_ {
    (
        span.origin_key(),
        span.file(),
        span.start_byte(),
        span.end_byte(),
        span.start_line(),
        span.start_col(),
        span.end_line(),
        span.end_col(),
        span.span_kind(),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceSpanFactBuildError {
    WrongNamespace(FactNamespaceError),
    InvalidSpan(SourceSpanExportError),
}

impl fmt::Display for SourceSpanFactBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongNamespace(err) => err.fmt(f),
            Self::InvalidSpan(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for SourceSpanFactBuildError {}

impl From<FactNamespaceError> for SourceSpanFactBuildError {
    fn from(err: FactNamespaceError) -> Self {
        Self::WrongNamespace(err)
    }
}

impl From<SourceSpanExportError> for SourceSpanFactBuildError {
    fn from(err: SourceSpanExportError) -> Self {
        Self::InvalidSpan(err)
    }
}

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

    pub(super) fn from_export(origin: FactId, span: SourceSpanExport) -> Self {
        Self::new(
            origin,
            span.span_kind,
            span.file,
            span.start_byte,
            span.end_byte,
            span.start_line,
            span.start_col,
            span.end_line,
            span.end_col,
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

impl<'de> Deserialize<'de> for SourceSpanFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawSourceSpanFact {
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

        let raw = RawSourceSpanFact::deserialize(deserializer)?;
        Self::try_new(
            raw.origin,
            raw.span_kind,
            raw.file,
            raw.start_byte,
            raw.end_byte,
            raw.start_line,
            raw.start_col,
            raw.end_line,
            raw.end_col,
        )
        .map_err(de::Error::custom)
    }
}
