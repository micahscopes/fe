//! Maps the instructions a failed test executed back to Fe source.
//!
//! A [`FailureLocator`] is built while the suite's database is alive, from the
//! trace facts of the same lowering that produced the test's bytecode. Its init
//! code is compared byte for byte with the bytecode the runner executes, and a
//! test whose bytes differ gets no locator, so no location can describe code
//! that did not run. After a failure the runner replays the call, collects the
//! executed program counters, and asks the locator to explain them.

use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
    sync::Arc,
};

use codegen::{
    OptLevel, TestMetadata,
    trace::{ObservableTestTraceInput, PcFunctionRange, emit_observable_test_trace_facts},
};
use common::{file::IngotFileKind, ingot::Ingot};
use contract_harness::PcPath;
use debug_export::{DebugBundle, InstructionClassification};
use driver::DriverDataBase;
use hir::hir_def::TopLevelMod;
use trace_facts::{TraceBundle, TraceMetadata, TraceSnapshot};
use url::Url;

/// Source locators by test display name.
pub(super) type Locators = HashMap<String, Locator>;

/// Either a locator for the executed bytecode or the reason none exists.
#[derive(Debug, Clone)]
pub(super) enum Locator {
    Ready(Arc<FailureLocator>),
    Unavailable(String),
}

impl Locator {
    /// Extra lines to print under a failed test, without indentation.
    pub(super) fn describe(&self, path: &PcPath) -> Vec<String> {
        match self {
            Self::Ready(locator) => locator.describe(path),
            Self::Unavailable(reason) => vec![format!("source location unavailable: {reason}")],
        }
    }
}

#[derive(Debug)]
pub(super) struct FailureLocator {
    /// Runtime instructions in program-counter order.
    instructions: Vec<LocatedInstruction>,
    /// Compiled functions by program-counter range, in program-counter order.
    functions: Vec<PcFunctionRange>,
    /// First program counter of each compiled function's entry block.
    entries: BTreeSet<u32>,
}

#[derive(Debug)]
struct LocatedInstruction {
    start: u32,
    end: u32,
    mnemonic: String,
    source: Option<Arc<SourceRef>>,
}

/// An exact source attribution for one instruction.
#[derive(Debug)]
struct SourceRef {
    path: String,
    line: u32,
    column: u32,
    snippet: Option<String>,
    /// False for the bundled standard library, whose text is not on disk.
    user_code: bool,
}

impl SourceRef {
    fn render(&self) -> String {
        match &self.snippet {
            Some(snippet) => format!("{}:{}:{}  `{snippet}`", self.path, self.line, self.column),
            None => format!("{}:{}:{}", self.path, self.line, self.column),
        }
    }
}

struct Frame {
    function: String,
    /// The call expression in the caller that entered this frame.
    call_site: Option<Arc<SourceRef>>,
}

impl FailureLocator {
    fn new(
        bundle: &DebugBundle,
        runtime_code_object: &common::origin::OriginExportKey,
        functions: Vec<PcFunctionRange>,
    ) -> Self {
        let files: HashMap<_, _> = bundle
            .sources
            .iter()
            .map(|file| (&file.file_key, file))
            .collect();
        let spans: HashMap<_, _> = bundle
            .source_spans
            .iter()
            .map(|span| (&span.origin, span))
            .collect();
        let mut texts: HashMap<String, Option<String>> = HashMap::new();
        let mut refs: HashMap<(&common::origin::OriginExportKey, u32, u32), Arc<SourceRef>> =
            HashMap::new();

        let mut instructions = Vec::new();
        for instruction in &bundle.instructions {
            if instruction.code_object.as_ref() != Some(runtime_code_object) {
                continue;
            }
            let source = (instruction.classification == InstructionClassification::SourceMapped)
                .then(|| instruction.primary_source.as_ref())
                .flatten()
                .and_then(|origin| spans.get(origin))
                .and_then(|span| {
                    let file = files.get(&span.file)?;
                    let key = (&span.file, span.start_byte, span.end_byte);
                    let source = refs.entry(key).or_insert_with(|| {
                        let user_code = file.uri.starts_with("file:");
                        let text = user_code.then(|| {
                            texts
                                .entry(file.uri.clone())
                                .or_insert_with(|| read_source(&file.uri))
                                .as_deref()
                        });
                        let snippet = text.flatten().and_then(|text| {
                            snippet(text, span.start_byte as usize, span.end_byte as usize)
                        });
                        Arc::new(SourceRef {
                            path: display_path(&file.uri),
                            line: span.start_line,
                            column: span.start_column,
                            snippet,
                            user_code,
                        })
                    });
                    Some(Arc::clone(source))
                });
            instructions.push(LocatedInstruction {
                start: instruction.pc_range.start,
                end: instruction.pc_range.end,
                mnemonic: instruction.opcode_or_mnemonic.clone(),
                source,
            });
        }
        instructions.sort_by_key(|instruction| instruction.start);

        let mut entries = BTreeSet::new();
        let mut first_pc: HashMap<&str, u32> = HashMap::new();
        for range in &functions {
            let first = first_pc.entry(range.function.as_str()).or_insert(u32::MAX);
            *first = (*first).min(range.pc_start);
        }
        for range in &functions {
            if range.block == 0 && first_pc.get(range.function.as_str()) == Some(&range.pc_start) {
                entries.insert(range.pc_start);
            }
        }
        Self {
            instructions,
            functions,
            entries,
        }
    }

