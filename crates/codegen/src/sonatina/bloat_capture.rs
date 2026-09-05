//! Opt-in, compiler-owned code-growth observations.
//!
//! This module intentionally has no riff-cat dependency. The JSONL boundary is
//! versioned and self-describing so external tools can ingest it without
//! changing optimization behavior.

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use sonatina_codegen::{
    isa::{
        naga::{ShaderCallableHelper, ShaderHelperAnalysis},
        spirv::SpirvArtifact,
    },
    optim::inliner::{FullInlineCloneRecord, InlineStats},
};
use sonatina_ir::{
    Module,
    inst::{InstDowncast, control_flow::CallIndirect},
    module::FuncRef,
};

const EVENT_SCHEMA: &str = "fe-bloat-event/1";
const MAX_EVENTS: usize = 100_000;
const MAX_EVENT_STREAM_BYTES: usize = 64 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: usize = 256 * 1024 * 1024;

// Producer-owned records. This enum describes only the compatibility projection,
// not a compiler-wide vocabulary or an optimization API.
#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Event<'a> {
    CaptureStarted {
        pipeline: &'a str,
        compiler: Compiler,
        entry_labels: Vec<String>,
        environment: BTreeMap<String, String>,
        intervention: Intervention,
        graph_semantics: &'a str,
    },
    Stage {
        stage_id: &'a str,
        stage_kind: &'a str,
        predecessors: &'a [&'a str],
        functions: Vec<FunctionRecord>,
        selected_entries: Vec<String>,
        direct_calls: Vec<DirectCallRecord>,
        call_graph: Coverage,
        unknown_indirect_calls: Vec<UnknownCallRecord>,
        measurements: Vec<Measurement<'a>>,
    },
    ExactFunctionMerge {
        input_stage: &'a str,
        output_stage: &'a str,
        candidate_functions: usize,
        merged_functions: usize,
        rewritten_references: usize,
        refinement_rounds: usize,
        evidence: Evidence,
    },
    HelperAnalysis {
        stage: &'a str,
        callable: Vec<Callable>,
        backend_rejected: Vec<Rejected>,
        evidence: Evidence,
    },
    HelperSelection {
        stage: &'a str,
        baseline_retained: Vec<NamedFunction>,
        selected_retained: Vec<NamedFunction>,
        forced_inline: Vec<NamedFunction>,
        consequential_inline: Vec<NamedFunction>,
        evidence: Evidence,
    },
    InlineEvent {
        event_id: String,
        caller: ScopedFunction,
        callee: ScopedFunction,
        output_stage: &'a str,
        callsites: u64,
        cloned_instructions: u64,
        surviving_original_ids: u64,
        evidence: Evidence,
    },
    CloneCensus {
        observation_stage: &'a str,
        compiler_frontier: usize,
        compiler_stage: &'a str,
        caller: ScopedFunction,
        callee: ScopedFunction,
        callsites: u64,
        cloned_instructions: u64,
        surviving_original_ids: u64,
        semantics: &'a str,
        evidence: Evidence,
    },
    Artifacts {
        stage: &'a str,
        artifacts: Vec<Artifact>,
    },
    CaptureCompleted {
        final_stage: &'a str,
    },
    CaptureFailed {
        last_stage: &'a str,
        message: &'a str,
    },
}
#[derive(Serialize)]
struct Compiler {
    name: &'static str,
    version: &'static str,
}
#[derive(Serialize)]
struct Evidence {
    kind: &'static str,
    producer: &'static str,
}
fn evidence(producer: &'static str) -> Evidence {
    Evidence {
        kind: "compiler_event",
        producer,
    }
}
#[derive(Serialize)]
#[serde(tag = "completeness", rename_all = "snake_case")]
enum Coverage {
    Complete,
    Incomplete { reason: &'static str },
}
#[derive(Serialize)]
struct Measurement<'a> {
    name: &'a str,
    scope: &'a str,
    unit: &'a str,
    value: u64,
    evidence: Evidence,
}
#[derive(Serialize)]
struct NamedFunction {
    function: String,
    display_name: String,
}
#[derive(Serialize)]
struct Rejected {
    function: String,
    display_name: String,
    reason: String,
}
#[derive(Serialize)]
struct Callable {
    function: String,
    display_name: String,
    variants: usize,
    instructions: usize,
    accesses_resource: bool,
    maximum_physical_parameters: usize,
}
#[derive(Serialize)]
struct ScopedFunction {
    stage: &'static str,
    function: String,
}
fn normalized(function: FuncRef) -> ScopedFunction {
    ScopedFunction {
        stage: "normalized",
        function: function_id(function),
    }
}
#[derive(Serialize)]
struct Artifact {
    id: String,
    role: String,
    path: String,
    sha256: String,
    bytes: usize,
}

