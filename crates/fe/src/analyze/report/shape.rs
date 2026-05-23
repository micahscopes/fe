use std::collections::{HashMap, HashSet};

use common::{
    facts::{
        OwnedTypedFactSetExport, ShapeHashDigest, ShapeHashScope, TypedFactRelationCount,
        TypedFactRelationName, TypedFactRelationSet, TypedFactSet,
    },
    shape::ShapeDimension,
};
use serde::{Deserialize, Deserializer, Serialize, de};

use super::validation::{
    EmptyAnalyzeReportField, validate_non_empty_report_field,
    validate_relation_count_completeness_for_facts, validate_relation_counts,
    validate_relation_table_rows_match_facts, validate_relation_tables,
};

#[derive(Debug, Serialize)]
pub(in crate::analyze) struct AnalyzeShapeReport {
    pub(in crate::analyze) scope: String,
    pub(in crate::analyze) label: String,
    pub(in crate::analyze) shape_nodes: usize,
    pub(in crate::analyze) shape_fields: usize,
    pub(in crate::analyze) shape_children: usize,
    pub(in crate::analyze) shape_edges: usize,
    pub(in crate::analyze) trace_events: usize,
    pub(in crate::analyze) data_flows: usize,
    pub(in crate::analyze) graph_hashes: Vec<AnalyzeShapeHashReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(in crate::analyze) relation_counts: Vec<TypedFactRelationCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::analyze) facts: Option<OwnedTypedFactSetExport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::analyze) relation_tables: Option<TypedFactRelationSet>,
}

impl AnalyzeShapeReport {
    pub(in crate::analyze) fn validate(&self) -> Result<(), AnalyzeShapeReportError> {
        validate_non_empty_report_field("analyze shape", "scope", &self.scope)
            .map_err(AnalyzeShapeReportError::EmptyField)?;
        validate_non_empty_report_field("analyze shape", "label", &self.label)
            .map_err(AnalyzeShapeReportError::EmptyField)?;

        self.validate_graph_hashes()?;
        let expected_shape_hashes = expected_shape_hash_count(self.shape_nodes)?;

        let facts = self
            .facts
            .as_ref()
            .map(|facts| TypedFactSet::new(facts.facts().to_vec()));
        if let Some(facts) = &facts {
            let shape_nodes = facts.shape_nodes().count();
            let shape_fields = facts.shape_fields().count();
            let shape_children = facts.shape_children().count();
            let shape_edges = facts.shape_edges().count();
            let trace_events = facts.trace_events().count();
            let data_flows = facts.data_flows().count();
            let shape_hashes = self.validate_shape_hash_facts(facts)?;
            let shape_fact_total = checked_shape_fact_total(
                shape_nodes,
                shape_fields,
                shape_children,
                shape_edges,
                trace_events,
                data_flows,
                shape_hashes,
            )?;

            validate_shape_fact_count("shape_nodes", self.shape_nodes, shape_nodes)?;
            validate_shape_fact_count("shape_fields", self.shape_fields, shape_fields)?;
            validate_shape_fact_count("shape_children", self.shape_children, shape_children)?;
            validate_shape_fact_count("shape_edges", self.shape_edges, shape_edges)?;
            validate_shape_fact_count("trace_events", self.trace_events, trace_events)?;
            validate_shape_fact_count("data_flows", self.data_flows, data_flows)?;
            validate_shape_fact_count("shape_hashes", expected_shape_hashes, shape_hashes)?;
            if shape_fact_total != facts.facts().len() {
                return Err(AnalyzeShapeReportError::NonShapeFactRows {
                    shape_total: shape_fact_total,
                    fact_rows: facts.facts().len(),
                });
            }
        }

        validate_relation_counts(
            &self.relation_counts,
            |relation| self.expected_shape_relation_count(relation, expected_shape_hashes),
            |relation| AnalyzeShapeReportError::DuplicateRelationCount { relation },
            |relation| AnalyzeShapeReportError::UnexpectedRelationCount { relation },
            |relation, declared, actual| AnalyzeShapeReportError::RelationCountMismatch {
                relation,
                declared,
                actual,
            },
        )?;
        if let Some(facts) = &facts {
            validate_relation_count_completeness_for_facts(
                &self.relation_counts,
                facts,
                |relation, rows| AnalyzeShapeReportError::MissingRelationCount { relation, rows },
            )?;
        }

        if let Some(relations) = &self.relation_tables {
            validate_relation_tables(
                relations,
                |relation| self.expected_shape_relation_count(relation, expected_shape_hashes),
                |relation, rows| AnalyzeShapeReportError::UnexpectedRelationTable {
                    relation,
                    rows,
                },
                |relation, declared, actual| AnalyzeShapeReportError::RelationTableCountMismatch {
                    relation,
                    declared,
                    actual,
                },
            )?;
            let Some(facts) = &facts else {
                return Err(AnalyzeShapeReportError::RelationTablesWithoutFacts);
            };
            validate_relation_table_rows_match_facts(relations, facts, |relation| {
                AnalyzeShapeReportError::RelationTableRowsMismatch { relation }
            })?;
        }

        Ok(())
    }

