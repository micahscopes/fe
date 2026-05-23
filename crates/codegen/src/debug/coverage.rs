use serde::{Deserialize, Deserializer, Serialize, de};

use crate::origin::{BytecodeOriginCoverage, SonatinaPostOptOriginCoverage};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct BytecodeOriginCoverageExport {
    total: usize,
    sonatina_post_opt: usize,
    sonatina_backend_prepared: usize,
    unmapped: usize,
}

impl From<BytecodeOriginCoverage> for BytecodeOriginCoverageExport {
    fn from(coverage: BytecodeOriginCoverage) -> Self {
        Self {
            total: coverage.total(),
            sonatina_post_opt: coverage.sonatina_post_opt(),
            sonatina_backend_prepared: coverage.sonatina_backend_prepared(),
            unmapped: coverage.unmapped(),
        }
    }
}

impl BytecodeOriginCoverageExport {
    pub const fn total(&self) -> usize {
        self.total
    }

    pub const fn sonatina_post_opt(&self) -> usize {
        self.sonatina_post_opt
    }

    pub const fn sonatina_backend_prepared(&self) -> usize {
        self.sonatina_backend_prepared
    }

    pub const fn unmapped(&self) -> usize {
        self.unmapped
    }

    pub const fn classified_total(&self) -> usize {
        self.sonatina_post_opt + self.sonatina_backend_prepared + self.unmapped
    }
}

impl<'de> Deserialize<'de> for BytecodeOriginCoverageExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCoverage {
            total: usize,
            sonatina_post_opt: usize,
            sonatina_backend_prepared: usize,
            unmapped: usize,
        }

        let raw = RawCoverage::deserialize(deserializer)?;
        let classified_total = raw
            .sonatina_post_opt
            .checked_add(raw.sonatina_backend_prepared)
            .and_then(|total| total.checked_add(raw.unmapped))
            .ok_or_else(|| {
                de::Error::custom("bytecode_origin_coverage classified total overflows usize")
            })?;
        if raw.total != classified_total {
            return Err(de::Error::custom(format!(
                "bytecode_origin_coverage total {} does not match classified total {}",
                raw.total, classified_total
            )));
        }

        Ok(Self {
            total: raw.total,
            sonatina_post_opt: raw.sonatina_post_opt,
            sonatina_backend_prepared: raw.sonatina_backend_prepared,
            unmapped: raw.unmapped,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SonatinaPostOptOriginCoverageExport {
    total: usize,
    same_inst_id: usize,
    created_or_unmatched_after_preopt_snapshot: usize,
    pre_opt_snapshot_losses: usize,
    observed_pre_opt_total: usize,
}

impl From<SonatinaPostOptOriginCoverage> for SonatinaPostOptOriginCoverageExport {
    fn from(coverage: SonatinaPostOptOriginCoverage) -> Self {
        Self {
            total: coverage.total(),
            same_inst_id: coverage.same_inst_id(),
            created_or_unmatched_after_preopt_snapshot: coverage
                .created_or_unmatched_after_preopt_snapshot(),
            pre_opt_snapshot_losses: coverage.pre_opt_snapshot_losses(),
            observed_pre_opt_total: coverage.observed_pre_opt_total(),
        }
    }
}

impl SonatinaPostOptOriginCoverageExport {
    pub const fn total(&self) -> usize {
        self.total
    }

    pub const fn same_inst_id(&self) -> usize {
        self.same_inst_id
    }

    pub const fn created_or_unmatched_after_preopt_snapshot(&self) -> usize {
        self.created_or_unmatched_after_preopt_snapshot
    }

    pub const fn pre_opt_snapshot_losses(&self) -> usize {
        self.pre_opt_snapshot_losses
    }

    pub const fn observed_pre_opt_total(&self) -> usize {
        self.observed_pre_opt_total
    }

    pub const fn post_opt_classified_total(&self) -> usize {
        self.same_inst_id + self.created_or_unmatched_after_preopt_snapshot
    }

    pub const fn computed_observed_pre_opt_total(&self) -> usize {
        self.same_inst_id + self.pre_opt_snapshot_losses
    }
}

impl<'de> Deserialize<'de> for SonatinaPostOptOriginCoverageExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCoverage {
            total: usize,
            same_inst_id: usize,
            created_or_unmatched_after_preopt_snapshot: usize,
            pre_opt_snapshot_losses: usize,
            observed_pre_opt_total: usize,
        }

        let raw = RawCoverage::deserialize(deserializer)?;
        let classified_total = raw
            .same_inst_id
            .checked_add(raw.created_or_unmatched_after_preopt_snapshot)
            .ok_or_else(|| {
                de::Error::custom("post_opt_origin_coverage classified total overflows usize")
            })?;
        if raw.total != classified_total {
            return Err(de::Error::custom(format!(
                "post_opt_origin_coverage total {} does not match classified total {}",
                raw.total, classified_total
            )));
        }
        let observed_pre_opt_total = raw
            .same_inst_id
            .checked_add(raw.pre_opt_snapshot_losses)
            .ok_or_else(|| {
                de::Error::custom("post_opt_origin_coverage observed pre-opt total overflows usize")
            })?;
        if raw.observed_pre_opt_total != observed_pre_opt_total {
            return Err(de::Error::custom(format!(
                "post_opt_origin_coverage observed_pre_opt_total {} does not match same_inst_id plus pre_opt_snapshot_losses {}",
                raw.observed_pre_opt_total, observed_pre_opt_total
            )));
        }

        Ok(Self {
            total: raw.total,
            same_inst_id: raw.same_inst_id,
            created_or_unmatched_after_preopt_snapshot: raw
                .created_or_unmatched_after_preopt_snapshot,
            pre_opt_snapshot_losses: raw.pre_opt_snapshot_losses,
            observed_pre_opt_total: raw.observed_pre_opt_total,
        })
    }
}
