use std::fmt;

use crate::origin::OriginExportKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginReachabilitySummaryError {
    ZeroReachablePairsForKind {
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    },
    DuplicateKindPair {
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    },
    ReachablePairTotalOverflow,
    ReachablePairTotalMismatch {
        declared: usize,
        actual: usize,
    },
}

impl fmt::Display for OriginReachabilitySummaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroReachablePairsForKind { from_kind, to_kind } => write!(
                f,
                "reachable origin kind pair {} -> {} must have at least one reachable pair",
                from_kind.as_str(),
                to_kind.as_str()
            ),
            Self::DuplicateKindPair { from_kind, to_kind } => write!(
                f,
                "duplicate reachable origin kind pair {} -> {}",
                from_kind.as_str(),
                to_kind.as_str()
            ),
            Self::ReachablePairTotalOverflow => {
                write!(f, "reachable origin kind-pair total overflowed")
            }
            Self::ReachablePairTotalMismatch { declared, actual } => write!(
                f,
                "reachable origin kind-pair total {declared} does not match per-kind sum {actual}"
            ),
        }
    }
}

impl std::error::Error for OriginReachabilitySummaryError {}
