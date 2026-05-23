use std::{collections::BTreeSet, fmt};

use common::facts::TypedFactSet;
use hir::analysis::diagnostics::SpannedHirAnalysisDb;
use mir::RuntimePackageOrigins;
use sonatina_codegen::object::{ObjectArtifact, PcMapEntry};
use sonatina_ir::module::FuncRef;

use super::{
    bytecode_coverage::{BytecodeOriginCoverage, bytecode_origin_coverage_for_records},
    bytecode_graph::push_bytecode_origin_record,
    bytecode_keys::{
        BytecodeObjectKey, BytecodePcOrigin, BytecodePcRange, BytecodeSectionKey,
        BytecodeSectionNameKey, BytecodeUnmappedReason,
    },
    codegen_graph::{CodegenOriginGraph, codegen_origin_graph_facts},
    end_to_end_graph::{
        EndToEndOriginGraph, collect_end_to_end_graph_function_keys,
        end_to_end_origin_graph_facts_with_function_keys, push_bytecode_end_to_end_origin_record,
        push_selected_runtime_and_sonatina_origins,
    },
    frontend_labels::{
        FrontendOriginLabelMap, frontend_label_for_pre_opt_source,
        pre_opt_source_has_frontend_label,
    },
    function_keys::{
        MissingSonatinaFunctionKey, SonatinaFunctionExportKey, SonatinaFunctionKeyMap,
    },
    sonatina_post_opt::{
        SonatinaBackendPreparedOriginRecord, SonatinaBackendPreparedOriginSource,
        SonatinaPostOptOriginCoverage, SonatinaPostOptOriginRecord, SonatinaPostOptOriginSource,
        SonatinaPostOptPackageOrigins,
    },
    sonatina_pre_opt::{SonatinaInstOrigin, SonatinaPackageOrigins},
    source_resolution::{self, BytecodeSourceResolution},
};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BytecodePackageOriginsError {
    InvalidPcRange {
        object: String,
        section: String,
        pc_start: u32,
        pc_end: u32,
    },
    OverlappingPcRange {
        object: String,
        section: String,
        previous_start: u32,
        previous_end: u32,
        current_start: u32,
        current_end: u32,
    },
}

impl fmt::Display for BytecodePackageOriginsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BytecodePackageOriginsError::InvalidPcRange {
                object,
                section,
                pc_start,
                pc_end,
            } => write!(
                f,
                "bytecode origin PC-map ranges must be non-empty and ordered: object `{object}` section `{section}` range {pc_start}..{pc_end}"
            ),
            BytecodePackageOriginsError::OverlappingPcRange {
                object,
                section,
                previous_start,
                previous_end,
                current_start,
                current_end,
            } => write!(
                f,
                "bytecode origin PC ranges must not overlap within one object section: object `{object}` section `{section}` range {previous_start}..{previous_end} overlaps {current_start}..{current_end}"
            ),
        }
    }
}

impl std::error::Error for BytecodePackageOriginsError {}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BytecodePackageOrigins<'db> {
    pub(super) records: Vec<BytecodeOriginRecord<'db>>,
}

