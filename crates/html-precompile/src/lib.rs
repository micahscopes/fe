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
/// Opt-in page policy used by the canonical gallery. It turns attribution from
/// descriptive metadata into a build gate: authored browser scripts are
/// rejected, and every render bundle must carry a complete compiler-produced
/// Fe ownership ledger with no JS/Rust/WGSL/Wasm/generated-manifest inputs.
pub const ATTRIBUTION_POLICY_META_NAME: &str = "fe-attribution-policy";
pub const CANONICAL_FE_GALLERY_POLICY: &str = "canonical_fe_gallery";
/// Valueless marker on a rewritten `application/fe+wasm` script: the bootstrap
/// hands this module to `mountRenderSurface` from the render runtime module
/// instead of instantiating it and calling its entry with zero arguments.
pub const RENDER_SCRIPT_MARKER: &str = "data-fe-render";
/// Points at the published, content-addressed render runtime module
/// (`fe_codegen::render_runtime_js()`) a `data-fe-render` script hands off to.
pub const RENDER_RUNTIME_ATTR: &str = "data-fe-render-runtime";
/// The authored web-component render lane's tag name: `<fe-surface src="...">`
/// is walked exactly like `script[data-fe-src]` and rewritten to
/// `<fe-surface manifest="...">` (FE_WEB_V5_ORCHESTRATION_DESIGN.md 3.3, form 2).
pub const SURFACE_ELEMENT_TAG: &str = "fe-surface";
/// Valueless marker on the injected `<script type="module">` that loads the
/// render runtime for a document whose ONLY render sources are authored
/// `<fe-surface src>` elements (no `data-fe-render` script to carry the
/// dynamic-import handoff, so the element's defining side effect needs a
/// static import instead). Injected at most once per document, the same
/// idempotent posture as [`BOOTSTRAP_MARKER`].
pub const SURFACE_RUNTIME_MARKER: &str = "data-fe-surface-runtime";
const BOOTSTRAP_SOURCE: &str = include_str!("../assets/bootstrap.js");
const RESIDENT_ACTOR_INITIALIZE_EXPORT: &str = "fe_actor_initialize_v1";

