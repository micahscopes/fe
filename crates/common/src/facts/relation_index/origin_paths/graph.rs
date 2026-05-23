mod links;
mod nodes;
mod ordinals;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{
    facts::{TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName},
    origin::{OriginExportKey, OriginLinkKind},
};

use super::super::TypedFactRelationIndex;
use links::origin_outgoing_by_id;
use nodes::{origin_node_ids_in_fact_order, origin_node_keys_by_id};
pub(super) use ordinals::origin_node_id_ordinal;

pub(super) struct OriginRelationGraph<'a> {
    node_ids: Vec<&'a str>,
    keys_by_id: BTreeMap<&'a str, OriginExportKey>,
    outgoing: BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>,
}

impl<'a> OriginRelationGraph<'a> {
    pub(super) fn from_index(
        index: &TypedFactRelationIndex<'a>,
    ) -> Result<Self, TypedFactRelationError> {
        Ok(Self {
            node_ids: origin_node_ids_in_fact_order(index)?,
            keys_by_id: origin_node_keys_by_id(index)?,
            outgoing: origin_outgoing_by_id(index)?,
        })
    }

    pub(super) fn node_ids(&self) -> &[&'a str] {
        &self.node_ids
    }

    pub(super) fn keys_by_id(&self) -> &BTreeMap<&'a str, OriginExportKey> {
        &self.keys_by_id
    }

    pub(super) fn outgoing(&self) -> &BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>> {
        &self.outgoing
    }

    pub(super) fn id_for_key(&self, key: &OriginExportKey) -> Option<&'a str> {
        self.keys_by_id
            .iter()
            .find_map(|(id, candidate)| (candidate == key).then_some(*id))
    }

    pub(super) fn reachable_origin_ids_from(
        &self,
        start: &'a str,
    ) -> Result<Vec<&'a str>, TypedFactRelationError> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);

        while let Some(current) = queue.pop_front() {
            if let Some(targets) = self.outgoing.get(current) {
                for (target, _) in targets {
                    if seen.insert(*target) {
                        queue.push_back(*target);
                    }
                }
            }
        }

        let mut ids = seen
            .into_iter()
            .map(|id| {
                origin_node_id_ordinal(
                    TypedFactRelationName::OriginLink,
                    TypedFactRelationColumnName::To,
                    id,
                )
                .map(|ordinal| (ordinal, id))
            })
            .collect::<Result<Vec<_>, _>>()?;
        ids.sort_by_key(|(ordinal, _)| *ordinal);
        Ok(ids.into_iter().map(|(_, id)| id).collect())
    }
}
