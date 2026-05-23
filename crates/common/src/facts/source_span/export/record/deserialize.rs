use serde::{Deserialize, Deserializer, de};

use crate::origin::OriginExportKey;

use super::super::SourceSpanKind;
use super::SourceSpanExport;

impl<'de> Deserialize<'de> for SourceSpanExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawSourceSpan::deserialize(deserializer)?;
        SourceSpanExport::try_new(
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
