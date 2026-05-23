use std::fmt;

use crate::facts::FactNamespaceError;

use super::super::SourceSpanExportError;

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
