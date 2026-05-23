use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::origin::OriginExportKind;

use super::{
    OriginReachabilitySummaryError, OriginReachableKindPairSummary,
    validation::validate_origin_reachability_summary,
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct OriginReachabilitySummary {
    reachable_pairs: usize,
    reachable_pairs_by_kind: Vec<OriginReachableKindPairSummary>,
}

impl OriginReachabilitySummary {
    pub fn new(
        reachable_pairs: usize,
        reachable_pairs_by_kind: Vec<OriginReachableKindPairSummary>,
    ) -> Self {
        Self::try_new(reachable_pairs, reachable_pairs_by_kind)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        reachable_pairs: usize,
        reachable_pairs_by_kind: Vec<OriginReachableKindPairSummary>,
    ) -> Result<Self, OriginReachabilitySummaryError> {
        validate_origin_reachability_summary(reachable_pairs, &reachable_pairs_by_kind)?;
        Ok(Self {
            reachable_pairs,
            reachable_pairs_by_kind,
        })
    }

    pub(in crate::facts) fn from_pair_counts(
        pair_counts: BTreeMap<(OriginExportKind, OriginExportKind), usize>,
    ) -> Self {
        let reachable_pairs = pair_counts.values().sum();
        let reachable_pairs_by_kind = pair_counts
            .into_iter()
            .map(|((from_kind, to_kind), reachable_pairs)| {
                OriginReachableKindPairSummary::new(from_kind, to_kind, reachable_pairs)
            })
            .collect();
        Self::new(reachable_pairs, reachable_pairs_by_kind)
    }

    pub const fn reachable_pairs(&self) -> usize {
        self.reachable_pairs
    }

    pub fn reachable_pairs_by_kind(&self) -> &[OriginReachableKindPairSummary] {
        &self.reachable_pairs_by_kind
    }

    pub fn pair_count(&self, from_kind: OriginExportKind, to_kind: OriginExportKind) -> usize {
        self.reachable_pairs_by_kind
            .iter()
            .find(|pair| pair.from_kind() == from_kind && pair.to_kind() == to_kind)
            .map(OriginReachableKindPairSummary::reachable_pairs)
            .unwrap_or_default()
    }
}

impl<'de> Deserialize<'de> for OriginReachabilitySummary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawSummary {
            reachable_pairs: usize,
            reachable_pairs_by_kind: Vec<OriginReachableKindPairSummary>,
        }

        let raw = RawSummary::deserialize(deserializer)?;
        Self::try_new(raw.reachable_pairs, raw.reachable_pairs_by_kind).map_err(de::Error::custom)
    }
}
