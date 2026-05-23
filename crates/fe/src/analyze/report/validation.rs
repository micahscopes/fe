use std::collections::HashSet;

use common::facts::{
    TypedFactRelationCount, TypedFactRelationName, TypedFactRelationSet, TypedFactSet,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::analyze) struct EmptyAnalyzeReportField {
    pub(in crate::analyze) report: &'static str,
    pub(in crate::analyze) field: &'static str,
}

impl std::fmt::Display for EmptyAnalyzeReportField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} must not be empty", self.report, self.field)
    }
}

impl std::error::Error for EmptyAnalyzeReportField {}

pub(super) fn validate_non_empty_report_field(
    report: &'static str,
    field: &'static str,
    value: &str,
) -> Result<(), EmptyAnalyzeReportField> {
    if value.is_empty() {
        Err(EmptyAnalyzeReportField { report, field })
    } else {
        Ok(())
    }
}

pub(super) fn validate_relation_counts<E>(
    counts: &[TypedFactRelationCount],
    mut expected_rows: impl FnMut(TypedFactRelationName) -> Option<usize>,
    duplicate_error: impl Fn(TypedFactRelationName) -> E,
    unexpected_error: impl Fn(TypedFactRelationName) -> E,
    mismatch_error: impl Fn(TypedFactRelationName, usize, usize) -> E,
) -> Result<(), E> {
    if let Some(relation) = duplicate_relation_count(counts) {
        return Err(duplicate_error(relation));
    }
    for count in counts {
        let relation = count.relation();
        let declared = count.rows();
        let Some(actual) = expected_rows(relation) else {
            return Err(unexpected_error(relation));
        };
        if declared != actual {
            return Err(mismatch_error(relation, declared, actual));
        }
    }
    Ok(())
}

pub(super) fn validate_relation_tables<E>(
    relations: &TypedFactRelationSet,
    mut expected_rows: impl FnMut(TypedFactRelationName) -> Option<usize>,
    unexpected_error: impl Fn(TypedFactRelationName, usize) -> E,
    mismatch_error: impl Fn(TypedFactRelationName, usize, usize) -> E,
) -> Result<(), E> {
    for relation_table in relations.relations() {
        let relation = relation_table.relation_name();
        let actual = relation_table.row_count();
        match expected_rows(relation) {
            Some(declared) if actual == declared => {}
            Some(declared) => return Err(mismatch_error(relation, declared, actual)),
            None if actual == 0 => {}
            None => return Err(unexpected_error(relation, actual)),
        }
    }
    Ok(())
}

pub(super) fn validate_relation_count_completeness_for_facts<E>(
    counts: &[TypedFactRelationCount],
    facts: &TypedFactSet,
    missing_error: impl Fn(TypedFactRelationName, usize) -> E,
) -> Result<(), E> {
    let present = counts
        .iter()
        .map(TypedFactRelationCount::relation)
        .collect::<HashSet<_>>();
    for relation in facts.relation_export().relations() {
        let rows = relation.row_count();
        if rows > 0 && !present.contains(&relation.relation_name()) {
            return Err(missing_error(relation.relation_name(), rows));
        }
    }
    Ok(())
}

pub(super) fn validate_relation_table_rows_match_facts<E>(
    relations: &TypedFactRelationSet,
    facts: &TypedFactSet,
    mismatch_error: impl Fn(TypedFactRelationName) -> E,
) -> Result<(), E> {
    let expected_relations = facts.relation_export();
    for expected in expected_relations.relations() {
        let relation = expected.relation_name();
        let actual = relations
            .relation(relation)
            .expect("validated typed fact relation set should contain every declared relation");
        let mut actual_rows = actual.rows().to_vec();
        let mut expected_rows = expected.rows().to_vec();
        actual_rows.sort();
        expected_rows.sort();
        if actual_rows != expected_rows {
            return Err(mismatch_error(relation));
        }
    }
    Ok(())
}

fn duplicate_relation_count(counts: &[TypedFactRelationCount]) -> Option<TypedFactRelationName> {
    let mut seen = HashSet::new();
    for count in counts {
        if !seen.insert(count.relation()) {
            return Some(count.relation());
        }
    }
    None
}
