use crate::facts::FactId;

pub(in crate::facts::relation_export) fn fact_id_cell(id: FactId) -> String {
    id.stable_key()
}

pub(in crate::facts::relation_export) fn shape_hash_node_cell(node: Option<FactId>) -> String {
    node.map(fact_id_cell)
        .unwrap_or_else(|| "graph".to_string())
}
