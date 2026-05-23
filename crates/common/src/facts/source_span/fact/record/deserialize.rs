use serde::{Deserialize, Deserializer, de};

use crate::facts::{FactId, SourceSpanKind};

use super::SourceSpanFact;

impl<'de> Deserialize<'de> for SourceSpanFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawSourceSpanFact::deserialize(deserializer)?;
        SourceSpanFact::try_new(
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