    fn instruction_at(&self, pc: u32) -> Option<&LocatedInstruction> {
        let index = self
            .instructions
            .partition_point(|instruction| instruction.start <= pc)
            .checked_sub(1)?;
        let instruction = &self.instructions[index];
        (pc < instruction.end).then_some(instruction)
    }

    fn function_at(&self, pc: u32) -> Option<&str> {
        let index = self
            .functions
            .partition_point(|range| range.pc_start <= pc)
            .checked_sub(1)?;
        let range = &self.functions[index];
        (pc < range.pc_end).then_some(range.function.as_str())
    }

    fn describe(&self, path: &PcPath) -> Vec<String> {
        let Some(&last) = path.pcs.last() else {
            return vec!["the failing call executed no instructions".to_string()];
        };
        let mut lines = Vec::new();
        let truncated = path.total_steps > path.pcs.len() as u64;
        let stack = self.frames(path);
        let innermost = stack
            .iter()
            .rev()
            .find(|frame| !frame.function.starts_with("test_root_"))
            .map(|frame| frame.function.as_str());
        let inside = |source: &SourceRef| match innermost {
            Some(function) => format!(
                "`{function}` ({}:{}:{})",
                source.path, source.line, source.column
            ),
            None => format!(
                "library code ({}:{}:{})",
                source.path, source.line, source.column
            ),
        };

        let failing = self.instruction_at(last);
        let mnemonic = failing.map_or("?", |instruction| instruction.mnemonic.as_str());
        let failing_source = failing.and_then(|instruction| instruction.source.as_ref());
        // The nearest user-code position at or before the failing instruction.
        let user_site = path.pcs.iter().rev().find_map(|&pc| {
            self.instruction_at(pc)
                .and_then(|instruction| instruction.source.as_ref())
                .filter(|source| source.user_code)
        });
        match (failing_source, user_site) {
            (Some(source), _) if source.user_code => {
                lines.push(format!("{mnemonic} at {}", source.render()));
            }
            (Some(source), Some(site)) => lines.push(format!(
                "{mnemonic} inside {}, reached from {}",
                inside(source),
                site.render()
            )),
            (source, None) => {
                let place = source.map_or_else(
                    || format!("at pc {last} has no source attribution"),
                    |source| format!("is inside {}", inside(source)),
                );
                lines.push(format!(
                    "{mnemonic} {place}, and no Fe source was reached before it; optimization \
                     may have folded or merged the code (`-O 0` keeps more)"
                ));
            }
            (None, Some(site)) => {
                lines.push(format!(
                    "{mnemonic} at pc {last} has no source attribution; last Fe source reached: {}",
                    site.render()
                ));
            }
        }

        let shown: Vec<_> = stack
            .iter()
            .enumerate()
            .filter(|(_, frame)| !frame.function.starts_with("test_root_"))
            .collect();
        if shown.len() > 1 || truncated {
            lines.push("call stack, innermost first:".to_string());
            for &(index, frame) in shown.iter().rev() {
                let position = match stack.get(index + 1) {
                    Some(callee) => callee.call_site.as_deref(),
                    None => failing_source.map(|source| &**source),
                };
                let position =
                    position.map_or_else(String::new, |site| format!("  {}", site.render()));
                lines.push(format!("  {}{position}", frame.function));
            }
            if truncated {
                lines.push(format!(
                    "  (only the last {} of {} executed steps were kept; outer frames may be missing)",
                    path.pcs.len(),
                    path.total_steps
                ));
            }
        }
        lines
    }

