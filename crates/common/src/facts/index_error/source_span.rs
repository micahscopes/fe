use std::fmt;

use crate::{facts::FactIndexError, origin::OriginExportKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceSpanFactError {
    InvalidFacts(FactIndexError),
    MissingOriginKey(OriginExportKey),
}

impl fmt::Display for SourceSpanFactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFacts(err) => write!(f, "invalid origin facts: {err}"),
            Self::MissingOriginKey(key) => write!(
                f,
                "source span references missing origin key {}:{}:{}",
                key.kind().as_str(),
                key.owner_key(),
                key.local_key()
            ),
        }
    }
}

impl std::error::Error for SourceSpanFactError {}