    fn validate_graph_hashes(&self) -> Result<(), AnalyzeShapeReportError> {
        let mut seen = HashSet::new();
        for hash in &self.graph_hashes {
            if !seen.insert(hash.dimension) {
                return Err(AnalyzeShapeReportError::DuplicateGraphHashDimension {
                    dimension: hash.dimension,
                });
            }
        }
        for dimension in ShapeDimension::ALL {
            if !seen.contains(&dimension) {
                return Err(AnalyzeShapeReportError::MissingGraphHashDimension { dimension });
            }
        }
        Ok(())
    }

    fn validate_shape_hash_facts(
        &self,
        facts: &TypedFactSet,
    ) -> Result<usize, AnalyzeShapeReportError> {
        let graph_hashes = self
            .graph_hashes
            .iter()
            .map(|hash| (hash.dimension, &hash.digest_hex))
            .collect::<HashMap<_, _>>();
        let mut fact_graph_hashes = HashMap::new();
        let mut shape_hashes = 0usize;

        for hash in facts.shape_hashes() {
            shape_hashes = shape_hashes
                .checked_add(1)
                .ok_or(AnalyzeShapeReportError::CountOverflow)?;
            if hash.scope() == ShapeHashScope::Graph && hash.node().is_none() {
                fact_graph_hashes.insert(hash.dimension(), hash.digest());
            }
        }

        for hash in &self.graph_hashes {
            let Some(fact_digest) = fact_graph_hashes.get(&hash.dimension) else {
                return Err(AnalyzeShapeReportError::MissingGraphShapeHashFact {
                    dimension: hash.dimension,
                });
            };
            let report_digest = graph_hashes
                .get(&hash.dimension)
                .expect("graph hash dimensions should be validated before fact coverage");
            if fact_digest != report_digest {
                return Err(AnalyzeShapeReportError::GraphHashDigestMismatch {
                    dimension: hash.dimension,
                    report_digest: report_digest.to_string(),
                    fact_digest: fact_digest.to_string(),
                });
            }
        }

        Ok(shape_hashes)
    }

    fn expected_shape_relation_count(
        &self,
        relation: TypedFactRelationName,
        expected_shape_hashes: usize,
    ) -> Option<usize> {
        if !relation.is_shape_relation() {
            return None;
        }
        let expected = match relation {
            TypedFactRelationName::ShapeNode => self.shape_nodes,
            TypedFactRelationName::ShapeField => self.shape_fields,
            TypedFactRelationName::ShapeChild => self.shape_children,
            TypedFactRelationName::ShapeEdge => self.shape_edges,
            TypedFactRelationName::TraceEvent => self.trace_events,
            TypedFactRelationName::DataFlow => self.data_flows,
            TypedFactRelationName::ShapeHash => expected_shape_hashes,
            _ => unreachable!("shape relation should be handled above"),
        };
        Some(expected)
    }
}

impl<'de> Deserialize<'de> for AnalyzeShapeReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawReport {
            scope: String,
            label: String,
            shape_nodes: usize,
            shape_fields: usize,
            shape_children: usize,
            shape_edges: usize,
            trace_events: usize,
            data_flows: usize,
            graph_hashes: Vec<AnalyzeShapeHashReport>,
            #[serde(default)]
            relation_counts: Vec<TypedFactRelationCount>,
            facts: Option<OwnedTypedFactSetExport>,
            relation_tables: Option<TypedFactRelationSet>,
        }

        let raw = RawReport::deserialize(deserializer)?;
        let report = Self {
            scope: raw.scope,
            label: raw.label,
            shape_nodes: raw.shape_nodes,
            shape_fields: raw.shape_fields,
            shape_children: raw.shape_children,
            shape_edges: raw.shape_edges,
            trace_events: raw.trace_events,
            data_flows: raw.data_flows,
            graph_hashes: raw.graph_hashes,
            relation_counts: raw.relation_counts,
            facts: raw.facts,
            relation_tables: raw.relation_tables,
        };
        report.validate().map_err(de::Error::custom)?;
        Ok(report)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::analyze) struct AnalyzeShapeHashReport {
    pub(in crate::analyze) dimension: ShapeDimension,
    pub(in crate::analyze) digest_hex: ShapeHashDigest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::analyze) enum AnalyzeShapeReportError {
    EmptyField(EmptyAnalyzeReportField),
    CountOverflow,
    DuplicateGraphHashDimension {
        dimension: ShapeDimension,
    },
    MissingGraphHashDimension {
        dimension: ShapeDimension,
    },
    FactCountMismatch {
        field: &'static str,
        declared: usize,
        actual: usize,
    },
    NonShapeFactRows {
        shape_total: usize,
        fact_rows: usize,
    },
    MissingGraphShapeHashFact {
        dimension: ShapeDimension,
    },
    GraphHashDigestMismatch {
        dimension: ShapeDimension,
        report_digest: String,
        fact_digest: String,
    },
    DuplicateRelationCount {
        relation: TypedFactRelationName,
    },
    UnexpectedRelationCount {
        relation: TypedFactRelationName,
    },
    RelationCountMismatch {
        relation: TypedFactRelationName,
        declared: usize,
        actual: usize,
    },
    MissingRelationCount {
        relation: TypedFactRelationName,
        rows: usize,
    },
    UnexpectedRelationTable {
        relation: TypedFactRelationName,
        rows: usize,
    },
    RelationTableCountMismatch {
        relation: TypedFactRelationName,
        declared: usize,
        actual: usize,
    },
    RelationTableRowsMismatch {
        relation: TypedFactRelationName,
    },
    RelationTablesWithoutFacts,
}

