use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de};

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