/// A compiled render bundle for one `data-fe-src` naming a GpuProgram-actor
/// ingot, produced through the SAME seam `fe web build` uses
/// (`resolve_web_entry` + `WebBundle::compile`), ready for content-addressed
/// publication by [`precompile_html_with_render_lane`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderBundleArtifact {
    /// Optional CPU fallback module. Typed GPU pass graphs deliberately omit
    /// this so a missing WebGPU implementation is reported instead of hiding
    /// a graph failure behind a different execution path.
    pub wasm: Option<Vec<u8>>,
    pub wgsl: Vec<u8>,
    /// Every shader in an ordered pass graph. Empty for the legacy one-shader
    /// bundle shape, where `wgsl` alone is sufficient.
    pub pass_wgsl: Vec<RenderShaderArtifact>,
    /// The bundle's fe-web-bundle manifest exactly as
    /// `WebBundle::manifest_json()` produces it. Publication rewrites the
    /// bundle-local artifact and pass shader paths to content-addressed names,
    /// the same precedent as the wasm lane's `PublishedArtifact::from_artifact`.
    pub manifest_json: Vec<u8>,
    /// The render source's structural dependency closure (ingot files),
    /// folded into the development watch graph exactly like the wasm lane's
    /// `PublishedModuleManifest::source_dependencies`.
    pub source_dependencies: Option<SourceDependencyInventory>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderShaderArtifact {
    pub path: String,
    pub bytes: Vec<u8>,
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
    AttributionPolicy {
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
        mut render_compile: impl FnMut(
            &Url,
            Option<&str>,
        ) -> Result<Option<RenderBundleArtifact>, String>,
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
/// as [`precompile_html`]. Walks both `script[data-fe-src]` (the facade/render
/// script lanes) and authored `fe-surface[src]` elements (the render web
/// component lane, FE_WEB_V5_ORCHESTRATION_DESIGN.md 3.3 form 2), so the
/// development watch graph tracks an authored surface's source from the very
/// first build, not only after a first successful compile populates
/// `render_dependencies`.
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
    let mut surfaces = Vec::new();
    collect_elements_with_attr(&dom.document, SURFACE_ELEMENT_TAG, "src", &mut surfaces);
    let sources = scripts
        .iter()
        .filter_map(|script| attr(script, "data-fe-src"))
        .chain(surfaces.iter().filter_map(|surface| attr(surface, "src")));
    let dependencies = sources
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
        let manifest_ref = required_attr(script, "data-fe-manifest", &context)?;
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
        if attr(script, RENDER_SCRIPT_MARKER).is_some() {
            verify_render_deployment(root, script, &manifest, &value, &context, &mut verified)?;
            verified.insert(manifest);
            continue;
        }

        let wasm_ref = required_attr(script, "data-fe-src", &context)?;
        let wasm = deployment_file(root, &wasm_ref, &format!("{context} data-fe-src"))?;
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

fn verify_render_deployment(
    root: &Path,
    script: &Handle,
    manifest: &Path,
    value: &serde_json::Value,
    context: &str,
    verified: &mut BTreeSet<PathBuf>,
) -> Result<(), VerificationError> {
    if value["protocol"].as_str() != Some("fe-web-bundle") {
        return Err(VerificationError {
            context: format!("{context} manifest {}", manifest.display()),
            detail: "render manifest protocol is not `fe-web-bundle`".to_owned(),
        });
    }
    let version = value["protocol_version"]
        .as_u64()
        .ok_or_else(|| VerificationError {
            context: format!("{context} manifest {}", manifest.display()),
            detail: "render manifest has no protocol_version".to_owned(),
        })?;
    if !(4..=6).contains(&version) {
        return Err(VerificationError {
            context: format!("{context} manifest {}", manifest.display()),
            detail: format!("unsupported fe-web-bundle protocol version {version}"),
        });
    }
    let artifacts = value["artifacts"]
        .as_object()
        .ok_or_else(|| VerificationError {
            context: format!("{context} manifest {}", manifest.display()),
            detail: "render manifest has no artifacts object".to_owned(),
        })?;
    let manifest_root = manifest.parent().ok_or_else(|| VerificationError {
        context: format!("{context} manifest {}", manifest.display()),
        detail: "render manifest has no parent directory".to_owned(),
    })?;

    let primary_ref = artifacts
        .get("wgsl")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| VerificationError {
            context: format!("{context} manifest artifacts.wgsl"),
            detail: "required string is missing".to_owned(),
        })?;
    let primary = deployment_file(
        manifest_root,
        primary_ref,
        &format!("{context} manifest artifacts.wgsl"),
    )?;
    verify_addressed_file(&primary, &format!("{context} primary WGSL"))?;
    verify_byte_length(
        &primary,
        artifacts
            .get("wgsl_bytes")
            .and_then(serde_json::Value::as_u64),
        &format!("{context} primary WGSL"),
    )?;
    verified.insert(primary);

    match artifacts.get("wasm").and_then(serde_json::Value::as_str) {
        Some(wasm_ref) => {
            let script_ref = required_attr(script, "data-fe-src", context)?;
            let script_wasm =
                deployment_file(root, &script_ref, &format!("{context} data-fe-src"))?;
            let declared_wasm = deployment_file(
                manifest_root,
                wasm_ref,
                &format!("{context} manifest artifacts.wasm"),
            )?;
            if script_wasm != declared_wasm {
                return Err(VerificationError {
                    context: format!("{context} manifest artifacts.wasm"),
                    detail: format!(
                        "resolves to {}, but data-fe-src resolves to {}",
                        declared_wasm.display(),
                        script_wasm.display()
                    ),
                });
            }
            verify_addressed_file(&declared_wasm, &format!("{context} Wasm"))?;
            verify_byte_length(
                &declared_wasm,
                artifacts
                    .get("wasm_bytes")
                    .and_then(serde_json::Value::as_u64),
                &format!("{context} Wasm"),
            )?;
            if let Some(integrity) = attr(script, "data-fe-integrity") {
                verify_integrity(&declared_wasm, &integrity, context)?;
            }
            verified.insert(declared_wasm);
        }
        None => {
            if attr(script, "data-fe-src").is_some() || attr(script, "data-fe-integrity").is_some()
            {
                return Err(VerificationError {
                    context: context.to_owned(),
                    detail: "GPU-only render graph declares a script Wasm reference or integrity"
                        .to_owned(),
                });
            }
        }
    }

    if let Some(passes) = value.get("passes").and_then(serde_json::Value::as_array) {
        for (index, pass) in passes.iter().enumerate() {
            let pass_context = format!("{context} pass #{}", index + 1);
            let shader_ref = pass["shader"].as_str().ok_or_else(|| VerificationError {
                context: pass_context.clone(),
                detail: "pass has no shader path".to_owned(),
            })?;
            let shader = deployment_file(manifest_root, shader_ref, &pass_context)?;
            verify_addressed_file(&shader, &pass_context)?;
            verify_byte_length(&shader, pass["shader_bytes"].as_u64(), &pass_context)?;
            verified.insert(shader);
        }
    }

    let runtime_ref = required_attr(script, RENDER_RUNTIME_ATTR, context)?;
    let runtime = deployment_file(
        root,
        &runtime_ref,
        &format!("{context} {RENDER_RUNTIME_ATTR}"),
    )?;
    verify_addressed_file(&runtime, &format!("{context} render runtime"))?;
    verified.insert(runtime);
    Ok(())
}

fn verify_byte_length(
    path: &Path,
    expected: Option<u64>,
    context: &str,
) -> Result<(), VerificationError> {
    let expected = expected.ok_or_else(|| VerificationError {
        context: context.to_owned(),
        detail: "manifest byte length is missing".to_owned(),
    })?;
    let actual = std::fs::metadata(path)
        .map_err(|error| VerificationError {
            context: context.to_owned(),
            detail: format!("cannot inspect {}: {error}", path.display()),
        })?
        .len();
    if actual != expected {
        return Err(VerificationError {
            context: context.to_owned(),
            detail: format!(
                "{} has {actual} bytes, but the manifest declares {expected}",
                path.display()
            ),
        });
    }
    Ok(())
}

fn verify_integrity(path: &Path, integrity: &str, context: &str) -> Result<(), VerificationError> {
    let bytes = std::fs::read(path).map_err(|error| VerificationError {
        context: context.to_owned(),
        detail: format!("cannot read {}: {error}", path.display()),
    })?;
    let expected = format!(
        "sha256-{}",
        base64::engine::general_purpose::STANDARD.encode(hex_to_bytes(&sha256_hex(&bytes)))
    );
    if integrity != expected {
        return Err(VerificationError {
            context: format!("{context} data-fe-integrity"),
            detail: format!("expected {expected:?}, found {integrity:?}"),
        });
    }
    Ok(())
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
        PrecompileError::AttributionPolicy { source_url, detail } => DevelopmentDiagnostic {
            code: "attribution_policy".to_owned(),
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
                | PrecompileError::AdapterSelection { .. }
                | PrecompileError::AttributionPolicy { .. } => unreachable!(),
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
    let canonical_gallery = find_meta_content(&dom.document, ATTRIBUTION_POLICY_META_NAME)
        .is_some_and(|value| {
            value
                .trim()
                .eq_ignore_ascii_case(CANONICAL_FE_GALLERY_POLICY)
        });
    if canonical_gallery {
        validate_canonical_gallery_document(&dom.document, &document_url)?;
    }
    let base_url = document_base_url(&dom.document, &document_url)?;
    let document_source = PublishedDocumentSource {
        id: document_url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .filter(|segment| !segment.is_empty())
            .unwrap_or("index.html")
            .to_owned(),
        sha256: sha256_hex(html.as_bytes()),
    };
    let mut scripts = Vec::new();
    collect_fe_scripts(&dom.document, &mut scripts);

    let mut assets = BTreeMap::new();
    let mut modules = Vec::new();
    let mut render_dependencies = Vec::new();
    let mut render_runtime_asset: Option<PublishedRenderRuntime> = None;
    for (index, script) in scripts.into_iter().enumerate() {
        let src = attr(&script, "data-fe-src");
        let entry_attr = attr(&script, "data-fe-entry");
        let is_component = attr(&script, "data-fe-component").is_some();

        if let Some(src) = src.as_deref() {
            let url = base_url
                .join(src)
                .map_err(|error| PrecompileError::SourceLoad {
                    url: src.to_owned(),
                    detail: error.to_string(),
                })?;
            if let Some(bundle) = render_compile(&url, entry_attr.as_deref()).map_err(|detail| {
                PrecompileError::Compile {
                    source_url: url.to_string(),
                    detail,
                }
            })? {
                if canonical_gallery {
                    validate_canonical_render_bundle(&bundle, &url)?;
                }
                if let Some(dependencies) = &bundle.source_dependencies {
                    render_dependencies.push(dependencies.clone());
                }
                let runtime = publish_render_runtime(
                    render_runtime_js,
                    &mut assets,
                    &mut render_runtime_asset,
                )?;
                publish_render_bundle(
                    &script,
                    &base_url,
                    &document_url,
                    bundle,
                    &runtime,
                    &document_source,
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
        let entry = if is_component {
            if let Some(entry) = entry_attr
                && entry != RESIDENT_ACTOR_INITIALIZE_EXPORT
            {
                return Err(PrecompileError::Compile {
                    source_url,
                    detail: format!(
                        "a role-selected resident component has fixed entry `{RESIDENT_ACTOR_INITIALIZE_EXPORT}`, not `{entry}`"
                    ),
                });
            }
            RESIDENT_ACTOR_INITIALIZE_EXPORT.to_owned()
        } else {
            entry_attr.unwrap_or_else(|| "main".to_owned())
        };
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

    // Authored `<fe-surface src="...">` elements (the render web component
    // lane, FE_WEB_V5_ORCHESTRATION_DESIGN.md 3.3 form 2): walked exactly like
    // `script[data-fe-src]` above (collect, compile via the SAME
    // `render_compile` closure), but rewritten to `manifest="..."` on the
    // element itself instead of a script-tag rewrite. Unlike a script's
    // `data-fe-src`, a bare `fe-surface[src]` is ALWAYS a render source (there
    // is no wasm-only facade fallback for this tag), so `render_compile`
    // returning `Ok(None)` (a single `.fe` file, not an ingot directory) is a
    // hard error naming the source, not a silent fall-through.
    let mut surface_elements = Vec::new();
    collect_elements_with_attr(
        &dom.document,
        SURFACE_ELEMENT_TAG,
        "src",
        &mut surface_elements,
    );
    let mut published_a_surface = false;
    for element in surface_elements {
        let src = attr(&element, "src").expect("collected `fe-surface[src]` elements have `src`");
        let url = base_url
            .join(&src)
            .map_err(|error| PrecompileError::SourceLoad {
                url: src.clone(),
                detail: error.to_string(),
            })?;
        let entry_attr = attr(&element, "entry");
        let bundle = render_compile(&url, entry_attr.as_deref())
            .map_err(|detail| PrecompileError::Compile {
                source_url: url.to_string(),
                detail,
            })?
            .ok_or_else(|| PrecompileError::SourceLoad {
                url: url.to_string(),
                detail: "`fe-surface[src]` must name a render-bundle ingot directory (a single \
                          .fe file has no `actor` declaration to derive a surface from)"
                    .to_owned(),
            })?;
        if canonical_gallery {
            validate_canonical_render_bundle(&bundle, &url)?;
        }
        if let Some(dependencies) = &bundle.source_dependencies {
            render_dependencies.push(dependencies.clone());
        }
        let runtime =
            publish_render_runtime(render_runtime_js, &mut assets, &mut render_runtime_asset)?;
        publish_authored_surface(
            &element,
            &base_url,
            &document_url,
            bundle,
            &runtime,
            &document_source,
            &mut assets,
        )?;
        published_a_surface = true;
    }
    if published_a_surface {
        // The element's defining `customElements.define("fe-surface", ...)`
        // is a side effect of importing the runtime module; a `<script
        // data-fe-render>` handoff triggers that import dynamically at
        // runtime (bootstrap.js), but a page with ONLY authored `fe-surface`
        // elements has no such script, so a static module import is injected
        // once instead.
        let runtime =
            publish_render_runtime(render_runtime_js, &mut assets, &mut render_runtime_asset)?;
        publish_surface_runtime_loader(&dom.document, &document_url, &base_url, &runtime.path)?;
    }

    publish_bootstrap(&dom.document, &document_url, &base_url, &mut assets)?;

    materialize_template_contents_for_serialization(&dom.document);
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

/// `markup5ever_rcdom::SerializableHandle` currently walks an element's
/// ordinary `children` but not the separate fragment stored in
/// `template_contents`. Move each parsed template fragment into the traversal
/// tree immediately before serialization so standards-authored inert
/// templates survive precompilation. Re-parsing the output restores the
/// browser/rcdom template-content representation.
fn materialize_template_contents_for_serialization(root: &Handle) {
    let contents = match &root.data {
        NodeData::Element {
            template_contents, ..
        } => template_contents.borrow_mut().take(),
        _ => None,
    };
    if let Some(contents) = contents {
        let moved: Vec<_> = contents.children.borrow_mut().drain(..).collect();
        for child in &moved {
            child.parent.set(Some(Rc::downgrade(root)));
        }
        root.children.borrow_mut().extend(moved);
    }
    let children = root.children.borrow().clone();
    for child in children {
        materialize_template_contents_for_serialization(&child);
    }
}

/// Publish the fixed render runtime module once, content-addressed. Returns
/// its (document-relative, un-retargeted) publication path, from cache after
/// the first call.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PublishedRenderRuntime {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublishedDocumentSource {
    id: String,
    sha256: String,
}

fn publish_render_runtime(
    render_runtime_js: &str,
    assets: &mut BTreeMap<String, Vec<u8>>,
    published: &mut Option<PublishedRenderRuntime>,
) -> Result<PublishedRenderRuntime, PrecompileError> {
    if let Some(runtime) = published {
        return Ok(runtime.clone());
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
    let runtime = PublishedRenderRuntime {
        path,
        bytes: render_runtime_js.len() as u64,
        sha256: digest,
    };
    *published = Some(runtime.clone());
    Ok(runtime)
}

/// Publish one render bundle's optional wasm, shaders, and manifest
/// content-addressed and rewrite its manifest's artifact/pass paths to the
/// published names (the same precedent as the wasm lane's
/// `PublishedArtifact::from_artifact` rewriting paths). Shared by both render
/// authoring forms: the `<script data-fe-render>` lane
/// ([`publish_render_bundle`]) and the authored `<fe-surface src>` lane
/// ([`publish_authored_surface`]); both converge on identical per-tile
/// artifacts, only the tag rewrite at the end differs
/// (FE_WEB_V5_ORCHESTRATION_DESIGN.md 3.3: "one pipeline, two idioms, never
/// two runtimes").
fn publish_render_artifacts(
    base_url: &Url,
    document_url: &Url,
    bundle: RenderBundleArtifact,
    runtime: &PublishedRenderRuntime,
    document_source: &PublishedDocumentSource,
    assets: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(Option<String>, String, Option<String>), PrecompileError> {
    let RenderBundleArtifact {
        wasm,
        wgsl,
        pass_wgsl,
        manifest_json,
        source_dependencies: _,
    } = bundle;
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&manifest_json).map_err(|error| {
            PrecompileError::Serialize(format!("render bundle manifest is not valid JSON: {error}"))
        })?;
    pin_published_attribution(&mut manifest, runtime, document_source)?;
    let artifacts = manifest
        .get_mut("artifacts")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            PrecompileError::Serialize("render bundle manifest has no `artifacts`".to_owned())
        })?;
    let original_primary = artifacts
        .get("wgsl")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            PrecompileError::Serialize(
                "render bundle manifest has no string `artifacts.wgsl`".to_owned(),
            )
        })?
        .to_owned();

    let (wasm_ref, wasm_sha256) = if let Some(wasm) = wasm {
        let digest = sha256_hex(&wasm);
        let path = format!("assets/fe-render-{}.wasm", &digest[..16]);
        insert_identical(assets, path.clone(), wasm)?;
        artifacts.insert(
            "wasm".to_owned(),
            serde_json::Value::String(basename(&path).to_owned()),
        );
        (
            Some(published_reference(base_url, document_url, &path)),
            Some(digest),
        )
    } else {
        artifacts.remove("wasm");
        artifacts.remove("wasm_bytes");
        (None, None)
    };

    let shaders = if pass_wgsl.is_empty() {
        vec![RenderShaderArtifact {
            path: original_primary.clone(),
            bytes: wgsl,
        }]
    } else {
        if pass_wgsl
            .iter()
            .find(|shader| shader.path == original_primary)
            .is_none_or(|shader| shader.bytes != wgsl)
        {
            return Err(PrecompileError::Serialize(
                "render bundle primary WGSL does not match its pass shader".to_owned(),
            ));
        }
        pass_wgsl
    };
    let mut published_shaders = BTreeMap::new();
    for shader in shaders {
        let digest = sha256_hex(&shader.bytes);
        let path = format!("assets/fe-render-{}.wgsl", &digest[..16]);
        insert_identical(assets, path.clone(), shader.bytes)?;
        if let Some(previous) = published_shaders.get(&shader.path) {
            if previous != &path {
                return Err(PrecompileError::Serialize(format!(
                    "render bundle repeats shader path `{}` with different content",
                    shader.path
                )));
            }
        } else {
            published_shaders.insert(shader.path, path);
        }
    }
    let primary_path = published_shaders.get(&original_primary).ok_or_else(|| {
        PrecompileError::Serialize(format!(
            "render bundle has no shader content for primary path `{original_primary}`"
        ))
    })?;
    artifacts.insert(
        "wgsl".to_owned(),
        serde_json::Value::String(basename(primary_path).to_owned()),
    );

    if let Some(passes) = manifest
        .get_mut("passes")
        .and_then(serde_json::Value::as_array_mut)
    {
        for pass in passes {
            let shader = pass
                .get("shader")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    PrecompileError::Serialize(
                        "render bundle pass has no string `shader` path".to_owned(),
                    )
                })?;
            let published = published_shaders.get(shader).ok_or_else(|| {
                PrecompileError::Serialize(format!(
                    "render bundle has no shader content for pass path `{shader}`"
                ))
            })?;
            pass["shader"] = serde_json::Value::String(basename(published).to_owned());
        }
    }
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| PrecompileError::Serialize(error.to_string()))?;
    let manifest_sha256 = sha256_hex(&manifest_bytes);
    let manifest_path = format!("assets/fe-render-{}.json", &manifest_sha256[..16]);
    insert_identical(assets, manifest_path.clone(), manifest_bytes)?;

    Ok((
        wasm_ref,
        published_reference(base_url, document_url, &manifest_path),
        wasm_sha256,
    ))
}