impl<'db> BytecodePackageOrigins<'db> {
    pub fn from_artifacts(
        artifacts: &[ObjectArtifact],
        post_opt: &SonatinaPostOptPackageOrigins<'db>,
    ) -> Self {
        Self::try_from_artifacts(artifacts, post_opt).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_from_artifacts(
        artifacts: &[ObjectArtifact],
        post_opt: &SonatinaPostOptPackageOrigins<'db>,
    ) -> Result<Self, BytecodePackageOriginsError> {
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
                    let range = bytecode_pc_range_from_entry(
                        &artifact.object.0,
                        &section_name.0,
                        entry.pc_start,
                        entry.pc_end,
                    )?;
                    let pc = BytecodePcOrigin::new(section_key.clone(), range);
                    let source = bytecode_source_from_pc_entry(entry, post_opt);
                    records.push(BytecodeOriginRecord::new(pc, source));
                }
            }
        }
        sort_bytecode_origin_records(&mut records);
        ensure_bytecode_origin_records_do_not_overlap(&records)?;
        Ok(Self { records })
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

    pub fn post_opt_origin_coverage_for_object(
        &self,
        object: &BytecodeObjectKey,
        post_opt_origins: &SonatinaPostOptPackageOrigins<'db>,
    ) -> SonatinaPostOptOriginCoverage {
        let functions = self.post_opt_functions_for_object(object);
        post_opt_origins.coverage_for_functions(&functions)
    }

    pub fn post_opt_origin_coverage_for_section(
        &self,
        section: &BytecodeSectionKey,
        post_opt_origins: &SonatinaPostOptPackageOrigins<'db>,
    ) -> SonatinaPostOptOriginCoverage {
        let functions = self.post_opt_functions_for_section(section);
        post_opt_origins.coverage_for_functions(&functions)
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
                    source_resolution::resolve_bytecode_source(
                        db,
                        runtime_origins,
                        record.source(),
                    ),
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
        stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
    ) -> FrontendOriginLabelMap {
        self.try_frontend_origin_label_map(stable_function_key)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_frontend_origin_label_map(
        &self,
        mut stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
    ) -> Result<FrontendOriginLabelMap, MissingSonatinaFunctionKey> {
        let mut map = FrontendOriginLabelMap::default();
        for record in &self.records {
            let BytecodeOriginSource::SonatinaPostOpt(post_opt) = record.source() else {
                continue;
            };
            let SonatinaPostOptOriginSource::SameInstId(pre_opt) = post_opt.source() else {
                continue;
            };
            let source = pre_opt.source();
            if !pre_opt_source_has_frontend_label(source) {
                continue;
            }
            let function_key = stable_function_key(post_opt.origin().function())
                .ok_or_else(|| MissingSonatinaFunctionKey::new(post_opt.origin().function()))?;
            let label = frontend_label_for_pre_opt_source(source, &function_key)
                .expect("checked frontend label source should produce a label");
            map.insert_if_absent(
                post_opt.origin().function(),
                post_opt.origin().inst(),
                label,
            );
        }
        Ok(map)
    }

    fn post_opt_functions_for_object(&self, object: &BytecodeObjectKey) -> BTreeSet<FuncRef> {
        self.records
            .iter()
            .filter(|record| record.origin().section().object() == object)
            .filter_map(bytecode_origin_record_function)
            .collect()
    }

    fn post_opt_functions_for_section(&self, section: &BytecodeSectionKey) -> BTreeSet<FuncRef> {
        self.records
            .iter()
            .filter(|record| record.origin().section() == section)
            .filter_map(bytecode_origin_record_function)
            .collect()
    }
}

fn bytecode_origin_record_function(record: &BytecodeOriginRecord<'_>) -> Option<FuncRef> {
    match record.source() {
        BytecodeOriginSource::SonatinaPostOpt(post_opt) => Some(post_opt.origin().function()),
        BytecodeOriginSource::SonatinaBackendPrepared(backend_prepared) => {
            Some(backend_prepared.origin().function())
        }
        BytecodeOriginSource::Unmapped(_) => None,
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

fn bytecode_pc_range_from_entry(
    object: &str,
    section: &str,
    pc_start: u32,
    pc_end: u32,
) -> Result<BytecodePcRange, BytecodePackageOriginsError> {
    BytecodePcRange::new(pc_start, pc_end).ok_or_else(|| {
        BytecodePackageOriginsError::InvalidPcRange {
            object: object.to_string(),
            section: section.to_string(),
            pc_start,
            pc_end,
        }
    })
}

fn ensure_bytecode_origin_records_do_not_overlap(
    records: &[BytecodeOriginRecord<'_>],
) -> Result<(), BytecodePackageOriginsError> {
    for pair in records.windows(2) {
        let previous = &pair[0];
        let current = &pair[1];
        if previous.origin().section() != current.origin().section() {
            continue;
        }
        if previous.origin().range().end() > current.origin().range().start() {
            return Err(BytecodePackageOriginsError::OverlappingPcRange {
                object: current.origin().section().object().as_str().to_string(),
                section: current.origin().section().section().to_string(),
                previous_start: previous.origin().range().start(),
                previous_end: previous.origin().range().end(),
                current_start: current.origin().range().start(),
                current_end: current.origin().range().end(),
            });
        }
    }
    Ok(())
}

pub(super) fn bytecode_source_from_pc_entry<'db>(
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