impl std::fmt::Display for AnalyzeShapeReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyField(err) => err.fmt(f),
            Self::CountOverflow => write!(f, "analyze shape report count overflowed"),
            Self::DuplicateGraphHashDimension { dimension } => write!(
                f,
                "analyze shape graph_hashes contains duplicate dimension {}",
                dimension.as_str()
            ),
            Self::MissingGraphHashDimension { dimension } => write!(
                f,
                "analyze shape graph_hashes is missing dimension {}",
                dimension.as_str()
            ),
            Self::FactCountMismatch {
                field,
                declared,
                actual,
            } => write!(
                f,
                "analyze shape {field} {declared} does not match typed fact count {actual}"
            ),
            Self::NonShapeFactRows {
                shape_total,
                fact_rows,
            } => write!(
                f,
                "analyze shape typed fact total {shape_total} does not cover all typed fact rows {fact_rows}"
            ),
            Self::MissingGraphShapeHashFact { dimension } => write!(
                f,
                "analyze shape is missing graph {} shape_hash row",
                dimension.as_str()
            ),
            Self::GraphHashDigestMismatch {
                dimension,
                report_digest,
                fact_digest,
            } => write!(
                f,
                "analyze shape graph_hashes {} digest {report_digest} does not match typed fact digest {fact_digest}",
                dimension.as_str()
            ),
            Self::DuplicateRelationCount { relation } => write!(
                f,
                "analyze shape relation_counts contains duplicate relation {}",
                relation.as_str()
            ),
            Self::UnexpectedRelationCount { relation } => write!(
                f,
                "analyze shape relation_counts contains non-shape relation {}",
                relation.as_str()
            ),
            Self::RelationCountMismatch {
                relation,
                declared,
                actual,
            } => write!(
                f,
                "analyze shape relation count {}={declared} does not match report count {actual}",
                relation.as_str()
            ),
            Self::MissingRelationCount { relation, rows } => write!(
                f,
                "analyze shape relation_counts is missing {}={rows}",
                relation.as_str()
            ),
            Self::UnexpectedRelationTable { relation, rows } => write!(
                f,
                "analyze shape relation table {} has non-shape rows {rows}",
                relation.as_str()
            ),
            Self::RelationTableCountMismatch {
                relation,
                declared,
                actual,
            } => write!(
                f,
                "analyze shape relation table {} has {actual} rows, expected {declared}",
                relation.as_str()
            ),
            Self::RelationTableRowsMismatch { relation } => write!(
                f,
                "analyze shape relation table {} rows do not match typed facts",
                relation.as_str()
            ),
            Self::RelationTablesWithoutFacts => write!(
                f,
                "analyze shape relation_tables require emitted typed facts"
            ),
        }
    }
}

impl std::error::Error for AnalyzeShapeReportError {}

fn expected_shape_hash_count(shape_nodes: usize) -> Result<usize, AnalyzeShapeReportError> {
    shape_nodes
        .checked_mul(2)
        .and_then(|node_scopes| node_scopes.checked_add(1))
        .and_then(|hash_scopes| hash_scopes.checked_mul(ShapeDimension::ALL.len()))
        .ok_or(AnalyzeShapeReportError::CountOverflow)
}

fn checked_shape_fact_total(
    shape_nodes: usize,
    shape_fields: usize,
    shape_children: usize,
    shape_edges: usize,
    trace_events: usize,
    data_flows: usize,
    shape_hashes: usize,
) -> Result<usize, AnalyzeShapeReportError> {
    [
        shape_nodes,
        shape_fields,
        shape_children,
        shape_edges,
        trace_events,
        data_flows,
        shape_hashes,
    ]
    .into_iter()
    .try_fold(0usize, |sum, value| {
        sum.checked_add(value)
            .ok_or(AnalyzeShapeReportError::CountOverflow)
    })
}

fn validate_shape_fact_count(
    field: &'static str,
    declared: usize,
    actual: usize,
) -> Result<(), AnalyzeShapeReportError> {
    if declared == actual {
        Ok(())
    } else {
        Err(shape_fact_count_mismatch(field, declared, actual))
    }
}

fn shape_fact_count_mismatch(
    field: &'static str,
    declared: usize,
    actual: usize,
) -> AnalyzeShapeReportError {
    AnalyzeShapeReportError::FactCountMismatch {
        field,
        declared,
        actual,
    }
}
