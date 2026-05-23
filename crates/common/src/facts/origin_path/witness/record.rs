use serde::Serialize;

use crate::origin::{OriginExportKey, OriginExportKind, OriginLinkKind};

use super::OriginPathWitnessExportError;

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
