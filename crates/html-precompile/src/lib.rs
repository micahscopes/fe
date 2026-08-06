//! HTML5-parsed production precompilation for inert Fe script elements.
//!
//! This library shares the compiler protocol and artifacts with the browser
//! Worker. HTML parsing and URL resolution are host tooling concerns; neither
//! Web IDL nor the Fe compiler knows about script elements.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use base64::Engine;
use fe_compiler_protocol::{
    ArtifactKind, CompileOptions, CompileRequest, CompileTarget, Diagnostic, ProtocolVersion,
    PublishedArtifact, PublishedModuleManifest, SourceDependencyInventory, VirtualSource,
    sha256_hex,
};
use fe_webidl_bindgen::{
    AdapterOperationMetadata, AdapterPlan, World as WebIdlWorld, adapter_operation_metadata,
    emit_js_selected_adapter, select_adapter_operations,
};
use html5ever::{
    Attribute, LocalName, QualName, ns,
    serialize::{SerializeOpts, serialize},
    tendril::TendrilSink,
};
use markup5ever_rcdom::{Handle, Node, NodeData, RcDom, SerializableHandle};
use serde::Serialize;
use url::Url;

pub const SOURCE_SCRIPT_TYPE: &str = "application/fe";
pub const ARTIFACT_SCRIPT_TYPE: &str = "application/fe+wasm";
pub const BOOTSTRAP_MARKER: &str = "data-fe-bootstrap";
pub const BOOTSTRAP_META_NAME: &str = "fe-bootstrap";
/// Valueless marker on a rewritten `application/fe+wasm` script: the bootstrap
/// hands this module to `mountRenderSurface` from the render runtime module
/// instead of instantiating it and calling its entry with zero arguments.
pub const RENDER_SCRIPT_MARKER: &str = "data-fe-render";
/// Points at the published, content-addressed render runtime module
/// (`fe_codegen::render_runtime_js()`) a `data-fe-render` script hands off to.
pub const RENDER_RUNTIME_ATTR: &str = "data-fe-render-runtime";
const BOOTSTRAP_SOURCE: &str = include_str!("../assets/bootstrap.js");

/// A compiled render bundle for one `data-fe-src` naming a GpuProgram-actor
/// ingot, produced through the SAME seam `fe web build` uses
/// (`resolve_web_entry` + `WebBundle::compile`), ready for content-addressed
/// publication by [`precompile_html_with_render_lane`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderBundleArtifact {
    pub wasm: Vec<u8>,
    pub wgsl: Vec<u8>,
    /// The bundle's fe-web-bundle v4 manifest exactly as
    /// `WebBundle::manifest_json()` produces it: `artifacts.wasm` /
    /// `artifacts.wgsl` still name the bundle-local `module.wasm` /
    /// `shader.wgsl` placeholders. Publication rewrites those two fields to
    /// the content-addressed published names, the same precedent as the
    /// wasm lane's `PublishedArtifact::from_artifact`.
    pub manifest_json: Vec<u8>,
    /// The render source's structural dependency closure (ingot files),
    /// folded into the development watch graph exactly like the wasm lane's
    /// `PublishedModuleManifest::source_dependencies`.
    pub source_dependencies: Option<SourceDependencyInventory>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationReport {
    pub modules: usize,
    pub files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationError {
    pub context: String,
    pub detail: String,
}

impl std::fmt::Display for VerificationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.context, self.detail)
    }
}

impl std::error::Error for VerificationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrecompileOutput {
    pub html: String,
    /// URL-path relative artifact files, ordered for deterministic publication.
    pub assets: BTreeMap<String, Vec<u8>>,
    pub modules: Vec<PublishedModuleManifest>,
    /// Structural dependency inventories contributed by render-lane sources
    /// (see [`RenderBundleArtifact::source_dependencies`]). Always empty when
    /// no script routed through the render lane. Folded into the development
    /// watch graph alongside `modules[].source_dependencies`.
    pub render_dependencies: Vec<SourceDependencyInventory>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrecompileError {
    InvalidDocumentUrl(String),
    InvalidBaseUrl {
        value: String,
        detail: String,
    },
    SourceLoad {
        url: String,
        detail: String,
    },
    Compile {
        source_url: String,
        detail: String,
    },
    Diagnostics {
        source_url: String,
        diagnostics: Vec<Diagnostic>,
    },
    AdapterSelection {
        source_url: String,
        detail: String,
    },
    MissingWasm(String),
    Serialize(String),
}

/// Minimal development dependency graph for HTML Fe data blocks.
///
/// This graph deliberately records only dependencies that the HTML precompiler
/// can prove: documents and their external `data-fe-src` URLs. Fe module/import
/// edges require compiler-reported dependency data and are not guessed here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DevelopmentDependencyGraph {
    forward: BTreeMap<String, BTreeSet<String>>,
    reverse: BTreeMap<String, BTreeSet<String>>,
}

impl DevelopmentDependencyGraph {
    pub fn dependencies(&self, document_url: &str) -> Vec<String> {
        self.forward
            .get(document_url)
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }

    pub fn affected_documents(&self, changed_url: &str) -> Vec<String> {
        let mut affected = self.reverse.get(changed_url).cloned().unwrap_or_default();
        if self.forward.contains_key(changed_url) {
            affected.insert(changed_url.to_owned());
        }
        affected.into_iter().collect()
    }

    fn replace(&mut self, document_url: String, dependencies: BTreeSet<String>) {
        if let Some(previous) = self
            .forward
            .insert(document_url.clone(), dependencies.clone())
        {
            for dependency in previous {
                if let Some(documents) = self.reverse.get_mut(&dependency) {
                    documents.remove(&document_url);
                    if documents.is_empty() {
                        self.reverse.remove(&dependency);
                    }
                }
            }
        }
        for dependency in dependencies {
            self.reverse
                .entry(dependency)
                .or_default()
                .insert(document_url.clone());
        }
    }
}

/// One immutable, content-addressed successful publication.
#[derive(Debug, Clone)]
pub struct DevelopmentPublication {
    digest: String,
    output: Arc<PrecompileOutput>,
}

