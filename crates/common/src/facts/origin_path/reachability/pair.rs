use serde::{Deserialize, Deserializer, Serialize, de};

use crate::origin::OriginExportKind;

use super::OriginReachabilitySummaryError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OriginReachableKindPairSummary {
    from_kind: OriginExportKind,
    to_kind: OriginExportKind,
    reachable_pairs: usize,
}

impl OriginReachableKindPairSummary {
    pub fn new(
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
        reachable_pairs: usize,
    ) -> Self {
        Self::try_new(from_kind, to_kind, reachable_pairs).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
        reachable_pairs: usize,
    ) -> Result<Self, OriginReachabilitySummaryError> {
        if reachable_pairs == 0 {
            return Err(OriginReachabilitySummaryError::ZeroReachablePairsForKind {
                from_kind,
                to_kind,
            });
        }
        Ok(Self {
            from_kind,
            to_kind,
            reachable_pairs,
        })
    }

    pub const fn from_kind(&self) -> OriginExportKind {
        self.from_kind
    }

    pub const fn to_kind(&self) -> OriginExportKind {
        self.to_kind
    }

    pub const fn reachable_pairs(&self) -> usize {
        self.reachable_pairs
    }
}

impl<'de> Deserialize<'de> for OriginReachableKindPairSummary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawPair {
            from_kind: OriginExportKind,
            to_kind: OriginExportKind,
            reachable_pairs: usize,
        }

        let raw = RawPair::deserialize(deserializer)?;
        Self::try_new(raw.from_kind, raw.to_kind, raw.reachable_pairs).map_err(de::Error::custom)
    }
}
