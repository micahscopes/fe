use std::collections::BTreeMap;

use crate::facts::{
    TypedFactRelation, TypedFactRelationName, TypedFactRelationSet, TypedFactSet,
    typed_fact_relation_schemas,
};

use super::row::TypedFactRelationRowExport;

pub(in crate::facts) fn typed_fact_relation_export(facts: &TypedFactSet) -> TypedFactRelationSet {
    let mut rows = TypedFactRelationRows::new();

    for fact in facts.facts() {
        rows.push(fact.relation_row());
    }

    rows.into_relation_set()
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
            .get_mut(&row.relation())
            .expect("typed fact relation rows should be initialized from declared schemas")
            .push(row.into_cells());
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
