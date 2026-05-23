use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::origin::{OriginExportKey, OriginExportKind, OriginLinkKind};

use super::{
    FactId, FactNamespace, FactNamespaceError, ids::validated_fact_namespace,
    source_span::SourceSpanExport,
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

    pub(super) fn from_pair_counts(
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
            .find(|pair| pair.from_kind == from_kind && pair.to_kind == to_kind)
            .map(|pair| pair.reachable_pairs)
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

fn validate_origin_reachability_summary(
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginPathError {
    EmptyPath,
    LengthMismatch { nodes: usize, links: usize },
    WrongNamespace(FactNamespaceError),
}

impl From<FactNamespaceError> for OriginPathError {
    fn from(err: FactNamespaceError) -> Self {
        Self::WrongNamespace(err)
    }
}

impl fmt::Display for OriginPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => write!(f, "origin path must contain at least one node"),
            Self::LengthMismatch { nodes, links } => write!(
                f,
                "origin path has {nodes} nodes and {links} links; expected exactly one more node than link"
            ),
            Self::WrongNamespace(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for OriginPathError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OriginPath {
    nodes: Vec<FactId>,
    links: Vec<OriginLinkKind>,
}

impl OriginPath {
    pub fn new(nodes: Vec<FactId>, links: Vec<OriginLinkKind>) -> Self {
        Self::try_new(nodes, links).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        nodes: Vec<FactId>,
        links: Vec<OriginLinkKind>,
    ) -> Result<Self, OriginPathError> {
        if nodes.is_empty() {
            return Err(OriginPathError::EmptyPath);
        }
        if nodes.len() != links.len() + 1 {
            return Err(OriginPathError::LengthMismatch {
                nodes: nodes.len(),
                links: links.len(),
            });
        }
        for node in &nodes {
            validated_fact_namespace(*node, FactNamespace::OriginNode)?;
        }
        Ok(Self { nodes, links })
    }

    pub fn nodes(&self) -> &[FactId] {
        &self.nodes
    }

    pub fn links(&self) -> &[OriginLinkKind] {
        &self.links
    }
}

impl<'de> Deserialize<'de> for OriginPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawPath {
            nodes: Vec<FactId>,
            links: Vec<OriginLinkKind>,
        }

        let raw = RawPath::deserialize(deserializer)?;
        Self::try_new(raw.nodes, raw.links).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginKindPathWitness {
    from_kind: OriginExportKind,
    to_kind: OriginExportKind,
    path: OriginPath,
}

impl OriginKindPathWitness {
    pub fn new(from_kind: OriginExportKind, to_kind: OriginExportKind, path: OriginPath) -> Self {
        Self {
            from_kind,
            to_kind,
            path,
        }
    }

    pub const fn from_kind(&self) -> OriginExportKind {
        self.from_kind
    }

    pub const fn to_kind(&self) -> OriginExportKind {
        self.to_kind
    }

    pub fn path(&self) -> &OriginPath {
        &self.path
    }
}

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OriginPathWitnessExport {
    from_kind: OriginExportKind,
    to_kind: OriginExportKind,
    nodes: Vec<OriginExportKey>,
    links: Vec<OriginLinkKind>,
}

impl OriginPathWitnessExport {
    pub fn new(
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
        nodes: Vec<OriginExportKey>,
        links: Vec<OriginLinkKind>,
    ) -> Self {
        Self::try_new(from_kind, to_kind, nodes, links).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
        nodes: Vec<OriginExportKey>,
        links: Vec<OriginLinkKind>,
    ) -> Result<Self, OriginPathWitnessExportError> {
        if nodes.is_empty() {
            return Err(OriginPathWitnessExportError::EmptyPath);
        }
        if nodes.len() != links.len() + 1 {
            return Err(OriginPathWitnessExportError::LengthMismatch {
                nodes: nodes.len(),
                links: links.len(),
            });
        }
        let actual_from = nodes
            .first()
            .expect("non-empty path witness should have a first node")
            .kind();
        if actual_from != from_kind {
            return Err(OriginPathWitnessExportError::FromKindMismatch {
                expected: from_kind,
                actual: actual_from,
            });
        }
        let actual_to = nodes
            .last()
            .expect("non-empty path witness should have a last node")
            .kind();
        if actual_to != to_kind {
            return Err(OriginPathWitnessExportError::ToKindMismatch {
                expected: to_kind,
                actual: actual_to,
            });
        }

        Ok(Self {
            from_kind,
            to_kind,
            nodes,
            links,
        })
    }

    pub const fn from_kind(&self) -> OriginExportKind {
        self.from_kind
    }

    pub const fn to_kind(&self) -> OriginExportKind {
        self.to_kind
    }

    pub fn nodes(&self) -> &[OriginExportKey] {
        &self.nodes
    }

    pub fn links(&self) -> &[OriginLinkKind] {
        &self.links
    }
}

impl<'de> Deserialize<'de> for OriginPathWitnessExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawPathWitness {
            from_kind: OriginExportKind,
            to_kind: OriginExportKind,
            nodes: Vec<OriginExportKey>,
            links: Vec<OriginLinkKind>,
        }

        let raw = RawPathWitness::deserialize(deserializer)?;
        Self::try_new(raw.from_kind, raw.to_kind, raw.nodes, raw.links).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginSourcePathWitnessExportError {
    SourceSpanTargetMismatch {
        path_target: OriginExportKey,
        source_origin: OriginExportKey,
    },
}

impl fmt::Display for OriginSourcePathWitnessExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceSpanTargetMismatch {
                path_target,
                source_origin,
            } => write!(
                f,
                "origin source path witness attaches source span {}:{}:{} to path ending at {}:{}:{}",
                source_origin.kind().as_str(),
                source_origin.owner_key(),
                source_origin.local_key(),
                path_target.kind().as_str(),
                path_target.owner_key(),
                path_target.local_key()
            ),
        }
    }
}

impl std::error::Error for OriginSourcePathWitnessExportError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OriginSourcePathWitnessExport {
    path: OriginPathWitnessExport,
    source_span: SourceSpanExport,
}

impl OriginSourcePathWitnessExport {
    pub fn new(path: OriginPathWitnessExport, source_span: SourceSpanExport) -> Self {
        Self::try_new(path, source_span).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        path: OriginPathWitnessExport,
        source_span: SourceSpanExport,
    ) -> Result<Self, OriginSourcePathWitnessExportError> {
        let path_target = path
            .nodes()
            .last()
            .expect("validated path witness should have a terminal node");
        if path_target != source_span.origin_key() {
            return Err(
                OriginSourcePathWitnessExportError::SourceSpanTargetMismatch {
                    path_target: path_target.clone(),
                    source_origin: source_span.origin_key().clone(),
                },
            );
        }

        Ok(Self { path, source_span })
    }

    pub const fn path(&self) -> &OriginPathWitnessExport {
        &self.path
    }

    pub const fn source_span(&self) -> &SourceSpanExport {
        &self.source_span
    }
}

impl<'de> Deserialize<'de> for OriginSourcePathWitnessExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawSourcePathWitness {
            path: OriginPathWitnessExport,
            source_span: SourceSpanExport,
        }

        let raw = RawSourcePathWitness::deserialize(deserializer)?;
        Self::try_new(raw.path, raw.source_span).map_err(de::Error::custom)
    }
}
