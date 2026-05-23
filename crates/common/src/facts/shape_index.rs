mod build;
mod lookup;

use std::collections::BTreeMap;

use crate::shape::ShapeNodeId;

use super::{FactId, ShapeHashFact, ShapeHashFactKey, ShapeNodeFact};

#[derive(Clone, Debug)]
pub struct ShapeFactIndex<'a> {
    nodes_by_id: BTreeMap<FactId, &'a ShapeNodeFact>,
    ids_by_source_id: BTreeMap<ShapeNodeId, FactId>,
    ids_by_stable_key: BTreeMap<String, FactId>,
    hashes_by_key: BTreeMap<ShapeHashFactKey, &'a ShapeHashFact>,
}
