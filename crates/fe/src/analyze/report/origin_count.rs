use common::origin::OriginExportKind;
use mir::RuntimeOriginSource;
use serde::{Deserialize, Deserializer, Serialize, de};

pub(in crate::analyze) const ORIGIN_PATH_WITNESS_LIMIT: usize = 12;
pub(in crate::analyze) const ORIGIN_PATH_WITNESS_PRIORITY: &[(
    OriginExportKind,
    OriginExportKind,
)] = &[
    (OriginExportKind::Semantic, OriginExportKind::RuntimeStmt),
    (
        OriginExportKind::Semantic,
        OriginExportKind::RuntimeTerminator,
    ),
    (OriginExportKind::Semantic, OriginExportKind::BytecodePc),
    (
        OriginExportKind::RuntimeStmt,
        OriginExportKind::SonatinaInst,
    ),
    (
        OriginExportKind::RuntimeTerminator,
        OriginExportKind::SonatinaInst,
    ),
    (OriginExportKind::RuntimeStmt, OriginExportKind::BytecodePc),
    (
        OriginExportKind::RuntimeTerminator,
        OriginExportKind::BytecodePc,
    ),
    (OriginExportKind::SonatinaInst, OriginExportKind::BytecodePc),
    (
        OriginExportKind::BytecodeUnmapped,
        OriginExportKind::BytecodePc,
    ),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub(in crate::analyze) struct OriginCount {
    pub(in crate::analyze) total: usize,
    pub(in crate::analyze) semantic: usize,
    pub(in crate::analyze) synthetic: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::analyze) enum OriginCountError {
    TotalOverflow,
    TotalMismatch { declared: usize, actual: usize },
}

impl std::fmt::Display for OriginCountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TotalOverflow => write!(f, "origin count total overflowed"),
            Self::TotalMismatch { declared, actual } => write!(
                f,
                "origin count total {declared} does not match semantic plus synthetic count {actual}"
            ),
        }
    }
}

impl std::error::Error for OriginCountError {}

impl OriginCount {
    pub(in crate::analyze) fn try_new(
        total: usize,
        semantic: usize,
        synthetic: usize,
    ) -> Result<Self, OriginCountError> {
        let actual = semantic
            .checked_add(synthetic)
            .ok_or(OriginCountError::TotalOverflow)?;
        if total != actual {
            return Err(OriginCountError::TotalMismatch {
                declared: total,
                actual,
            });
        }
        Ok(Self {
            total,
            semantic,
            synthetic,
        })
    }

    pub(in crate::analyze) fn push(&mut self, source: RuntimeOriginSource<'_>) {
        self.total += 1;
        match source {
            RuntimeOriginSource::Semantic(_) => self.semantic += 1,
            RuntimeOriginSource::Synthetic => self.synthetic += 1,
        }
    }

    pub(in crate::analyze) fn extend(&mut self, other: Self) {
        self.total += other.total;
        self.semantic += other.semantic;
        self.synthetic += other.synthetic;
    }

    pub(in crate::analyze) fn checked_add(self, other: Self) -> Result<Self, OriginCountError> {
        let total = self
            .total
            .checked_add(other.total)
            .ok_or(OriginCountError::TotalOverflow)?;
        let semantic = self
            .semantic
            .checked_add(other.semantic)
            .ok_or(OriginCountError::TotalOverflow)?;
        let synthetic = self
            .synthetic
            .checked_add(other.synthetic)
            .ok_or(OriginCountError::TotalOverflow)?;
        Self::try_new(total, semantic, synthetic)
    }
}

impl std::fmt::Display for OriginCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "total={} semantic={} synthetic={}",
            self.total, self.semantic, self.synthetic
        )
    }
}

impl<'de> Deserialize<'de> for OriginCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCount {
            total: usize,
            semantic: usize,
            synthetic: usize,
        }

        let raw = RawCount::deserialize(deserializer)?;
        Self::try_new(raw.total, raw.semantic, raw.synthetic).map_err(de::Error::custom)
    }
}