    /// Reconstructs the compiled-function call stack from the executed jumps.
    ///
    /// The EVM has no call frames. A JUMP that lands on a function's entry is
    /// read as a call, and a JUMP back into the middle of another compiled
    /// function as a return. Functions the optimizer inlined have no frame of
    /// their own.
    fn frames(&self, path: &PcPath) -> Vec<Frame> {
        let Some(&first) = path.pcs.first() else {
            return Vec::new();
        };
        let name = |pc| self.function_at(pc).unwrap_or("?").to_string();
        let mut stack = vec![Frame {
            function: name(first),
            call_site: None,
        }];
        for window in path.pcs.windows(2) {
            let (from, to) = (window[0], window[1]);
            let Some(jump) = self.instruction_at(from).filter(|i| i.mnemonic == "JUMP") else {
                continue;
            };
            if self.entries.contains(&to) {
                stack.push(Frame {
                    function: name(to),
                    call_site: jump.source.clone(),
                });
            } else if stack.len() > 1 && self.function_at(from) != self.function_at(to) {
                stack.pop();
            }
        }
        stack
    }
}

/// Builds locators for the tests of one module.
pub(super) fn locators_for_module(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    file_url: &Url,
    source_text: &str,
    opt_level: OptLevel,
    filter: Option<&str>,
    cases: &[TestMetadata],
    out: &mut Locators,
) {
    let display_name = file_url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .unwrap_or("source.fe")
        .to_string();
    let observed = match emit_observable_test_trace_facts(
        db,
        ObservableTestTraceInput {
            top_mod,
            input_owner_key: file_url.as_str(),
            source_uri: file_url.as_str(),
            source_display_name: &display_name,
            source_text,
            opt_level,
            filter,
        },
    ) {
        Ok(observed) => observed,
        Err(err) => {
            for case in cases {
                out.entry(case.display_name.clone()).or_insert_with(|| {
                    Locator::Unavailable(format!("tracing the test module failed: {err}"))
                });
            }
            return;
        }
    };
    if observed.contracts.is_empty() {
        return;
    }
    let metadata = TraceMetadata::compiler_emitted(
        crate::trace::compiler_commit(),
        "evm/sonatina",
        vec!["fe".to_string(), "test".to_string()],
        file_url.as_str(),
        vec![format!("optimize={opt_level}")],
    );
    let snapshot = match TraceSnapshot::new(TraceBundle::new(metadata, observed.facts)) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            for case in cases {
                if observed.contracts.contains_key(&case.object_name) {
                    out.insert(
                        case.display_name.clone(),
                        Locator::Unavailable(format!("trace facts failed validation: {err}")),
                    );
                }
            }
            return;
        }
    };
    let bundle = DebugBundle::from_snapshot(&snapshot);
    for case in cases {
        let Some(contract) = observed.contracts.get(&case.object_name) else {
            continue;
        };
        let locator = if contract.bytecode.deploy != case.bytecode {
            Locator::Unavailable(
                "the traced compile produced different bytecode than the one that ran".to_string(),
            )
        } else {
            Locator::Ready(Arc::new(FailureLocator::new(
                &bundle,
                &contract.runtime_code_object,
                contract.runtime_function_ranges(),
            )))
        };
        out.insert(case.display_name.clone(), locator);
    }
}

/// Builds locators for the tests of every source module of an ingot.
pub(super) fn locators_for_ingot(
    db: &DriverDataBase,
    ingot: Ingot<'_>,
    opt_level: OptLevel,
    filter: Option<&str>,
    cases: &[TestMetadata],
    out: &mut Locators,
) {
    for (file_url, file) in ingot.files(db).iter() {
        if file.kind(db) != Some(IngotFileKind::Source) {
            continue;
        }
        let top_mod = db.top_mod(file);
        if !super::has_test_functions(db, top_mod) {
            continue;
        }
        locators_for_module(
            db,
            top_mod,
            &file_url,
            file.text(db),
            opt_level,
            filter,
            cases,
            out,
        );
    }
}

fn read_source(uri: &str) -> Option<String> {
    let path = Url::parse(uri).ok()?.to_file_path().ok()?;
    std::fs::read_to_string(path).ok()
}

/// The first line of the span, whitespace-collapsed and shortened.
fn snippet(text: &str, start: usize, end: usize) -> Option<String> {
    const MAX: usize = 60;
    let span = text.get(start..end.min(text.len()))?;
    let first_line = span.lines().next()?;
    let collapsed = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    let multiline = span.lines().nth(1).is_some();
    if collapsed.chars().count() > MAX || multiline {
        let shortened: String = collapsed.chars().take(MAX).collect();
        return Some(format!("{shortened}..."));
    }
    Some(collapsed)
}

/// A path relative to the working directory when the file is under it.
fn display_path(uri: &str) -> String {
    let Some(path) = Url::parse(uri)
        .ok()
        .filter(|url| url.scheme() == "file")
        .and_then(|url| url.to_file_path().ok())
    else {
        return uri.to_string();
    };
    std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(&cwd).ok().map(Path::to_path_buf))
        .unwrap_or(path)
        .display()
        .to_string()
}
