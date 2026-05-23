use std::collections::HashSet;

use serde::{Deserialize, Deserializer, Serialize, de};

mod origin_count;
mod origin_facts;
mod shape;
mod source_map;
mod validation;

#[cfg(test)]
pub(super) use origin_count::OriginCountError;
pub(super) use origin_count::{
    ORIGIN_PATH_WITNESS_LIMIT, ORIGIN_PATH_WITNESS_PRIORITY, OriginCount,
};
pub(super) use origin_facts::AnalyzeOriginFactReport;
pub(super) use shape::{AnalyzeShapeHashReport, AnalyzeShapeReport};
pub(super) use source_map::{AnalyzeSourceMapReport, AnalyzeSourceMapReportError};

pub(super) const ANALYZE_REPORT_SCHEMA_VERSION: u32 = 1;
pub(super) const ANALYZE_SOURCE_MAP_ALL_SECTIONS: &str = "<all>";

#[derive(Debug, Serialize)]
pub(super) struct AnalyzeReport {
    pub(super) schema_version: u32,
    pub(super) profile: String,
    pub(super) package_kind: AnalyzePackageKind,
    pub(super) targets: Vec<AnalyzeTargetReport>,
}

impl AnalyzeReport {
    pub(super) fn validate(&self) -> Result<(), AnalyzeReportError> {
        if self.schema_version != ANALYZE_REPORT_SCHEMA_VERSION {
            return Err(AnalyzeReportError::UnsupportedSchemaVersion {
                actual: self.schema_version,
                expected: ANALYZE_REPORT_SCHEMA_VERSION,
            });
        }
        if self.profile.is_empty() {
            return Err(AnalyzeReportError::EmptyProfile);
        }
        let mut target_labels = HashSet::new();
        for target in &self.targets {
            target
                .validate()
                .map_err(AnalyzeReportError::InvalidTarget)?;
            if !target_labels.insert(target.label.as_str()) {
                return Err(AnalyzeReportError::DuplicateTargetLabel {
                    label: target.label.clone(),
                });
            }
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for AnalyzeReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawReport {
            schema_version: u32,
            profile: String,
            package_kind: AnalyzePackageKind,
            targets: Vec<AnalyzeTargetReport>,
        }

        let raw = RawReport::deserialize(deserializer)?;
        let report = Self {
            schema_version: raw.schema_version,
            profile: raw.profile,
            package_kind: raw.package_kind,
            targets: raw.targets,
        };
        report.validate().map_err(de::Error::custom)?;
        Ok(report)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AnalyzeReportError {
    UnsupportedSchemaVersion { actual: u32, expected: u32 },
    EmptyProfile,
    DuplicateTargetLabel { label: String },
    InvalidTarget(AnalyzeTargetReportError),
}

impl std::fmt::Display for AnalyzeReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { actual, expected } => write!(
                f,
                "unsupported analyze report schema_version {actual}; expected {expected}"
            ),
            Self::EmptyProfile => write!(f, "analyze report profile must not be empty"),
            Self::DuplicateTargetLabel { label } => {
                write!(
                    f,
                    "analyze report contains duplicate target label `{label}`"
                )
            }
            Self::InvalidTarget(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for AnalyzeReportError {}

common::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum AnalyzePackageKind {
        Runtime => "runtime",
        Tests => "tests",
    }
}

#[derive(Debug, Serialize)]
pub(super) struct AnalyzeTargetReport {
    pub(super) label: String,
    pub(super) runtime_bodies: usize,
    pub(super) runtime_statements: OriginCount,
    pub(super) runtime_terminators: OriginCount,
    pub(super) bodies: Vec<AnalyzeBodyReport>,
    pub(super) source_maps: Vec<AnalyzeSourceMapReport>,
    pub(super) origin_facts: Vec<AnalyzeOriginFactReport>,
    pub(super) shapes: Vec<AnalyzeShapeReport>,
}

impl AnalyzeTargetReport {
    pub(super) fn validate(&self) -> Result<(), AnalyzeTargetReportError> {
        if self.label.is_empty() {
            return Err(AnalyzeTargetReportError::EmptyTargetLabel);
        }
        if self.runtime_bodies != self.bodies.len() {
            return Err(AnalyzeTargetReportError::RuntimeBodyCountMismatch {
                label: self.label.clone(),
                declared: self.runtime_bodies,
                actual: self.bodies.len(),
            });
        }
        let mut body_symbols = HashSet::new();
        for body in &self.bodies {
            body.validate()
                .map_err(|err| AnalyzeTargetReportError::InvalidBody {
                    label: self.label.clone(),
                    err,
                })?;
            if !body_symbols.insert(body.symbol.as_str()) {
                return Err(AnalyzeTargetReportError::DuplicateBodySymbol {
                    label: self.label.clone(),
                    symbol: body.symbol.clone(),
                });
            }
        }

        self.validate_body_origin_sum(
            "runtime_statements",
            self.runtime_statements,
            self.bodies.iter().map(|body| body.statements),
        )?;
        self.validate_body_origin_sum(
            "runtime_terminators",
            self.runtime_terminators,
            self.bodies.iter().map(|body| body.terminators),
        )?;
        Ok(())
    }

    fn validate_body_origin_sum(
        &self,
        field: &'static str,
        declared: OriginCount,
        body_counts: impl IntoIterator<Item = OriginCount>,
    ) -> Result<(), AnalyzeTargetReportError> {
        let actual = sum_body_origin_counts(&self.label, field, body_counts)?;
        if declared == actual {
            Ok(())
        } else {
            Err(AnalyzeTargetReportError::RuntimeOriginCountMismatch {
                label: self.label.clone(),
                field,
                declared,
                actual,
            })
        }
    }
}

impl<'de> Deserialize<'de> for AnalyzeTargetReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawTarget {
            label: String,
            runtime_bodies: usize,
            runtime_statements: OriginCount,
            runtime_terminators: OriginCount,
            bodies: Vec<AnalyzeBodyReport>,
            source_maps: Vec<AnalyzeSourceMapReport>,
            origin_facts: Vec<AnalyzeOriginFactReport>,
            shapes: Vec<AnalyzeShapeReport>,
        }

        let raw = RawTarget::deserialize(deserializer)?;
        let target = Self {
            label: raw.label,
            runtime_bodies: raw.runtime_bodies,
            runtime_statements: raw.runtime_statements,
            runtime_terminators: raw.runtime_terminators,
            bodies: raw.bodies,
            source_maps: raw.source_maps,
            origin_facts: raw.origin_facts,
            shapes: raw.shapes,
        };
        target.validate().map_err(de::Error::custom)?;
        Ok(target)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AnalyzeTargetReportError {
    EmptyTargetLabel,
    InvalidBody {
        label: String,
        err: AnalyzeBodyReportError,
    },
    DuplicateBodySymbol {
        label: String,
        symbol: String,
    },
    RuntimeBodyCountMismatch {
        label: String,
        declared: usize,
        actual: usize,
    },
    RuntimeOriginCountOverflow {
        label: String,
        field: &'static str,
    },
    RuntimeOriginCountMismatch {
        label: String,
        field: &'static str,
        declared: OriginCount,
        actual: OriginCount,
    },
}

impl std::fmt::Display for AnalyzeTargetReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyTargetLabel => write!(f, "analyze target label must not be empty"),
            Self::InvalidBody { label, err } => {
                write!(f, "analyze target `{label}` {err}")
            }
            Self::DuplicateBodySymbol { label, symbol } => write!(
                f,
                "analyze target `{label}` contains duplicate body symbol `{symbol}`"
            ),
            Self::RuntimeBodyCountMismatch {
                label,
                declared,
                actual,
            } => write!(
                f,
                "analyze target `{label}` runtime_bodies {declared} does not match body count {actual}"
            ),
            Self::RuntimeOriginCountOverflow { label, field } => write!(
                f,
                "analyze target `{label}` {field} body count sum overflowed"
            ),
            Self::RuntimeOriginCountMismatch {
                label,
                field,
                declared,
                actual,
            } => write!(
                f,
                "analyze target `{label}` {field} {declared} does not match body sum {actual}"
            ),
        }
    }
}

