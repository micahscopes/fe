use std::fmt;

use crate::origin::OriginExportKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginPathWitnessExportError {
    EmptyPath,
    LengthMismatch {
        nodes: usize,
        links: usize,
    },
    FromKindMismatch {
        expected: OriginExportKind,
        actual: OriginExportKind,
    },
    ToKindMismatch {
        expected: OriginExportKind,
        actual: OriginExportKind,
    },
}

impl fmt::Display for OriginPathWitnessExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => write!(f, "origin path witness must contain at least one node"),
            Self::LengthMismatch { nodes, links } => write!(
                f,
                "origin path witness has {nodes} nodes and {links} links; expected exactly one more node than link"
            ),
            Self::FromKindMismatch { expected, actual } => write!(
                f,
                "origin path witness starts with {}, expected {}",
                actual.as_str(),
                expected.as_str()
            ),
            Self::ToKindMismatch { expected, actual } => write!(
                f,
                "origin path witness ends with {}, expected {}",
                actual.as_str(),
                expected.as_str()
            ),
        }
    }
}

impl std::error::Error for OriginPathWitnessExportError {}
