use super::bytecode_origins::{BytecodeOriginRecord, BytecodeOriginSource};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BytecodeOriginCoverage {
    total: usize,
    sonatina_post_opt: usize,
    sonatina_backend_prepared: usize,
    unmapped: usize,
}

impl BytecodeOriginCoverage {
    pub const fn new(
        sonatina_post_opt: usize,
        sonatina_backend_prepared: usize,
        unmapped: usize,
    ) -> Self {
        Self {
            total: sonatina_post_opt + sonatina_backend_prepared + unmapped,
            sonatina_post_opt,
            sonatina_backend_prepared,
            unmapped,
        }
    }

    pub const fn total(self) -> usize {
        self.total
    }

    pub const fn sonatina_post_opt(self) -> usize {
        self.sonatina_post_opt
    }

    pub const fn sonatina_backend_prepared(self) -> usize {
        self.sonatina_backend_prepared
    }

    pub const fn unmapped(self) -> usize {
        self.unmapped
    }

    pub const fn classified_total(self) -> usize {
        self.sonatina_post_opt + self.sonatina_backend_prepared + self.unmapped
    }

    pub const fn is_partitioned(self) -> bool {
        self.total == self.classified_total()
    }

    pub const fn is_empty(self) -> bool {
        self.total == 0
    }
}

pub(super) fn bytecode_origin_coverage_for_records<'a, 'db: 'a>(
    records: impl IntoIterator<Item = &'a BytecodeOriginRecord<'db>>,
) -> BytecodeOriginCoverage {
    let mut coverage = BytecodeOriginCoverage::default();
    for record in records {
        coverage.total += 1;
        match record.source() {
            BytecodeOriginSource::SonatinaPostOpt(_) => coverage.sonatina_post_opt += 1,
            BytecodeOriginSource::SonatinaBackendPrepared(_) => {
                coverage.sonatina_backend_prepared += 1;
            }
            BytecodeOriginSource::Unmapped(_) => coverage.unmapped += 1,
        }
    }
    coverage
}
