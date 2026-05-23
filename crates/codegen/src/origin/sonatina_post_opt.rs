use std::collections::{BTreeMap, BTreeSet};

use common::{facts::TypedFactSet, origin::OriginLinkKind};
use sonatina_ir::{InstId, Module, module::FuncRef};

use super::{
    CodegenOriginGraph, CodegenOriginNode, MissingSonatinaFunctionKey, SonatinaFunctionExportKey,
    SonatinaInstOrigin, SonatinaInstOriginRecord, SonatinaInstStage, SonatinaPackageOrigins,
    SonatinaSyntheticOrigin, codegen_origin_graph_facts,
};

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

common::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum SonatinaPreOptSnapshotLossReason {
        /// The pre-opt instruction has no post-opt instruction with the same
        /// function-local `InstId`. This is a conservative snapshot
        /// classification: the precise pass event could be deletion,
        /// replacement, merge, split, or another rewrite until Sonatina exposes
        /// pass-level origin hooks.
        ElidedOrRewrittenBeforePostOptSnapshot => "elided_or_rewritten_before_postopt_snapshot",
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
    pub(super) functions: Vec<SonatinaPostOptFunctionOrigins<'db>>,
    pub(super) pre_opt_snapshot_losses: Vec<SonatinaPreOptSnapshotLossRecord<'db>>,
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

    pub fn coverage_for_functions(
        &self,
        functions: &BTreeSet<FuncRef>,
    ) -> SonatinaPostOptOriginCoverage {
        let mut coverage = SonatinaPostOptOriginCoverage::default();
        for function in self
            .functions
            .iter()
            .filter(|function| functions.contains(&function.function()))
        {
            let function_coverage = function.coverage();
            coverage.total += function_coverage.total;
            coverage.same_inst_id += function_coverage.same_inst_id;
            coverage.created_or_unmatched_after_preopt_snapshot +=
                function_coverage.created_or_unmatched_after_preopt_snapshot;
        }
        coverage.pre_opt_snapshot_losses = self
            .pre_opt_snapshot_losses
            .iter()
            .filter(|record| functions.contains(&record.pre_opt().origin().function()))
            .count();
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

pub(super) fn push_sonatina_post_opt_origin_record<'db>(
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

pub(super) fn push_sonatina_backend_prepared_origin_record(
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

pub(super) fn push_sonatina_pre_opt_snapshot_loss_record<'db>(
    graph: &mut CodegenOriginGraph,
    record: SonatinaPreOptSnapshotLossRecord<'db>,
) {
    graph.push(
        CodegenOriginNode::SonatinaInst(record.pre_opt().origin()),
        CodegenOriginNode::SonatinaSynthetic(SonatinaSyntheticOrigin::PreOptSnapshotLoss),
        OriginLinkKind::Synthetic,
    );
}
