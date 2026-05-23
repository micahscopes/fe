use std::collections::BTreeSet;

use super::{OriginReachabilitySummaryError, OriginReachableKindPairSummary};

pub(super) fn validate_origin_reachability_summary(
    reachable_pairs: usize,
    reachable_pairs_by_kind: &[OriginReachableKindPairSummary],
) -> Result<(), OriginReachabilitySummaryError> {
    let mut seen_pairs = BTreeSet::new();
    let mut actual = 0usize;
    for pair in reachable_pairs_by_kind {
        if pair.reachable_pairs() == 0 {
            return Err(OriginReachabilitySummaryError::ZeroReachablePairsForKind {
                from_kind: pair.from_kind(),
                to_kind: pair.to_kind(),
            });
        }
        if !seen_pairs.insert((pair.from_kind(), pair.to_kind())) {
            return Err(OriginReachabilitySummaryError::DuplicateKindPair {
                from_kind: pair.from_kind(),
                to_kind: pair.to_kind(),
            });
        }
        actual = actual
            .checked_add(pair.reachable_pairs())
            .ok_or(OriginReachabilitySummaryError::ReachablePairTotalOverflow)?;
    }
    if actual != reachable_pairs {
        return Err(OriginReachabilitySummaryError::ReachablePairTotalMismatch {
            declared: reachable_pairs,
            actual,
        });
    }
    Ok(())
}
