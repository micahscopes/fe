use common::diagnostics::Span;
use hir::{analysis::diagnostics::SpannedHirAnalysisDb, origin::SemanticOrigin};
use mir::{RuntimeOriginSource, RuntimePackageOrigins, RuntimeStmtOrigin, RuntimeTerminatorOrigin};

use super::{
    BytecodeOriginRecord, BytecodeOriginSource, BytecodePcOrigin, BytecodeUnmappedReason,
    SonatinaBackendPreparedOriginSource, SonatinaOriginSource, SonatinaPostOptOriginSource,
    SonatinaSyntheticOrigin, SonatinaUnmappedReason,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BytecodeSourceResolution<'db> {
    record: BytecodeOriginRecord<'db>,
    result: BytecodeSourceResolutionResult<'db>,
}

impl<'db> BytecodeSourceResolution<'db> {
    pub const fn new(
        record: BytecodeOriginRecord<'db>,
        result: BytecodeSourceResolutionResult<'db>,
    ) -> Self {
        Self { record, result }
    }

    pub const fn record(&self) -> &BytecodeOriginRecord<'db> {
        &self.record
    }

    pub const fn origin(&self) -> &BytecodePcOrigin {
        self.record.origin()
    }

    pub const fn result(&self) -> &BytecodeSourceResolutionResult<'db> {
        &self.result
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BytecodeSourceResolutionResult<'db> {
    SourceSpan {
        semantic: SemanticOrigin<'db>,
        span: Span,
    },
    SemanticSpanMissing(SemanticOrigin<'db>),
    RuntimeStmtMissing(RuntimeStmtOrigin<'db>),
    RuntimeTerminatorMissing(RuntimeTerminatorOrigin<'db>),
    RuntimeSynthetic,
    SonatinaSynthetic(SonatinaSyntheticOrigin),
    SonatinaUnmapped(SonatinaUnmappedReason),
    PostPreOptSnapshotGap,
    BytecodeUnmapped(BytecodeUnmappedReason),
}

pub(super) fn resolve_bytecode_source<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    runtime_origins: &RuntimePackageOrigins<'db>,
    source: BytecodeOriginSource<'db>,
) -> BytecodeSourceResolutionResult<'db> {
    match source {
        BytecodeOriginSource::SonatinaPostOpt(post_opt) => {
            resolve_post_opt_source(db, runtime_origins, post_opt.source())
        }
        BytecodeOriginSource::SonatinaBackendPrepared(backend_prepared) => {
            resolve_backend_prepared_source(backend_prepared.source())
        }
        BytecodeOriginSource::Unmapped(reason) => {
            BytecodeSourceResolutionResult::BytecodeUnmapped(reason)
        }
    }
}

fn resolve_post_opt_source<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    runtime_origins: &RuntimePackageOrigins<'db>,
    source: SonatinaPostOptOriginSource<'db>,
) -> BytecodeSourceResolutionResult<'db> {
    match source {
        SonatinaPostOptOriginSource::SameInstId(pre_opt) => {
            resolve_pre_opt_source(db, runtime_origins, pre_opt.source())
        }
        SonatinaPostOptOriginSource::CreatedOrUnmatchedAfterPreOptSnapshot => {
            BytecodeSourceResolutionResult::PostPreOptSnapshotGap
        }
    }
}

fn resolve_backend_prepared_source<'db>(
    source: SonatinaBackendPreparedOriginSource,
) -> BytecodeSourceResolutionResult<'db> {
    match source {
        SonatinaBackendPreparedOriginSource::MissingPostOptSnapshotRecord => {
            BytecodeSourceResolutionResult::PostPreOptSnapshotGap
        }
    }
}

fn resolve_pre_opt_source<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    runtime_origins: &RuntimePackageOrigins<'db>,
    source: SonatinaOriginSource<'db>,
) -> BytecodeSourceResolutionResult<'db> {
    match source {
        SonatinaOriginSource::RuntimeStmt(origin) => runtime_origins
            .stmt_origin(origin)
            .map(|record| resolve_runtime_source(db, record.source()))
            .unwrap_or(BytecodeSourceResolutionResult::RuntimeStmtMissing(origin)),
        SonatinaOriginSource::RuntimeTerminator(origin) => runtime_origins
            .terminator_origin(origin)
            .map(|record| resolve_runtime_source(db, record.source()))
            .unwrap_or(BytecodeSourceResolutionResult::RuntimeTerminatorMissing(
                origin,
            )),
        SonatinaOriginSource::Synthetic(origin) => {
            BytecodeSourceResolutionResult::SonatinaSynthetic(origin)
        }
        SonatinaOriginSource::Unmapped(reason) => {
            BytecodeSourceResolutionResult::SonatinaUnmapped(reason)
        }
    }
}

fn resolve_runtime_source<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    source: RuntimeOriginSource<'db>,
) -> BytecodeSourceResolutionResult<'db> {
    match source {
        RuntimeOriginSource::Semantic(origin) => origin
            .resolve_source_span(db)
            .map(|span| BytecodeSourceResolutionResult::SourceSpan {
                semantic: origin,
                span,
            })
            .unwrap_or(BytecodeSourceResolutionResult::SemanticSpanMissing(origin)),
        RuntimeOriginSource::Synthetic => BytecodeSourceResolutionResult::RuntimeSynthetic,
    }
}
