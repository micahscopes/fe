use crate::facts::{TypedFact, TypedFactRelationName};

use super::cell::{fact_id_cell, shape_hash_node_cell};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::facts::relation_export) struct TypedFactRelationRowExport {
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

    pub(in crate::facts::relation_export) const fn relation(&self) -> TypedFactRelationName {
        self.relation
    }

    pub(in crate::facts::relation_export) fn into_cells(self) -> Vec<String> {
        self.cells
    }
}

impl TypedFact {
    pub(in crate::facts::relation_export) fn relation_row(&self) -> TypedFactRelationRowExport {
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
