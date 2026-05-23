use std::collections::BTreeMap;

use super::{
    FactId, TypedFact, TypedFactRelation, TypedFactRelationName, TypedFactRelationSet,
    typed_fact_relation_schemas,
};

pub(super) fn typed_fact_relation_export(facts: &super::TypedFactSet) -> TypedFactRelationSet {
    let mut rows = TypedFactRelationRows::new();

    for fact in facts.facts() {
        rows.push(fact.relation_row());
    }

    rows.into_relation_set()
}

impl TypedFact {
    fn relation_row(&self) -> TypedFactRelationRowExport {
        match self {
            Self::OriginNode(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::OriginNode,
                vec![
                    fact_id_cell(fact.id()),
                    fact.key().kind().as_str().to_string(),
                    fact.key().owner_key().to_string(),
                    fact.key().local_key().to_string(),
                ],
            ),
            Self::OriginLink(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::OriginLink,
                vec![
                    fact_id_cell(fact.from()),
                    fact_id_cell(fact.to()),
                    fact.kind().as_str().to_string(),
                ],
            ),
            Self::SourceSpan(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::SourceSpan,
                vec![
                    fact_id_cell(fact.origin()),
                    fact.span_kind().as_str().to_string(),
                    fact.file().to_string(),
                    fact.start_byte().to_string(),
                    fact.end_byte().to_string(),
                    fact.start_line().to_string(),
                    fact.start_col().to_string(),
                    fact.end_line().to_string(),
                    fact.end_col().to_string(),
                ],
            ),
            Self::ShapeNode(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::ShapeNode,
                vec![
                    fact_id_cell(fact.id()),
                    fact.source_id().as_u32().to_string(),
                    fact.stable_key().to_string(),
                    fact.kind().to_string(),
                ],
            ),
            Self::ShapeField(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::ShapeField,
                vec![
                    fact_id_cell(fact.node()),
                    fact.dimension().as_str().to_string(),
                    fact.name().to_string(),
                    fact.value().to_string(),
                ],
            ),
            Self::ShapeChild(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::ShapeChild,
                vec![
                    fact_id_cell(fact.parent()),
                    fact_id_cell(fact.child()),
                    fact.label().to_string(),
                    fact.order().to_string(),
                ],
            ),
            Self::ShapeEdge(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::ShapeEdge,
                vec![
                    fact_id_cell(fact.from()),
                    fact_id_cell(fact.to()),
                    fact.label().to_string(),
                ],
            ),
            Self::TraceEvent(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::TraceEvent,
                vec![
                    fact_id_cell(fact.node()),
                    fact.event_kind().to_string(),
                    fact.value().to_string(),
                ],
            ),
            Self::DataFlow(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::DataFlow,
                vec![
                    fact_id_cell(fact.source()),
                    fact_id_cell(fact.target()),
                    fact.kind().to_string(),
                ],
            ),
            Self::ShapeHash(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::ShapeHash,
                vec![
                    shape_hash_node_cell(fact.node()),
                    fact.scope().as_str().to_string(),
                    fact.dimension().as_str().to_string(),
                    fact.digest_hex().to_string(),
                ],
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TypedFactRelationRowExport {
    relation: TypedFactRelationName,
    cells: Vec<String>,
}

impl TypedFactRelationRowExport {
    fn new(relation: TypedFactRelationName, cells: Vec<String>) -> Self {
        let expected_width = relation.schema().columns().len();
        assert_eq!(
            cells.len(),
            expected_width,
            "typed fact relation `{}` row should match declared schema width",
            relation.as_str()
        );

        Self { relation, cells }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TypedFactRelationRows {
    rows_by_relation: BTreeMap<TypedFactRelationName, Vec<Vec<String>>>,
}

impl TypedFactRelationRows {
    fn new() -> Self {
        Self {
            rows_by_relation: typed_fact_relation_schemas()
                .iter()
                .map(|schema| (schema.name(), Vec::new()))
                .collect(),
        }
    }

    fn push(&mut self, row: TypedFactRelationRowExport) {
        self.rows_by_relation
            .get_mut(&row.relation)
            .expect("typed fact relation rows should be initialized from declared schemas")
            .push(row.cells);
    }

    fn into_relation_set(mut self) -> TypedFactRelationSet {
        let relations = typed_fact_relation_schemas()
            .iter()
            .map(|schema| {
                let mut rows = self
                    .rows_by_relation
                    .remove(&schema.name())
                    .expect("typed fact relation rows should contain every declared schema");
                rows.sort();
                typed_fact_relation_from_schema(schema.name(), rows)
            })
            .collect::<Vec<_>>();

        debug_assert!(
            self.rows_by_relation.is_empty(),
            "typed fact relation rows should not contain undeclared schemas"
        );

        TypedFactRelationSet::new(relations)
            .expect("typed fact relation export should produce a complete declared schema")
    }
}

fn typed_fact_relation_from_schema(
    name: TypedFactRelationName,
    rows: Vec<Vec<String>>,
) -> TypedFactRelation {
    TypedFactRelation::new(name, rows)
        .expect("typed fact relation export should use declared relation schemas")
}

fn fact_id_cell(id: FactId) -> String {
    id.stable_key()
}

fn shape_hash_node_cell(node: Option<FactId>) -> String {
    node.map(fact_id_cell)
        .unwrap_or_else(|| "graph".to_string())
}