/// Caller-owned request configuration. The recorder never reads process policy.
pub(crate) struct CaptureConfig {
    pub directory: PathBuf,
    pub request_id: String,
    pub environment: BTreeMap<String, String>,
    pub intervention: Intervention,
    pub strict: bool,
    pub max_events: usize,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Intervention {
    None,
    ForceInlineNamedRetainedHelpers { requested: String },
}

pub(crate) struct CaptureObserver {
    request_id: String,
    directory: PathBuf,
    writer: BufWriter<File>,
    sequence: usize,
    bytes_written: usize,
    completed: bool,
    strict: bool,
    max_events: usize,
    failure: Option<String>,
}

impl CaptureObserver {
    pub(crate) fn is_recording(&self) -> bool {
        self.failure.is_none()
    }
    pub(crate) fn new(
        config: CaptureConfig,
        module: &Module,
        roots: &[FuncRef],
        pipeline: &str,
    ) -> Result<Self, String> {
        let base = config.directory;
        fs::create_dir_all(&base).map_err(|error| {
            format!(
                "FE_BLOAT_CAPTURE_DIR `{}` cannot be created: {error}",
                base.display()
            )
        })?;
        let request_id = sanitize(&config.request_id);
        let directory = base.join(&request_id);
        fs::create_dir(&directory).map_err(|error| {
            format!(
                "capture request directory `{}` must be new: {error}",
                directory.display()
            )
        })?;
        let path = directory.join("events.jsonl");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                format!(
                    "cannot create capture event file `{}`: {error}",
                    path.display()
                )
            })?;
        let mut observer = Self {
            request_id,
            directory,
            writer: BufWriter::new(file),
            sequence: 0,
            bytes_written: 0,
            completed: false,
            strict: config.strict,
            max_events: config.max_events.min(MAX_EVENTS),
            failure: None,
        };
        observer.event(Event::CaptureStarted {
            pipeline: pipeline,
            compiler: Compiler { name: "fe-codegen", version: env!("CARGO_PKG_VERSION") },
            entry_labels: roots.iter().map(|root| function_name(module, *root)).collect::<Vec<_>>(),
            environment: config.environment,
            intervention: config.intervention,
            graph_semantics: "Each function count scans every instruction in every layout block. Reachable unions are static direct-function call closure, not CFG path-feasible or entry-block-reachable instruction counts. Detailed inline and clone rows cover only Sonatina full-inliner clone records; trivial remove, rewrite, and splice paths are visible only in aggregate stage measurements.",
        })?;
        Ok(observer)
    }

    pub(crate) fn stage(
        &mut self,
        module: &Module,
        roots: &[FuncRef],
        stage_id: &str,
        stage_kind: &str,
        predecessors: &[&str],
        graph_detail: bool,
        inline_stats: Option<&InlineStats>,
    ) -> Result<(), String> {
        if !self.is_recording() {
            return Ok(());
        }
        let root_instructions = root_instruction_count(module, roots)?;
        let module_instructions = module_instruction_count(module)?;
        let (functions, direct_calls, unknown_indirect_calls, call_graph) = if graph_detail {
            let graph = capture_graph(module)?;
            (
                graph.functions,
                graph.direct_calls,
                graph.unknown_indirect_calls,
                if graph.has_indirect {
                    Coverage::Incomplete {
                        reason: "indirect call targets are not statically known",
                    }
                } else {
                    Coverage::Complete
                },
            )
        } else {
            (
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Coverage::Incomplete {
                    reason: "metric-only stage omitted the call graph",
                },
            )
        };
        let mut measurements = vec![
            measurement(
                "instructions",
                "all_module",
                "instructions",
                module_instructions,
            ),
            measurement(
                "selected_root_bodies",
                "selected_entry_bodies",
                "instructions",
                root_instructions,
            ),
            measurement(
                "functions",
                "all_module",
                "functions",
                u64::try_from(module.funcs().len()).map_err(display_error)?,
            ),
        ];
        if let Some(stats) = inline_stats {
            for (name, value) in [
                ("inliner_calls_removed", stats.calls_removed),
                ("inliner_calls_rewritten", stats.calls_rewritten),
                ("inliner_calls_spliced", stats.calls_spliced),
                ("inliner_full_calls_inlined", stats.full_calls_inlined),
            ] {
                measurements.push(measurement(
                    name,
                    "selected_entry_bodies",
                    "callsites",
                    u64::try_from(value).map_err(display_error)?,
                ));
            }
            measurements.push(measurement(
                "inliner_full_instructions_cloned",
                "selected_entry_bodies",
                "instructions",
                u64::try_from(stats.full_insts_cloned).map_err(display_error)?,
            ));
        }
        self.event(Event::Stage {
            stage_id: stage_id,
            stage_kind: stage_kind,
            predecessors: predecessors,
            functions: functions,
            selected_entries: if graph_detail {
                roots.iter().copied().map(function_id).collect::<Vec<_>>()
            } else {
                Vec::<String>::new()
            },
            direct_calls: direct_calls,
            call_graph: call_graph,
            unknown_indirect_calls: unknown_indirect_calls,
            measurements: measurements,
        })
    }

    pub(crate) fn exact_merge(
        &mut self,
        candidates: usize,
        merged: usize,
        rewritten_references: usize,
        rounds: usize,
    ) -> Result<(), String> {
        if !self.is_recording() {
            return Ok(());
        }
        self.event(Event::ExactFunctionMerge {
            input_stage: "pre-merge",
            output_stage: "post-merge",
            candidate_functions: candidates,
            merged_functions: merged,
            rewritten_references: rewritten_references,
            refinement_rounds: rounds,
            evidence: evidence("fe-codegen"),
        })
    }

    pub(crate) fn helper_analysis(
        &mut self,
        module: &Module,
        analysis: &ShaderHelperAnalysis,
    ) -> Result<(), String> {
        if !self.is_recording() {
            return Ok(());
        }
        let callable = analysis
            .callable
            .iter()
            .map(|helper| callable_helper(module, helper))
            .collect::<Vec<_>>();
        let rejected = analysis
            .rejected
            .iter()
            .map(|(function, reason)| Rejected {
                function: function_id(*function),
                display_name: function_name(module, *function),
                reason: reason.clone(),
            })
            .collect::<Vec<_>>();
        self.event(Event::HelperAnalysis {
            stage: "normalized",
            callable: callable,
            backend_rejected: rejected,
            evidence: evidence("sonatina-naga-helper-analysis"),
        })
    }

    pub(crate) fn helper_selection(
        &mut self,
        module: &Module,
        baseline: &[FuncRef],
        selected: &[FuncRef],
        forced_inline: &[FuncRef],
        consequential_inline: &[FuncRef],
    ) -> Result<(), String> {
        if !self.is_recording() {
            return Ok(());
        }
        let names = |functions: &[FuncRef]| {
            functions
                .iter()
                .map(|function| NamedFunction {
                    function: function_id(*function),
                    display_name: function_name(module, *function),
                })
                .collect::<Vec<_>>()
        };
        self.event(Event::HelperSelection {
            stage: "normalized",
            baseline_retained: names(baseline),
            selected_retained: names(selected),
            forced_inline: names(forced_inline),
            consequential_inline: names(consequential_inline),
            evidence: evidence("fe-helper-policy"),
        })
    }

    pub(crate) fn inline_events(
        &mut self,
        module: &Module,
        records: &[FullInlineCloneRecord],
        frontier: usize,
        output_stage: &str,
    ) -> Result<(), String> {
        if !self.is_recording() {
            return Ok(());
        }
        let mut grouped = BTreeMap::<(FuncRef, FuncRef), (u64, u64, u64)>::new();
        for record in records {
            let surviving = module.func_store.view(record.caller, |function| {
                record
                    .instructions
                    .iter()
                    .filter(|&&inst| function.layout.is_inst_inserted(inst))
                    .count()
            });
            let row = grouped.entry((record.caller, record.callee)).or_default();
            row.0 = row
                .0
                .checked_add(1)
                .ok_or("inline callsite count overflow")?;
            row.1 = row
                .1
                .checked_add(u64::try_from(record.instructions.len()).map_err(display_error)?)
                .ok_or("inline clone count overflow")?;
            row.2 = row
                .2
                .checked_add(u64::try_from(surviving).map_err(display_error)?)
                .ok_or("inline survival count overflow")?;
        }
        for ((caller, callee), (callsites, cloned, surviving)) in grouped {
            self.event(Event::InlineEvent {
                event_id: format!(
                    "frontier-{frontier:02}-{}-{}",
                    function_id(caller),
                    function_id(callee)
                ),
                caller: normalized(caller),
                callee: normalized(callee),
                output_stage: output_stage,
                callsites: callsites,
                cloned_instructions: cloned,
                surviving_original_ids: surviving,
                evidence: evidence("sonatina-rooted-inliner"),
            })?;
        }
        Ok(())
    }

    pub(crate) fn clone_census(
        &mut self,
        module: &Module,
        records: &[FullInlineCloneRecord],
        frontier: usize,
        compiler_stage: &str,
        observation_stage: &str,
    ) -> Result<(), String> {
        if !self.is_recording() {
            return Ok(());
        }
        let mut grouped = BTreeMap::<(FuncRef, FuncRef), (u64, u64, u64)>::new();
        for record in records {
            let surviving = module.func_store.view(record.caller, |function| {
                record
                    .instructions
                    .iter()
                    .filter(|&&inst| function.layout.is_inst_inserted(inst))
                    .count()
            });
            let row = grouped.entry((record.caller, record.callee)).or_default();
            row.0 = row
                .0
                .checked_add(1)
                .ok_or("clone census callsite count overflow")?;
            row.1 = row
                .1
                .checked_add(u64::try_from(record.instructions.len()).map_err(display_error)?)
                .ok_or("clone census instruction count overflow")?;
            row.2 = row
                .2
                .checked_add(u64::try_from(surviving).map_err(display_error)?)
                .ok_or("clone census survival count overflow")?;
        }
        for ((caller, callee), (callsites, cloned, surviving)) in grouped {
            self.event(Event::CloneCensus {
                observation_stage: observation_stage,
                compiler_frontier: frontier,
                compiler_stage: compiler_stage,
                caller: normalized(caller),
                callee: normalized(callee),
                callsites: callsites,
                cloned_instructions: cloned,
                surviving_original_ids: surviving,
                semantics: "cumulative_literal_original_instruction_ids",
                evidence: evidence("fe-codegen"),
            })?;
        }
        Ok(())
    }

    pub(crate) fn complete(
        mut self,
        artifact: &SpirvArtifact,
        final_stage: &str,
    ) -> Result<(), String> {
        let mut artifacts = Vec::new();
        if let Some(wgsl) = &artifact.wgsl {
            artifacts.push(write_artifact(
                &self.directory,
                "shader.wgsl",
                "emitted_wgsl",
                wgsl.as_bytes(),
            )?);
        }
        let spirv_bytes = artifact
            .words
            .len()
            .checked_mul(std::mem::size_of::<u32>())
            .ok_or("SPIR-V artifact byte count overflow")?;
        if spirv_bytes > MAX_ARTIFACT_BYTES {
            return Err(format!(
                "artifact `shader.spv` exceeds {MAX_ARTIFACT_BYTES} byte limit"
            ));
        }
        let spirv = artifact
            .words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>();
        artifacts.push(write_artifact(
            &self.directory,
            "shader.spv",
            "emitted_spirv",
            &spirv,
        )?);
        self.event(Event::Artifacts {
            stage: final_stage,
            artifacts: artifacts,
        })?;
        self.event(Event::CaptureCompleted {
            final_stage: final_stage,
        })?;
        self.writer.flush().map_err(display_error)?;
        self.completed = true;
        Ok(())
    }

    pub(crate) fn fail(mut self, message: &str, last_stage: &str) -> Result<(), String> {
        self.event(Event::CaptureFailed {
            last_stage,
            message,
        })?;
        self.writer.flush().map_err(display_error)?;
        self.completed = true;
        Ok(())
    }

    fn event(&mut self, event: Event<'_>) -> Result<(), String> {
        if self.failure.is_some() {
            return Ok(());
        }
        let result = self.write_event(event);
        match result {
            Ok(()) => Ok(()),
            Err(error) => self.record_error(error),
        }
    }

    pub(crate) fn record_error(&mut self, error: String) -> Result<(), String> {
        if self.failure.is_none() {
            eprintln!("fe observation incomplete: {error}");
            self.failure = Some(error.clone());
        }
        if self.strict { Err(error) } else { Ok(()) }
    }

    fn write_event(&mut self, event: Event<'_>) -> Result<(), String> {
        if self.sequence >= self.max_events {
            return Err(format!("capture exceeds {} event limit", self.max_events));
        }
        let record = EventRecord {
            schema: EVENT_SCHEMA,
            request_id: &self.request_id,
            sequence: self.sequence,
            event,
        };
        let mut bytes = serde_json::to_vec(&record).map_err(display_error)?;
        bytes.push(b'\n');
        let next_bytes = self
            .bytes_written
            .checked_add(bytes.len())
            .ok_or("capture event-stream byte count overflow")?;
        if next_bytes > MAX_EVENT_STREAM_BYTES {
            return Err(format!(
                "capture exceeds {MAX_EVENT_STREAM_BYTES} byte event-stream limit"
            ));
        }
        self.writer.write_all(&bytes).map_err(display_error)?;
        self.sequence += 1;
        self.bytes_written = next_bytes;
        Ok(())
    }
}

