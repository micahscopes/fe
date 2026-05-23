use serde::{Deserialize, Deserializer, Serialize, de};

use crate::origin::{OriginExportKey, OriginLinkKind};

use super::{FactId, FactNamespace, FactNamespaceError, ids::validated_fact_namespace};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OriginNodeFact {
    id: FactId,
    key: OriginExportKey,
}

impl OriginNodeFact {
    pub fn new(id: FactId, key: OriginExportKey) -> Self {
        Self::try_new(id, key).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(id: FactId, key: OriginExportKey) -> Result<Self, FactNamespaceError> {
        Ok(Self {
            id: validated_fact_namespace(id, FactNamespace::OriginNode)?,
            key,
        })
    }

    pub const fn id(&self) -> FactId {
        self.id
    }

    pub const fn key(&self) -> &OriginExportKey {
        &self.key
    }
}

impl<'de> Deserialize<'de> for OriginNodeFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawOriginNode {
            id: FactId,
            key: OriginExportKey,
        }

        let raw = RawOriginNode::deserialize(deserializer)?;
        Self::try_new(raw.id, raw.key).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OriginLinkFact {
    from: FactId,
    to: FactId,
    kind: OriginLinkKind,
}

impl OriginLinkFact {
    pub fn new(from: FactId, to: FactId, kind: OriginLinkKind) -> Self {
        Self::try_new(from, to, kind).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        from: FactId,
        to: FactId,
        kind: OriginLinkKind,
    ) -> Result<Self, FactNamespaceError> {
        Ok(Self {
            from: validated_fact_namespace(from, FactNamespace::OriginNode)?,
            to: validated_fact_namespace(to, FactNamespace::OriginNode)?,
            kind,
        })
    }

    pub const fn from(&self) -> FactId {
        self.from
    }

    pub const fn to(&self) -> FactId {
        self.to
    }

    pub const fn kind(&self) -> OriginLinkKind {
        self.kind
    }
}

impl<'de> Deserialize<'de> for OriginLinkFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawOriginLink {
            from: FactId,
            to: FactId,
            kind: OriginLinkKind,
        }

        let raw = RawOriginLink::deserialize(deserializer)?;
        Self::try_new(raw.from, raw.to, raw.kind).map_err(de::Error::custom)
    }
}
