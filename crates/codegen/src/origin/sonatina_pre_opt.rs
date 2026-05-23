use common::origin::{OriginExportKey, OriginExportKind, OriginKey, OriginLinkKind};
use mir::{RuntimeInstance, RuntimeStmtOrigin, RuntimeTerminatorOrigin};
use sonatina_ir::{InstId, module::FuncRef};

use super::function_keys::SonatinaFunctionExportKey;

common::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum SonatinaInstStage {
        PreOpt => "pre_opt",
        PostOpt => "post_opt",
        BackendPrepared => "backend_prepared",
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
}

impl common::origin::OriginExportLocalKey for SonatinaInstOrigin {
    fn to_export_local_key(&self) -> String {
        format!("{}:inst:{}", self.stage().as_str(), self.inst().0)
    }
}

pub fn sonatina_inst_export_key(
    origin: SonatinaInstOrigin,
    stable_function_key: &SonatinaFunctionExportKey,
) -> OriginExportKey {
    OriginExportKey::new(OriginExportKind::SonatinaInst, stable_function_key, &origin)
}

pub fn sonatina_synthetic_export_key(origin: SonatinaSyntheticOrigin) -> OriginExportKey {
    OriginExportKey::new(
        OriginExportKind::SonatinaSynthetic,
        &SonatinaSyntheticOwnerKey::new("sonatina"),
        &origin,
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

common::define_origin_owner_key! {
    pub struct SonatinaSyntheticOwnerKey;
}

impl common::origin::OriginExportLocalKey for SonatinaSyntheticOrigin {
    fn to_export_local_key(&self) -> String {
        self.as_str().to_string()
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