impl Drop for CaptureObserver {
    fn drop(&mut self) {
        if !self.completed {
            let _ = self.writer.flush();
        }
    }
}

#[derive(Serialize)]
struct EventRecord<'a> {
    schema: &'static str,
    request_id: &'a str,
    sequence: usize,
    #[serde(flatten)]
    event: Event<'a>,
}

#[derive(Serialize)]
struct FunctionRecord {
    id: String,
    display_name: String,
    instructions: u64,
}
#[derive(Serialize)]
struct DirectCallRecord {
    caller: String,
    callee: String,
    callsites: u64,
}
#[derive(Serialize)]
struct UnknownCallRecord {
    caller: String,
    callsites: u64,
    reason: String,
}
struct GraphRecord {
    functions: Vec<FunctionRecord>,
    direct_calls: Vec<DirectCallRecord>,
    unknown_indirect_calls: Vec<UnknownCallRecord>,
    has_indirect: bool,
}

fn capture_graph(module: &Module) -> Result<GraphRecord, String> {
    let mut functions = Vec::new();
    let mut direct = BTreeMap::<(FuncRef, FuncRef), u64>::new();
    let mut indirect = BTreeMap::<FuncRef, u64>::new();
    for function_ref in module.funcs() {
        let instructions = module
            .func_store
            .try_view(function_ref, |function| {
                let mut count = 0u64;
                for instruction in function
                    .layout
                    .iter_block()
                    .flat_map(|block| function.layout.iter_inst(block))
                {
                    count = count
                        .checked_add(1)
                        .ok_or("function instruction count overflow")?;
                    if let Some(call) = function.dfg.call_info(instruction) {
                        let row = direct.entry((function_ref, call.callee())).or_default();
                        *row = row.checked_add(1).ok_or("direct callsite count overflow")?;
                    } else if <&CallIndirect as InstDowncast>::downcast(
                        function.dfg.inst_set(),
                        function.dfg.inst(instruction),
                    )
                    .is_some()
                    {
                        let row = indirect.entry(function_ref).or_default();
                        *row = row
                            .checked_add(1)
                            .ok_or("indirect callsite count overflow")?;
                    }
                }
                Ok::<_, &'static str>(count)
            })
            .ok_or_else(|| format!("function {} has no body", function_id(function_ref)))?
            .map_err(str::to_owned)?;
        functions.push(FunctionRecord {
            id: function_id(function_ref),
            display_name: function_name(module, function_ref),
            instructions,
        });
    }
    let direct_calls = direct
        .into_iter()
        .map(|((caller, callee), callsites)| DirectCallRecord {
            caller: function_id(caller),
            callee: function_id(callee),
            callsites,
        })
        .collect();
    let has_indirect = !indirect.is_empty();
    let unknown_indirect_calls = indirect
        .into_iter()
        .map(|(caller, callsites)| UnknownCallRecord {
            caller: function_id(caller),
            callsites,
            reason: "CallIndirect target set is unknown".into(),
        })
        .collect();
    Ok(GraphRecord {
        functions,
        direct_calls,
        unknown_indirect_calls,
        has_indirect,
    })
}

