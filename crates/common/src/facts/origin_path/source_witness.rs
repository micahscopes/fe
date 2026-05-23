use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{facts::source_span::SourceSpanExport, origin::OriginExportKey};

use super::witness::OriginPathWitnessExport;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginSourcePathWitnessExportError {
    SourceSpanTargetMismatch {
        path_target: OriginExportKey,
        source_origin: OriginExportKey,
    },
}

impl fmt::Display for OriginSourcePathWitnessExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceSpanTargetMismatch {
                path_target,
                source_origin,
            } => write!(
                f,
                "origin source path witness attaches source span {}:{}:{} to path ending at {}:{}:{}",
                source_origin.kind().as_str(),
                source_origin.owner_key(),
                source_origin.local_key(),
                path_target.kind().as_str(),
                path_target.owner_key(),
                path_target.local_key()
            ),
        }
    }
}

impl std::error::Error for OriginSourcePathWitnessExportError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OriginSourcePathWitnessExport {
    path: OriginPathWitnessExport,
    source_span: SourceSpanExport,
}

impl OriginSourcePathWitnessExport {
    pub fn new(path: OriginPathWitnessExport, source_span: SourceSpanExport) -> Self {
        Self::try_new(path, source_span).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        path: OriginPathWitnessExport,
        source_span: SourceSpanExport,
    ) -> Result<Self, OriginSourcePathWitnessExportError> {
        let path_target = path
            .nodes()
            .last()
            .expect("validated path witness should have a terminal node");
        if path_target != source_span.origin_key() {
            return Err(
                OriginSourcePathWitnessExportError::SourceSpanTargetMismatch {
                    path_target: path_target.clone(),
                    source_origin: source_span.origin_key().clone(),
                },
            );
        }

        Ok(Self { path, source_span })
    }

    pub const fn path(&self) -> &OriginPathWitnessExport {
        &self.path
    }

    pub const fn source_span(&self) -> &SourceSpanExport {
        &self.source_span
    }
}

impl<'de> Deserialize<'de> for OriginSourcePathWitnessExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawSourcePathWitness {
            path: OriginPathWitnessExport,
            source_span: SourceSpanExport,
        }

        let raw = RawSourcePathWitness::deserialize(deserializer)?;
        Self::try_new(raw.path, raw.source_span).map_err(de::Error::custom)
    }
}
