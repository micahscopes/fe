use serde::{Deserialize, Deserializer, de};

use crate::origin::{OriginExportKey, OriginExportKind, OriginLinkKind};

use super::record::OriginPathWitnessExport;

impl<'de> Deserialize<'de> for OriginPathWitnessExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawPathWitness::deserialize(deserializer)?;
        OriginPathWitnessExport::try_new(raw.from_kind, raw.to_kind, raw.nodes, raw.links)
            .map_err(de::Error::custom)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPathWitness {
    from_kind: OriginExportKind,
    to_kind: OriginExportKind,
    nodes: Vec<OriginExportKey>,
    links: Vec<OriginLinkKind>,
}