fn root_instruction_count(module: &Module, roots: &[FuncRef]) -> Result<u64, String> {
    roots.iter().try_fold(0u64, |total, root| {
        let count = module
            .func_store
            .try_view(*root, |function| {
                function
                    .layout
                    .iter_block()
                    .map(|block| function.layout.iter_inst(block).count())
                    .sum::<usize>()
            })
            .ok_or_else(|| format!("root {} has no body", function_id(*root)))?;
        total
            .checked_add(u64::try_from(count).map_err(display_error)?)
            .ok_or_else(|| "root instruction count overflow".into())
    })
}

fn module_instruction_count(module: &Module) -> Result<u64, String> {
    module
        .funcs()
        .into_iter()
        .try_fold(0u64, |total, function_ref| {
            let count = module
                .func_store
                .try_view(function_ref, |function| {
                    function
                        .layout
                        .iter_block()
                        .map(|block| function.layout.iter_inst(block).count())
                        .sum::<usize>()
                })
                .ok_or_else(|| format!("function {} has no body", function_id(function_ref)))?;
            total
                .checked_add(u64::try_from(count).map_err(display_error)?)
                .ok_or_else(|| "module instruction count overflow".into())
        })
}

fn measurement<'a>(name: &'a str, scope: &'a str, unit: &'a str, value: u64) -> Measurement<'a> {
    Measurement {
        name,
        scope,
        unit,
        value,
        evidence: evidence("fe-codegen"),
    }
}
fn callable_helper(module: &Module, helper: &ShaderCallableHelper) -> Callable {
    Callable {
        function: function_id(helper.function),
        display_name: function_name(module, helper.function),
        variants: helper.variants,
        instructions: helper.instruction_count,
        accesses_resource: helper.accesses_resource,
        maximum_physical_parameters: helper.maximum_physical_parameters,
    }
}
fn function_id(function: FuncRef) -> String {
    format!("f{}", function.as_u32())
}
fn function_name(module: &Module, function: FuncRef) -> String {
    module
        .ctx
        .get_sig(function)
        .map(|signature| signature.name().to_owned())
        .unwrap_or_else(|| function_id(function))
}
fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}
fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn write_artifact(
    directory: &Path,
    name: &str,
    role: &str,
    bytes: &[u8],
) -> Result<Artifact, String> {
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(format!(
            "artifact `{name}` exceeds {MAX_ARTIFACT_BYTES} byte limit"
        ));
    }
    let path = directory.join(name);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(display_error)?;
    output.write_all(bytes).map_err(display_error)?;
    output.flush().map_err(display_error)?;
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    Ok(Artifact {
        id: name.into(),
        role: role.into(),
        path: name.into(),
        sha256,
        bytes: bytes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer(directory: &Path, strict: bool, max_events: usize) -> CaptureObserver {
        CaptureObserver {
            request_id: "test-request".into(),
            directory: directory.into(),
            writer: BufWriter::new(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(directory.join("events.jsonl"))
                    .unwrap(),
            ),
            sequence: 0,
            bytes_written: 0,
            completed: false,
            strict,
            max_events,
            failure: None,
        }
    }

    #[test]
    fn ordinary_budget_exhaustion_never_emits_false_completion() {
        let directory = tempfile::tempdir().unwrap();
        let mut observer = writer(directory.path(), false, 1);
        observer
            .event(Event::ExactFunctionMerge {
                input_stage: "pre-merge",
                output_stage: "post-merge",
                candidate_functions: 0,
                merged_functions: 0,
                rewritten_references: 0,
                refinement_rounds: 0,
                evidence: evidence("fe-codegen"),
            })
            .unwrap();
        observer
            .event(Event::CaptureCompleted {
                final_stage: "final",
            })
            .unwrap();
        assert!(observer.failure.is_some());
        drop(observer);
        let text = fs::read_to_string(directory.path().join("events.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 1);
        let record: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(record["schema"], EVENT_SCHEMA);
        assert_eq!(record["event"], "exact_function_merge");
        assert_eq!(record["sequence"], 0);
        assert!(!text.contains("capture_completed"));
    }

    #[test]
    fn strict_budget_exhaustion_returns_recording_error() {
        let directory = tempfile::tempdir().unwrap();
        let mut observer = writer(directory.path(), true, 0);
        assert!(
            observer
                .event(Event::CaptureCompleted {
                    final_stage: "final"
                })
                .unwrap_err()
                .contains("event limit")
        );
    }

    #[test]
    fn typed_terminal_record_keeps_legacy_wire_shape() {
        let directory = tempfile::tempdir().unwrap();
        let mut observer = writer(directory.path(), false, 1);
        observer
            .event(Event::CaptureFailed {
                last_stage: "normalized",
                message: "test failure",
            })
            .unwrap();
        drop(observer);
        let text = fs::read_to_string(directory.path().join("events.jsonl")).unwrap();
        let record: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(record["event"], "capture_failed");
        assert_eq!(record["last_stage"], "normalized");
        assert_eq!(record["message"], "test failure");
    }

    #[test]
    fn artifact_and_request_names_are_path_safe() {
        assert_eq!(sanitize("main/render:0"), "main_render_0");
    }
}
