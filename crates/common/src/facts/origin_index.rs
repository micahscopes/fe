use std::collections::BTreeMap;

use crate::origin::OriginExportKey;

use super::{FactId, OriginLinkFact, OriginNodeFact, SourceSpanFact};

mod build;
mod paths;
mod reachability;
mod source_spans;

#[derive(Clone, Debug)]
pub struct OriginFactIndex<'a> {
    nodes_by_id: BTreeMap<FactId, &'a OriginNodeFact>,
    ids_by_key: BTreeMap<OriginExportKey, FactId>,
    outgoing: BTreeMap<FactId, Vec<&'a OriginLinkFact>>,
    source_spans_by_origin: BTreeMap<FactId, Vec<&'a SourceSpanFact>>,
}

impl<'a> OriginFactIndex<'a> {
    pub fn origin_id(&self, key: &OriginExportKey) -> Option<FactId> {
        self.ids_by_key.get(key).copied()
    }

    pub fn origin_key(&self, id: FactId) -> Option<&OriginExportKey> {
        self.nodes_by_id.get(&id).map(|node| node.key())
    }

    pub fn origin_node(&self, id: FactId) -> Option<&OriginNodeFact> {
        self.nodes_by_id.get(&id).copied()
    }

    pub fn outgoing(&self, id: FactId) -> impl Iterator<Item = &'a OriginLinkFact> + '_ {
        self.outgoing
            .get(&id)
            .into_iter()
            .flat_map(|links| links.iter().copied())
    }
}