impl DevelopmentPublication {
    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn output(&self) -> &PrecompileOutput {
        &self.output
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DevelopmentDiagnostic {
    pub code: String,
    pub source_url: Option<String>,
    pub message: String,
    pub compiler_diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
pub struct DevelopmentBuildReport {
    /// Active last-good output after this attempt, including on failure.
    pub active: Option<DevelopmentPublication>,
    /// True only when this attempt installed different successful content.
    pub published: bool,
    pub diagnostics: Vec<DevelopmentDiagnostic>,
}

/// A deterministic debounced unit of rebuild work.
///
/// Batches are generation-stamped. A batch becomes cancelled when a later
/// relevant change is queued before it is executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevelopmentRebuildBatch {
    pub generation: u64,
    pub changed_urls: Vec<String>,
    pub document_urls: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DevelopmentRebuildEvent {
    Scheduled {
        generation: u64,
        deadline_ms: u64,
        changed_urls: Vec<String>,
        document_urls: Vec<String>,
    },
    Cancelled {
        generation: u64,
        document_urls: Vec<String>,
    },
    Publication {
        document_url: String,
        digest: String,
        changed: bool,
    },
    Diagnostic {
        document_url: String,
        diagnostic: DevelopmentDiagnostic,
        serving_last_good: bool,
    },
    Reload {
        document_url: String,
        digest: String,
    },
}

/// Server-neutral debounce and rebuild coordination.
///
/// Time is supplied by the caller as monotonic milliseconds. Watching files,
/// loading documents, serving artifacts, and transporting reload events remain
/// host policies.
#[derive(Debug)]
pub struct DevelopmentRebuildCoordinator {
    precompiler: DevelopmentPrecompiler,
    debounce_ms: u64,
    generation: u64,
    deadline_ms: Option<u64>,
    changed_urls: BTreeSet<String>,
    document_urls: BTreeSet<String>,
}

impl DevelopmentRebuildCoordinator {
    pub fn new(debounce_ms: u64) -> Self {
        Self {
            precompiler: DevelopmentPrecompiler::default(),
            debounce_ms,
            generation: 0,
            deadline_ms: None,
            changed_urls: BTreeSet::new(),
            document_urls: BTreeSet::new(),
        }
    }

    pub fn precompiler(&self) -> &DevelopmentPrecompiler {
        &self.precompiler
    }

    pub fn precompiler_mut(&mut self) -> &mut DevelopmentPrecompiler {
        &mut self.precompiler
    }

    /// Queue changed source URLs and restart the debounce window.
    ///
    /// URLs unknown to the proven dependency graph are ignored.
    pub fn queue_changes(
        &mut self,
        now_ms: u64,
        changed_urls: impl IntoIterator<Item = String>,
    ) -> Option<DevelopmentRebuildEvent> {
        let mut relevant_changes = BTreeSet::new();
        let mut affected = BTreeSet::new();
        for changed_url in changed_urls {
            let documents = self.precompiler.graph().affected_documents(&changed_url);
            if !documents.is_empty() {
                relevant_changes.insert(changed_url);
                affected.extend(documents);
            }
        }
        if affected.is_empty() {
            return None;
        }

        self.generation = self.generation.wrapping_add(1);
        self.changed_urls.extend(relevant_changes);
        self.document_urls.extend(affected);
        let deadline_ms = now_ms.saturating_add(self.debounce_ms);
        self.deadline_ms = Some(deadline_ms);
        Some(DevelopmentRebuildEvent::Scheduled {
            generation: self.generation,
            deadline_ms,
            changed_urls: self.changed_urls.iter().cloned().collect(),
            document_urls: self.document_urls.iter().cloned().collect(),
        })
    }

    pub fn take_ready(&mut self, now_ms: u64) -> Option<DevelopmentRebuildBatch> {
        if now_ms < self.deadline_ms? {
            return None;
        }
        self.deadline_ms = None;
        Some(DevelopmentRebuildBatch {
            generation: self.generation,
            changed_urls: std::mem::take(&mut self.changed_urls).into_iter().collect(),
            document_urls: std::mem::take(&mut self.document_urls)
                .into_iter()
                .collect(),
        })
    }

    /// Execute a ready batch in document URL order, wasm-only lane.
    ///
    /// A stale generation is cancelled before any loader or compiler work.
    pub fn execute(
        &mut self,
        batch: DevelopmentRebuildBatch,
        load_document: impl FnMut(&str) -> Result<String, String>,
        load_source: impl FnMut(&Url) -> Result<String, String>,
    ) -> Vec<DevelopmentRebuildEvent> {
        self.execute_with_render_lane(batch, load_document, "", load_source, |_, _| Ok(None))
    }

    /// Execute a ready batch in document URL order, with the render-bundle
    /// lane wired in (see [`DevelopmentPrecompiler::build_with_render_lane`]).
    ///
    /// A stale generation is cancelled before any loader or compiler work.
    pub fn execute_with_render_lane(
        &mut self,
        batch: DevelopmentRebuildBatch,
        mut load_document: impl FnMut(&str) -> Result<String, String>,
        render_runtime_js: &str,
        mut load_source: impl FnMut(&Url) -> Result<String, String>,
        mut render_compile: impl FnMut(&Url, Option<&str>) -> Result<Option<RenderBundleArtifact>, String>,
    ) -> Vec<DevelopmentRebuildEvent> {
        if batch.generation != self.generation {
            return vec![DevelopmentRebuildEvent::Cancelled {
                generation: batch.generation,
                document_urls: batch.document_urls,
            }];
        }

        let mut events = Vec::new();
        for document_url in batch.document_urls {
            let html = match load_document(&document_url) {
                Ok(html) => html,
                Err(message) => {
                    events.push(DevelopmentRebuildEvent::Diagnostic {
                        serving_last_good: self.precompiler.last_good.contains_key(&document_url),
                        document_url: document_url.clone(),
                        diagnostic: DevelopmentDiagnostic {
                            code: "document_load".to_owned(),
                            source_url: Some(document_url),
                            message,
                            compiler_diagnostics: Vec::new(),
                        },
                    });
                    continue;
                }
            };
            let report = self.precompiler.build_with_render_lane(
                &document_url,
                &html,
                render_runtime_js,
                &mut load_source,
                &mut render_compile,
            );
            append_build_events(&mut events, document_url, report);
        }
        events
    }
}

fn append_build_events(
    events: &mut Vec<DevelopmentRebuildEvent>,
    document_url: String,
    report: DevelopmentBuildReport,
) {
    let serving_last_good = !report.diagnostics.is_empty() && report.active.is_some();
    for diagnostic in report.diagnostics {
        events.push(DevelopmentRebuildEvent::Diagnostic {
            document_url: document_url.clone(),
            diagnostic,
            serving_last_good,
        });
    }
    if let Some(active) = report.active {
        let digest = active.digest().to_owned();
        events.push(DevelopmentRebuildEvent::Publication {
            document_url: document_url.clone(),
            digest: digest.clone(),
            changed: report.published,
        });
        if report.published {
            events.push(DevelopmentRebuildEvent::Reload {
                document_url,
                digest,
            });
        }
    }
}

impl DevelopmentBuildReport {
    pub fn succeeded(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn serving_last_good(&self) -> bool {
        !self.succeeded() && self.active.is_some()
    }
}

/// Stateful development wrapper around [`precompile_html`].
///
/// Failed attempts never mutate the last-good publication. Consumers can keep
/// serving the returned immutable snapshot while presenting attempt
/// diagnostics through a separate channel.
#[derive(Debug, Default)]
pub struct DevelopmentPrecompiler {
    graph: DevelopmentDependencyGraph,
    last_good: BTreeMap<String, DevelopmentPublication>,
}

impl DevelopmentPrecompiler {
    pub fn graph(&self) -> &DevelopmentDependencyGraph {
        &self.graph
    }

    pub fn publication(&self, document_url: &str) -> Option<DevelopmentPublication> {
        self.last_good.get(document_url).cloned()
    }

    /// Build using only the wasm-only facade lane (no render bundle lane).
    /// Directory `data-fe-src` sources fail via `load` exactly as before this
    /// crate grew a render lane.
    pub fn build(
        &mut self,
        document_url: &str,
        html: &str,
        load: impl FnMut(&Url) -> Result<String, String>,
    ) -> DevelopmentBuildReport {
        self.build_with_render_lane(document_url, html, "", load, |_, _| Ok(None))
    }

    /// Build with the render-bundle lane wired in. `render_compile` is asked
    /// about every external `data-fe-src`: `Ok(None)` falls through to the
    /// unchanged wasm-only lane; `Ok(Some(_))` publishes a render bundle and
    /// marks the script `data-fe-render`. `render_runtime_js` is the fixed
    /// render runtime module's source text (`fe_codegen::render_runtime_js()`),
    /// published once, content-addressed, the first time any script routes
    /// through the render lane.
    pub fn build_with_render_lane(
        &mut self,
        document_url: &str,
        html: &str,
        render_runtime_js: &str,
        load: impl FnMut(&Url) -> Result<String, String>,
        render_compile: impl FnMut(&Url, Option<&str>) -> Result<Option<RenderBundleArtifact>, String>,
    ) -> DevelopmentBuildReport {
        match discover_external_dependencies(document_url, html) {
            Ok(dependencies) => {
                self.graph
                    .replace(document_url.to_owned(), dependencies.into_iter().collect());
            }
            Err(error) => return self.failed(document_url, error),
        }

        match precompile_html_with_render_lane(
            document_url,
            html,
            render_runtime_js,
            load,
            render_compile,
        ) {
            Ok(output) => {
                let mut dependencies = self
                    .graph
                    .dependencies(document_url)
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                for dependency in output
                    .modules
                    .iter()
                    .filter_map(|module| module.source_dependencies.as_ref())
                    .flat_map(|inventory| &inventory.sources)
                {
                    if !dependency.url.starts_with("fe-inline:") {
                        dependencies.insert(dependency.url.clone());
                    }
                }
                for dependency in output
                    .render_dependencies
                    .iter()
                    .flat_map(|inventory| &inventory.sources)
                {
                    dependencies.insert(dependency.url.clone());
                }
                self.graph.replace(document_url.to_owned(), dependencies);
                let digest = publication_digest(&output);
                let previous = self.last_good.get(document_url);
                let published = previous.is_none_or(|publication| publication.digest != digest);
                let publication = if published {
                    DevelopmentPublication {
                        digest,
                        output: Arc::new(output),
                    }
                } else {
                    previous.expect("checked above").clone()
                };
                self.last_good
                    .insert(document_url.to_owned(), publication.clone());
                DevelopmentBuildReport {
                    active: Some(publication),
                    published,
                    diagnostics: Vec::new(),
                }
            }
            Err(error) => self.failed(document_url, error),
        }
    }

    fn failed(&self, document_url: &str, error: PrecompileError) -> DevelopmentBuildReport {
        DevelopmentBuildReport {
            active: self.last_good.get(document_url).cloned(),
            published: false,
            diagnostics: vec![development_diagnostic(error)],
        }
    }
}

impl std::fmt::Display for PrecompileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PrecompileError {}

/// Discover external Fe source dependencies using the same HTML5 and URL rules
/// as [`precompile_html`].
pub fn discover_external_dependencies(
    document_url: &str,
    html: &str,
) -> Result<Vec<String>, PrecompileError> {
    let document_url = Url::parse(document_url)
        .map_err(|error| PrecompileError::InvalidDocumentUrl(error.to_string()))?;
    let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
    let base_url = document_base_url(&dom.document, &document_url)?;
    let mut scripts = Vec::new();
    collect_fe_scripts(&dom.document, &mut scripts);
    let dependencies = scripts
        .into_iter()
        .filter_map(|script| attr(&script, "data-fe-src"))
        .map(|source| {
            base_url
                .join(&source)
                .map(|url| url.to_string())
                .map_err(|error| PrecompileError::SourceLoad {
                    url: source,
                    detail: error.to_string(),
                })
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(dependencies.into_iter().collect())
}

/// Read-only verification of one fully published Fe Web entry document.
pub fn verify_precompiled_site(index_path: &Path) -> Result<VerificationReport, VerificationError> {
    let index = index_path
        .canonicalize()
        .map_err(|error| VerificationError {
            context: format!("entry {}", index_path.display()),
            detail: format!("cannot resolve deployment entry: {error}"),
        })?;
    let root = index.parent().ok_or_else(|| VerificationError {
        context: format!("entry {}", index.display()),
        detail: "deployment entry has no parent directory".to_owned(),
    })?;
    let html = std::fs::read_to_string(&index).map_err(|error| VerificationError {
        context: format!("entry {}", index.display()),
        detail: format!("cannot read HTML: {error}"),
    })?;
    let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
    let mut source_scripts = Vec::new();
    collect_scripts_of_type(&dom.document, SOURCE_SCRIPT_TYPE, &mut source_scripts);
    if !source_scripts.is_empty() {
        return Err(VerificationError {
            context: "script[type=\"application/fe\"]".to_owned(),
            detail: "production deployment retains an Fe source block".to_owned(),
        });
    }
    let mut scripts = Vec::new();
    collect_scripts_of_type(&dom.document, ARTIFACT_SCRIPT_TYPE, &mut scripts);
    if scripts.is_empty() {
        return Err(VerificationError {
            context: format!("entry {}", index.display()),
            detail: "contains no application/fe+wasm modules".to_owned(),
        });
    }

    let mut verified = BTreeSet::new();
    for (position, script) in scripts.iter().enumerate() {
        let context = format!("application/fe+wasm script #{}", position + 1);
        let wasm_ref = required_attr(script, "data-fe-src", &context)?;
        let manifest_ref = required_attr(script, "data-fe-manifest", &context)?;
        let wasm = deployment_file(root, &wasm_ref, &format!("{context} data-fe-src"))?;
        let manifest =
            deployment_file(root, &manifest_ref, &format!("{context} data-fe-manifest"))?;
        if is_addressed_path(&manifest) {
            verify_addressed_file(&manifest, &format!("{context} manifest"))?;
        }
        let manifest_bytes = std::fs::read(&manifest).map_err(|error| VerificationError {
            context: format!("{context} manifest {}", manifest.display()),
            detail: format!("cannot read file: {error}"),
        })?;
        let value: serde_json::Value =
            serde_json::from_slice(&manifest_bytes).map_err(|error| VerificationError {
                context: format!("{context} manifest {}", manifest.display()),
                detail: format!("invalid JSON: {error}"),
            })?;
        let artifacts = value["artifacts"]
            .as_array()
            .ok_or_else(|| VerificationError {
                context: format!("{context} manifest {}", manifest.display()),
                detail: "missing artifacts array".to_owned(),
            })?;
        let artifact = artifacts
            .iter()
            .find(|artifact| {
                artifact["kind"]
                    .as_str()
                    .is_some_and(|kind| kind == "wasm_module")
            })
            .ok_or_else(|| VerificationError {
                context: format!("{context} manifest {}", manifest.display()),
                detail: "contains no wasm_module artifact".to_owned(),
            })?;
        if let Some(url) = artifact["url"].as_str() {
            let declared = deployment_file(root, url, &format!("{context} manifest artifact.url"))?;
            if declared != wasm {
                return Err(VerificationError {
                    context: format!("{context} manifest artifact.url"),
                    detail: format!(
                        "resolves to {}, but data-fe-src resolves to {}",
                        declared.display(),
                        wasm.display()
                    ),
                });
            }
        }
        verify_declared_file(&wasm, artifact, &format!("{context} Wasm"))?;
        if let Some(integrity) = attr(script, "data-fe-integrity") {
            let digest = artifact["sha256"].as_str().unwrap_or_default();
            let expected = format!(
                "sha256-{}",
                base64::engine::general_purpose::STANDARD.encode(hex_to_bytes(digest))
            );
            if integrity != expected {
                return Err(VerificationError {
                    context: format!("{context} data-fe-integrity"),
                    detail: format!("expected {expected:?}, found {integrity:?}"),
                });
            }
        }
        verified.insert(wasm);
        verified.insert(manifest);
        for attribute in ["data-fe-adapter", "data-fe-adapter-selection"] {
            if let Some(reference) = attr(script, attribute) {
                let path = deployment_file(root, &reference, &format!("{context} {attribute}"))?;
                verify_addressed_file(&path, &format!("{context} {attribute}"))?;
                verified.insert(path);
            }
        }
    }

    if let Some(bootstrap) = find_attr_element(&dom.document, BOOTSTRAP_MARKER)
        && let Some(reference) = attr(&bootstrap, "src")
    {
        let path = deployment_file(root, &reference, "script[data-fe-bootstrap] src")?;
        if is_addressed_path(&path) {
            verify_addressed_file(&path, "script[data-fe-bootstrap] src")?;
        } else {
            let bytes = std::fs::read(&path).map_err(|error| VerificationError {
                context: "script[data-fe-bootstrap] src".to_owned(),
                detail: format!("cannot read {}: {error}", path.display()),
            })?;
            if bytes != BOOTSTRAP_SOURCE.as_bytes() {
                return Err(VerificationError {
                    context: "script[data-fe-bootstrap] src".to_owned(),
                    detail: format!(
                        "{} is not content-addressed and does not exactly match the trusted Fe bootstrap ({} bytes, sha256 {})",
                        path.display(),
                        BOOTSTRAP_SOURCE.len(),
                        sha256_hex(BOOTSTRAP_SOURCE.as_bytes())
                    ),
                });
            }
        }
        verified.insert(path);
    }
    reject_compiler_wasm(root, root)?;
    Ok(VerificationReport {
        modules: scripts.len(),
        files: verified.len(),
    })
}

fn required_attr(node: &Handle, name: &str, context: &str) -> Result<String, VerificationError> {
    attr(node, name).ok_or_else(|| VerificationError {
        context: format!("{context} {name}"),
        detail: "required attribute is missing".to_owned(),
    })
}

fn deployment_file(
    root: &Path,
    reference: &str,
    context: &str,
) -> Result<PathBuf, VerificationError> {
    if reference.is_empty()
        || reference.starts_with('/')
        || reference.contains(['?', '#', '\\', '\0'])
        || Url::parse(reference).is_ok()
    {
        return Err(VerificationError {
            context: context.to_owned(),
            detail: format!("URL {reference:?} is not a contained deployment-relative path"),
        });
    }
    let relative = Path::new(reference);
    if !relative
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(VerificationError {
            context: context.to_owned(),
            detail: format!("URL {reference:?} escapes the deployment root"),
        });
    }
    let joined = root.join(relative);
    let canonical = joined.canonicalize().map_err(|error| VerificationError {
        context: context.to_owned(),
        detail: format!(
            "referenced file {} is missing or unreadable: {error}",
            joined.display()
        ),
    })?;
    if !canonical.starts_with(root) || !canonical.is_file() {
        return Err(VerificationError {
            context: context.to_owned(),
            detail: format!(
                "referenced path {} is outside the deployment root or not a file",
                canonical.display()
            ),
        });
    }
    Ok(canonical)
}

fn verify_declared_file(
    path: &Path,
    artifact: &serde_json::Value,
    context: &str,
) -> Result<(), VerificationError> {
    let bytes = std::fs::read(path).map_err(|error| VerificationError {
        context: context.to_owned(),
        detail: format!("cannot read {}: {error}", path.display()),
    })?;
    let expected_len = artifact["byte_len"]
        .as_u64()
        .ok_or_else(|| VerificationError {
            context: context.to_owned(),
            detail: "manifest artifact has no byte_len".to_owned(),
        })?;
    let expected_digest = artifact["sha256"]
        .as_str()
        .ok_or_else(|| VerificationError {
            context: context.to_owned(),
            detail: "manifest artifact has no sha256".to_owned(),
        })?;
    let actual_digest = sha256_hex(&bytes);
    if bytes.len() as u64 != expected_len || actual_digest != expected_digest {
        return Err(VerificationError {
            context: context.to_owned(),
            detail: format!(
                "{} failed manifest verification: expected {} bytes sha256 {}, found {} bytes sha256 {}",
                path.display(),
                expected_len,
                expected_digest,
                bytes.len(),
                actual_digest
            ),
        });
    }
    Ok(())
}

fn verify_addressed_file(path: &Path, context: &str) -> Result<(), VerificationError> {
    let bytes = std::fs::read(path).map_err(|error| VerificationError {
        context: context.to_owned(),
        detail: format!("cannot read {}: {error}", path.display()),
    })?;
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let prefix = stem.rsplit('-').next().unwrap_or("");
    if prefix.len() != 16 || !prefix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(VerificationError {
            context: context.to_owned(),
            detail: format!(
                "{} is not content-addressed with a 16-hex SHA-256 prefix",
                path.display()
            ),
        });
    }
    let digest = sha256_hex(&bytes);
    if !digest.starts_with(prefix) {
        return Err(VerificationError {
            context: context.to_owned(),
            detail: format!(
                "{} content digest {} does not match filename prefix {}",
                path.display(),
                digest,
                prefix
            ),
        });
    }
    Ok(())
}

fn is_addressed_path(path: &Path) -> bool {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let prefix = stem.rsplit('-').next().unwrap_or("");
    prefix.len() == 16 && prefix.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn reject_compiler_wasm(root: &Path, directory: &Path) -> Result<(), VerificationError> {
    for entry in std::fs::read_dir(directory).map_err(|error| VerificationError {
        context: format!("deployment {}", root.display()),
        detail: format!("cannot inspect directory {}: {error}", directory.display()),
    })? {
        let entry = entry.map_err(|error| VerificationError {
            context: format!("deployment {}", root.display()),
            detail: format!("cannot inspect directory entry: {error}"),
        })?;
        let path = entry.path();
        if path.is_dir() {
            reject_compiler_wasm(root, &path)?;
        } else {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".wasm") && name.contains("compiler") {
                return Err(VerificationError {
                    context: format!("deployment file {}", path.display()),
                    detail: "unexpected compiler Wasm remains in production output".to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn publication_digest(output: &PrecompileOutput) -> String {
    let mut bytes = Vec::new();
    append_digest_part(&mut bytes, b"index.html", output.html.as_bytes());
    for (path, asset) in &output.assets {
        append_digest_part(&mut bytes, path.as_bytes(), asset);
    }
    sha256_hex(&bytes)
}

fn append_digest_part(output: &mut Vec<u8>, name: &[u8], bytes: &[u8]) {
    output.extend_from_slice(&(name.len() as u64).to_le_bytes());
    output.extend_from_slice(name);
    output.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    output.extend_from_slice(bytes);
}

fn development_diagnostic(error: PrecompileError) -> DevelopmentDiagnostic {
    match error {
        PrecompileError::Diagnostics {
            source_url,
            diagnostics,
        } => DevelopmentDiagnostic {
            code: "compiler_diagnostics".to_owned(),
            source_url: Some(source_url),
            message: "Fe compilation produced diagnostics; last-good output was retained"
                .to_owned(),
            compiler_diagnostics: diagnostics,
        },
        PrecompileError::SourceLoad { url, detail } => DevelopmentDiagnostic {
            code: "source_load".to_owned(),
            source_url: Some(url),
            message: detail,
            compiler_diagnostics: Vec::new(),
        },
        PrecompileError::Compile { source_url, detail } => DevelopmentDiagnostic {
            code: "compile".to_owned(),
            source_url: Some(source_url),
            message: detail,
            compiler_diagnostics: Vec::new(),
        },
        PrecompileError::AdapterSelection { source_url, detail } => DevelopmentDiagnostic {
            code: "adapter_selection".to_owned(),
            source_url: Some(source_url),
            message: detail,
            compiler_diagnostics: Vec::new(),
        },
        other => DevelopmentDiagnostic {
            code: match &other {
                PrecompileError::InvalidDocumentUrl(_) => "invalid_document_url",
                PrecompileError::InvalidBaseUrl { .. } => "invalid_base_url",
                PrecompileError::MissingWasm(_) => "missing_wasm",
                PrecompileError::Serialize(_) => "serialize",
                PrecompileError::SourceLoad { .. }
                | PrecompileError::Compile { .. }
                | PrecompileError::Diagnostics { .. }
                | PrecompileError::AdapterSelection { .. } => unreachable!(),
            }
            .to_owned(),
            source_url: None,
            message: other.to_string(),
            compiler_diagnostics: Vec::new(),
        },
    }
}

/// Precompile all inert Fe script elements in HTML tree order.
///
/// `load` resolves external source bytes. The caller decides whether those
/// URLs map to a filesystem, cache, network, or virtual build graph.
pub fn precompile_html(
    document_url: &str,
    html: &str,
    load: impl FnMut(&Url) -> Result<String, String>,
) -> Result<PrecompileOutput, PrecompileError> {
    precompile_html_impl(document_url, html, None, None, "", load, |_, _| Ok(None))
}

/// Precompile and publish a versioned, minimal adapter selection inventory for
/// every module. Generated adapter metadata is a tooling input; the compiler
/// remains unaware of Web IDL.
pub fn precompile_html_with_adapter_metadata(
    document_url: &str,
    html: &str,
    adapter_metadata: &[AdapterOperationMetadata],
    load: impl FnMut(&Url) -> Result<String, String>,
) -> Result<PrecompileOutput, PrecompileError> {
    precompile_html_impl(
        document_url,
        html,
        Some(adapter_metadata),
        None,
        "",
        load,
        |_, _| Ok(None),
    )
}

/// Precompile and publish both the minimal selection inventory and an
/// executable semantic adapter slice for one generated provider.
pub fn precompile_html_with_adapter_plan(
    document_url: &str,
    html: &str,
    world: &WebIdlWorld,
    plan: &AdapterPlan,
    provider: &str,
    load: impl FnMut(&Url) -> Result<String, String>,
) -> Result<PrecompileOutput, PrecompileError> {
    let metadata = adapter_operation_metadata(plan, provider);
    precompile_html_impl(
        document_url,
        html,
        Some(&metadata),
        Some((world, plan, provider)),
        "",
        load,
        |_, _| Ok(None),
    )
}

/// Precompile Fe script elements, additionally routing any `data-fe-src` that
/// names an ingot directory (rather than a single `.fe` file) through a
/// render-bundle lane instead of the wasm-only facade lane.
///
/// `render_compile` is asked about every EXTERNAL `data-fe-src`, before
/// `load` is ever called: `Ok(None)` means "not a render source," falling
/// through to the unchanged wasm lane; `Ok(Some(_))` publishes the render
/// bundle and marks the rewritten script `data-fe-render`; `Err` fails the
/// whole document build (the same last-good-serving posture as a compile
/// error). This is what fixes directory `data-fe-src` crashing `load` with a
/// raw filesystem error: a directory source is routed to `render_compile`
/// before `load` ever sees it.
///
/// `render_runtime_js` is the fixed render runtime module's source text
/// (`fe_codegen::render_runtime_js()`); it is published once, content
/// addressed, the first time any script routes through the render lane. Same
/// posture as `load`: this crate assumes nothing about a filesystem or a
/// particular compiler; the native `fe web dev`/`fe web precompile` host
/// supplies both. A caller with no render sources (or the future
/// browser-Worker path, which fails closed on directory sources) can pass
/// `render_runtime_js = ""` and `render_compile = |_, _| Ok(None)`, exactly
/// [`precompile_html`]'s behavior.
pub fn precompile_html_with_render_lane(
    document_url: &str,
    html: &str,
    render_runtime_js: &str,
    load: impl FnMut(&Url) -> Result<String, String>,
    render_compile: impl FnMut(&Url, Option<&str>) -> Result<Option<RenderBundleArtifact>, String>,
) -> Result<PrecompileOutput, PrecompileError> {
    precompile_html_impl(
        document_url,
        html,
        None,
        None,
        render_runtime_js,
        load,
        render_compile,
    )
}

#[allow(clippy::too_many_arguments)]
fn precompile_html_impl(
    document_url: &str,
    html: &str,
    adapter_metadata: Option<&[AdapterOperationMetadata]>,
    adapter_plan: Option<(&WebIdlWorld, &AdapterPlan, &str)>,
    render_runtime_js: &str,
    mut load: impl FnMut(&Url) -> Result<String, String>,
    mut render_compile: impl FnMut(&Url, Option<&str>) -> Result<Option<RenderBundleArtifact>, String>,
) -> Result<PrecompileOutput, PrecompileError> {
    let document_url = Url::parse(document_url)
        .map_err(|error| PrecompileError::InvalidDocumentUrl(error.to_string()))?;
    let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
    let base_url = document_base_url(&dom.document, &document_url)?;
    let mut scripts = Vec::new();
    collect_fe_scripts(&dom.document, &mut scripts);

    let mut assets = BTreeMap::new();
    let mut modules = Vec::new();
    let mut render_dependencies = Vec::new();
    let mut render_runtime_asset: Option<String> = None;
    for (index, script) in scripts.into_iter().enumerate() {
        let src = attr(&script, "data-fe-src");
        let entry_attr = attr(&script, "data-fe-entry");

        if let Some(src) = src.as_deref() {
            let url = base_url
                .join(src)
                .map_err(|error| PrecompileError::SourceLoad {
                    url: src.to_owned(),
                    detail: error.to_string(),
                })?;
            if let Some(bundle) =
                render_compile(&url, entry_attr.as_deref()).map_err(|detail| {
                    PrecompileError::Compile {
                        source_url: url.to_string(),
                        detail,
                    }
                })?
            {
                if let Some(dependencies) = &bundle.source_dependencies {
                    render_dependencies.push(dependencies.clone());
                }
                let runtime_path = publish_render_runtime(
                    render_runtime_js,
                    &mut assets,
                    &mut render_runtime_asset,
                )?;
                publish_render_bundle(
                    &script,
                    &base_url,
                    &document_url,
                    bundle,
                    &runtime_path,
                    &mut assets,
                )?;
                continue;
            }
        }

        let (source_url, source) = if let Some(src) = src {
            let url = base_url
                .join(&src)
                .map_err(|error| PrecompileError::SourceLoad {
                    url: src,
                    detail: error.to_string(),
                })?;
            let source = load(&url).map_err(|detail| PrecompileError::SourceLoad {
                url: url.to_string(),
                detail,
            })?;
            (url.to_string(), source)
        } else {
            let document_hash = sha256_hex(document_url.as_str().as_bytes());
            (
                format!("fe-inline:///{}-{index}.fe", &document_hash[..16]),
                text_content(&script),
            )
        };
        let entry = entry_attr.unwrap_or_else(|| "main".to_owned());
        let request = CompileRequest {
            protocol: ProtocolVersion::CURRENT,
            root: source_url.clone(),
            sources: vec![VirtualSource::new(&source_url, source)],
            target: CompileTarget::Wasm,
            entries: vec![entry.clone()],
            options: CompileOptions::default(),
        };
        let result =
            fe_compiler_facade::compile(&request).map_err(|error| PrecompileError::Compile {
                source_url: source_url.clone(),
                detail: error.to_string(),
            })?;
        if result.artifacts.is_empty() {
            return Err(PrecompileError::Diagnostics {
                source_url,
                diagnostics: result.diagnostics,
            });
        }
        let wasm = result
            .artifacts
            .iter()
            .find(|artifact| artifact.kind == ArtifactKind::WasmModule)
            .ok_or_else(|| PrecompileError::MissingWasm(source_url.clone()))?;
        let wasm_path = format!("assets/fe-{}.wasm", &wasm.sha256[..16]);
        insert_identical(&mut assets, wasm_path.clone(), wasm.bytes.clone())?;

        let published = PublishedArtifact::from_artifact(wasm, &wasm_path);
        let manifest = PublishedModuleManifest {
            protocol: result.protocol,
            compiler: result.compiler,
            target: result.target,
            source_set_sha256: result.source_set_sha256,
            source_dependencies: result.source_dependencies,
            entry,
            interface: result.interface,
            artifacts: vec![published],
        };
        manifest
            .validate()
            .map_err(|error| PrecompileError::Compile {
                source_url: source_url.clone(),
                detail: error.to_string(),
            })?;
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| PrecompileError::Serialize(error.to_string()))?;
        let manifest_hash = sha256_hex(&manifest_bytes);
        let manifest_path = format!("assets/fe-{}.json", &manifest_hash[..16]);
        insert_identical(&mut assets, manifest_path.clone(), manifest_bytes)?;

        let (selection_path, adapter_path) = if let Some(metadata) = adapter_metadata {
            let selection =
                select_adapter_operations(&manifest.interface, metadata).map_err(|error| {
                    PrecompileError::AdapterSelection {
                        source_url: source_url.clone(),
                        detail: error.to_string(),
                    }
                })?;
            let bytes = serde_json::to_vec_pretty(&selection)
                .map_err(|error| PrecompileError::Serialize(error.to_string()))?;
            let hash = sha256_hex(&bytes);
            let path = format!("assets/fe-adapter-selection-{}.json", &hash[..16]);
            insert_identical(&mut assets, path.clone(), bytes)?;
            let adapter_path = if let Some((world, plan, provider)) = adapter_plan {
                let source = emit_js_selected_adapter(world, plan, provider, &selection).map_err(
                    |error| PrecompileError::AdapterSelection {
                        source_url: source_url.clone(),
                        detail: error.to_string(),
                    },
                )?;
                let hash = sha256_hex(source.as_bytes());
                let adapter_path = format!("assets/fe-adapter-{}.js", &hash[..16]);
                insert_identical(&mut assets, adapter_path.clone(), source.into_bytes())?;
                Some(published_reference(&base_url, &document_url, &adapter_path))
            } else {
                None
            };
            (
                Some(published_reference(&base_url, &document_url, &path)),
                adapter_path,
            )
        } else {
            (None, None)
        };
        rewrite_script(
            &script,
            &published_reference(&base_url, &document_url, &wasm_path),
            &published_reference(&base_url, &document_url, &manifest_path),
            selection_path.as_deref(),
            adapter_path.as_deref(),
            &wasm.sha256,
        );
        modules.push(manifest);
    }
    publish_bootstrap(&dom.document, &document_url, &base_url, &mut assets)?;

    let mut serialized = Vec::new();
    serialize(
        &mut serialized,
        &SerializableHandle::from(dom.document),
        SerializeOpts::default(),
    )
    .map_err(|error| PrecompileError::Serialize(error.to_string()))?;
    let html = String::from_utf8(serialized)
        .map_err(|error| PrecompileError::Serialize(error.to_string()))?;
    Ok(PrecompileOutput {
        html,
        assets,
        modules,
        render_dependencies,
    })
}

/// Publish the fixed render runtime module once, content-addressed. Returns
/// its (document-relative, un-retargeted) publication path, from cache after
/// the first call.
fn publish_render_runtime(
    render_runtime_js: &str,
    assets: &mut BTreeMap<String, Vec<u8>>,
    published: &mut Option<String>,
) -> Result<String, PrecompileError> {
    if let Some(path) = published {
        return Ok(path.clone());
    }
    if render_runtime_js.is_empty() {
        return Err(PrecompileError::Serialize(
            "a render script requires a render runtime module, but the host supplied none \
             (render_runtime_js was empty)"
                .to_owned(),
        ));
    }
    let digest = sha256_hex(render_runtime_js.as_bytes());
    let path = format!("assets/fe-render-runtime-{}.js", &digest[..16]);
    insert_identical(assets, path.clone(), render_runtime_js.as_bytes().to_vec())?;
    *published = Some(path.clone());
    Ok(path)
}

/// Publish one render bundle's wasm/wgsl/manifest content-addressed, rewrite
/// its manifest's `artifacts.wasm`/`artifacts.wgsl` to the published names
/// (the same precedent as the wasm lane's `PublishedArtifact::from_artifact`
/// rewriting paths), and rewrite the script tag to `data-fe-render`.
fn publish_render_bundle(
    script: &Handle,
    base_url: &Url,
    document_url: &Url,
    bundle: RenderBundleArtifact,
    render_runtime_path: &str,
    assets: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), PrecompileError> {
    let wasm_sha256 = sha256_hex(&bundle.wasm);
    let wasm_path = format!("assets/fe-render-{}.wasm", &wasm_sha256[..16]);
    insert_identical(assets, wasm_path.clone(), bundle.wasm)?;

    let wgsl_sha256 = sha256_hex(&bundle.wgsl);
    let wgsl_path = format!("assets/fe-render-{}.wgsl", &wgsl_sha256[..16]);
    insert_identical(assets, wgsl_path.clone(), bundle.wgsl)?;

    let mut manifest: serde_json::Value =
        serde_json::from_slice(&bundle.manifest_json).map_err(|error| {
            PrecompileError::Serialize(format!(
                "render bundle manifest is not valid JSON: {error}"
            ))
        })?;
    let artifacts = manifest.get_mut("artifacts").ok_or_else(|| {
        PrecompileError::Serialize("render bundle manifest has no `artifacts`".to_owned())
    })?;
    artifacts["wasm"] = serde_json::Value::String(basename(&wasm_path).to_owned());
    artifacts["wgsl"] = serde_json::Value::String(basename(&wgsl_path).to_owned());
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| PrecompileError::Serialize(error.to_string()))?;
    let manifest_sha256 = sha256_hex(&manifest_bytes);
    let manifest_path = format!("assets/fe-render-{}.json", &manifest_sha256[..16]);
    insert_identical(assets, manifest_path.clone(), manifest_bytes)?;

    rewrite_render_script(
        script,
        &published_reference(base_url, document_url, &wasm_path),
        &published_reference(base_url, document_url, &manifest_path),
        &published_reference(base_url, document_url, render_runtime_path),
        &wasm_sha256,
    );
    Ok(())
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn published_reference(base_url: &Url, document_url: &Url, path: &str) -> String {
    let target = document_url
        .join(path)
        .expect("content-addressed publication path is a relative URL");
    base_url
        .make_relative(&target)
        .unwrap_or_else(|| target.to_string())
}

fn publish_bootstrap(
    root: &Handle,
    document_url: &Url,
    base_url: &Url,
    assets: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), PrecompileError> {
    let mut artifacts = Vec::new();
    collect_scripts_of_type(root, ARTIFACT_SCRIPT_TYPE, &mut artifacts);
    if artifacts.is_empty() || find_attr_element(root, BOOTSTRAP_MARKER).is_some() {
        return Ok(());
    }

    let override_value = find_meta_content(root, BOOTSTRAP_META_NAME);
    if override_value
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("none"))
    {
        return Ok(());
    }
    let source = if let Some(value) = override_value {
        value
    } else {
        let digest = sha256_hex(BOOTSTRAP_SOURCE.as_bytes());
        let path = format!("assets/fe-bootstrap-{}.js", &digest[..16]);
        insert_identical(assets, path.clone(), BOOTSTRAP_SOURCE.as_bytes().to_vec())?;
        published_reference(base_url, document_url, &path)
    };
    inject_bootstrap(root, &source)
}

fn inject_bootstrap(root: &Handle, source: &str) -> Result<(), PrecompileError> {
    let parent = find_first_element(root, "body")
        .or_else(|| find_first_element(root, "head"))
        .ok_or_else(|| {
            PrecompileError::Serialize("HTML document has no body or head".to_owned())
        })?;
    let script = Node::new(NodeData::Element {
        name: QualName::new(None, ns!(html), LocalName::from("script")),
        attrs: RefCell::new(vec![
            Attribute {
                name: QualName::new(None, ns!(), LocalName::from("type")),
                value: "module".into(),
            },
            Attribute {
                name: QualName::new(None, ns!(), LocalName::from("src")),
                value: source.into(),
            },
            Attribute {
                name: QualName::new(None, ns!(), LocalName::from(BOOTSTRAP_MARKER)),
                value: String::new().into(),
            },
        ]),
        template_contents: RefCell::new(None),
        mathml_annotation_xml_integration_point: false,
    });
    script.parent.set(Some(Rc::downgrade(&parent)));
    parent.children.borrow_mut().push(script);
    Ok(())
}

fn document_base_url(root: &Handle, document_url: &Url) -> Result<Url, PrecompileError> {
    if let Some(base) = find_first_element(root, "base")
        && let Some(href) = attr(&base, "href")
    {
        return document_url
            .join(&href)
            .map_err(|error| PrecompileError::InvalidBaseUrl {
                value: href,
                detail: error.to_string(),
            });
    }
    Ok(document_url.clone())
}

fn collect_fe_scripts(root: &Handle, output: &mut Vec<Handle>) {
    collect_scripts_of_type(root, SOURCE_SCRIPT_TYPE, output);
}

fn collect_scripts_of_type(root: &Handle, script_type: &str, output: &mut Vec<Handle>) {
    if is_element(root, "script")
        && attr(root, "type").is_some_and(|value| value.trim().eq_ignore_ascii_case(script_type))
    {
        output.push(root.clone());
    }
    for child in root.children.borrow().iter() {
        collect_scripts_of_type(child, script_type, output);
    }
}

fn find_attr_element(root: &Handle, name: &str) -> Option<Handle> {
    if attr(root, name).is_some() {
        return Some(root.clone());
    }
    root.children
        .borrow()
        .iter()
        .find_map(|child| find_attr_element(child, name))
}

fn find_meta_content(root: &Handle, expected_name: &str) -> Option<String> {
    if is_element(root, "meta")
        && attr(root, "name").is_some_and(|name| name.eq_ignore_ascii_case(expected_name))
    {
        return attr(root, "content");
    }
    root.children
        .borrow()
        .iter()
        .find_map(|child| find_meta_content(child, expected_name))
}

fn find_first_element(root: &Handle, name: &str) -> Option<Handle> {
    if is_element(root, name) {
        return Some(root.clone());
    }
    root.children
        .borrow()
        .iter()
        .find_map(|child| find_first_element(child, name))
}

fn is_element(node: &Handle, expected: &str) -> bool {
    matches!(
        &node.data,
        NodeData::Element { name, .. } if name.local.as_ref() == expected
    )
}

fn attrs(node: &Handle) -> Option<&RefCell<Vec<Attribute>>> {
    match &node.data {
        NodeData::Element { attrs, .. } => Some(attrs),
        _ => None,
    }
}

fn attr(node: &Handle, expected: &str) -> Option<String> {
    attrs(node)?.borrow().iter().find_map(|attribute| {
        (attribute.name.ns == ns!() && attribute.name.local.as_ref() == expected)
            .then(|| attribute.value.to_string())
    })
}

fn set_attr(node: &Handle, name: &str, value: &str) {
    let attributes = attrs(node).expect("script is an element");
    let mut attributes = attributes.borrow_mut();
    if let Some(attribute) = attributes
        .iter_mut()
        .find(|attribute| attribute.name.ns == ns!() && attribute.name.local.as_ref() == name)
    {
        attribute.value = value.into();
    } else {
        attributes.push(Attribute {
            name: QualName::new(None, ns!(), LocalName::from(name)),
            value: value.into(),
        });
    }
}

fn remove_attr(node: &Handle, name: &str) {
    let attributes = attrs(node).expect("script is an element");
    attributes
        .borrow_mut()
        .retain(|attribute| attribute.name.ns != ns!() || attribute.name.local.as_ref() != name);
}

fn text_content(node: &Handle) -> String {
    let mut result = String::new();
    append_text(node, &mut result);
    result
}

fn append_text(node: &Handle, result: &mut String) {
    if let NodeData::Text { contents } = &node.data {
        result.push_str(&contents.borrow());
    }
    for child in node.children.borrow().iter() {
        append_text(child, result);
    }
}

fn rewrite_script(
    node: &Handle,
    wasm_path: &str,
    manifest_path: &str,
    selection_path: Option<&str>,
    adapter_path: Option<&str>,
    sha256: &str,
) {
    set_attr(node, "type", ARTIFACT_SCRIPT_TYPE);
    remove_attr(node, "src");
    remove_attr(node, "integrity");
    set_attr(node, "data-fe-src", wasm_path);
    set_attr(node, "data-fe-manifest", manifest_path);
    if let Some(selection_path) = selection_path {
        set_attr(node, "data-fe-adapter-selection", selection_path);
    } else {
        remove_attr(node, "data-fe-adapter-selection");
    }
    if let Some(adapter_path) = adapter_path {
        set_attr(node, "data-fe-adapter", adapter_path);
    } else {
        remove_attr(node, "data-fe-adapter");
    }
    let digest = hex_to_bytes(sha256);
    set_attr(
        node,
        "data-fe-integrity",
        &format!(
            "sha256-{}",
            base64::engine::general_purpose::STANDARD.encode(digest)
        ),
    );
    node.children.borrow_mut().clear();
}

/// Rewrite a render-lane script: no adapter machinery (WebGPU bundles have no
/// Web IDL interface), and a `data-fe-render` marker plus
/// `data-fe-render-runtime` pointing the bootstrap at the published render
/// runtime module, so it hands the element to `mountRenderSurface` instead
/// of instantiating the module and calling its entry with zero arguments.
fn rewrite_render_script(
    node: &Handle,
    wasm_path: &str,
    manifest_path: &str,
    render_runtime_path: &str,
    sha256: &str,
) {
    set_attr(node, "type", ARTIFACT_SCRIPT_TYPE);
    remove_attr(node, "src");
    remove_attr(node, "integrity");
    set_attr(node, "data-fe-src", wasm_path);
    set_attr(node, "data-fe-manifest", manifest_path);
    set_attr(node, RENDER_SCRIPT_MARKER, "");
    set_attr(node, RENDER_RUNTIME_ATTR, render_runtime_path);
    remove_attr(node, "data-fe-adapter-selection");
    remove_attr(node, "data-fe-adapter");
    let digest = hex_to_bytes(sha256);
    set_attr(
        node,
        "data-fe-integrity",
        &format!(
            "sha256-{}",
            base64::engine::general_purpose::STANDARD.encode(digest)
        ),
    );
    node.children.borrow_mut().clear();
}

fn hex_to_bytes(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("SHA-256 hex is ASCII");
            u8::from_str_radix(text, 16).expect("SHA-256 digest is hex")
        })
        .collect()
}

fn insert_identical(
    assets: &mut BTreeMap<String, Vec<u8>>,
    path: String,
    bytes: Vec<u8>,
) -> Result<(), PrecompileError> {
    if let Some(existing) = assets.get(&path) {
        if existing != &bytes {
            return Err(PrecompileError::Serialize(format!(
                "content-address collision at `{path}`"
            )));
        }
    } else {
        assets.insert(path, bytes);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_publication(root: &Path, output: &PrecompileOutput) {
        std::fs::write(root.join("index.html"), &output.html).unwrap();
        for (relative, bytes) in &output.assets {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    }

    fn verified_fixture(root: &Path) -> PrecompileOutput {
        let output = precompile_html(
            "file:///site/index.html",
            r#"<!doctype html><script type="application/fe">pub fn main() {}</script>"#,
            |_| unreachable!(),
        )
        .unwrap();
        write_publication(root, &output);
        output
    }

    #[test]
    fn rewrites_inline_and_external_scripts_with_standard_url_resolution() {
        let html = r#"<!doctype html>
<html><head><base href="/sources/"><title>Fe</title></head><body>
<script type="application/fe" data-fe-entry="main">
pub fn main() -> u32 { 42 }
</script>
<script type="application/fe" data-fe-src="other.fe"></script>
<script type="application/json">{"preserved": true}</script>
</body></html>"#;
        let mut loaded = Vec::new();
        let output = precompile_html("https://example.test/app/index.html", html, |url| {
            loaded.push(url.to_string());
            Ok("pub fn main() -> u32 { 7 }".to_owned())
        })
        .unwrap();
        assert_eq!(loaded, ["https://example.test/sources/other.fe"]);
        assert_eq!(output.modules.len(), 2);
        assert_eq!(
            output.html.matches(r#"type="application/fe+wasm""#).count(),
            2
        );
        assert!(output.html.contains(r#"type="application/json""#));
        assert!(
            output
                .html
                .contains(r#"data-fe-manifest="../app/assets/fe-"#)
        );
        assert!(output.html.contains(r#"data-fe-integrity="sha256-"#));
        assert!(!output.html.contains(r#" integrity="#));
        assert_eq!(output.html.matches(BOOTSTRAP_MARKER).count(), 1);
        assert_eq!(
            output
                .assets
                .keys()
                .filter(|path| path.contains("fe-bootstrap-"))
                .count(),
            1
        );
        assert!(!output.html.contains("pub fn main"));
        assert_eq!(
            output
                .assets
                .keys()
                .filter(|path| path.ends_with(".wasm"))
                .count(),
            2
        );
    }

    #[test]
    fn deterministic_rebuild_is_byte_identical() {
        let html = r#"<script type="application/fe">pub fn main() -> u32 { 42 }</script>"#;
        let first = precompile_html("https://example.test/index.html", html, |_| {
            panic!("inline source must not load")
        })
        .unwrap();
        let second = precompile_html("https://example.test/index.html", html, |_| {
            panic!("inline source must not load")
        })
        .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn bootstrap_is_injected_once_and_respects_override_and_opt_out() {
        let source = r#"<script type="application/fe">pub fn main() {}</script>"#;
        let output = precompile_html("https://example.test/index.html", source, |_| {
            panic!("inline source")
        })
        .unwrap();
        assert_eq!(output.html.matches(BOOTSTRAP_MARKER).count(), 1);

        let overridden = precompile_html(
            "https://example.test/index.html",
            r#"<meta name="fe-bootstrap" content="/dev/custom-loader.js">
               <script type="application/fe">pub fn main() {}</script>"#,
            |_| panic!("inline source"),
        )
        .unwrap();
        assert!(overridden.html.contains(r#"src="/dev/custom-loader.js""#));
        assert!(
            !overridden
                .assets
                .keys()
                .any(|path| path.contains("fe-bootstrap-"))
        );

        let opted_out = precompile_html(
            "https://example.test/index.html",
            r#"<meta name="fe-bootstrap" content="none">
               <script type="application/fe">pub fn main() {}</script>"#,
            |_| panic!("inline source"),
        )
        .unwrap();
        assert!(!opted_out.html.contains(BOOTSTRAP_MARKER));
    }

    #[test]
    fn bootstrap_publication_is_idempotent_on_one_parsed_document() {
        let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(
            r#"<script type="application/fe+wasm"
                       data-fe-src="assets/app.wasm"
                       data-fe-manifest="assets/app.json"></script>"#,
        );
        let document_url = Url::parse("https://example.test/index.html").unwrap();
        let mut assets = BTreeMap::new();
        publish_bootstrap(&dom.document, &document_url, &document_url, &mut assets).unwrap();
        publish_bootstrap(&dom.document, &document_url, &document_url, &mut assets).unwrap();

        let mut serialized = Vec::new();
        serialize(
            &mut serialized,
            &SerializableHandle::from(dom.document),
            SerializeOpts::default(),
        )
        .unwrap();
        let html = String::from_utf8(serialized).unwrap();
        assert_eq!(html.matches(BOOTSTRAP_MARKER).count(), 1);
        assert_eq!(assets.len(), 1);
        assert!(
            assets
                .keys()
                .next()
                .unwrap()
                .starts_with("assets/fe-bootstrap-")
        );
    }

    #[test]
    fn generated_references_ignore_document_base_retargeting() {
        let output = precompile_html(
            "https://example.test/app/index.html",
            r#"<base href="/sources/">
               <script type="application/fe">pub fn main() {}</script>"#,
            |_| panic!("inline source"),
        )
        .unwrap();
        assert!(output.html.contains(r#"data-fe-src="../app/assets/fe-"#));
        assert!(
            output
                .html
                .contains(r#"data-fe-bootstrap="" src="../app/assets/fe-bootstrap-"#)
                || output.html.contains(r#"src="../app/assets/fe-bootstrap-"#)
        );
    }

    #[test]
    fn publishes_versioned_adapter_selection_inventory() {
        let output = precompile_html_with_adapter_metadata(
            "https://example.test/index.html",
            r#"<script type="application/fe">pub fn main() {}</script>"#,
            &[],
            |_| panic!("inline source"),
        )
        .unwrap();
        assert!(output.html.contains("data-fe-adapter-selection="));
        let bytes = output
            .assets
            .iter()
            .find_map(|(path, bytes)| path.contains("fe-adapter-selection-").then_some(bytes))
            .unwrap();
        let selection: fe_webidl_bindgen::AdapterSelectionManifest =
            serde_json::from_slice(bytes).unwrap();
        assert_eq!(
            selection.version,
            fe_webidl_bindgen::ADAPTER_SELECTION_VERSION
        );
        assert!(selection.required_imports.is_empty());
        assert!(selection.operations.is_empty());
    }

    #[test]
    fn publishes_byte_identical_executable_adapter_slice() {
        let world = fe_webidl_bindgen::parse(
            r#"interface Console {
                undefined log(DOMString value);
                undefined warn(DOMString value);
            };"#,
        )
        .unwrap();
        let plan = fe_webidl_bindgen::build_adapter_plan(&world, "browser", "fe:web").unwrap();
        let build = || {
            precompile_html_with_adapter_plan(
                "https://example.test/index.html",
                r#"<script type="application/fe">pub fn main() {}</script>"#,
                &world,
                &plan,
                "generated-web",
                |_| panic!("inline source"),
            )
            .unwrap()
        };
        let first = build();
        let second = build();
        assert_eq!(first, second);
        assert!(first.html.contains("data-fe-adapter="));
        let adapter = first
            .assets
            .iter()
            .find_map(|(path, bytes)| {
                (path.contains("/fe-adapter-") && path.ends_with(".js")).then_some(bytes)
            })
            .unwrap();
        let adapter = std::str::from_utf8(adapter).unwrap();
        assert!(adapter.contains("createFeHostAdapter"));
        assert!(!adapter.contains("\"console_log\""));
        assert!(!adapter.contains("\"console_warn\""));
    }

    #[test]
    fn compilation_diagnostics_fail_without_publishing_partial_assets() {
        let error = precompile_html(
            "https://example.test/index.html",
            r#"<script type="application/fe">pub fn main() -> Missing { 42 }</script>"#,
            |_| panic!("inline source must not load"),
        )
        .unwrap_err();
        let PrecompileError::Diagnostics { diagnostics, .. } = error else {
            panic!("expected structured diagnostics, got {error:?}");
        };
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn development_graph_uses_html_base_and_only_data_fe_src() {
        let html = r#"
            <base href="/workspace/">
            <script type="application/fe" data-fe-src="./b.fe"></script>
            <script type="application/fe" data-fe-src="./a.fe"></script>
            <script type="application/fe" data-fe-src="./a.fe"></script>
            <script type="application/fe" src="./not-script-src.fe"></script>
            <script type="module" data-fe-src="./not-fe.fe"></script>
        "#;
        assert_eq!(
            discover_external_dependencies("https://example.test/site/index.html", html).unwrap(),
            [
                "https://example.test/workspace/a.fe",
                "https://example.test/workspace/b.fe",
            ]
        );
    }

    #[test]
    fn rebuild_coordinator_debounces_coalesces_and_cancels_stale_batches() {
        let mut coordinator = DevelopmentRebuildCoordinator::new(25);
        coordinator.precompiler.graph.replace(
            "https://example.test/b.html".to_owned(),
            BTreeSet::from(["https://example.test/shared.fe".to_owned()]),
        );
        coordinator.precompiler.graph.replace(
            "https://example.test/a.html".to_owned(),
            BTreeSet::from([
                "https://example.test/a.fe".to_owned(),
                "https://example.test/shared.fe".to_owned(),
            ]),
        );

        let first = coordinator
            .queue_changes(
                100,
                [
                    "https://example.test/shared.fe".to_owned(),
                    "https://example.test/unknown.fe".to_owned(),
                ],
            )
            .unwrap();
        assert_eq!(
            first,
            DevelopmentRebuildEvent::Scheduled {
                generation: 1,
                deadline_ms: 125,
                changed_urls: vec!["https://example.test/shared.fe".to_owned()],
                document_urls: vec![
                    "https://example.test/a.html".to_owned(),
                    "https://example.test/b.html".to_owned(),
                ],
            }
        );
        assert!(coordinator.take_ready(124).is_none());

        let second = coordinator
            .queue_changes(110, ["https://example.test/a.fe".to_owned()])
            .unwrap();
        assert!(matches!(
            second,
            DevelopmentRebuildEvent::Scheduled {
                generation: 2,
                deadline_ms: 135,
                ..
            }
        ));
        let batch = coordinator.take_ready(135).unwrap();
        assert_eq!(
            batch.changed_urls,
            [
                "https://example.test/a.fe",
                "https://example.test/shared.fe"
            ]
        );

        coordinator
            .queue_changes(136, ["https://example.test/shared.fe".to_owned()])
            .unwrap();
        let mut loaded = false;
        let events = coordinator.execute(
            batch,
            |_| {
                loaded = true;
                unreachable!("stale batches must not load documents")
            },
            |_| unreachable!("stale batches must not load sources"),
        );
        assert!(!loaded);
        assert!(matches!(
            events.as_slice(),
            [DevelopmentRebuildEvent::Cancelled { generation: 2, .. }]
        ));
    }

    #[test]
    fn rebuild_coordinator_reports_document_load_failure_without_transport_policy() {
        let document = "https://example.test/index.html";
        let mut coordinator = DevelopmentRebuildCoordinator::new(0);
        coordinator
            .precompiler
            .graph
            .replace(document.to_owned(), BTreeSet::new());
        coordinator.queue_changes(7, [document.to_owned()]).unwrap();
        let batch = coordinator.take_ready(7).unwrap();
        let events = coordinator.execute(
            batch,
            |_| Err("document store unavailable".to_owned()),
            |_| unreachable!("source loading must not run"),
        );
        assert_eq!(
            events,
            [DevelopmentRebuildEvent::Diagnostic {
                document_url: document.to_owned(),
                diagnostic: DevelopmentDiagnostic {
                    code: "document_load".to_owned(),
                    source_url: Some(document.to_owned()),
                    message: "document store unavailable".to_owned(),
                    compiler_diagnostics: Vec::new(),
                },
                serving_last_good: false,
            }]
        );
    }

    #[test]
    fn rebuild_events_publish_then_reload_only_for_changed_content() {
        let document = "https://example.test/index.html".to_owned();
        let publication = DevelopmentPublication {
            digest: "abc123".to_owned(),
            output: Arc::new(PrecompileOutput {
                html: String::new(),
                assets: BTreeMap::new(),
                modules: Vec::new(),
                render_dependencies: Vec::new(),
            }),
        };
        let mut events = Vec::new();
        append_build_events(
            &mut events,
            document.clone(),
            DevelopmentBuildReport {
                active: Some(publication.clone()),
                published: true,
                diagnostics: Vec::new(),
            },
        );
        assert_eq!(
            events,
            [
                DevelopmentRebuildEvent::Publication {
                    document_url: document.clone(),
                    digest: "abc123".to_owned(),
                    changed: true,
                },
                DevelopmentRebuildEvent::Reload {
                    document_url: document.clone(),
                    digest: "abc123".to_owned(),
                },
            ]
        );

        events.clear();
        append_build_events(
            &mut events,
            document.clone(),
            DevelopmentBuildReport {
                active: Some(publication),
                published: false,
                diagnostics: vec![DevelopmentDiagnostic {
                    code: "compile".to_owned(),
                    source_url: None,
                    message: "failed".to_owned(),
                    compiler_diagnostics: Vec::new(),
                }],
            },
        );
        assert!(matches!(
            events.as_slice(),
            [
                DevelopmentRebuildEvent::Diagnostic {
                    serving_last_good: true,
                    ..
                },
                DevelopmentRebuildEvent::Publication { changed: false, .. }
            ]
        ));
    }

    #[test]
    fn failed_development_build_keeps_immutable_last_good_and_diagnostics() {
        let document = "https://example.test/index.html";
        let good = r#"<script type="application/fe">pub fn main() -> u32 { 42 }</script>"#;
        let bad = r#"<script type="application/fe">pub fn main() -> Missing { 42 }</script>"#;
        let fixed = r#"<script type="application/fe">pub fn main() -> u32 { 7 }</script>"#;
        let mut development = DevelopmentPrecompiler::default();

        let first = development.build(document, good, |_| panic!("inline source"));
        assert!(first.succeeded());
        assert!(first.published);
        let first_publication = first.active.unwrap();
        let first_digest = first_publication.digest().to_owned();

        let unchanged = development.build(document, good, |_| panic!("inline source"));
        assert!(unchanged.succeeded());
        assert!(!unchanged.published);
        assert!(Arc::ptr_eq(
            &first_publication.output,
            &unchanged.active.as_ref().unwrap().output,
        ));

        let failed = development.build(document, bad, |_| panic!("inline source"));
        assert!(!failed.succeeded());
        assert!(failed.serving_last_good());
        assert!(!failed.published);
        assert_eq!(failed.active.as_ref().unwrap().digest(), first_digest);
        assert_eq!(failed.diagnostics[0].code, "compiler_diagnostics");
        assert!(!failed.diagnostics[0].compiler_diagnostics.is_empty());
        assert!(Arc::ptr_eq(
            &first_publication.output,
            &failed.active.as_ref().unwrap().output,
        ));

        let recovered = development.build(document, fixed, |_| panic!("inline source"));
        assert!(recovered.succeeded());
        assert!(recovered.published);
        assert_ne!(recovered.active.as_ref().unwrap().digest(), first_digest);
    }

    #[test]
    fn changed_external_source_selects_only_affected_documents() {
        let mut graph = DevelopmentDependencyGraph::default();
        graph.replace(
            "https://example.test/a.html".to_owned(),
            BTreeSet::from(["https://example.test/shared.fe".to_owned()]),
        );
        graph.replace(
            "https://example.test/b.html".to_owned(),
            BTreeSet::from([
                "https://example.test/other.fe".to_owned(),
                "https://example.test/shared.fe".to_owned(),
            ]),
        );
        assert_eq!(
            graph.affected_documents("https://example.test/shared.fe"),
            ["https://example.test/a.html", "https://example.test/b.html",]
        );
        assert_eq!(
            graph.affected_documents("https://example.test/other.fe"),
            ["https://example.test/b.html"]
        );
    }

    #[test]
    fn load_failure_updates_graph_but_publishes_nothing() {
        let document = "https://example.test/index.html";
        let html = r#"<script type="application/fe" data-fe-src="./missing.fe"></script>"#;
        let mut development = DevelopmentPrecompiler::default();
        let report = development.build(document, html, |_| Err("not found".to_owned()));
        assert!(!report.succeeded());
        assert!(report.active.is_none());
        assert_eq!(report.diagnostics[0].code, "source_load");
        assert_eq!(
            development.graph().dependencies(document),
            ["https://example.test/missing.fe"]
        );
    }

    #[test]
    fn deployment_verifier_accepts_a_precompiled_site_and_current_capstone() {
        let root = tempfile::tempdir().unwrap();
        let output = verified_fixture(root.path());
        let report = verify_precompiled_site(&root.path().join("index.html")).unwrap();
        assert_eq!(report.modules, 1);
        assert_eq!(report.files, output.assets.len());

        let capstone = Path::new("../../demos/webgpu-mandelbrot/index.html");
        if capstone.exists() {
            let report = verify_precompiled_site(capstone).unwrap();
            assert_eq!(report.modules, 1);
        }
    }

    #[test]
    fn deployment_verifier_reports_tamper_escape_missing_and_compiler_wasm() {
        let tamper = tempfile::tempdir().unwrap();
        let output = verified_fixture(tamper.path());
        let wasm = output
            .assets
            .keys()
            .find(|path| path.ends_with(".wasm"))
            .unwrap();
        std::fs::write(tamper.path().join(wasm), b"tampered").unwrap();
        let error = verify_precompiled_site(&tamper.path().join("index.html")).unwrap_err();
        assert!(error.to_string().contains("failed manifest verification"));

        let escape = tempfile::tempdir().unwrap();
        verified_fixture(escape.path());
        let html = std::fs::read_to_string(escape.path().join("index.html"))
            .unwrap()
            .replace("data-fe-manifest=\"assets/", "data-fe-manifest=\"../");
        std::fs::write(escape.path().join("index.html"), html).unwrap();
        let error = verify_precompiled_site(&escape.path().join("index.html")).unwrap_err();
        assert!(error.to_string().contains("escapes the deployment root"));

        let missing = tempfile::tempdir().unwrap();
        let output = verified_fixture(missing.path());
        let manifest = output
            .assets
            .keys()
            .find(|path| path.ends_with(".json"))
            .unwrap();
        std::fs::remove_file(missing.path().join(manifest)).unwrap();
        let error = verify_precompiled_site(&missing.path().join("index.html")).unwrap_err();
        assert!(error.to_string().contains("missing or unreadable"));

        let compiler = tempfile::tempdir().unwrap();
        verified_fixture(compiler.path());
        std::fs::write(
            compiler.path().join("fe-browser-compiler.wasm"),
            b"compiler",
        )
        .unwrap();
        let error = verify_precompiled_site(&compiler.path().join("index.html")).unwrap_err();
        assert!(error.to_string().contains("unexpected compiler Wasm"));
    }
}