impl std::error::Error for AnalyzeTargetReportError {}

fn sum_body_origin_counts(
    label: &str,
    field: &'static str,
    counts: impl IntoIterator<Item = OriginCount>,
) -> Result<OriginCount, AnalyzeTargetReportError> {
    counts
        .into_iter()
        .try_fold(OriginCount::default(), |sum, count| {
            sum.checked_add(count).map_err(|_| {
                AnalyzeTargetReportError::RuntimeOriginCountOverflow {
                    label: label.to_string(),
                    field,
                }
            })
        })
}

#[derive(Debug, Serialize)]
pub(super) struct AnalyzeBodyReport {
    pub(super) symbol: String,
    pub(super) statements: OriginCount,
    pub(super) terminators: OriginCount,
}

impl AnalyzeBodyReport {
    pub(super) fn validate(&self) -> Result<(), AnalyzeBodyReportError> {
        if self.symbol.is_empty() {
            return Err(AnalyzeBodyReportError::EmptySymbol);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for AnalyzeBodyReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawBody {
            symbol: String,
            statements: OriginCount,
            terminators: OriginCount,
        }

        let raw = RawBody::deserialize(deserializer)?;
        let body = Self {
            symbol: raw.symbol,
            statements: raw.statements,
            terminators: raw.terminators,
        };
        body.validate().map_err(de::Error::custom)?;
        Ok(body)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AnalyzeBodyReportError {
    EmptySymbol,
}

impl std::fmt::Display for AnalyzeBodyReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySymbol => write!(f, "body symbol must not be empty"),
        }
    }
}

impl std::error::Error for AnalyzeBodyReportError {}
