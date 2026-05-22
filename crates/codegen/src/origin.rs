use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use common::{
    diagnostics::Span,
    facts::{TypedFactSet, try_origin_graph_facts},
    origin::{OriginExportKey, OriginExportKind, OriginKey, OriginLink, OriginLinkKind},
};
use hir::{analysis::diagnostics::SpannedHirAnalysisDb, origin::SemanticOrigin};
use mir::{
    RuntimeInstance, RuntimeOriginOwnerKey, RuntimeOriginSource, RuntimePackageOrigins,
    RuntimeStmtOrigin, RuntimeTerminatorOrigin, runtime_stmt_export_key,
    runtime_terminator_export_key, runtime_terminator_local_key,
};
use sonatina_codegen::object::{ObjectArtifact, PcMapEntry, UnmappedReason};
use sonatina_ir::{InstId, Module, module::FuncRef};

/// Fe-owned wrapper around Sonatina's frontend provenance label map.
///
/// Sonatina's external observability API still uses "provenance"; the origin
/// overhaul keeps that spelling at the dependency boundary and uses origin
/// terminology inside Fe-owned APIs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrontendOriginLabelMap {
    inner: sonatina_codegen::object::FrontendProvenanceMap,
}

impl FrontendOriginLabelMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_if_absent(&mut self, function: FuncRef, inst: InstId, label: String) {
        self.inner.entry((function, inst)).or_insert(label);
    }

    pub fn as_sonatina_frontend_provenance(
        &self,
    ) -> &sonatina_codegen::object::FrontendProvenanceMap {
        &self.inner
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SonatinaInstStage {
    PreOpt,
    PostOpt,
    BackendPrepared,
}

impl SonatinaInstStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PreOpt => "pre_opt",
            Self::PostOpt => "post_opt",
            Self::BackendPrepared => "backend_prepared",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SonatinaInstLocal {
    stage: SonatinaInstStage,
    inst: InstId,
}

impl SonatinaInstLocal {
    pub const fn new(stage: SonatinaInstStage, inst: InstId) -> Self {
        Self { stage, inst }
    }

    pub const fn stage(self) -> SonatinaInstStage {
        self.stage
    }

    pub const fn inst(self) -> InstId {
        self.inst
    }
}

/// Origin key for a Sonatina instruction. `InstId` is function-local, so the
/// owning `FuncRef` is part of the key. The local key also carries the
/// compilation stage so pre-opt, post-opt, and backend-prepared IDs cannot be
/// confused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SonatinaInstOrigin {
    key: OriginKey<FuncRef, SonatinaInstLocal>,
}

impl SonatinaInstOrigin {
    pub const fn new(stage: SonatinaInstStage, function: FuncRef, inst: InstId) -> Self {
        Self {
            key: OriginKey::new(function, SonatinaInstLocal::new(stage, inst)),
        }
    }

    pub const fn pre_opt(function: FuncRef, inst: InstId) -> Self {
        Self::new(SonatinaInstStage::PreOpt, function, inst)
    }

    pub const fn post_opt(function: FuncRef, inst: InstId) -> Self {
        Self::new(SonatinaInstStage::PostOpt, function, inst)
    }

    pub const fn backend_prepared(function: FuncRef, inst: InstId) -> Self {
        Self::new(SonatinaInstStage::BackendPrepared, function, inst)
    }

    pub fn function(self) -> FuncRef {
        self.key.into_parts().0
    }

    pub fn inst(self) -> InstId {
        self.key.into_parts().1.inst()
    }

    pub fn stage(self) -> SonatinaInstStage {
        self.key.into_parts().1.stage()
    }

    pub fn key(self) -> OriginKey<FuncRef, SonatinaInstLocal> {
        self.key
    }
}

pub fn sonatina_inst_export_key(
    origin: SonatinaInstOrigin,
    stable_function_key: &SonatinaFunctionExportKey,
) -> OriginExportKey {
    OriginExportKey::new(
        OriginExportKind::SonatinaInst,
        stable_function_key.as_str(),
        format!("{}:inst:{}", origin.stage().as_str(), origin.inst().0),
    )
}

pub fn sonatina_synthetic_export_key(origin: SonatinaSyntheticOrigin) -> OriginExportKey {
    OriginExportKey::new(
        OriginExportKind::SonatinaSynthetic,
        "sonatina",
        origin.as_str(),
    )
}

common::define_origin_string_key! {
    pub struct BytecodeObjectKey;
}