/// Form 1 (`<script type="application/fe" data-fe-src=DIR data-fe-render>`):
/// publish the bundle and rewrite the script tag to `data-fe-render`.
fn publish_render_bundle(
    script: &Handle,
    base_url: &Url,
    document_url: &Url,
    bundle: RenderBundleArtifact,
    runtime: &PublishedRenderRuntime,
    document_source: &PublishedDocumentSource,
    assets: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), PrecompileError> {
    let (wasm_ref, manifest_ref, wasm_sha256) = publish_render_artifacts(
        base_url,
        document_url,
        bundle,
        runtime,
        document_source,
        assets,
    )?;
    rewrite_render_script(
        script,
        wasm_ref.as_deref(),
        &manifest_ref,
        &published_reference(base_url, document_url, &runtime.path),
        wasm_sha256.as_deref(),
    );
    Ok(())
}

/// Form 2 (`<fe-surface src=DIR>`): publish the bundle and rewrite the
/// element to `<fe-surface manifest="...">`. Unlike the script lane, the
/// element resolves its wasm/wgsl artifacts RELATIVE TO THE MANIFEST itself
/// (the runtime's own rule, fe-render-runtime.js), so only the manifest
/// reference is written back onto the element; no integrity attribute either
/// (the render lane carries none: WebGPU bundles have no Web IDL interface,
/// the same posture [`rewrite_render_script`] already takes).
fn publish_authored_surface(
    element: &Handle,
    base_url: &Url,
    document_url: &Url,
    bundle: RenderBundleArtifact,
    runtime: &PublishedRenderRuntime,
    document_source: &PublishedDocumentSource,
    assets: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), PrecompileError> {
    let (_wasm_ref, manifest_ref, _wasm_sha256) = publish_render_artifacts(
        base_url,
        document_url,
        bundle,
        runtime,
        document_source,
        assets,
    )?;
    remove_attr(element, "src");
    remove_attr(element, "entry");
    set_attr(element, "manifest", &manifest_ref);
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

/// Ensure a static `<script type="module">` importing the render runtime is
/// present exactly once, so its `customElements.define("fe-surface", ...)`
/// side effect runs even for a page whose ONLY render sources are authored
/// `<fe-surface src>` elements (no `data-fe-render` script exists to carry
/// bootstrap.js's dynamic-import handoff). Idempotent per parsed document,
/// the same posture as [`publish_bootstrap`]; loading the runtime module
/// twice (this static tag plus a script's dynamic import, on a page that
/// mixes both authoring forms) is harmless, since ES modules are cached by
/// URL and `customElements.define` only ever runs once per module evaluation.
fn publish_surface_runtime_loader(
    root: &Handle,
    document_url: &Url,
    base_url: &Url,
    runtime_path: &str,
) -> Result<(), PrecompileError> {
    if find_attr_element(root, SURFACE_RUNTIME_MARKER).is_some() {
        return Ok(());
    }
    let parent = find_first_element(root, "head")
        .or_else(|| find_first_element(root, "body"))
        .ok_or_else(|| {
            PrecompileError::Serialize("HTML document has no head or body".to_owned())
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
                value: published_reference(base_url, document_url, runtime_path).into(),
            },
            Attribute {
                name: QualName::new(None, ns!(), LocalName::from(SURFACE_RUNTIME_MARKER)),
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

/// Collect every `<tag attribute="...">` element in tree order (used for
/// `fe-surface[src]`, the authored render web component lane; unknown tags
/// like `fe-surface` parse as ordinary generic HTML elements, so this walks
/// them the same way [`collect_scripts_of_type`] walks `<script>`).
fn collect_elements_with_attr(root: &Handle, tag: &str, attribute: &str, output: &mut Vec<Handle>) {
    if is_element(root, tag) && attr(root, attribute).is_some() {
        output.push(root.clone());
    }
    for child in root.children.borrow().iter() {
        collect_elements_with_attr(child, tag, attribute, output);
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

fn collect_elements(root: &Handle, tag: &str, output: &mut Vec<Handle>) {
    if is_element(root, tag) {
        output.push(root.clone());
    }
    for child in root.children.borrow().iter() {
        collect_elements(child, tag, output);
    }
}

fn validate_canonical_gallery_document(
    root: &Handle,
    document_url: &Url,
) -> Result<(), PrecompileError> {
    let mut scripts = Vec::new();
    collect_elements(root, "script", &mut scripts);
    for script in scripts {
        let script_type = attr(&script, "type").unwrap_or_default();
        if !script_type.trim().eq_ignore_ascii_case(SOURCE_SCRIPT_TYPE) {
            return Err(PrecompileError::AttributionPolicy {
                source_url: document_url.to_string(),
                detail: format!(
                    "canonical Fe gallery forbids authored browser scripts; found <script type={script_type:?}> (only `{SOURCE_SCRIPT_TYPE}` is allowed)"
                ),
            });
        }
    }
    validate_no_inline_javascript(root, document_url)
}

fn validate_no_inline_javascript(root: &Handle, document_url: &Url) -> Result<(), PrecompileError> {
    if let Some(attributes) = attrs(root) {
        for attribute in attributes.borrow().iter() {
            let name = attribute.name.local.as_ref();
            let value = attribute.value.trim();
            if name
                .get(..2)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("on"))
                && name.len() > 2
            {
                return Err(PrecompileError::AttributionPolicy {
                    source_url: document_url.to_string(),
                    detail: format!(
                        "canonical Fe gallery forbids inline JavaScript event handlers; found `{name}`"
                    ),
                });
            }
            if value
                .get(.."javascript:".len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("javascript:"))
            {
                return Err(PrecompileError::AttributionPolicy {
                    source_url: document_url.to_string(),
                    detail: "canonical Fe gallery forbids `javascript:` URLs".to_owned(),
                });
            }
        }
    }
    for child in root.children.borrow().iter() {
        validate_no_inline_javascript(child, document_url)?;
    }
    Ok(())
}

fn validate_canonical_render_bundle(
    bundle: &RenderBundleArtifact,
    source_url: &Url,
) -> Result<(), PrecompileError> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&bundle.manifest_json).map_err(|error| {
            PrecompileError::AttributionPolicy {
                source_url: source_url.to_string(),
                detail: format!("render bundle has no inspectable attribution manifest: {error}"),
            }
        })?;
    let provenance = manifest
        .get("provenance")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| PrecompileError::AttributionPolicy {
            source_url: source_url.to_string(),
            detail: "canonical render bundle has no compiler-produced `provenance` ledger"
                .to_owned(),
        })?;
    let authored = provenance
        .get("authored_sources")
        .and_then(serde_json::Value::as_array)
        .filter(|sources| !sources.is_empty())
        .ok_or_else(|| PrecompileError::AttributionPolicy {
            source_url: source_url.to_string(),
            detail: "canonical render bundle has no digested authored Fe sources".to_owned(),
        })?;
    for source in authored {
        validate_source_ledger_entry(source, source_url, &["fe", "fe_manifest"])?;
    }
    if let Some(sources) = provenance
        .get("non_fe_authored_sources")
        .and_then(serde_json::Value::as_array)
    {
        for source in sources {
            validate_source_ledger_entry(source, source_url, &["html", "css", "asset", "other"])?;
        }
    }
    let generated = provenance
        .get("generated_artifacts")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| PrecompileError::AttributionPolicy {
            source_url: source_url.to_string(),
            detail: "canonical render bundle has no generated-artifact ownership ledger".to_owned(),
        })?;
    for required in ["manifest", "wgsl"] {
        if !generated.iter().any(|kind| kind.as_str() == Some(required)) {
            return Err(PrecompileError::AttributionPolicy {
                source_url: source_url.to_string(),
                detail: format!(
                    "canonical render bundle does not identify `{required}` as compiler-generated"
                ),
            });
        }
    }
    if bundle.wasm.is_some() && !generated.iter().any(|kind| kind.as_str() == Some("wasm")) {
        return Err(PrecompileError::AttributionPolicy {
            source_url: source_url.to_string(),
            detail: "canonical render bundle has Wasm bytes not attributed to the Fe compiler"
                .to_owned(),
        });
    }
    let contract = provenance
        .get("fixed_host")
        .and_then(|host| host.get("contract"))
        .and_then(serde_json::Value::as_str);
    if contract != Some("fixed_versioned_demo_blind_browser_host") {
        return Err(PrecompileError::AttributionPolicy {
            source_url: source_url.to_string(),
            detail: "canonical render bundle does not declare the fixed demo-blind host contract"
                .to_owned(),
        });
    }
    Ok(())
}

fn validate_source_ledger_entry(
    source: &serde_json::Value,
    source_url: &Url,
    admitted_kinds: &[&str],
) -> Result<(), PrecompileError> {
    let id = source
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let sha256 = source
        .get("sha256")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let kind = source
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let stable_id = !id.is_empty()
        && !id.contains("://")
        && !Path::new(id).is_absolute()
        && !id.split('/').any(|component| component == "..");
    let valid_digest = sha256.len() == 64 && sha256.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !stable_id || !valid_digest || !admitted_kinds.contains(&kind) {
        return Err(PrecompileError::AttributionPolicy {
            source_url: source_url.to_string(),
            detail: format!(
                "canonical source ledger rejects id={id:?}, kind={kind:?}; expected a stable relative identity, SHA-256 digest, and one of {admitted_kinds:?}"
            ),
        });
    }
    Ok(())
}

fn pin_published_attribution(
    manifest: &mut serde_json::Value,
    runtime: &PublishedRenderRuntime,
    document_source: &PublishedDocumentSource,
) -> Result<(), PrecompileError> {
    let root = manifest.as_object_mut().ok_or_else(|| {
        PrecompileError::Serialize("render bundle manifest root is not an object".to_owned())
    })?;
    let provenance = root
        .entry("provenance")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            PrecompileError::Serialize("render bundle `provenance` is not an object".to_owned())
        })?;
    let fixed_host = provenance
        .entry("fixed_host")
        .or_insert_with(|| {
            serde_json::json!({
                "name": "fe-render-runtime",
                "contract": "fixed_versioned_demo_blind_browser_host",
                "responsibilities": [
                    "dom_surface",
                    "input_transport",
                    "presentation_scheduler",
                    "web_gpu_executor",
                    "lifecycle",
                    "wasm_loader"
                ]
            })
        })
        .as_object_mut()
        .ok_or_else(|| {
            PrecompileError::Serialize(
                "render bundle `provenance.fixed_host` is not an object".to_owned(),
            )
        })?;
    fixed_host.insert(
        "artifact".to_owned(),
        serde_json::json!({
            "path": basename(&runtime.path),
            "bytes": runtime.bytes,
            "sha256": runtime.sha256,
        }),
    );
    let pages = provenance
        .entry("non_fe_authored_sources")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| {
            PrecompileError::Serialize(
                "render bundle `provenance.non_fe_authored_sources` is not an array".to_owned(),
            )
        })?;
    if !pages.iter().any(|source| {
        source.get("id").and_then(serde_json::Value::as_str) == Some(&document_source.id)
    }) {
        pages.push(serde_json::json!({
            "id": document_source.id,
            "sha256": document_source.sha256,
            "kind": "html",
        }));
    }
    Ok(())
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
    wasm_path: Option<&str>,
    manifest_path: &str,
    render_runtime_path: &str,
    sha256: Option<&str>,
) {
    set_attr(node, "type", ARTIFACT_SCRIPT_TYPE);
    remove_attr(node, "src");
    remove_attr(node, "integrity");
    if let Some(wasm_path) = wasm_path {
        set_attr(node, "data-fe-src", wasm_path);
    } else {
        remove_attr(node, "data-fe-src");
    }
    set_attr(node, "data-fe-manifest", manifest_path);
    set_attr(node, RENDER_SCRIPT_MARKER, "");
    set_attr(node, RENDER_RUNTIME_ATTR, render_runtime_path);
    remove_attr(node, "data-fe-adapter-selection");
    remove_attr(node, "data-fe-adapter");
    if let Some(sha256) = sha256 {
        let digest = hex_to_bytes(sha256);
        set_attr(
            node,
            "data-fe-integrity",
            &format!(
                "sha256-{}",
                base64::engine::general_purpose::STANDARD.encode(digest)
            ),
        );
    } else {
        remove_attr(node, "data-fe-integrity");
    }
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

    fn fake_render_bundle() -> RenderBundleArtifact {
        RenderBundleArtifact {
            wasm: Some(b"wasm-bytes".to_vec()),
            wgsl: b"wgsl-source".to_vec(),
            pass_wgsl: Vec::new(),
            manifest_json: br#"{
                "artifacts": {
                    "wasm": "module.wasm",
                    "wasm_bytes": 10,
                    "wgsl": "shader.wgsl",
                    "wgsl_bytes": 11
                }
            }"#
            .to_vec(),
            source_dependencies: None,
        }
    }

    fn fake_attributed_render_bundle(non_fe_kind: Option<&str>) -> RenderBundleArtifact {
        let mut bundle = fake_render_bundle();
        let non_fe_authored_sources = non_fe_kind
            .map(|kind| {
                vec![serde_json::json!({
                    "id": format!("demo/host.{kind}"),
                    "sha256": "22".repeat(32),
                    "kind": kind,
                })]
            })
            .unwrap_or_default();
        bundle.manifest_json = serde_json::to_vec(&serde_json::json!({
            "protocol": "fe-web-bundle",
            "protocol_version": 6,
            "artifacts": {
                "wasm": "module.wasm",
                "wasm_bytes": 10,
                "wgsl": "shader.wgsl",
                "wgsl_bytes": 11,
            },
            "provenance": {
                "source_id": "demo",
                "authored_sources": [
                    {
                        "id": "demo/fe.toml",
                        "sha256": "00".repeat(32),
                        "kind": "fe_manifest",
                    },
                    {
                        "id": "demo/src/lib.fe",
                        "sha256": "11".repeat(32),
                        "kind": "fe",
                    },
                ],
                "non_fe_authored_sources": non_fe_authored_sources,
                "generated_artifacts": ["manifest", "wasm", "wgsl"],
                "fe_responsibilities": ["gpu_program", "surface_declaration"],
                "fixed_host": {
                    "name": "fe-render-runtime",
                    "contract": "fixed_versioned_demo_blind_browser_host",
                    "responsibilities": [
                        "dom_surface",
                        "input_transport",
                        "presentation_scheduler",
                        "web_gpu_executor",
                        "lifecycle",
                        "wasm_loader",
                    ],
                },
            },
        }))
        .unwrap();
        bundle
    }

    fn fake_render_graph_bundle() -> RenderBundleArtifact {
        RenderBundleArtifact {
            wasm: None,
            wgsl: b"fragment-shader".to_vec(),
            pass_wgsl: vec![
                RenderShaderArtifact {
                    path: "passes/000-compute.wgsl".to_owned(),
                    bytes: b"compute-shader".to_vec(),
                },
                RenderShaderArtifact {
                    path: "passes/001-fragment.wgsl".to_owned(),
                    bytes: b"fragment-shader".to_vec(),
                },
            ],
            manifest_json: serde_json::to_vec(&serde_json::json!({
                "protocol": "fe-web-bundle",
                "protocol_version": 6,
                "artifacts": {
                    "wgsl": "passes/001-fragment.wgsl",
                    "wgsl_bytes": 15
                },
                "passes": [
                    { "shader": "passes/000-compute.wgsl", "shader_bytes": 14 },
                    { "shader": "passes/001-fragment.wgsl", "shader_bytes": 15 }
                ]
            }))
            .unwrap(),
            source_dependencies: None,
        }
    }

    #[test]
    fn canonical_gallery_rejects_authored_browser_javascript() {
        for authored_javascript in [
            r#"<script src="app.js"></script>"#,
            r#"<button onclick="runDemo()">run</button>"#,
            r#"<a href="javascript:runDemo()">run</a>"#,
        ] {
            let html = format!(
                r#"<!doctype html><meta name="{ATTRIBUTION_POLICY_META_NAME}" content="{CANONICAL_FE_GALLERY_POLICY}">{authored_javascript}"#
            );
            let error = precompile_html_with_render_lane(
                "https://example.test/gallery.html",
                &html,
                "runtime-js",
                |_| unreachable!("policy fails before source loading"),
                |_, _| unreachable!("policy fails before render compilation"),
            )
            .unwrap_err();
            assert!(
                matches!(error, PrecompileError::AttributionPolicy { .. }),
                "{error}"
            );
        }
    }

    #[test]
    fn canonical_gallery_rejects_forbidden_non_fe_bundle_inputs() {
        let source_url = Url::parse("https://example.test/sketches/demo").unwrap();
        for kind in ["javascript", "rust", "wgsl", "wasm", "json"] {
            let error = validate_canonical_render_bundle(
                &fake_attributed_render_bundle(Some(kind)),
                &source_url,
            )
            .unwrap_err();
            assert!(
                matches!(error, PrecompileError::AttributionPolicy { .. }),
                "kind={kind}: {error}"
            );
            assert!(error.to_string().contains(kind), "kind={kind}: {error}");
        }
    }

    #[test]
    fn canonical_gallery_pins_exact_host_and_document_provenance() {
        let html = format!(
            r#"<!doctype html>
<meta name="{ATTRIBUTION_POLICY_META_NAME}" content="{CANONICAL_FE_GALLERY_POLICY}">
<script type="application/fe" data-fe-src="sketches/demo" data-fe-render></script>"#
        );
        let runtime = "export function mountRenderSurface() {}\n";
        let output = precompile_html_with_render_lane(
            "https://example.test/gallery.html",
            &html,
            runtime,
            |_| unreachable!("render ingot routes through the render lane"),
            |_, _| Ok(Some(fake_attributed_render_bundle(None))),
        )
        .unwrap();

        let manifest = output
            .assets
            .iter()
            .filter(|(path, _)| path.ends_with(".json"))
            .find_map(|(_, bytes)| {
                let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
                (value["protocol"] == "fe-web-bundle").then_some(value)
            })
            .unwrap();
        let host_artifact = &manifest["provenance"]["fixed_host"]["artifact"];
        let runtime_digest = sha256_hex(runtime.as_bytes());
        assert_eq!(host_artifact["sha256"], runtime_digest);
        assert_eq!(host_artifact["bytes"], runtime.len() as u64);
        assert_eq!(
            host_artifact["path"],
            format!("fe-render-runtime-{}.js", &runtime_digest[..16])
        );
        assert!(output.assets.contains_key(&format!(
            "assets/fe-render-runtime-{}.js",
            &runtime_digest[..16]
        )));

        let document_source = manifest["provenance"]["non_fe_authored_sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|source| source["id"] == "gallery.html")
            .unwrap();
        assert_eq!(document_source["kind"], "html");
        assert_eq!(document_source["sha256"], sha256_hex(html.as_bytes()));
    }

    #[test]
    fn authored_fe_surface_src_routes_through_the_render_lane_and_rewrites_to_manifest() {
        let html = r#"<!doctype html>
<html><body>
<fe-surface src="sketches/cga3d"><span slot="caption">cga3d</span></fe-surface>
</body></html>"#;
        let output = precompile_html_with_render_lane(
            "https://example.test/index.html",
            html,
            "export function mountRenderSurface() {}\n",
            |_| panic!("no application/fe script sources to load"),
            |url, entry| {
                assert_eq!(url.as_str(), "https://example.test/sketches/cga3d");
                assert_eq!(entry, None);
                Ok(Some(fake_render_bundle()))
            },
        )
        .unwrap();

        assert!(output.html.contains("<fe-surface"));
        assert!(!output.html.contains(r#"src="sketches/cga3d""#));
        assert!(output.html.contains(r#"manifest="assets/fe-render-"#));
        // The caption's light-DOM content survives the rewrite untouched.
        assert!(output.html.contains(r#"<span slot="caption">cga3d</span>"#));
        // A static runtime-loader module script is injected exactly once.
        assert_eq!(output.html.matches(SURFACE_RUNTIME_MARKER).count(), 1);
        assert!(output.html.contains(r#"type="module""#));
        assert!(
            output
                .assets
                .keys()
                .any(|path| path.ends_with(".wasm") && path.contains("fe-render-"))
        );
        assert!(
            output
                .assets
                .keys()
                .any(|path| path.ends_with(".wgsl") && path.contains("fe-render-"))
        );
        assert!(
            output
                .assets
                .keys()
                .any(|path| path.contains("fe-render-runtime-"))
        );
    }

    #[test]
    fn typed_pass_graph_publishes_every_shader_without_a_wasm_fallback() {
        let html = r#"<!doctype html><script type="application/fe" data-fe-src="sketches/graph" data-fe-render></script>"#;
        let output = precompile_html_with_render_lane(
            "https://example.test/index.html",
            html,
            "runtime-js",
            |_| panic!("no application/fe source files"),
            |_url, _entry| Ok(Some(fake_render_graph_bundle())),
        )
        .unwrap();

        assert!(output.html.contains(RENDER_SCRIPT_MARKER));
        assert!(!output.html.contains("data-fe-src"));
        assert!(!output.html.contains("data-fe-integrity"));
        assert_eq!(
            output
                .assets
                .keys()
                .filter(|path| path.ends_with(".wgsl"))
                .count(),
            2
        );
        assert!(!output.assets.keys().any(|path| path.ends_with(".wasm")));

        let manifest = output
            .assets
            .iter()
            .filter(|(path, _)| path.ends_with(".json"))
            .find_map(|(_, bytes)| {
                let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
                (value["protocol"] == "fe-web-bundle").then_some(value)
            })
            .unwrap();
        assert!(manifest["artifacts"].get("wasm").is_none());
        assert!(manifest["artifacts"].get("wasm_bytes").is_none());
        let primary = manifest["artifacts"]["wgsl"].as_str().unwrap();
        assert!(primary.starts_with("fe-render-"));
        for pass in manifest["passes"].as_array().unwrap() {
            let shader = pass["shader"].as_str().unwrap();
            assert!(shader.starts_with("fe-render-"));
            assert!(!shader.contains('/'));
            assert!(output.assets.contains_key(&format!("assets/{shader}")));
        }

        let deployment = tempfile::tempdir().unwrap();
        write_publication(deployment.path(), &output);
        let report = verify_precompiled_site(&deployment.path().join("index.html")).unwrap();
        assert_eq!(report.modules, 1);
        assert_eq!(report.files, output.assets.len());
    }

    #[test]
    fn authored_fe_surface_entry_attribute_is_forwarded_and_stripped() {
        let html = r#"<fe-surface src="sketches/qcga" entry="shade"></fe-surface>"#;
        let output = precompile_html_with_render_lane(
            "https://example.test/index.html",
            html,
            "runtime-js",
            |_| panic!("no application/fe script sources"),
            |_url, entry| {
                assert_eq!(entry, Some("shade"));
                Ok(Some(fake_render_bundle()))
            },
        )
        .unwrap();
        assert!(!output.html.contains("entry="));
        assert!(output.html.contains("manifest="));
    }

    #[test]
    fn authored_fe_surface_requires_a_render_bundle_source() {
        let html = r#"<fe-surface src="sketches/plain.fe"></fe-surface>"#;
        let error = precompile_html_with_render_lane(
            "https://example.test/index.html",
            html,
            "runtime-js",
            |_| panic!("no application/fe script sources"),
            |_, _| Ok(None),
        )
        .unwrap_err();
        assert!(matches!(error, PrecompileError::SourceLoad { .. }));
        assert!(error.to_string().contains("render-bundle ingot directory"));
    }

    #[test]
    fn discover_external_dependencies_includes_authored_fe_surface_sources() {
        let html = r#"<fe-surface src="sketches/cga3d"></fe-surface>
                      <script type="application/fe" data-fe-src="other.fe"></script>"#;
        let dependencies =
            discover_external_dependencies("https://example.test/index.html", html).unwrap();
        assert_eq!(
            dependencies,
            [
                "https://example.test/other.fe",
                "https://example.test/sketches/cga3d",
            ]
        );
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
    fn resident_fe_component_uses_normal_wasm_lane_and_fixed_bootstrap_without_new_json_protocol() {
        let html = r##"<!doctype html><html><head></head><body>
<script type="application/fe" data-fe-src="./source-inspector.fe"
        data-fe-component data-fe-mount="#inspector"></script>
<fe-component id="inspector">
  <button data-fe-action="0">Fe</button>
  <button data-fe-action="1">Wasm</button>
  <section data-fe-view="0">authored</section>
  <section data-fe-view="1">generated</section>
</fe-component>
</body></html>"##;
        let source = include_str!("../../codegen/tests/fixtures/web_component_actor/src/lib.fe");
        let output = precompile_html("https://example.test/component.html", html, |url| {
            assert_eq!(url.as_str(), "https://example.test/source-inspector.fe");
            Ok(source.to_owned())
        })
        .expect("precompile resident Fe component");

        assert_eq!(output.modules.len(), 1);
        let module = &output.modules[0];
        assert_eq!(module.entry, RESIDENT_ACTOR_INITIALIZE_EXPORT);
        for fixed in [
            "fe_actor_initialize_v1",
            "fe_actor_transition_v1",
            "fe_actor_project_v1",
        ] {
            assert!(
                module
                    .interface
                    .exports
                    .iter()
                    .any(|export| export.name == fixed),
                "published component interface missing {fixed}"
            );
        }
        assert!(output.html.contains("data-fe-component=\"\""));
        assert!(output.html.contains("data-fe-mount=\"#inspector\""));
        assert!(output.html.contains("<fe-component id=\"inspector\""));
        assert!(
            !output.html.contains("actor_transition")
                && !output.html.contains("ComponentEventKind")
                && !output.html.contains("visible_mask"),
            "Fe policy must not be serialized into page or manifest JSON"
        );
        let bootstrap = output
            .assets
            .iter()
            .find(|(path, _)| path.contains("fe-bootstrap-"))
            .map(|(_, bytes)| std::str::from_utf8(bytes).expect("bootstrap UTF-8"))
            .expect("published fixed bootstrap");
        assert!(bootstrap.contains("customElements.define(\"fe-component\""));
        assert!(bootstrap.contains("fe_actor_transition_v1"));
        assert!(bootstrap.contains("[data-fe-view]"));
    }

    #[test]
    fn precompile_preserves_inert_html_template_contents() {
        let output = precompile_html(
            "https://example.test/component.html",
            r#"<!doctype html><template data-fe-template="7">
                 <li data-fe-key="fixture"><span>projected row</span></li>
               </template>"#,
            |_| panic!("template-only page has no Fe source"),
        )
        .expect("precompile template-only page");

        let reparsed = html5ever::parse_document(RcDom::default(), Default::default())
            .one(output.html.as_str());
        let template = find_first_element(&reparsed.document, "template")
            .expect("serialized template element");
        let contents = match &template.data {
            NodeData::Element {
                template_contents, ..
            } => template_contents
                .borrow()
                .clone()
                .expect("reparsed template content fragment"),
            _ => unreachable!("template is an element"),
        };
        let row = find_first_element(&contents, "li").expect("template row survived serialization");
        assert_eq!(attr(&row, "data-fe-key").as_deref(), Some("fixture"));
        assert!(find_first_element(&contents, "span").is_some());
        assert!(output.html.contains("projected row"));
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