common::define_origin_string_key! {
    pub struct BytecodeSectionNameKey;
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BytecodeSectionKey {
    object: BytecodeObjectKey,
    section: BytecodeSectionNameKey,
}

impl BytecodeSectionKey {
    pub fn new(object: BytecodeObjectKey, section: BytecodeSectionNameKey) -> Self {
        Self { object, section }
    }

    pub fn object(&self) -> &BytecodeObjectKey {
        &self.object
    }

    pub fn section(&self) -> &str {
        self.section.as_str()
    }

    pub fn section_key(&self) -> &BytecodeSectionNameKey {
        &self.section
    }

    pub fn export_owner_key(self) -> String {
        format!(
            "object:{}:section:{}",
            self.object.as_str(),
            self.section.as_str()
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BytecodePcRange {
    start: u32,
    end: u32,
}

impl BytecodePcRange {
    /// Creates a non-empty half-open bytecode PC range: `[start, end)`.
    pub fn new(start: u32, end: u32) -> Option<Self> {
        (start < end).then_some(Self { start, end })
    }

    pub const fn start(self) -> u32 {
        self.start
    }

    pub const fn end(self) -> u32 {
        self.end
    }

    pub fn export_local_key(self) -> String {
        format!("pc:{}..{}", self.start, self.end)
    }
}

/// Origin key for a bytecode PC range. PC offsets are section-local in
/// Sonatina observability, so both object and section are part of the owner.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BytecodePcOrigin {
    key: OriginKey<BytecodeSectionKey, BytecodePcRange>,
}

impl BytecodePcOrigin {
    pub fn new(section: BytecodeSectionKey, range: BytecodePcRange) -> Self {
        Self {
            key: OriginKey::new(section, range),
        }
    }

    pub fn section(&self) -> &BytecodeSectionKey {
        self.key.owner()
    }

    pub fn range(&self) -> BytecodePcRange {
        *self.key.local()
    }

    pub fn key(self) -> OriginKey<BytecodeSectionKey, BytecodePcRange> {
        self.key
    }
}

pub fn bytecode_pc_export_key(origin: BytecodePcOrigin) -> OriginExportKey {
    let (section, range) = origin.key().into_parts();
    OriginExportKey::new(
        OriginExportKind::BytecodePc,
        section.export_owner_key(),
        range.export_local_key(),
    )
}

pub fn bytecode_unmapped_export_key(reason: BytecodeUnmappedReason) -> OriginExportKey {
    OriginExportKey::new(
        OriginExportKind::BytecodeUnmapped,
        "bytecode",
        reason.as_str(),
    )
}

common::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum SonatinaSyntheticOrigin {
        Prologue => "prologue",
        PostPreOptSnapshotGap => "post_preopt_snapshot_gap",
        PreOptSnapshotLoss => "pre_opt_snapshot_loss",
    }
}

common::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum SonatinaUnmappedReason {
        InsertedOutsideLoweringSegment => "inserted_outside_lowering_segment",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SonatinaOriginSource<'db> {
    RuntimeStmt(RuntimeStmtOrigin<'db>),
    RuntimeTerminator(RuntimeTerminatorOrigin<'db>),
    Synthetic(SonatinaSyntheticOrigin),
    Unmapped(SonatinaUnmappedReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SonatinaInstOriginRecord<'db> {
    origin: SonatinaInstOrigin,
    source: SonatinaOriginSource<'db>,
}

impl<'db> SonatinaInstOriginRecord<'db> {
    pub fn new(origin: SonatinaInstOrigin, source: SonatinaOriginSource<'db>) -> Self {
        assert_eq!(
            origin.stage(),
            SonatinaInstStage::PreOpt,
            "pre-opt Sonatina origin records must use pre-opt instruction origins"
        );
        Self { origin, source }
    }

    pub const fn origin(self) -> SonatinaInstOrigin {
        self.origin
    }

    pub const fn source(self) -> SonatinaOriginSource<'db> {
        self.source
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SonatinaOriginNode<'db> {
    RuntimeStmt(RuntimeStmtOrigin<'db>),
    RuntimeTerminator(RuntimeTerminatorOrigin<'db>),
    Synthetic(SonatinaSyntheticOrigin),
    SonatinaInst(SonatinaInstOrigin),
}

common::define_origin_graph_type! {
    pub struct SonatinaOriginGraph<'db>(SonatinaOriginNode<'db>);
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SonatinaFunctionOrigins<'db> {
    function: FuncRef,
    runtime_instance: RuntimeInstance<'db>,
    records: Vec<SonatinaInstOriginRecord<'db>>,
}

impl<'db> SonatinaFunctionOrigins<'db> {
    pub fn new(function: FuncRef, runtime_instance: RuntimeInstance<'db>) -> Self {
        Self {
            function,
            runtime_instance,
            records: Vec::new(),
        }
    }

    pub const fn function(&self) -> FuncRef {
        self.function
    }

    pub const fn runtime_instance(&self) -> RuntimeInstance<'db> {
        self.runtime_instance
    }

    pub fn records(&self) -> &[SonatinaInstOriginRecord<'db>] {
        &self.records
    }

    pub fn record_for_inst(&self, inst: InstId) -> Option<SonatinaInstOriginRecord<'db>> {
        self.records
            .iter()
            .copied()
            .find(|record| record.origin().inst() == inst)
    }

    pub fn push_inst(&mut self, inst: InstId, source: SonatinaOriginSource<'db>) {
        assert!(
            !self.has_inst(inst),
            "Sonatina instruction origin recorded more than once"
        );
        self.records.push(SonatinaInstOriginRecord::new(
            SonatinaInstOrigin::pre_opt(self.function, inst),
            source,
        ));
    }

    pub fn has_inst(&self, inst: InstId) -> bool {
        self.records
            .iter()
            .any(|record| record.origin().inst() == inst)
    }

    pub fn retain_insts(&mut self, mut keep: impl FnMut(InstId) -> bool) {
        self.records.retain(|record| keep(record.origin().inst()));
    }

    pub fn coverage(&self) -> SonatinaOriginCoverage {
        let mut coverage = SonatinaOriginCoverage::default();
        for record in &self.records {
            coverage.total += 1;
            match record.source() {
                SonatinaOriginSource::RuntimeStmt(_) => coverage.runtime_stmt += 1,
                SonatinaOriginSource::RuntimeTerminator(_) => coverage.runtime_terminator += 1,
                SonatinaOriginSource::Synthetic(_) => coverage.synthetic += 1,
                SonatinaOriginSource::Unmapped(_) => coverage.unmapped += 1,
            }
        }
        coverage
    }

    pub fn origin_graph(&self) -> SonatinaOriginGraph<'db> {
        let mut graph = SonatinaOriginGraph::new();
        for record in &self.records {
            let target = SonatinaOriginNode::SonatinaInst(record.origin());
            match record.source() {
                SonatinaOriginSource::RuntimeStmt(origin) => graph.push(
                    SonatinaOriginNode::RuntimeStmt(origin),
                    target,
                    OriginLinkKind::Lowered,
                ),
                SonatinaOriginSource::RuntimeTerminator(origin) => graph.push(
                    SonatinaOriginNode::RuntimeTerminator(origin),
                    target,
                    OriginLinkKind::Lowered,
                ),
                SonatinaOriginSource::Synthetic(origin) => graph.push(
                    SonatinaOriginNode::Synthetic(origin),
                    target,
                    OriginLinkKind::Synthetic,
                ),
                SonatinaOriginSource::Unmapped(_) => {}
            }
        }
        graph
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SonatinaOriginCoverage {
    total: usize,
    runtime_stmt: usize,
    runtime_terminator: usize,
    synthetic: usize,
    unmapped: usize,
}

impl SonatinaOriginCoverage {
    pub const fn new(
        runtime_stmt: usize,
        runtime_terminator: usize,
        synthetic: usize,
        unmapped: usize,
    ) -> Self {
        Self {
            total: runtime_stmt + runtime_terminator + synthetic + unmapped,
            runtime_stmt,
            runtime_terminator,
            synthetic,
            unmapped,
        }
    }

    pub const fn total(self) -> usize {
        self.total
    }

    pub const fn runtime_stmt(self) -> usize {
        self.runtime_stmt
    }

    pub const fn runtime_terminator(self) -> usize {
        self.runtime_terminator
    }

    pub const fn synthetic(self) -> usize {
        self.synthetic
    }

    pub const fn unmapped(self) -> usize {
        self.unmapped
    }

    pub const fn classified_total(self) -> usize {
        self.runtime_stmt + self.runtime_terminator + self.synthetic + self.unmapped
    }

    pub const fn is_partitioned(self) -> bool {
        self.total == self.classified_total()
    }

    pub const fn is_empty(self) -> bool {
        self.total == 0
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SonatinaPackageOrigins<'db> {
    functions: Vec<SonatinaFunctionOrigins<'db>>,
}

impl<'db> SonatinaPackageOrigins<'db> {
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
        }
    }

    pub fn push_function(&mut self, origins: SonatinaFunctionOrigins<'db>) {
        assert!(
            !self
                .functions
                .iter()
                .any(|existing| existing.function() == origins.function()),
            "Sonatina package origins cannot contain the same function more than once"
        );
        self.functions.push(origins);
    }

    pub fn functions(&self) -> &[SonatinaFunctionOrigins<'db>] {
        &self.functions
    }

    pub fn records(&self) -> impl Iterator<Item = SonatinaInstOriginRecord<'db>> + '_ {
        self.functions
            .iter()
            .flat_map(|function| function.records().iter().copied())
    }

    pub fn record_for_inst(
        &self,
        function: FuncRef,
        inst: InstId,
    ) -> Option<SonatinaInstOriginRecord<'db>> {
        self.functions
            .iter()
            .find(|origins| origins.function() == function)
            .and_then(|origins| origins.record_for_inst(inst))
    }
}

common::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum BytecodeUnmappedReason {
        NoIrInst => "no_ir_inst",
        LabelOrFixupOnly => "label_or_fixup_only",
        Synthetic => "synthetic",
        Unknown => "unknown",
    }
}

impl From<UnmappedReason> for BytecodeUnmappedReason {
    fn from(reason: UnmappedReason) -> Self {
        match reason {
            UnmappedReason::NoIrInst => Self::NoIrInst,
            UnmappedReason::LabelOrFixupOnly => Self::LabelOrFixupOnly,
            UnmappedReason::Synthetic => Self::Synthetic,
            UnmappedReason::Unknown => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SonatinaPostOptOriginSource<'db> {
    /// The post-opt instruction still has the same function-local `InstId` as
    /// a pre-opt instruction. This is a snapshot identity join, not a record of
    /// which optimization pass preserved or rewrote it.
    SameInstId(SonatinaInstOriginRecord<'db>),
    /// The instruction was not present in the pre-opt snapshot under the same
    /// `InstId`. Without Sonatina pass-event hooks, Fe cannot currently
    /// distinguish optimizer-created, rewritten, and unmatched post-opt cases.
    CreatedOrUnmatchedAfterPreOptSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SonatinaBackendPreparedOriginSource {
    /// A bytecode PC-map entry referenced an instruction ID that was not present
    /// in the optimized Sonatina snapshot. The current Sonatina API does not
    /// expose a prepared-instruction snapshot between optimization and bytecode
    /// emission, so this remains a conservative backend-prepared classification.
    MissingPostOptSnapshotRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SonatinaBackendPreparedOriginRecord {
    origin: SonatinaInstOrigin,
    source: SonatinaBackendPreparedOriginSource,
}

impl SonatinaBackendPreparedOriginRecord {
    pub fn new(origin: SonatinaInstOrigin, source: SonatinaBackendPreparedOriginSource) -> Self {
        assert_eq!(
            origin.stage(),
            SonatinaInstStage::BackendPrepared,
            "backend-prepared Sonatina origin records must use backend-prepared instruction origins"
        );
        Self { origin, source }
    }

    pub const fn origin(self) -> SonatinaInstOrigin {
        self.origin
    }

    pub const fn source(self) -> SonatinaBackendPreparedOriginSource {
        self.source
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SonatinaPreOptSnapshotLossReason {
    /// The pre-opt instruction has no post-opt instruction with the same
    /// function-local `InstId`. This is a conservative snapshot classification:
    /// the precise pass event could be deletion, replacement, merge, split, or
    /// another rewrite until Sonatina exposes pass-level origin hooks.
    ElidedOrRewrittenBeforePostOptSnapshot,
}

impl SonatinaPreOptSnapshotLossReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ElidedOrRewrittenBeforePostOptSnapshot => {
                "elided_or_rewritten_before_postopt_snapshot"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SonatinaPreOptSnapshotLossRecord<'db> {
    pre_opt: SonatinaInstOriginRecord<'db>,
    reason: SonatinaPreOptSnapshotLossReason,
}

impl<'db> SonatinaPreOptSnapshotLossRecord<'db> {
    pub const fn new(
        pre_opt: SonatinaInstOriginRecord<'db>,
        reason: SonatinaPreOptSnapshotLossReason,
    ) -> Self {
        Self { pre_opt, reason }
    }

    pub const fn pre_opt(self) -> SonatinaInstOriginRecord<'db> {
        self.pre_opt
    }

    pub const fn reason(self) -> SonatinaPreOptSnapshotLossReason {
        self.reason
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SonatinaPostOptOriginRecord<'db> {
    origin: SonatinaInstOrigin,
    source: SonatinaPostOptOriginSource<'db>,
}

impl<'db> SonatinaPostOptOriginRecord<'db> {
    pub fn new(origin: SonatinaInstOrigin, source: SonatinaPostOptOriginSource<'db>) -> Self {
        assert_eq!(
            origin.stage(),
            SonatinaInstStage::PostOpt,
            "post-opt Sonatina origin records must use post-opt instruction origins"
        );
        if let SonatinaPostOptOriginSource::SameInstId(pre_opt) = source {
            assert!(
                pre_opt.origin().function() == origin.function()
                    && pre_opt.origin().inst() == origin.inst(),
                "same-inst-id post-opt origins must reference the matching pre-opt function and instruction ID"
            );
        }
        Self { origin, source }
    }

    pub const fn origin(self) -> SonatinaInstOrigin {
        self.origin
    }

    pub const fn source(self) -> SonatinaPostOptOriginSource<'db> {
        self.source
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SonatinaPostOptFunctionOrigins<'db> {
    function: FuncRef,
    records: Vec<SonatinaPostOptOriginRecord<'db>>,
}

impl<'db> SonatinaPostOptFunctionOrigins<'db> {
    pub fn new(function: FuncRef, records: Vec<SonatinaPostOptOriginRecord<'db>>) -> Self {
        assert!(
            records
                .iter()
                .all(|record| record.origin().function() == function),
            "post-opt Sonatina function origins cannot contain records from another function"
        );
        let mut seen = BTreeSet::new();
        assert!(
            records
                .iter()
                .all(|record| seen.insert(record.origin().inst())),
            "post-opt Sonatina function origins cannot contain duplicate instruction origins"
        );
        Self { function, records }
    }

    pub const fn function(&self) -> FuncRef {
        self.function
    }

    pub fn records(&self) -> &[SonatinaPostOptOriginRecord<'db>] {
        &self.records
    }

    pub fn record_for_inst(&self, inst: InstId) -> Option<SonatinaPostOptOriginRecord<'db>> {
        self.records
            .iter()
            .copied()
            .find(|record| record.origin().inst() == inst)
    }

    pub fn coverage(&self) -> SonatinaPostOptOriginCoverage {
        let mut coverage = SonatinaPostOptOriginCoverage::default();
        for record in &self.records {
            coverage.total += 1;
            match record.source() {
                SonatinaPostOptOriginSource::SameInstId(_) => coverage.same_inst_id += 1,
                SonatinaPostOptOriginSource::CreatedOrUnmatchedAfterPreOptSnapshot => {
                    coverage.created_or_unmatched_after_preopt_snapshot += 1;
                }
            }
        }
        coverage
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SonatinaPostOptPackageOrigins<'db> {
    functions: Vec<SonatinaPostOptFunctionOrigins<'db>>,
    pre_opt_snapshot_losses: Vec<SonatinaPreOptSnapshotLossRecord<'db>>,
}

impl<'db> SonatinaPostOptPackageOrigins<'db> {
    pub fn from_module(module: &Module, pre_opt: &SonatinaPackageOrigins<'db>) -> Self {
        let post_opt_insts_by_function = module
            .funcs()
            .into_iter()
            .map(|function| {
                let post_opt_insts = module.func_store.view(function, |func| {
                    func.layout
                        .iter_block()
                        .flat_map(|block| func.layout.iter_inst(block))
                        .collect::<BTreeSet<_>>()
                });
                (function, post_opt_insts)
            })
            .collect::<BTreeMap<_, _>>();

        let mut functions = post_opt_insts_by_function
            .iter()
            .map(|(&function, post_opt_insts)| {
                let mut records = post_opt_insts
                    .iter()
                    .copied()
                    .map(|inst| {
                        let origin = SonatinaInstOrigin::post_opt(function, inst);
                        let source = pre_opt
                            .record_for_inst(function, inst)
                            .map(SonatinaPostOptOriginSource::SameInstId)
                            .unwrap_or(
                                SonatinaPostOptOriginSource::CreatedOrUnmatchedAfterPreOptSnapshot,
                            );
                        SonatinaPostOptOriginRecord::new(origin, source)
                    })
                    .collect::<Vec<_>>();
                records.sort_by_key(|record| record.origin().inst().0);
                SonatinaPostOptFunctionOrigins::new(function, records)
            })
            .collect::<Vec<_>>();
        functions.sort_by_key(|origins| origins.function());

        let mut pre_opt_snapshot_losses = pre_opt
            .records()
            .filter(|record| {
                post_opt_insts_by_function
                    .get(&record.origin().function())
                    .is_none_or(|post_opt_insts| !post_opt_insts.contains(&record.origin().inst()))
            })
            .map(|record| {
                SonatinaPreOptSnapshotLossRecord::new(
                    record,
                    SonatinaPreOptSnapshotLossReason::ElidedOrRewrittenBeforePostOptSnapshot,
                )
            })
            .collect::<Vec<_>>();
        pre_opt_snapshot_losses.sort_by_key(|record| {
            (
                record.pre_opt().origin().function(),
                record.pre_opt().origin().inst().0,
            )
        });

        Self {
            functions,
            pre_opt_snapshot_losses,
        }
    }

    pub fn functions(&self) -> &[SonatinaPostOptFunctionOrigins<'db>] {
        &self.functions
    }

    pub fn records(&self) -> impl Iterator<Item = SonatinaPostOptOriginRecord<'db>> + '_ {
        self.functions
            .iter()
            .flat_map(|function| function.records().iter().copied())
    }

    pub fn pre_opt_snapshot_losses(
        &self,
    ) -> impl Iterator<Item = SonatinaPreOptSnapshotLossRecord<'db>> + '_ {
        self.pre_opt_snapshot_losses.iter().copied()
    }

    pub fn record_for_inst(
        &self,
        function: FuncRef,
        inst: InstId,
    ) -> Option<SonatinaPostOptOriginRecord<'db>> {
        self.functions
            .iter()
            .find(|origins| origins.function() == function)
            .and_then(|origins| origins.record_for_inst(inst))
    }

    pub fn coverage(&self) -> SonatinaPostOptOriginCoverage {
        let mut coverage = SonatinaPostOptOriginCoverage::default();
        for function in &self.functions {
            let function_coverage = function.coverage();
            coverage.total += function_coverage.total;
            coverage.same_inst_id += function_coverage.same_inst_id;
            coverage.created_or_unmatched_after_preopt_snapshot +=
                function_coverage.created_or_unmatched_after_preopt_snapshot;
        }
        coverage.pre_opt_snapshot_losses = self.pre_opt_snapshot_losses.len();
        coverage
    }

    pub fn origin_graph(&self) -> CodegenOriginGraph {
        let mut graph = CodegenOriginGraph::new();
        for record in self.records() {
            push_sonatina_post_opt_origin_record(&mut graph, record);
        }
        for record in self.pre_opt_snapshot_losses() {
            push_sonatina_pre_opt_snapshot_loss_record(&mut graph, record);
        }
        graph
    }

    pub fn origin_facts(
        &self,
        stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
    ) -> Result<Option<TypedFactSet>, MissingSonatinaFunctionKey> {
        let graph = self.origin_graph();
        if graph.is_empty() {
            return Ok(None);
        }

        codegen_origin_graph_facts(&graph, stable_function_key).map(Some)
    }

    pub fn origin_graph_for_functions(&self, functions: &BTreeSet<FuncRef>) -> CodegenOriginGraph {
        let mut graph = CodegenOriginGraph::new();
        for record in self
            .records()
            .filter(|record| functions.contains(&record.origin().function()))
        {
            push_sonatina_post_opt_origin_record(&mut graph, record);
        }
        for record in self
            .pre_opt_snapshot_losses()
            .filter(|record| functions.contains(&record.pre_opt().origin().function()))
        {
            push_sonatina_pre_opt_snapshot_loss_record(&mut graph, record);
        }
        graph
    }

    pub fn origin_facts_for_functions(
        &self,
        functions: &BTreeSet<FuncRef>,
        stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
    ) -> Result<Option<TypedFactSet>, MissingSonatinaFunctionKey> {
        let graph = self.origin_graph_for_functions(functions);
        if graph.is_empty() {
            return Ok(None);
        }

        codegen_origin_graph_facts(&graph, stable_function_key).map(Some)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SonatinaPostOptOriginCoverage {
    total: usize,
    same_inst_id: usize,
    created_or_unmatched_after_preopt_snapshot: usize,
    pre_opt_snapshot_losses: usize,
}

impl SonatinaPostOptOriginCoverage {
    pub const fn new(
        same_inst_id: usize,
        created_or_unmatched_after_preopt_snapshot: usize,
        pre_opt_snapshot_losses: usize,
    ) -> Self {
        Self {
            total: same_inst_id + created_or_unmatched_after_preopt_snapshot,
            same_inst_id,
            created_or_unmatched_after_preopt_snapshot,
            pre_opt_snapshot_losses,
        }
    }

    pub const fn total(self) -> usize {
        self.total
    }

    pub const fn same_inst_id(self) -> usize {
        self.same_inst_id
    }

    pub const fn created_or_unmatched_after_preopt_snapshot(self) -> usize {
        self.created_or_unmatched_after_preopt_snapshot
    }

    pub const fn pre_opt_snapshot_losses(self) -> usize {
        self.pre_opt_snapshot_losses
    }

    pub const fn post_opt_classified_total(self) -> usize {
        self.same_inst_id + self.created_or_unmatched_after_preopt_snapshot
    }

    pub const fn is_post_opt_partitioned(self) -> bool {
        self.total == self.post_opt_classified_total()
    }

    pub const fn observed_pre_opt_total(self) -> usize {
        self.same_inst_id + self.pre_opt_snapshot_losses
    }

    pub const fn is_empty(self) -> bool {
        self.total == 0 && self.pre_opt_snapshot_losses == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BytecodeOriginSource<'db> {
    SonatinaPostOpt(SonatinaPostOptOriginRecord<'db>),
    SonatinaBackendPrepared(SonatinaBackendPreparedOriginRecord),
    Unmapped(BytecodeUnmappedReason),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BytecodeOriginRecord<'db> {
    origin: BytecodePcOrigin,
    source: BytecodeOriginSource<'db>,
}

impl<'db> BytecodeOriginRecord<'db> {
    pub const fn new(origin: BytecodePcOrigin, source: BytecodeOriginSource<'db>) -> Self {
        Self { origin, source }
    }

    pub const fn origin(&self) -> &BytecodePcOrigin {
        &self.origin
    }

    pub const fn source(&self) -> BytecodeOriginSource<'db> {
        self.source
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BytecodePackageOrigins<'db> {
    records: Vec<BytecodeOriginRecord<'db>>,
}

impl<'db> BytecodePackageOrigins<'db> {
    pub fn from_artifacts(
        artifacts: &[ObjectArtifact],
        post_opt: &SonatinaPostOptPackageOrigins<'db>,
    ) -> Self {
        let mut records = Vec::new();
        for artifact in artifacts {
            for (section_name, section) in &artifact.sections {
                let Some(observability) = &section.observability else {
                    continue;
                };
                let section_key = BytecodeSectionKey::new(
                    BytecodeObjectKey::new(artifact.object.0.clone()),
                    BytecodeSectionNameKey::new(section_name.0.clone()),
                );
                for entry in &observability.pc_map {
                    let Some(range) = BytecodePcRange::new(entry.pc_start, entry.pc_end) else {
                        continue;
                    };
                    let pc = BytecodePcOrigin::new(section_key.clone(), range);
                    let source = bytecode_source_from_pc_entry(entry, post_opt);
                    records.push(BytecodeOriginRecord::new(pc, source));
                }
            }
        }
        sort_bytecode_origin_records(&mut records);
        assert_bytecode_origin_records_do_not_overlap(&records);
        Self { records }
    }

    pub fn records(&self) -> &[BytecodeOriginRecord<'db>] {
        &self.records
    }

    pub fn coverage(&self) -> BytecodeOriginCoverage {
        bytecode_origin_coverage_for_records(self.records.iter())
    }

    pub fn coverage_for_object(&self, object: &BytecodeObjectKey) -> BytecodeOriginCoverage {
        bytecode_origin_coverage_for_records(
            self.records
                .iter()
                .filter(|record| record.origin().section().object() == object),
        )
    }

    pub fn coverage_for_section(&self, section: &BytecodeSectionKey) -> BytecodeOriginCoverage {
        bytecode_origin_coverage_for_records(
            self.records
                .iter()
                .filter(|record| record.origin().section() == section),
        )
    }

    pub fn resolve_source_spans(
        &self,
        db: &'db dyn SpannedHirAnalysisDb,
        runtime_origins: &RuntimePackageOrigins<'db>,
    ) -> Vec<BytecodeSourceResolution<'db>> {
        self.records
            .iter()
            .map(|record| {
                BytecodeSourceResolution::new(
                    record.clone(),
                    resolve_bytecode_source(db, runtime_origins, record.source()),
                )
            })
            .collect()
    }

    pub fn origin_graph(&self) -> CodegenOriginGraph {
        let mut graph = CodegenOriginGraph::new();
        for record in &self.records {
            push_bytecode_origin_record(&mut graph, record);
        }
        graph
    }

    pub fn origin_graph_for_object(&self, object: &BytecodeObjectKey) -> CodegenOriginGraph {
        let mut graph = CodegenOriginGraph::new();
        for record in self
            .records
            .iter()
            .filter(|record| record.origin().section().object() == object)
        {
            push_bytecode_origin_record(&mut graph, record);
        }
        graph
    }

    pub fn origin_facts_for_object(
        &self,
        object: &BytecodeObjectKey,
        stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
    ) -> Result<Option<TypedFactSet>, MissingSonatinaFunctionKey> {
        let graph = self.origin_graph_for_object(object);
        if graph.is_empty() {
            return Ok(None);
        }

        codegen_origin_graph_facts(&graph, stable_function_key).map(Some)
    }

    pub fn post_opt_snapshot_origin_facts_for_object(
        &self,
        object: &BytecodeObjectKey,
        post_opt_origins: &SonatinaPostOptPackageOrigins<'db>,
        stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
    ) -> Result<Option<TypedFactSet>, MissingSonatinaFunctionKey> {
        let functions = self.post_opt_functions_for_object(object);
        if functions.is_empty() {
            return Ok(None);
        }

        post_opt_origins.origin_facts_for_functions(&functions, stable_function_key)
    }

    pub fn end_to_end_origin_facts_for_object(
        &self,
        object: &BytecodeObjectKey,
        sonatina_origins: &SonatinaPackageOrigins<'db>,
        runtime_origins: &RuntimePackageOrigins<'db>,
        stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
    ) -> Result<Option<TypedFactSet>, MissingSonatinaFunctionKey> {
        let (graph, function_keys) = self.end_to_end_origin_graph_for_object_with_function_keys(
            object,
            sonatina_origins,
            runtime_origins,
            stable_function_key,
        )?;
        if graph.is_empty() {
            return Ok(None);
        }

        end_to_end_origin_graph_facts_with_function_keys(&graph, &function_keys).map(Some)
    }

    pub fn end_to_end_origin_graph_for_object(
        &self,
        object: &BytecodeObjectKey,
        sonatina_origins: &SonatinaPackageOrigins<'db>,
        runtime_origins: &RuntimePackageOrigins<'db>,
        stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
    ) -> Result<EndToEndOriginGraph<'db>, MissingSonatinaFunctionKey> {
        self.end_to_end_origin_graph_for_object_with_function_keys(
            object,
            sonatina_origins,
            runtime_origins,
            stable_function_key,
        )
        .map(|(graph, _)| graph)
    }

    fn end_to_end_origin_graph_for_object_with_function_keys(
        &self,
        object: &BytecodeObjectKey,
        sonatina_origins: &SonatinaPackageOrigins<'db>,
        runtime_origins: &RuntimePackageOrigins<'db>,
        mut stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
    ) -> Result<(EndToEndOriginGraph<'db>, SonatinaFunctionKeyMap), MissingSonatinaFunctionKey>
    {
        let mut graph = EndToEndOriginGraph::new();
        for record in self
            .records
            .iter()
            .filter(|record| record.origin().section().object() == object)
        {
            push_bytecode_end_to_end_origin_record(&mut graph, record);
        }

        let function_keys =
            collect_end_to_end_graph_function_keys(&graph, &mut stable_function_key)?;
        push_selected_runtime_and_sonatina_origins(
            &mut graph,
            sonatina_origins,
            runtime_origins,
            &function_keys,
        );

        Ok((graph, function_keys))
    }

    pub fn frontend_origin_label_map(
        &self,
        mut stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
    ) -> FrontendOriginLabelMap {
        let mut map = FrontendOriginLabelMap::default();
        for record in &self.records {
            let BytecodeOriginSource::SonatinaPostOpt(post_opt) = record.source() else {
                continue;
            };
            let SonatinaPostOptOriginSource::SameInstId(pre_opt) = post_opt.source() else {
                continue;
            };
            let Some(function_key) = stable_function_key(post_opt.origin().function()) else {
                continue;
            };
            let Some(label) = frontend_label_for_pre_opt_source(pre_opt.source(), &function_key)
            else {
                continue;
            };
            map.insert_if_absent(
                post_opt.origin().function(),
                post_opt.origin().inst(),
                label,
            );
        }
        map
    }

    fn post_opt_functions_for_object(&self, object: &BytecodeObjectKey) -> BTreeSet<FuncRef> {
        self.records
            .iter()
            .filter(|record| record.origin().section().object() == object)
            .filter_map(|record| match record.source() {
                BytecodeOriginSource::SonatinaPostOpt(post_opt) => {
                    Some(post_opt.origin().function())
                }
                BytecodeOriginSource::SonatinaBackendPrepared(backend_prepared) => {
                    Some(backend_prepared.origin().function())
                }
                BytecodeOriginSource::Unmapped(_) => None,
            })
            .collect()
    }
}

fn sort_bytecode_origin_records(records: &mut [BytecodeOriginRecord<'_>]) {
    records.sort_by(|left, right| {
        left.origin()
            .section()
            .object()
            .as_str()
            .cmp(right.origin().section().object().as_str())
            .then_with(|| {
                left.origin()
                    .section()
                    .section()
                    .cmp(right.origin().section().section())
            })
            .then_with(|| {
                left.origin()
                    .range()
                    .start()
                    .cmp(&right.origin().range().start())
            })
            .then_with(|| {
                left.origin()
                    .range()
                    .end()
                    .cmp(&right.origin().range().end())
            })
    });
}

fn assert_bytecode_origin_records_do_not_overlap(records: &[BytecodeOriginRecord<'_>]) {
    for pair in records.windows(2) {
        let previous = &pair[0];
        let current = &pair[1];
        if previous.origin().section() != current.origin().section() {
            continue;
        }
        assert!(
            previous.origin().range().end() <= current.origin().range().start(),
            "bytecode origin PC ranges must not overlap within one object section"
        );
    }
}

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

fn bytecode_origin_coverage_for_records<'a, 'db: 'a>(
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

fn push_bytecode_origin_record<'db>(
    graph: &mut CodegenOriginGraph,
    record: &BytecodeOriginRecord<'db>,
) {
    let pc = CodegenOriginNode::BytecodePc(record.origin.clone());
    match record.source() {
        BytecodeOriginSource::SonatinaPostOpt(post_opt) => {
            let post_opt_node = push_sonatina_post_opt_origin_record(graph, post_opt);
            graph.push(post_opt_node, pc, OriginLinkKind::Lowered);
        }
        BytecodeOriginSource::SonatinaBackendPrepared(backend_prepared) => {
            let backend_prepared_node =
                push_sonatina_backend_prepared_origin_record(graph, backend_prepared);
            graph.push(backend_prepared_node, pc, OriginLinkKind::Lowered);
        }
        BytecodeOriginSource::Unmapped(reason) => graph.push(
            CodegenOriginNode::BytecodeUnmapped(reason),
            pc,
            OriginLinkKind::Synthetic,
        ),
    }
}

fn push_sonatina_post_opt_origin_record<'db>(
    graph: &mut CodegenOriginGraph,
    record: SonatinaPostOptOriginRecord<'db>,
) -> CodegenOriginNode {
    let post_opt_node = CodegenOriginNode::SonatinaInst(record.origin());
    match record.source() {
        SonatinaPostOptOriginSource::SameInstId(pre_opt) => graph.push(
            CodegenOriginNode::SonatinaInst(pre_opt.origin()),
            post_opt_node.clone(),
            OriginLinkKind::Alias,
        ),
        SonatinaPostOptOriginSource::CreatedOrUnmatchedAfterPreOptSnapshot => graph.push(
            CodegenOriginNode::SonatinaSynthetic(SonatinaSyntheticOrigin::PostPreOptSnapshotGap),
            post_opt_node.clone(),
            OriginLinkKind::Synthetic,
        ),
    }
    post_opt_node
}

fn push_sonatina_backend_prepared_origin_record(
    graph: &mut CodegenOriginGraph,
    record: SonatinaBackendPreparedOriginRecord,
) -> CodegenOriginNode {
    let backend_prepared_node = CodegenOriginNode::SonatinaInst(record.origin());
    match record.source() {
        SonatinaBackendPreparedOriginSource::MissingPostOptSnapshotRecord => graph.push(
            CodegenOriginNode::SonatinaSynthetic(SonatinaSyntheticOrigin::PostPreOptSnapshotGap),
            backend_prepared_node.clone(),
            OriginLinkKind::Synthetic,
        ),
    }
    backend_prepared_node
}

fn push_sonatina_pre_opt_snapshot_loss_record<'db>(
    graph: &mut CodegenOriginGraph,
    record: SonatinaPreOptSnapshotLossRecord<'db>,
) {
    graph.push(
        CodegenOriginNode::SonatinaInst(record.pre_opt().origin()),
        CodegenOriginNode::SonatinaSynthetic(SonatinaSyntheticOrigin::PreOptSnapshotLoss),
        OriginLinkKind::Synthetic,
    );
}

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

fn resolve_bytecode_source<'db>(
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

fn bytecode_source_from_pc_entry<'db>(
    entry: &PcMapEntry,
    post_opt: &SonatinaPostOptPackageOrigins<'db>,
) -> BytecodeOriginSource<'db> {
    let Some(ir_inst) = entry.ir_inst else {
        return BytecodeOriginSource::Unmapped(
            entry
                .unmapped_reason
                .map(BytecodeUnmappedReason::from)
                .unwrap_or(BytecodeUnmappedReason::Unknown),
        );
    };

    post_opt
        .record_for_inst(entry.func, ir_inst)
        .map(BytecodeOriginSource::SonatinaPostOpt)
        .unwrap_or_else(|| {
            BytecodeOriginSource::SonatinaBackendPrepared(SonatinaBackendPreparedOriginRecord::new(
                SonatinaInstOrigin::backend_prepared(entry.func, ir_inst),
                SonatinaBackendPreparedOriginSource::MissingPostOptSnapshotRecord,
            ))
        })
}

fn frontend_label_for_pre_opt_source(
    source: SonatinaOriginSource<'_>,
    function_key: &SonatinaFunctionExportKey,
) -> Option<String> {
    let key = match source {
        SonatinaOriginSource::RuntimeStmt(origin) => runtime_stmt_export_key(origin, function_key),
        SonatinaOriginSource::RuntimeTerminator(origin) => {
            runtime_terminator_export_key(origin, function_key)
        }
        SonatinaOriginSource::Synthetic(_) | SonatinaOriginSource::Unmapped(_) => return None,
    };
    Some(key.display_label())
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CodegenOriginNode {
    SonatinaInst(SonatinaInstOrigin),
    SonatinaSynthetic(SonatinaSyntheticOrigin),
    BytecodeUnmapped(BytecodeUnmappedReason),
    BytecodePc(BytecodePcOrigin),
}

common::define_origin_owner_key! {
    pub struct SonatinaFunctionExportKey;
}

impl RuntimeOriginOwnerKey for SonatinaFunctionExportKey {}

pub fn codegen_origin_node_export_key(
    node: &CodegenOriginNode,
    mut stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
) -> Option<OriginExportKey> {
    match node {
        CodegenOriginNode::SonatinaInst(origin) => stable_function_key(origin.function())
            .map(|function_key| sonatina_inst_export_key(*origin, &function_key)),
        CodegenOriginNode::SonatinaSynthetic(origin) => {
            Some(sonatina_synthetic_export_key(*origin))
        }
        CodegenOriginNode::BytecodeUnmapped(reason) => Some(bytecode_unmapped_export_key(*reason)),
        CodegenOriginNode::BytecodePc(origin) => Some(bytecode_pc_export_key(origin.clone())),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MissingSonatinaFunctionKey {
    function: FuncRef,
}

impl MissingSonatinaFunctionKey {
    pub const fn new(function: FuncRef) -> Self {
        Self { function }
    }

    pub const fn function(self) -> FuncRef {
        self.function
    }
}

impl fmt::Display for MissingSonatinaFunctionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "missing stable Sonatina function key for func{}",
            self.function.as_u32()
        )
    }
}

impl std::error::Error for MissingSonatinaFunctionKey {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SonatinaFunctionKeyMap {
    keys: BTreeMap<FuncRef, SonatinaFunctionExportKey>,
}

impl SonatinaFunctionKeyMap {
    fn resolve_function(
        &mut self,
        function: FuncRef,
        stable_function_key: &mut impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
    ) -> Result<(), MissingSonatinaFunctionKey> {
        if self.keys.contains_key(&function) {
            return Ok(());
        }

        let Some(key) = stable_function_key(function) else {
            return Err(MissingSonatinaFunctionKey::new(function));
        };
        self.keys.insert(function, key);
        Ok(())
    }

    fn get(
        &self,
        function: FuncRef,
    ) -> Result<&SonatinaFunctionExportKey, MissingSonatinaFunctionKey> {
        self.keys
            .get(&function)
            .ok_or_else(|| MissingSonatinaFunctionKey::new(function))
    }

    fn get_optional(&self, function: FuncRef) -> Option<&SonatinaFunctionExportKey> {
        self.keys.get(&function)
    }
}

fn collect_sonatina_function_keys<'a, Node: 'a>(
    links: impl IntoIterator<Item = &'a OriginLink<Node>>,
    stable_function_key: &mut impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
    node_function: impl Fn(&Node) -> Option<FuncRef>,
) -> Result<SonatinaFunctionKeyMap, MissingSonatinaFunctionKey> {
    let mut function_keys = SonatinaFunctionKeyMap::default();
    for link in links {
        if let Some(function) = node_function(link.from()) {
            function_keys.resolve_function(function, stable_function_key)?;
        }
        if let Some(function) = node_function(link.to()) {
            function_keys.resolve_function(function, stable_function_key)?;
        }
    }
    Ok(function_keys)
}

pub fn codegen_origin_graph_facts(
    graph: &CodegenOriginGraph,
    mut stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
) -> Result<TypedFactSet, MissingSonatinaFunctionKey> {
    let function_keys = collect_codegen_graph_function_keys(graph, &mut stable_function_key)?;
    codegen_origin_graph_facts_with_function_keys(graph, &function_keys)
}

fn codegen_origin_graph_facts_with_function_keys(
    graph: &CodegenOriginGraph,
    function_keys: &SonatinaFunctionKeyMap,
) -> Result<TypedFactSet, MissingSonatinaFunctionKey> {
    try_origin_graph_facts(graph.as_origin_graph(), |node| {
        codegen_origin_node_export_key_checked(node, function_keys)
    })
}

fn collect_codegen_graph_function_keys(
    graph: &CodegenOriginGraph,
    stable_function_key: &mut impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
) -> Result<SonatinaFunctionKeyMap, MissingSonatinaFunctionKey> {
    collect_sonatina_function_keys(graph.links(), stable_function_key, codegen_node_function)
}

fn codegen_node_function(node: &CodegenOriginNode) -> Option<FuncRef> {
    match node {
        CodegenOriginNode::SonatinaInst(origin) => Some(origin.function()),
        CodegenOriginNode::SonatinaSynthetic(_)
        | CodegenOriginNode::BytecodeUnmapped(_)
        | CodegenOriginNode::BytecodePc(_) => None,
    }
}

fn codegen_origin_node_export_key_checked(
    node: &CodegenOriginNode,
    function_keys: &SonatinaFunctionKeyMap,
) -> Result<OriginExportKey, MissingSonatinaFunctionKey> {
    match node {
        CodegenOriginNode::SonatinaInst(origin) => {
            let function_key = function_keys.get(origin.function())?;
            Ok(sonatina_inst_export_key(*origin, function_key))
        }
        CodegenOriginNode::SonatinaSynthetic(origin) => Ok(sonatina_synthetic_export_key(*origin)),
        CodegenOriginNode::BytecodeUnmapped(reason) => Ok(bytecode_unmapped_export_key(*reason)),
        CodegenOriginNode::BytecodePc(origin) => Ok(bytecode_pc_export_key(origin.clone())),
    }
}

common::define_origin_graph_type! {
    pub struct CodegenOriginGraph(CodegenOriginNode);
}

common::define_origin_owner_key! {
    pub struct EndToEndSemanticOwnerKey;
}

impl hir::origin::SemanticOriginOwnerKey for EndToEndSemanticOwnerKey {}

common::define_origin_owner_key! {
    pub struct EndToEndRuntimeOwnerKey;
}

impl RuntimeOriginOwnerKey for EndToEndRuntimeOwnerKey {}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EndToEndOriginOwnerKeys {
    semantic: EndToEndSemanticOwnerKey,
    runtime: EndToEndRuntimeOwnerKey,
}

impl EndToEndOriginOwnerKeys {
    fn new(semantic: EndToEndSemanticOwnerKey, runtime: EndToEndRuntimeOwnerKey) -> Self {
        Self { semantic, runtime }
    }

    pub fn for_function(function_key: &SonatinaFunctionExportKey) -> Self {
        Self::new(
            EndToEndSemanticOwnerKey::new(function_key.as_str()),
            EndToEndRuntimeOwnerKey::new(function_key.as_str()),
        )
    }

    pub fn semantic(&self) -> &EndToEndSemanticOwnerKey {
        &self.semantic
    }

    pub fn runtime(&self) -> &EndToEndRuntimeOwnerKey {
        &self.runtime
    }
}

common::define_origin_string_key! {
    pub struct EndToEndRuntimeSyntheticLocalKey;
}

impl EndToEndRuntimeSyntheticLocalKey {
    pub fn for_stmt_site(site: mir::RuntimeStmtSite) -> Self {
        Self::new(site.export_local_key())
    }

    pub fn for_terminator(block: mir::RBlockId) -> Self {
        Self::new(runtime_terminator_local_key(block))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EndToEndOriginNode<'db> {
    Semantic {
        origin: SemanticOrigin<'db>,
        owner_key: EndToEndSemanticOwnerKey,
    },
    RuntimeSynthetic {
        owner_key: EndToEndRuntimeOwnerKey,
        local_key: EndToEndRuntimeSyntheticLocalKey,
    },
    RuntimeStmt {
        origin: RuntimeStmtOrigin<'db>,
        owner_key: EndToEndRuntimeOwnerKey,
    },
    RuntimeTerminator {
        origin: RuntimeTerminatorOrigin<'db>,
        owner_key: EndToEndRuntimeOwnerKey,
    },
    SonatinaInst(SonatinaInstOrigin),
    SonatinaSynthetic(SonatinaSyntheticOrigin),
    BytecodeUnmapped(BytecodeUnmappedReason),
    BytecodePc(BytecodePcOrigin),
}

common::define_origin_graph_type! {
    pub struct EndToEndOriginGraph<'db>(EndToEndOriginNode<'db>);
}

pub fn end_to_end_origin_graph_facts(
    graph: &EndToEndOriginGraph<'_>,
    mut stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
) -> Result<TypedFactSet, MissingSonatinaFunctionKey> {
    let function_keys = collect_end_to_end_graph_function_keys(graph, &mut stable_function_key)?;
    end_to_end_origin_graph_facts_with_function_keys(graph, &function_keys)
}

fn end_to_end_origin_graph_facts_with_function_keys(
    graph: &EndToEndOriginGraph<'_>,
    function_keys: &SonatinaFunctionKeyMap,
) -> Result<TypedFactSet, MissingSonatinaFunctionKey> {
    try_origin_graph_facts(graph.as_origin_graph(), |node| {
        end_to_end_origin_node_export_key_checked(node, function_keys)
    })
}

pub fn end_to_end_origin_node_export_key(
    node: &EndToEndOriginNode<'_>,
    mut stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
) -> Option<OriginExportKey> {
    match node {
        EndToEndOriginNode::Semantic { origin, owner_key } => Some(origin.export_key(owner_key)),
        EndToEndOriginNode::RuntimeSynthetic {
            owner_key,
            local_key,
        } => Some(OriginExportKey::new(
            OriginExportKind::RuntimeSynthetic,
            owner_key.as_str(),
            local_key.as_str(),
        )),
        EndToEndOriginNode::RuntimeStmt { origin, owner_key } => {
            Some(runtime_stmt_export_key(*origin, owner_key))
        }
        EndToEndOriginNode::RuntimeTerminator { origin, owner_key } => {
            Some(runtime_terminator_export_key(*origin, owner_key))
        }
        EndToEndOriginNode::SonatinaInst(origin) => stable_function_key(origin.function())
            .map(|function_key| sonatina_inst_export_key(*origin, &function_key)),
        EndToEndOriginNode::SonatinaSynthetic(origin) => {
            Some(sonatina_synthetic_export_key(*origin))
        }
        EndToEndOriginNode::BytecodeUnmapped(reason) => Some(bytecode_unmapped_export_key(*reason)),
        EndToEndOriginNode::BytecodePc(origin) => Some(bytecode_pc_export_key(origin.clone())),
    }
}

fn end_to_end_origin_node_export_key_checked(
    node: &EndToEndOriginNode<'_>,
    function_keys: &SonatinaFunctionKeyMap,
) -> Result<OriginExportKey, MissingSonatinaFunctionKey> {
    match node {
        EndToEndOriginNode::Semantic { origin, owner_key } => Ok(origin.export_key(owner_key)),
        EndToEndOriginNode::RuntimeSynthetic {
            owner_key,
            local_key,
        } => Ok(OriginExportKey::new(
            OriginExportKind::RuntimeSynthetic,
            owner_key.as_str(),
            local_key.as_str(),
        )),
        EndToEndOriginNode::RuntimeStmt { origin, owner_key } => {
            Ok(runtime_stmt_export_key(*origin, owner_key))
        }
        EndToEndOriginNode::RuntimeTerminator { origin, owner_key } => {
            Ok(runtime_terminator_export_key(*origin, owner_key))
        }
        EndToEndOriginNode::SonatinaInst(origin) => {
            let function_key = function_keys.get(origin.function())?;
            Ok(sonatina_inst_export_key(*origin, function_key))
        }
        EndToEndOriginNode::SonatinaSynthetic(origin) => Ok(sonatina_synthetic_export_key(*origin)),
        EndToEndOriginNode::BytecodeUnmapped(reason) => Ok(bytecode_unmapped_export_key(*reason)),
        EndToEndOriginNode::BytecodePc(origin) => Ok(bytecode_pc_export_key(origin.clone())),
    }
}

fn push_bytecode_end_to_end_origin_record<'db>(
    graph: &mut EndToEndOriginGraph<'db>,
    record: &BytecodeOriginRecord<'db>,
) {
    let pc = EndToEndOriginNode::BytecodePc(record.origin.clone());
    match record.source() {
        BytecodeOriginSource::SonatinaPostOpt(post_opt) => {
            let post_opt_node = EndToEndOriginNode::SonatinaInst(post_opt.origin());
            match post_opt.source() {
                SonatinaPostOptOriginSource::SameInstId(pre_opt) => graph.push(
                    EndToEndOriginNode::SonatinaInst(pre_opt.origin()),
                    post_opt_node.clone(),
                    OriginLinkKind::Alias,
                ),
                SonatinaPostOptOriginSource::CreatedOrUnmatchedAfterPreOptSnapshot => graph.push(
                    EndToEndOriginNode::SonatinaSynthetic(
                        SonatinaSyntheticOrigin::PostPreOptSnapshotGap,
                    ),
                    post_opt_node.clone(),
                    OriginLinkKind::Synthetic,
                ),
            }
            graph.push(post_opt_node, pc, OriginLinkKind::Lowered);
        }
        BytecodeOriginSource::SonatinaBackendPrepared(backend_prepared) => {
            let backend_prepared_node = EndToEndOriginNode::SonatinaInst(backend_prepared.origin());
            match backend_prepared.source() {
                SonatinaBackendPreparedOriginSource::MissingPostOptSnapshotRecord => graph.push(
                    EndToEndOriginNode::SonatinaSynthetic(
                        SonatinaSyntheticOrigin::PostPreOptSnapshotGap,
                    ),
                    backend_prepared_node.clone(),
                    OriginLinkKind::Synthetic,
                ),
            }
            graph.push(backend_prepared_node, pc, OriginLinkKind::Lowered);
        }
        BytecodeOriginSource::Unmapped(reason) => graph.push(
            EndToEndOriginNode::BytecodeUnmapped(reason),
            pc,
            OriginLinkKind::Synthetic,
        ),
    }
}

fn push_selected_runtime_and_sonatina_origins<'db>(
    graph: &mut EndToEndOriginGraph<'db>,
    sonatina_origins: &SonatinaPackageOrigins<'db>,
    runtime_origins: &RuntimePackageOrigins<'db>,
    function_keys: &SonatinaFunctionKeyMap,
) {
    for function_origins in sonatina_origins.functions() {
        let Some(owner_key) = function_keys.get_optional(function_origins.function()) else {
            continue;
        };
        let owner_keys = EndToEndOriginOwnerKeys::for_function(owner_key);
        push_runtime_source_origins(
            graph,
            runtime_origins.body_for_instance(function_origins.runtime_instance()),
            &owner_keys,
        );
        push_sonatina_preopt_origins(graph, function_origins, &owner_keys);
    }
}

fn push_runtime_source_origins<'db>(
    graph: &mut EndToEndOriginGraph<'db>,
    body: Option<&mir::RuntimePackageBodyOrigins<'db>>,
    owner_keys: &EndToEndOriginOwnerKeys,
) {
    let Some(body) = body else {
        return;
    };
    let semantic_owner_key = owner_keys.semantic().clone();
    let runtime_owner_key = owner_keys.runtime().clone();

    for record in body.origins().stmt_origins() {
        let local_key = EndToEndRuntimeSyntheticLocalKey::for_stmt_site(record.origin().site());
        let target = EndToEndOriginNode::RuntimeStmt {
            origin: record.origin(),
            owner_key: runtime_owner_key.clone(),
        };
        push_runtime_source_link(
            graph,
            record.source(),
            semantic_owner_key.clone(),
            runtime_owner_key.clone(),
            local_key,
            target,
        );
    }

    for record in body.origins().terminator_origins() {
        let local_key = EndToEndRuntimeSyntheticLocalKey::for_terminator(record.origin().block());
        let target = EndToEndOriginNode::RuntimeTerminator {
            origin: record.origin(),
            owner_key: runtime_owner_key.clone(),
        };
        push_runtime_source_link(
            graph,
            record.source(),
            semantic_owner_key.clone(),
            runtime_owner_key.clone(),
            local_key,
            target,
        );
    }
}

fn push_runtime_source_link<'db>(
    graph: &mut EndToEndOriginGraph<'db>,
    source: RuntimeOriginSource<'db>,
    semantic_owner_key: EndToEndSemanticOwnerKey,
    runtime_owner_key: EndToEndRuntimeOwnerKey,
    local_key: EndToEndRuntimeSyntheticLocalKey,
    target: EndToEndOriginNode<'db>,
) {
    match source {
        RuntimeOriginSource::Semantic(origin) => graph.push(
            EndToEndOriginNode::Semantic {
                origin,
                owner_key: semantic_owner_key,
            },
            target,
            OriginLinkKind::Lowered,
        ),
        RuntimeOriginSource::Synthetic => graph.push(
            EndToEndOriginNode::RuntimeSynthetic {
                owner_key: runtime_owner_key,
                local_key,
            },
            target,
            OriginLinkKind::Synthetic,
        ),
    }
}

fn push_sonatina_preopt_origins<'db>(
    graph: &mut EndToEndOriginGraph<'db>,
    function_origins: &SonatinaFunctionOrigins<'db>,
    owner_keys: &EndToEndOriginOwnerKeys,
) {
    let runtime_owner_key = owner_keys.runtime().clone();
    for record in function_origins.records() {
        let target = EndToEndOriginNode::SonatinaInst(record.origin());
        match record.source() {
            SonatinaOriginSource::RuntimeStmt(origin) => graph.push(
                EndToEndOriginNode::RuntimeStmt {
                    origin,
                    owner_key: runtime_owner_key.clone(),
                },
                target,
                OriginLinkKind::Lowered,
            ),
            SonatinaOriginSource::RuntimeTerminator(origin) => graph.push(
                EndToEndOriginNode::RuntimeTerminator {
                    origin,
                    owner_key: runtime_owner_key.clone(),
                },
                target,
                OriginLinkKind::Lowered,
            ),
            SonatinaOriginSource::Synthetic(origin) => graph.push(
                EndToEndOriginNode::SonatinaSynthetic(origin),
                target,
                OriginLinkKind::Synthetic,
            ),
            SonatinaOriginSource::Unmapped(_) => {}
        }
    }
}

fn collect_end_to_end_graph_function_keys(
    graph: &EndToEndOriginGraph<'_>,
    stable_function_key: &mut impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
) -> Result<SonatinaFunctionKeyMap, MissingSonatinaFunctionKey> {
    collect_sonatina_function_keys(graph.links(), stable_function_key, end_to_end_node_function)
}

fn end_to_end_node_function(node: &EndToEndOriginNode<'_>) -> Option<FuncRef> {
    match node {
        EndToEndOriginNode::SonatinaInst(origin) => Some(origin.function()),
        EndToEndOriginNode::Semantic { .. }
        | EndToEndOriginNode::RuntimeSynthetic { .. }
        | EndToEndOriginNode::RuntimeStmt { .. }
        | EndToEndOriginNode::RuntimeTerminator { .. }
        | EndToEndOriginNode::SonatinaSynthetic(_)
        | EndToEndOriginNode::BytecodeUnmapped(_)
        | EndToEndOriginNode::BytecodePc(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use common::origin::{OriginExportKey, OriginExportKind, OriginLinkKind};
    use sonatina_codegen::{
        machinst::vcode::VCodeInst,
        object::{
            ObjectArtifact, PcMapEntry, SectionArtifact, SectionObservability,
            UnmappedReasonCoverage,
        },
    };
    use sonatina_ir::{
        BlockId, InstId,
        module::FuncRef,
        object::{ObjectName, SectionName},
    };

    use super::{
        BytecodeObjectKey, BytecodeOriginCoverage, BytecodeOriginRecord, BytecodeOriginSource,
        BytecodePackageOrigins, BytecodePcOrigin, BytecodePcRange, BytecodeSectionKey,
        BytecodeSectionNameKey, CodegenOriginGraph, CodegenOriginNode, EndToEndOriginGraph,
        EndToEndOriginNode, EndToEndOriginOwnerKeys, EndToEndRuntimeOwnerKey,
        EndToEndRuntimeSyntheticLocalKey, SonatinaBackendPreparedOriginRecord,
        SonatinaBackendPreparedOriginSource, SonatinaFunctionExportKey, SonatinaInstOrigin,
        SonatinaInstOriginRecord, SonatinaInstStage, SonatinaOriginCoverage, SonatinaOriginSource,
        SonatinaPostOptFunctionOrigins, SonatinaPostOptOriginCoverage, SonatinaPostOptOriginRecord,
        SonatinaPostOptOriginSource, SonatinaPostOptPackageOrigins,
        SonatinaPreOptSnapshotLossReason, SonatinaPreOptSnapshotLossRecord,
        SonatinaSyntheticOrigin, bytecode_pc_export_key, bytecode_source_from_pc_entry,
        bytecode_unmapped_export_key, codegen_origin_graph_facts, codegen_origin_node_export_key,
        end_to_end_origin_graph_facts, end_to_end_origin_node_export_key, sonatina_inst_export_key,
        sonatina_synthetic_export_key,
    };

    fn bytecode_section_key(object: &str, section: &str) -> BytecodeSectionKey {
        BytecodeSectionKey::new(
            BytecodeObjectKey::new(object),
            BytecodeSectionNameKey::new(section),
        )
    }

    fn pc_map_entry(pc_start: u32, pc_end: u32) -> PcMapEntry {
        PcMapEntry {
            pc_start,
            pc_end,
            func: FuncRef::from_u32(0),
            func_name: "test_func".to_string(),
            block: BlockId::from_u32(0),
            vcode_inst: VCodeInst(0),
            ir_inst: None,
            frontend_provenance: None,
            unmapped_reason: None,
        }
    }

    fn section_observability(
        section: impl Into<SectionName>,
        pc_start: u32,
        pc_end: u32,
    ) -> SectionObservability {
        SectionObservability {
            schema_version: "test",
            section: section.into(),
            section_bytes: pc_end,
            code_bytes: pc_end,
            data_bytes: 0,
            embed_bytes: 0,
            mapped_code_bytes: 0,
            unmapped_code_bytes: pc_end.saturating_sub(pc_start),
            unmapped_reason_coverage: UnmappedReasonCoverage::default(),
            pc_map: vec![pc_map_entry(pc_start, pc_end)],
        }
    }

    fn object_artifact(
        object: impl Into<ObjectName>,
        sections: impl IntoIterator<Item = (&'static str, u32, u32)>,
    ) -> ObjectArtifact {
        ObjectArtifact {
            object: object.into(),
            sections: sections
                .into_iter()
                .map(|(section, pc_start, pc_end)| {
                    (
                        SectionName::from(section),
                        SectionArtifact {
                            bytes: Vec::new(),
                            symtab: Default::default(),
                            observability: Some(section_observability(section, pc_start, pc_end)),
                        },
                    )
                })
                .collect(),
        }
    }

    fn push_pc_map_entry(
        artifact: &mut ObjectArtifact,
        section: &'static str,
        pc_start: u32,
        pc_end: u32,
    ) {
        artifact
            .sections
            .get_mut(&SectionName::from(section))
            .expect("test section should exist")
            .observability
            .as_mut()
            .expect("test section should have observability")
            .pc_map
            .push(pc_map_entry(pc_start, pc_end));
    }

    #[test]
    fn bytecode_origin_coverage_constructor_derives_partitioned_total() {
        let coverage = BytecodeOriginCoverage::new(2, 3, 5);

        assert_eq!(coverage.total(), 10);
        assert_eq!(coverage.classified_total(), 10);
        assert!(coverage.is_partitioned());
        assert!(!coverage.is_empty());
    }

    #[test]
    fn sonatina_coverage_constructors_derive_partitioned_totals() {
        let pre_opt = SonatinaOriginCoverage::new(2, 3, 5, 7);

        assert_eq!(pre_opt.total(), 17);
        assert_eq!(pre_opt.runtime_stmt(), 2);
        assert_eq!(pre_opt.runtime_terminator(), 3);
        assert_eq!(pre_opt.synthetic(), 5);
        assert_eq!(pre_opt.unmapped(), 7);
        assert_eq!(pre_opt.classified_total(), 17);
        assert!(pre_opt.is_partitioned());
        assert!(!pre_opt.is_empty());

        let post_opt = SonatinaPostOptOriginCoverage::new(11, 13, 17);

        assert_eq!(post_opt.total(), 24);
        assert_eq!(post_opt.same_inst_id(), 11);
        assert_eq!(post_opt.created_or_unmatched_after_preopt_snapshot(), 13);
        assert_eq!(post_opt.pre_opt_snapshot_losses(), 17);
        assert_eq!(post_opt.post_opt_classified_total(), 24);
        assert_eq!(post_opt.observed_pre_opt_total(), 28);
        assert!(post_opt.is_post_opt_partitioned());
        assert!(!post_opt.is_empty());
    }

    #[test]
    fn sonatina_instruction_origin_includes_function_owner_and_stage() {
        let inst = InstId::from_u32(7);

        let first = SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), inst);
        let second = SonatinaInstOrigin::pre_opt(FuncRef::from_u32(1), inst);
        let post_opt = SonatinaInstOrigin::post_opt(FuncRef::from_u32(0), inst);
        let backend_prepared = SonatinaInstOrigin::backend_prepared(FuncRef::from_u32(0), inst);

        assert_ne!(first, second);
        assert_ne!(first, post_opt);
        assert_ne!(post_opt, backend_prepared);
        assert_eq!(first.stage(), SonatinaInstStage::PreOpt);
        assert_eq!(post_opt.stage(), SonatinaInstStage::PostOpt);
        assert_eq!(backend_prepared.stage(), SonatinaInstStage::BackendPrepared);
    }

    #[test]
    #[should_panic(
        expected = "pre-opt Sonatina origin records must use pre-opt instruction origins"
    )]
    fn sonatina_pre_opt_records_reject_post_opt_instruction_origins() {
        SonatinaInstOriginRecord::new(
            SonatinaInstOrigin::post_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
            SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
        );
    }

    #[test]
    #[should_panic(
        expected = "post-opt Sonatina origin records must use post-opt instruction origins"
    )]
    fn sonatina_post_opt_records_reject_pre_opt_instruction_origins() {
        let pre_opt = SonatinaInstOriginRecord::new(
            SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
            SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
        );

        SonatinaPostOptOriginRecord::new(
            SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
            SonatinaPostOptOriginSource::SameInstId(pre_opt),
        );
    }

    #[test]
    #[should_panic(
        expected = "same-inst-id post-opt origins must reference the matching pre-opt function and instruction ID"
    )]
    fn sonatina_post_opt_records_reject_same_inst_id_source_from_another_function() {
        let pre_opt = SonatinaInstOriginRecord::new(
            SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
            SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
        );

        SonatinaPostOptOriginRecord::new(
            SonatinaInstOrigin::post_opt(FuncRef::from_u32(1), InstId::from_u32(7)),
            SonatinaPostOptOriginSource::SameInstId(pre_opt),
        );
    }

    #[test]
    #[should_panic(
        expected = "same-inst-id post-opt origins must reference the matching pre-opt function and instruction ID"
    )]
    fn sonatina_post_opt_records_reject_same_inst_id_source_from_another_inst() {
        let pre_opt = SonatinaInstOriginRecord::new(
            SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
            SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
        );

        SonatinaPostOptOriginRecord::new(
            SonatinaInstOrigin::post_opt(FuncRef::from_u32(0), InstId::from_u32(8)),
            SonatinaPostOptOriginSource::SameInstId(pre_opt),
        );
    }

    #[test]
    #[should_panic(
        expected = "backend-prepared Sonatina origin records must use backend-prepared instruction origins"
    )]
    fn sonatina_backend_prepared_records_reject_post_opt_instruction_origins() {
        SonatinaBackendPreparedOriginRecord::new(
            SonatinaInstOrigin::post_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
            SonatinaBackendPreparedOriginSource::MissingPostOptSnapshotRecord,
        );
    }

    #[test]
    #[should_panic(
        expected = "post-opt Sonatina function origins cannot contain records from another function"
    )]
    fn sonatina_post_opt_function_origins_reject_wrong_function_records() {
        let pre_opt = SonatinaInstOriginRecord::new(
            SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
            SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
        );
        let post_opt = SonatinaPostOptOriginRecord::new(
            SonatinaInstOrigin::post_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
            SonatinaPostOptOriginSource::SameInstId(pre_opt),
        );

        SonatinaPostOptFunctionOrigins::new(FuncRef::from_u32(1), vec![post_opt]);
    }

    #[test]
    #[should_panic(
        expected = "post-opt Sonatina function origins cannot contain duplicate instruction origins"
    )]
    fn sonatina_post_opt_function_origins_reject_duplicate_instruction_records() {
        let post_opt = SonatinaPostOptOriginRecord::new(
            SonatinaInstOrigin::post_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
            SonatinaPostOptOriginSource::CreatedOrUnmatchedAfterPreOptSnapshot,
        );

        SonatinaPostOptFunctionOrigins::new(FuncRef::from_u32(0), vec![post_opt, post_opt]);
    }

    #[test]
    fn bytecode_pc_origin_includes_object_and_section_owner() {
        let range = BytecodePcRange::new(4, 8).expect("valid range");

        let first = BytecodePcOrigin::new(bytecode_section_key("Foo", "init"), range);
        let second = BytecodePcOrigin::new(bytecode_section_key("Foo", "runtime"), range);

        assert_ne!(first, second);
    }

    #[test]
    #[should_panic(expected = "origin string key must not be empty")]
    fn bytecode_object_key_rejects_empty_keys() {
        BytecodeObjectKey::new("");
    }

    #[test]
    #[should_panic(expected = "origin string key must not be empty")]
    fn bytecode_section_key_rejects_empty_sections() {
        BytecodeSectionNameKey::new("");
    }

    #[test]
    fn bytecode_pc_range_rejects_inverted_offsets() {
        assert_eq!(BytecodePcRange::new(8, 4), None);
    }

    #[test]
    fn bytecode_pc_range_rejects_empty_ranges() {
        assert_eq!(BytecodePcRange::new(4, 4), None);
    }

    #[test]
    fn bytecode_package_origins_from_artifacts_are_deterministically_ordered() {
        let artifacts = vec![
            object_artifact("B", [("runtime", 20, 24), ("init", 10, 14)]),
            object_artifact("A", [("runtime", 8, 12), ("init", 4, 6)]),
        ];
        let post_opt_origins = SonatinaPostOptPackageOrigins {
            functions: Vec::new(),
            pre_opt_snapshot_losses: Vec::new(),
        };

        let origins = BytecodePackageOrigins::from_artifacts(&artifacts, &post_opt_origins);
        let record_keys = origins
            .records()
            .iter()
            .map(|record| {
                (
                    record.origin().section().object().as_str().to_string(),
                    record.origin().section().section().to_string(),
                    record.origin().range().start(),
                    record.origin().range().end(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            record_keys,
            vec![
                ("A".to_string(), "init".to_string(), 4, 6),
                ("A".to_string(), "runtime".to_string(), 8, 12),
                ("B".to_string(), "init".to_string(), 10, 14),
                ("B".to_string(), "runtime".to_string(), 20, 24),
            ]
        );
        assert!(origins.records().iter().all(|record| {
            matches!(
                record.source(),
                BytecodeOriginSource::Unmapped(super::BytecodeUnmappedReason::Unknown)
            )
        }));
    }

    #[test]
    #[should_panic(
        expected = "bytecode origin PC ranges must not overlap within one object section"
    )]
    fn bytecode_package_origins_reject_overlapping_pc_ranges_in_one_section() {
        let mut artifact = object_artifact("A", [("runtime", 4, 8)]);
        push_pc_map_entry(&mut artifact, "runtime", 7, 10);
        let post_opt_origins = SonatinaPostOptPackageOrigins {
            functions: Vec::new(),
            pre_opt_snapshot_losses: Vec::new(),
        };

        BytecodePackageOrigins::from_artifacts(&[artifact], &post_opt_origins);
    }

    #[test]
    fn bytecode_package_origins_allow_adjacent_pc_ranges_in_one_section() {
        let mut artifact = object_artifact("A", [("runtime", 4, 8)]);
        push_pc_map_entry(&mut artifact, "runtime", 8, 12);
        let post_opt_origins = SonatinaPostOptPackageOrigins {
            functions: Vec::new(),
            pre_opt_snapshot_losses: Vec::new(),
        };

        let origins = BytecodePackageOrigins::from_artifacts(&[artifact], &post_opt_origins);

        assert_eq!(
            origins
                .records()
                .iter()
                .map(|record| (
                    record.origin().range().start(),
                    record.origin().range().end()
                ))
                .collect::<Vec<_>>(),
            vec![(4, 8), (8, 12)]
        );
    }

    #[test]
    fn codegen_origin_export_keys_include_kind_owner_and_local_identity() {
        let inst_key = sonatina_inst_export_key(
            SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
            &SonatinaFunctionExportKey::new("sonatina:func:a"),
        );
        let pc_key = bytecode_pc_export_key(BytecodePcOrigin::new(
            bytecode_section_key("Foo", "runtime"),
            BytecodePcRange::new(4, 8).expect("valid range"),
        ));

        assert_eq!(
            inst_key,
            OriginExportKey::new(
                OriginExportKind::SonatinaInst,
                "sonatina:func:a",
                "pre_opt:inst:7"
            )
        );
        assert_eq!(
            pc_key,
            OriginExportKey::new(
                OriginExportKind::BytecodePc,
                "object:Foo:section:runtime",
                "pc:4..8"
            )
        );
    }

    #[test]
    fn codegen_origin_node_export_keys_cover_synthetic_unmapped_and_pc_nodes() {
        let synthetic = codegen_origin_node_export_key(
            &CodegenOriginNode::SonatinaSynthetic(SonatinaSyntheticOrigin::Prologue),
            |_| None,
        )
        .expect("synthetic node does not need a function key");
        let unmapped = codegen_origin_node_export_key(
            &CodegenOriginNode::BytecodeUnmapped(super::BytecodeUnmappedReason::NoIrInst),
            |_| None,
        )
        .expect("unmapped node does not need a function key");
        let pc_origin = BytecodePcOrigin::new(
            bytecode_section_key("Foo", "runtime"),
            BytecodePcRange::new(4, 8).expect("valid range"),
        );
        let pc = codegen_origin_node_export_key(
            &CodegenOriginNode::BytecodePc(pc_origin.clone()),
            |_| None,
        )
        .expect("bytecode PC node does not need a function key");

        assert_eq!(
            synthetic,
            sonatina_synthetic_export_key(SonatinaSyntheticOrigin::Prologue)
        );
        assert_eq!(
            codegen_origin_node_export_key(
                &CodegenOriginNode::SonatinaSynthetic(SonatinaSyntheticOrigin::PreOptSnapshotLoss),
                |_| None,
            )
            .expect("snapshot-loss synthetic node does not need a function key"),
            sonatina_synthetic_export_key(SonatinaSyntheticOrigin::PreOptSnapshotLoss)
        );
        assert_eq!(
            unmapped,
            bytecode_unmapped_export_key(super::BytecodeUnmappedReason::NoIrInst)
        );
        assert_eq!(pc, bytecode_pc_export_key(pc_origin));
    }

    #[test]
    fn end_to_end_runtime_synthetic_export_uses_typed_owner_and_local_keys() {
        let node = EndToEndOriginNode::RuntimeSynthetic {
            owner_key: EndToEndRuntimeOwnerKey::new("sonatina:func:test"),
            local_key: EndToEndRuntimeSyntheticLocalKey::new("block:0:stmt:0"),
        };

        assert_eq!(
            end_to_end_origin_node_export_key(&node, |_| None),
            Some(OriginExportKey::new(
                OriginExportKind::RuntimeSynthetic,
                "sonatina:func:test",
                "block:0:stmt:0"
            ))
        );
    }

    #[test]
    fn sonatina_inst_node_export_key_requires_function_key() {
        let node = CodegenOriginNode::SonatinaInst(SonatinaInstOrigin::post_opt(
            FuncRef::from_u32(2),
            InstId::from_u32(9),
        ));

        assert_eq!(codegen_origin_node_export_key(&node, |_| None), None);
        assert_eq!(
            codegen_origin_node_export_key(&node, |func| {
                assert_eq!(func, FuncRef::from_u32(2));
                Some(SonatinaFunctionExportKey::new("sonatina:func:foo"))
            }),
            Some(OriginExportKey::new(
                OriginExportKind::SonatinaInst,
                "sonatina:func:foo",
                "post_opt:inst:9"
            ))
        );
    }

    #[test]
    fn end_to_end_origin_owner_keys_are_derived_from_typed_function_key() {
        let function_key = SonatinaFunctionExportKey::new("sonatina:func:test");
        let owner_keys = EndToEndOriginOwnerKeys::for_function(&function_key);

        assert_eq!(owner_keys.semantic().as_str(), "sonatina:func:test");
        assert_eq!(owner_keys.runtime().as_str(), "sonatina:func:test");
    }

    #[test]
    fn codegen_origin_graph_facts_require_stable_function_keys() {
        let inst = CodegenOriginNode::SonatinaInst(SonatinaInstOrigin::post_opt(
            FuncRef::from_u32(2),
            InstId::from_u32(9),
        ));
        let pc = CodegenOriginNode::BytecodePc(BytecodePcOrigin::new(
            bytecode_section_key("Foo", "runtime"),
            BytecodePcRange::new(4, 8).expect("valid range"),
        ));
        let mut graph = CodegenOriginGraph::new();
        graph.push(inst, pc, OriginLinkKind::Lowered);

        let err = codegen_origin_graph_facts(&graph, |_| None)
            .expect_err("Sonatina instruction nodes require stable function keys");
        assert_eq!(err.function(), FuncRef::from_u32(2));

        let facts = codegen_origin_graph_facts(&graph, |func| {
            assert_eq!(func, FuncRef::from_u32(2));
            Some(SonatinaFunctionExportKey::new("sonatina:func:foo"))
        })
        .expect("stable function key should export codegen origin facts");

        assert!(facts.origin_nodes().any(|node| {
            node.key().kind() == OriginExportKind::SonatinaInst
                && node.key().owner_key() == "sonatina:func:foo"
        }));
        assert!(
            facts
                .origin_links()
                .any(|link| link.kind() == OriginLinkKind::Lowered)
        );
    }

    #[test]
    fn codegen_origin_graph_facts_resolve_each_function_key_once() {
        let function = FuncRef::from_u32(2);
        let first_inst = CodegenOriginNode::SonatinaInst(SonatinaInstOrigin::pre_opt(
            function,
            InstId::from_u32(8),
        ));
        let second_inst = CodegenOriginNode::SonatinaInst(SonatinaInstOrigin::post_opt(
            function,
            InstId::from_u32(8),
        ));
        let pc = CodegenOriginNode::BytecodePc(BytecodePcOrigin::new(
            bytecode_section_key("Foo", "runtime"),
            BytecodePcRange::new(4, 8).expect("valid range"),
        ));
        let mut graph = CodegenOriginGraph::new();
        graph.push(first_inst, second_inst.clone(), OriginLinkKind::Alias);
        graph.push(second_inst, pc, OriginLinkKind::Lowered);

        let mut calls = 0;
        let facts = codegen_origin_graph_facts(&graph, |func| {
            calls += 1;
            assert_eq!(func, function);
            Some(SonatinaFunctionExportKey::new("sonatina:func:foo"))
        })
        .expect("stable function key should export repeated-function graph facts");

        assert_eq!(calls, 1);
        assert!(facts.origin_nodes().any(|node| {
            node.key().kind() == OriginExportKind::SonatinaInst
                && node.key().owner_key() == "sonatina:func:foo"
        }));
    }

    #[test]
    fn end_to_end_origin_graph_facts_require_stable_function_keys() {
        let inst = EndToEndOriginNode::SonatinaInst(SonatinaInstOrigin::post_opt(
            FuncRef::from_u32(2),
            InstId::from_u32(9),
        ));
        let pc = EndToEndOriginNode::BytecodePc(BytecodePcOrigin::new(
            bytecode_section_key("Foo", "runtime"),
            BytecodePcRange::new(4, 8).expect("valid range"),
        ));
        let mut graph = EndToEndOriginGraph::new();
        graph.push(inst, pc, OriginLinkKind::Lowered);

        let err = end_to_end_origin_graph_facts(&graph, |_| None)
            .expect_err("end-to-end Sonatina nodes require stable function keys");
        assert_eq!(err.function(), FuncRef::from_u32(2));

        let facts = end_to_end_origin_graph_facts(&graph, |func| {
            assert_eq!(func, FuncRef::from_u32(2));
            Some(SonatinaFunctionExportKey::new("sonatina:func:foo"))
        })
        .expect("stable function key should export end-to-end origin facts");

        assert!(facts.origin_nodes().any(|node| {
            node.key().kind() == OriginExportKind::SonatinaInst
                && node.key().owner_key() == "sonatina:func:foo"
        }));
        assert!(
            facts
                .origin_links()
                .any(|link| link.kind() == OriginLinkKind::Lowered)
        );
    }

    #[test]
    fn pc_map_entries_missing_postopt_snapshot_use_backend_prepared_origin() {
        let function = FuncRef::from_u32(2);
        let missing_inst = InstId::from_u32(99);
        let post_opt_origins = SonatinaPostOptPackageOrigins {
            functions: vec![SonatinaPostOptFunctionOrigins::new(function, Vec::new())],
            pre_opt_snapshot_losses: Vec::new(),
        };
        let pc_entry = PcMapEntry {
            pc_start: 4,
            pc_end: 8,
            func: function,
            func_name: "test_func".to_string(),
            block: BlockId::from_u32(0),
            vcode_inst: VCodeInst(0),
            ir_inst: Some(missing_inst),
            frontend_provenance: None,
            unmapped_reason: None,
        };

        let source = bytecode_source_from_pc_entry(&pc_entry, &post_opt_origins);
        let BytecodeOriginSource::SonatinaBackendPrepared(backend_prepared) = source else {
            panic!("PC-map entries missing the post-opt snapshot must not fake post-opt origins");
        };
        assert_eq!(
            backend_prepared.origin(),
            SonatinaInstOrigin::backend_prepared(function, missing_inst)
        );
        assert_eq!(
            backend_prepared.source(),
            SonatinaBackendPreparedOriginSource::MissingPostOptSnapshotRecord
        );

        let object = BytecodeObjectKey::new("Foo");
        let pc = BytecodePcOrigin::new(
            BytecodeSectionKey::new(object.clone(), BytecodeSectionNameKey::new("runtime")),
            BytecodePcRange::new(4, 8).expect("valid PC range"),
        );
        let origins = BytecodePackageOrigins {
            records: vec![BytecodeOriginRecord::new(
                pc,
                BytecodeOriginSource::SonatinaBackendPrepared(backend_prepared),
            )],
        };
        assert_eq!(origins.coverage().total(), 1);
        assert_eq!(origins.coverage().sonatina_backend_prepared(), 1);
        assert_eq!(origins.coverage().sonatina_post_opt(), 0);
        assert_eq!(origins.coverage().unmapped(), 0);
        assert!(origins.coverage().is_partitioned());

        let graph = origins.origin_graph();
        assert!(graph.links().iter().any(|link| {
            link.kind() == OriginLinkKind::Synthetic
                && matches!(
                    link.from(),
                    CodegenOriginNode::SonatinaSynthetic(
                        SonatinaSyntheticOrigin::PostPreOptSnapshotGap
                    )
                )
                && matches!(
                    link.to(),
                    CodegenOriginNode::SonatinaInst(origin)
                        if origin.stage() == SonatinaInstStage::BackendPrepared
                            && origin.inst() == missing_inst
                )
        }));
        assert!(graph.links().iter().any(|link| {
            link.kind() == OriginLinkKind::Lowered
                && matches!(
                    link.from(),
                    CodegenOriginNode::SonatinaInst(origin)
                        if origin.stage() == SonatinaInstStage::BackendPrepared
                            && origin.inst() == missing_inst
                )
                && matches!(link.to(), CodegenOriginNode::BytecodePc(_))
        }));
        assert!(graph.links().iter().all(|link| {
            !matches!(
                link.from(),
                CodegenOriginNode::SonatinaInst(origin)
                    if origin.stage() == SonatinaInstStage::PostOpt
                        && origin.inst() == missing_inst
            ) && !matches!(
                link.to(),
                CodegenOriginNode::SonatinaInst(origin)
                    if origin.stage() == SonatinaInstStage::PostOpt
                        && origin.inst() == missing_inst
            )
        }));

        let facts = origins
            .origin_facts_for_object(&object, |func| {
                assert_eq!(func, function);
                Some(SonatinaFunctionExportKey::new("sonatina:func:test"))
            })
            .expect("backend-prepared bytecode facts should export with a function key")
            .expect("backend-prepared bytecode facts should be non-empty");
        assert!(facts.origin_nodes().any(|node| {
            node.key().kind() == OriginExportKind::SonatinaInst
                && node.key().owner_key() == "sonatina:func:test"
                && node.key().local_key() == "backend_prepared:inst:99"
        }));
    }

    #[test]
    fn post_opt_snapshot_origin_facts_include_preopt_losses() {
        let function = FuncRef::from_u32(2);
        let kept_pre_opt = SonatinaInstOriginRecord::new(
            SonatinaInstOrigin::pre_opt(function, InstId::from_u32(9)),
            SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
        );
        let lost_pre_opt = SonatinaInstOriginRecord::new(
            SonatinaInstOrigin::pre_opt(function, InstId::from_u32(11)),
            SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
        );
        let post_opt = SonatinaPostOptOriginRecord::new(
            SonatinaInstOrigin::post_opt(function, InstId::from_u32(9)),
            SonatinaPostOptOriginSource::SameInstId(kept_pre_opt),
        );
        let origins = SonatinaPostOptPackageOrigins {
            functions: vec![SonatinaPostOptFunctionOrigins::new(
                function,
                vec![post_opt],
            )],
            pre_opt_snapshot_losses: vec![SonatinaPreOptSnapshotLossRecord::new(
                lost_pre_opt,
                SonatinaPreOptSnapshotLossReason::ElidedOrRewrittenBeforePostOptSnapshot,
            )],
        };

        let graph = origins.origin_graph();
        assert!(graph.links().iter().any(|link| {
            link.kind() == OriginLinkKind::Alias
                && matches!(
                    link.from(),
                    CodegenOriginNode::SonatinaInst(origin)
                        if origin.stage() == SonatinaInstStage::PreOpt
                            && origin.inst() == InstId::from_u32(9)
                )
                && matches!(
                    link.to(),
                    CodegenOriginNode::SonatinaInst(origin)
                        if origin.stage() == SonatinaInstStage::PostOpt
                            && origin.inst() == InstId::from_u32(9)
                )
        }));
        assert!(graph.links().iter().any(|link| {
            link.kind() == OriginLinkKind::Synthetic
                && matches!(
                    link.from(),
                    CodegenOriginNode::SonatinaInst(origin)
                        if origin.stage() == SonatinaInstStage::PreOpt
                            && origin.inst() == InstId::from_u32(11)
                )
                && matches!(
                    link.to(),
                    CodegenOriginNode::SonatinaSynthetic(
                        SonatinaSyntheticOrigin::PreOptSnapshotLoss
                    )
                )
        }));
        assert!(
            graph.links().iter().all(|link| {
                !(link.kind() == OriginLinkKind::Transformed
                    && matches!(
                        link.to(),
                        CodegenOriginNode::SonatinaSynthetic(
                            SonatinaSyntheticOrigin::PreOptSnapshotLoss
                        )
                    ))
            }),
            "snapshot-loss facts must not pretend to be precise pass transforms"
        );

        let facts = origins
            .origin_facts(|func| {
                assert_eq!(func, function);
                Some(SonatinaFunctionExportKey::new("sonatina:func:test"))
            })
            .expect("snapshot diff facts should export with a function key")
            .expect("non-empty snapshot diff should produce facts");

        assert!(facts.origin_nodes().any(|node| {
            node.key().kind() == OriginExportKind::SonatinaInst
                && node.key().owner_key() == "sonatina:func:test"
                && node.key().local_key() == "pre_opt:inst:11"
        }));
        assert!(facts.origin_nodes().any(|node| {
            node.key()
                == &sonatina_synthetic_export_key(SonatinaSyntheticOrigin::PreOptSnapshotLoss)
        }));
        assert!(
            facts
                .origin_links()
                .any(|link| link.kind() == OriginLinkKind::Synthetic)
        );
    }

    #[test]
    fn codegen_origin_graph_uses_typed_codegen_nodes() {
        let inst = CodegenOriginNode::SonatinaInst(SonatinaInstOrigin::new(
            SonatinaInstStage::PostOpt,
            FuncRef::from_u32(0),
            InstId::from_u32(7),
        ));
        let pc = CodegenOriginNode::BytecodePc(BytecodePcOrigin::new(
            bytecode_section_key("Foo", "runtime"),
            BytecodePcRange::new(4, 8).expect("valid range"),
        ));
        let mut graph = CodegenOriginGraph::new();

        graph.push(inst.clone(), pc.clone(), OriginLinkKind::Lowered);

        let link = graph
            .links()
            .first()
            .expect("origin graph should have a link");

        assert_eq!(link.from(), &inst);
        assert_eq!(link.to(), &pc);
        assert_eq!(link.kind(), OriginLinkKind::Lowered);
    }
}
