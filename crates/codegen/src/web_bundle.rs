//! Compiler-owned Wasm + WebGPU bundle construction.
//!
//! Both targets are lowered from one [`mir::RuntimePackage`]. This module owns
//! the browser-profile validation and the stable, typed JSON contract; browser
//! runners therefore consume compiler facts instead of reconstructing ABI
//! details from WGSL text.

use std::{
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use compiler_db::DriverDataBase;
use hir::hir_def::TopLevelMod;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sonatina_codegen::isa::spirv::{
    Access, LayoutMode, Role, SpirvBuiltinSource, SpirvLayout, SpirvScalarKind, WordKind,
};

use crate::sonatina::{
    WasmCompileOptions, compile_runtime_package_spirv_grid, compile_runtime_package_spirv_render,
    compile_runtime_package_wasm_with_options,
};
use crate::{
    CanonicalInterfaceManifest, canonical_lane_decl_from_entry, verify_canonical_wasm_abi,
};

pub const WEB_BUNDLE_PROTOCOL: &str = "fe-web-bundle";
pub const WEB_BUNDLE_PROTOCOL_VERSION: u32 = 5;
pub const WEB_ACTOR_RUNTIME_PROTOCOL: &str = "fe-browser-actor-runtime";
pub const WEB_ACTOR_RUNTIME_VERSION: u32 = 4;

const WASM_FILE: &str = "module.wasm";
const WGSL_FILE: &str = "shader.wgsl";
const MANIFEST_FILE: &str = "manifest.json";
const INTERFACE_JS_FILE: &str = "interface.js";
const INTERFACE_D_TS_FILE: &str = "interface.d.ts";
const CANONICAL_INTERFACE_JS: &str = include_str!("../assets/canonical-interface.js");
/// Compiler-emitted host page for render bundles. It reads `manifest.json` and
/// drives the two lowerings of the render kernel it describes: `shader.wgsl`
/// via WebGPU, with a per-pixel `module.wasm` fallback. Emitted verbatim next to
/// the bundle so `--mode render` output is directly openable, not hand-authored.
const RENDER_INDEX_FILE: &str = "index.html";
const RENDER_RUNTIME_HTML: &str = include_str!("../assets/render-runtime/index.html");
/// The ONE fixed, versioned, demo-blind render kernel driver (fetch manifest,
/// drive WebGPU via the binding table, fall back to per-pixel wasm, generate
/// uniform controls). `RENDER_INDEX_FILE` imports it as a sibling file; the
/// standards `fe web dev` gallery path publishes this SAME text
/// content-addressed and hands a `data-fe-render` script element to it
/// (crates/html-precompile/assets/bootstrap.js), so both paths share one
/// render runtime instead of maintaining two.
const RENDER_RUNTIME_JS_FILE: &str = "fe-render-runtime.js";
const RENDER_RUNTIME_JS: &str = include_str!("../assets/render-runtime/fe-render-runtime.js");

/// The fixed render runtime module's source text, for hosts (the standards
/// `fe web dev`/`fe web precompile` bundle lane) that publish it
/// content-addressed alongside a render bundle they compile outside
/// [`WebBundle::write_atomic`]/[`WebBundle::materialized_files`].
pub fn render_runtime_js() -> &'static str {
    RENDER_RUNTIME_JS
}
const WEB_ACTOR_RUNTIME: &[(&str, &str)] = &[
    (
        "runtime/actor-coordinator.js",
        include_str!("../assets/browser-runtime/actor-coordinator.js"),
    ),
    (
        "runtime/actor-endpoint.js",
        include_str!("../assets/browser-runtime/actor-endpoint.js"),
    ),
    (
        "runtime/actor-router.js",
        include_str!("../assets/browser-runtime/actor-router.js"),
    ),
    (
        "runtime/gpu-actor.js",
        include_str!("../assets/browser-runtime/gpu-actor.js"),
    ),
    (
        "runtime/message-port-actor.js",
        include_str!("../assets/browser-runtime/message-port-actor.js"),
    ),
    (
        "runtime/module-worker-actor.js",
        include_str!("../assets/browser-runtime/module-worker-actor.js"),
    ),
    (
        "runtime/worker-host.js",
        include_str!("../assets/browser-runtime/worker-host.js"),
    ),
    (
        "runtime/actor-client.js",
        include_str!("../assets/browser-runtime/actor-client.js"),
    ),
];
static NEXT_STAGING_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebBundleMode {
    Render,
    Grid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebCanonicalPolicy {
    Disabled,
    Optional,
    Required,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebProvenance {
    pub compiler: String,
    pub compiler_version: String,
    /// Stable caller-supplied identity, such as an ingot-relative source path
    /// or content digest. No timestamp or ambient Git state is injected.
    pub source_id: Option<String>,
}

impl WebProvenance {
    pub fn new(source_id: Option<String>) -> Self {
        Self {
            compiler: "fe".to_string(),
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            source_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebBuildOptions {
    /// Exact public top-level Fe function to expose as the Wasm/WebGPU entry.
    pub source_entry: String,
    pub mode: WebBundleMode,
    /// Used only by grid mode.
    pub workgroup_size: [u32; 3],
    pub provenance: WebProvenance,
    pub canonical_policy: WebCanonicalPolicy,
    /// Ordered, deduplicated message-lane entries. An empty set means the GPU
    /// source entry is also the sole canonical lane for direct API callers.
    pub canonical_entries: Vec<String>,
}

impl WebBuildOptions {
    pub fn render(source_entry: impl Into<String>, source_id: Option<String>) -> Self {
        Self {
            source_entry: source_entry.into(),
            mode: WebBundleMode::Render,
            workgroup_size: [0, 0, 0],
            provenance: WebProvenance::new(source_id),
            canonical_policy: WebCanonicalPolicy::Disabled,
            canonical_entries: Vec::new(),
        }
    }

    pub fn grid(
        source_entry: impl Into<String>,
        workgroup_size: [u32; 3],
        source_id: Option<String>,
    ) -> Self {
        Self {
            source_entry: source_entry.into(),
            mode: WebBundleMode::Grid,
            workgroup_size,
            provenance: WebProvenance::new(source_id),
            canonical_policy: WebCanonicalPolicy::Disabled,
            canonical_entries: Vec::new(),
        }
    }

    pub fn with_canonical_policy(mut self, policy: WebCanonicalPolicy) -> Self {
        self.canonical_policy = policy;
        self
    }

    pub fn with_canonical_entry(mut self, entry: impl Into<String>) -> Self {
        let entry = entry.into();
        if !self.canonical_entries.contains(&entry) {
            self.canonical_entries.push(entry);
        }
        self
    }

    pub fn with_canonical_entries(
        mut self,
        entries: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        for entry in entries {
            let entry = entry.into();
            if !self.canonical_entries.contains(&entry) {
                self.canonical_entries.push(entry);
            }
        }
        self
    }
}

/// The placement-row marker naming a surface program: `uses (GpuProgram<B>)`.
const GPU_PROGRAM_MARKER: &str = "GpuProgram";
/// The behavior-role marker naming the per-pixel fragment stage.
const FRAGMENT_SURFACE_MARKER: &str = "FragmentSurface";

/// The render entry and mode derived from a module's unique GPU-program actor,
/// or `None` when the module declares no such actor (the pre-actor flag path).
///
/// This is R-A1's zero-config derivation: it reads the `actor` declaration
/// structurally (via [`hir::lower::module_actor_decls`]) and maps the unique
/// `FragmentSurface` behavior to `(entry, WebBundleMode::Render)`. The single
/// `FragmentSurface -> Render` shell-key row is a deliberate, temporary closed
/// recognizer (design 7, edit 4), removed when the role recognizer opens.
pub fn actor_web_entry(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
) -> Result<Option<(String, WebBundleMode)>, WebBundleError> {
    let decls = hir::lower::module_actor_decls(db, top_mod);
    let gpu_actors: Vec<&hir::lower::ActorDecl> = decls
        .iter()
        .filter(|actor| {
            actor
                .row_markers
                .iter()
                .any(|marker| marker == GPU_PROGRAM_MARKER)
        })
        .collect();
    let actor = match gpu_actors.as_slice() {
        [] => return Ok(None),
        [actor] => *actor,
        _ => {
            return Err(WebBundleError::EntryDerivation(format!(
                "module declares {} `GpuProgram` actors; the render entry cannot be derived from more than one",
                gpu_actors.len()
            )));
        }
    };

    let fragment_behaviors: Vec<&hir::lower::ActorBehaviorDecl> = actor
        .behaviors
        .iter()
        .filter(|behavior| {
            behavior
                .role_markers
                .iter()
                .any(|marker| marker == FRAGMENT_SURFACE_MARKER)
        })
        .collect();
    match fragment_behaviors.as_slice() {
        [behavior] => Ok(Some((behavior.name.clone(), WebBundleMode::Render))),
        [] => Err(WebBundleError::EntryDerivation(format!(
            "actor `{}` has a `GpuProgram` row but no `FragmentSurface` behavior to serve as the render entry",
            actor.name
        ))),
        _ => Err(WebBundleError::EntryDerivation(format!(
            "actor `{}` declares {} `FragmentSurface` behaviors; a render program has exactly one",
            actor.name,
            fragment_behaviors.len()
        ))),
    }
}

/// Resolves the render entry and mode, reconciling any explicit `--entry`/
/// `--mode` with the module's `actor` declaration.
///
/// - With an actor present, absent flags are DERIVED, and present flags must
///   MATCH the declaration or this errors naming both sources.
/// - With no actor, the explicit flags are required (today's path, unchanged).
pub fn resolve_web_entry(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    explicit_entry: Option<String>,
    explicit_mode: Option<WebBundleMode>,
) -> Result<(String, WebBundleMode), WebBundleError> {
    match actor_web_entry(db, top_mod)? {
        Some((derived_entry, derived_mode)) => {
            if let Some(entry) = &explicit_entry
                && entry != &derived_entry
            {
                return Err(WebBundleError::EntryDerivation(format!(
                    "explicit --entry `{entry}` contradicts the render entry `{derived_entry}` derived from the actor declaration"
                )));
            }
            if let Some(mode) = explicit_mode
                && mode != derived_mode
            {
                return Err(WebBundleError::EntryDerivation(format!(
                    "explicit --mode `{mode:?}` contradicts the mode `{derived_mode:?}` derived from the actor declaration"
                )));
            }
            Ok((derived_entry, derived_mode))
        }
        None => match (explicit_entry, explicit_mode) {
            (Some(entry), Some(mode)) => Ok((entry, mode)),
            _ => Err(WebBundleError::EntryDerivation(
                "no `actor` declaration to derive from; both --entry and --mode are required"
                    .to_owned(),
            )),
        },
    }
}

/// Projects each render actor's state-field NAME and doc comment onto the
/// uniform binding members those fields flattened into (web-bundle protocol
/// v5, slice L0). A no-op for a non-actor render entry: the members keep the
/// empty name / absent doc that [`WebLayout::from_spirv`] gave them.
///
/// Correspondence. [`hir::lower`]'s `lower_actor_behavior` flattens a
/// behavior's `self` state fields, in declaration order, into positional
/// parameters AFTER the behavior's own explicit params. For a `FragmentSurface`
/// behavior those explicit params are the fragment-position builtins, so the
/// actor fields are exactly the input-binding uniform members and carry the
/// trailing, ascending arg_indices. Field `d` (declaration order) therefore
/// owns the member whose `arg_index` sits `d` positions above the binding's
/// lowest member arg_index; we map by that offset rather than by vec position
/// so the field-to-member correspondence is explicit and order-independent.
fn project_actor_field_metadata(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    source_entry: &str,
    layout: &mut WebLayout,
) {
    let decls = hir::lower::module_actor_decls(db, top_mod);
    // Bind the fields of the actor whose behavior IS this render entry, exactly
    // the unique GpuProgram actor `resolve_web_entry` derived `source_entry`
    // from.
    let Some(actor) = decls.iter().find(|actor| {
        actor
            .row_markers
            .iter()
            .any(|marker| marker == GPU_PROGRAM_MARKER)
            && actor
                .behaviors
                .iter()
                .any(|behavior| behavior.name == source_entry)
    }) else {
        return;
    };
    if actor.fields.is_empty() {
        return;
    }
    for binding in &mut layout.bindings {
        if binding.role != WebBindingRole::Input {
            continue;
        }
        let Some(base) = binding.members.iter().map(|member| member.arg_index).min() else {
            continue;
        };
        for member in &mut binding.members {
            let field_index = (member.arg_index - base) as usize;
            if let Some(field) = actor.fields.get(field_index) {
                member.name = field.name.clone();
                member.doc = field.doc.clone();
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebArtifactManifest {
    pub wasm: String,
    pub wasm_bytes: u64,
    pub wgsl: String,
    pub wgsl_bytes: u64,
    /// Added in web-bundle protocol v3. The serde default keeps compiler tools
    /// able to inspect v2 manifests structurally; consumers must still branch
    /// on `protocol_version` and must not infer generated adapters for v2.
    #[serde(default)]
    pub canonical_adapters: Vec<WebGeneratedArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebGeneratedArtifact {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebBrowserRuntimeManifest {
    pub protocol: String,
    pub protocol_version: u32,
    pub artifacts: Vec<WebGeneratedArtifact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebBindingAccess {
    Read,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebBindingRole {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebBinding {
    pub group: u32,
    pub binding: u32,
    pub name: String,
    pub access: WebBindingAccess,
    pub role: WebBindingRole,
    pub stride: u32,
    pub span: u32,
    pub members: Vec<WebBindingMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebBindingMember {
    /// The actor state field this member projects (empty for a non-actor
    /// render entry). Added in web-bundle protocol v5; the serde default keeps
    /// compiler tooling able to inspect v4 manifests structurally (the v3
    /// `canonical_adapters` precedent), and lets the render runtime label a
    /// control by its real name instead of `scalar @arg_index`.
    #[serde(default)]
    pub name: String,
    /// The projected field's doc comment, when the source declared one. Also v5.
    #[serde(default)]
    pub doc: Option<String>,
    pub arg_index: u32,
    pub offset: u32,
    pub width: u32,
    pub scalar: WebScalarKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebScalarKind {
    I1,
    I32,
    U32,
    I64,
    F32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebBuiltinSource {
    GlobalInvocationIdX,
    GlobalInvocationIdY,
    FragmentPositionX,
    FragmentPositionY,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebBuiltinInput {
    pub arg_index: u32,
    pub source: WebBuiltinSource,
    pub scalar: WebScalarKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebResult {
    pub group: u32,
    pub binding: u32,
    pub offset: u32,
    pub width: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebLayout {
    pub entry_point: String,
    pub mode: WebBundleMode,
    pub workgroup_size: [u32; 3],
    pub word: String,
    pub word_bytes: u32,
    pub bindings: Vec<WebBinding>,
    pub builtin_inputs: Vec<WebBuiltinInput>,
    pub result: Option<WebResult>,
    pub vertex_entry: Option<String>,
    pub fragment_entry: Option<String>,
    pub color_target_format: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebBundleManifest {
    pub protocol: String,
    pub protocol_version: u32,
    pub source_entry: String,
    pub artifacts: WebArtifactManifest,
    pub layout: WebLayout,
    pub provenance: WebProvenance,
    pub canonical_interface: Option<CanonicalInterfaceManifest>,
    pub canonical_status: WebCanonicalStatus,
    /// Present exactly when a canonical browser interface is embedded. These
    /// compiler-owned modules implement its Worker/MessagePort and WebGPU
    /// actor transport; applications provide effect handlers, not wire glue.
    #[serde(default)]
    pub browser_runtime: Option<WebBrowserRuntimeManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebCanonicalStatus {
    pub policy: WebCanonicalPolicy,
    pub embedded: bool,
    pub omission_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebBundle {
    pub wasm: Vec<u8>,
    pub wgsl: String,
    pub manifest: WebBundleManifest,
    pub interface_js: Option<String>,
    pub interface_d_ts: Option<String>,
}

/// One immutable file in a fully materialized [`WebBundle`].
///
/// The path is bundle-relative and validated when the snapshot is built.
/// Shared byte storage makes a snapshot cheap to clone into HTTP request
/// handlers without exposing mutable compiler artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebBundleFile {
    path: String,
    bytes: Arc<[u8]>,
}

impl WebBundleFile {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl WebBundle {
    /// Compile Wasm and browser-profile WGSL from the same resolved module.
    /// Canonical message lanes may be different public entries from the GPU
    /// kernel. Wasm receives the ordered selected root set while WebGPU retains
    /// the exact source entry. Validation is part of construction: an invalid
    /// target can never be represented as a `WebBundle`.
    pub fn compile(
        db: &DriverDataBase,
        top_mod: TopLevelMod<'_>,
        options: WebBuildOptions,
    ) -> Result<Self, WebBundleError> {
        let mut canonical_entries = if options.canonical_entries.is_empty() {
            vec![options.source_entry.clone()]
        } else {
            options.canonical_entries.clone()
        };
        let mut seen_entries = std::collections::BTreeSet::new();
        canonical_entries.retain(|entry| seen_entries.insert(entry.clone()));
        let (canonical_candidate, mut canonical_status) = match options.canonical_policy {
            WebCanonicalPolicy::Disabled => (
                None,
                WebCanonicalStatus {
                    policy: WebCanonicalPolicy::Disabled,
                    embedded: false,
                    omission_reason: Some("canonical interface was not requested".to_owned()),
                },
            ),
            policy @ (WebCanonicalPolicy::Optional | WebCanonicalPolicy::Required) => {
                let derived = canonical_entries
                    .iter()
                    .map(|entry| canonical_lane_decl_from_entry(db, top_mod, entry, entry))
                    .collect::<Result<Vec<_>, _>>()
                    .and_then(CanonicalInterfaceManifest::build);
                match derived {
                    Ok(interface) => (
                        Some(interface),
                        WebCanonicalStatus {
                            policy,
                            embedded: false,
                            omission_reason: None,
                        },
                    ),
                    Err(error) if policy == WebCanonicalPolicy::Optional => (
                        None,
                        WebCanonicalStatus {
                            policy,
                            embedded: false,
                            omission_reason: Some(format!(
                                "semantic canonical derivation unavailable: {error}"
                            )),
                        },
                    ),
                    Err(error) => {
                        return Err(WebBundleError::CanonicalRequired(format!(
                            "semantic canonical derivation failed: {error}"
                        )));
                    }
                }
            }
        };
        let wasm_entries = match canonical_candidate.as_ref() {
            Some(interface) => canonical_entries
                .iter()
                .zip(&interface.lanes)
                .filter(|(_, lane)| lane.intent.execution == crate::CanonicalExecution::Wasm)
                .map(|(entry, _)| entry.clone())
                .collect::<Vec<_>>(),
            None => canonical_entries.clone(),
        };
        if wasm_entries.is_empty() {
            return Err(WebBundleError::CanonicalRequired(
                "canonical bundle requires at least one executable Wasm lane".to_owned(),
            ));
        }
        let wasm_package = mir::build_wasm_runtime_package_for_entries(db, top_mod, &wasm_entries)
            .map_err(|error| WebBundleError::Lower(error.to_string()))?;

        let wasm_options = match options.canonical_policy {
            WebCanonicalPolicy::Disabled => WasmCompileOptions::default(),
            WebCanonicalPolicy::Optional | WebCanonicalPolicy::Required => canonical_candidate
                .as_ref()
                .map(|interface: &CanonicalInterfaceManifest| {
                    interface
                        .lanes
                        .iter()
                        .filter(|lane| lane.intent.execution == crate::CanonicalExecution::Wasm)
                        .cloned()
                        .collect::<Vec<crate::CanonicalLane>>()
                })
                .filter(|lanes| !lanes.is_empty())
                .map(|lanes| WasmCompileOptions::default().with_canonical_lanes(lanes))
                .unwrap_or_else(|| WasmCompileOptions::default().with_canonical_arena()),
        };
        let wasm = compile_runtime_package_wasm_with_options(
            db,
            &wasm_package,
            wasm_options.with_optimization(),
        )
        .map_err(|error| WebBundleError::Lower(error.to_string()))?
        .bytes;
        wasmparser::validate(&wasm)
            .map_err(|error| WebBundleError::WasmValidation(error.to_string()))?;
        let canonical_interface =
            verify_canonical_candidate(&wasm, canonical_candidate, &mut canonical_status)?;
        let (interface_js, interface_d_ts, canonical_adapters) =
            generated_canonical_adapters(canonical_interface.as_ref())?;
        let browser_runtime = generated_browser_runtime(canonical_interface.is_some());

        let gpu_package =
            mir::build_wasm_runtime_package_for_entry(db, top_mod, &options.source_entry)
                .map_err(|error| WebBundleError::Lower(error.to_string()))?;
        let artifact = match options.mode {
            WebBundleMode::Render => compile_runtime_package_spirv_render(db, &gpu_package),
            WebBundleMode::Grid => {
                compile_runtime_package_spirv_grid(db, &gpu_package, options.workgroup_size)
            }
        }
        .map_err(|error| WebBundleError::Lower(error.to_string()))?;
        let wgsl = normalize_generated_text(&artifact.wgsl.ok_or(WebBundleError::MissingWgsl)?);
        validate_browser_wgsl(&wgsl)?;
        let mut layout = WebLayout::from_spirv(&artifact.layout)?;
        project_actor_field_metadata(db, top_mod, &options.source_entry, &mut layout);

        let manifest = WebBundleManifest {
            protocol: WEB_BUNDLE_PROTOCOL.to_string(),
            protocol_version: WEB_BUNDLE_PROTOCOL_VERSION,
            source_entry: options.source_entry,
            artifacts: WebArtifactManifest {
                wasm: WASM_FILE.to_string(),
                wasm_bytes: wasm.len() as u64,
                wgsl: WGSL_FILE.to_string(),
                wgsl_bytes: wgsl.len() as u64,
                canonical_adapters,
            },
            layout,
            provenance: options.provenance,
            canonical_interface,
            canonical_status,
            browser_runtime,
        };
        Ok(Self {
            wasm,
            wgsl,
            manifest,
            interface_js,
            interface_d_ts,
        })
    }

    pub fn manifest_json(&self) -> Result<Vec<u8>, WebBundleError> {
        let mut json = serde_json::to_vec_pretty(&self.manifest)
            .map_err(|error| WebBundleError::Manifest(error.to_string()))?;
        json.push(b'\n');
        Ok(json)
    }

    /// Materialize the exact, immutable file set described by this bundle's
    /// manifest. Both disk publication and browser servers consume this API so
    /// generated adapters cannot silently diverge between the two paths.
    pub fn materialized_files(&self) -> Result<Vec<WebBundleFile>, WebBundleError> {
        let runtime_artifact_count = self
            .manifest
            .browser_runtime
            .as_ref()
            .map_or(0, |runtime| runtime.artifacts.len());
        let mut files = Vec::with_capacity(
            3 + self.manifest.artifacts.canonical_adapters.len() + runtime_artifact_count,
        );
        let mut paths = std::collections::BTreeSet::new();
        let mut push = |path: &str, bytes: Arc<[u8]>| -> Result<(), WebBundleError> {
            validate_materialized_path(path)?;
            if !paths.insert(path.to_owned()) {
                return Err(WebBundleError::Materialization(format!(
                    "duplicate artifact path `{path}`"
                )));
            }
            files.push(WebBundleFile {
                path: path.to_owned(),
                bytes,
            });
            Ok(())
        };

        if self.wasm.len() as u64 != self.manifest.artifacts.wasm_bytes {
            return Err(WebBundleError::Materialization(format!(
                "artifact `{}` does not match its manifest byte length",
                self.manifest.artifacts.wasm
            )));
        }
        if self.wgsl.len() as u64 != self.manifest.artifacts.wgsl_bytes {
            return Err(WebBundleError::Materialization(format!(
                "artifact `{}` does not match its manifest byte length",
                self.manifest.artifacts.wgsl
            )));
        }
        push(
            &self.manifest.artifacts.wasm,
            Arc::from(self.wasm.as_slice()),
        )?;
        push(
            &self.manifest.artifacts.wgsl,
            Arc::from(self.wgsl.as_bytes()),
        )?;
        for artifact in &self.manifest.artifacts.canonical_adapters {
            let bytes: Arc<[u8]> = match artifact.path.as_str() {
                INTERFACE_JS_FILE => self
                    .interface_js
                    .as_ref()
                    .map(|source| Arc::from(source.as_bytes()))
                    .ok_or_else(|| {
                        WebBundleError::Materialization(format!(
                            "manifest declares `{}` but its content is absent",
                            artifact.path
                        ))
                    })?,
                INTERFACE_D_TS_FILE => self
                    .interface_d_ts
                    .as_ref()
                    .map(|source| Arc::from(source.as_bytes()))
                    .ok_or_else(|| {
                        WebBundleError::Materialization(format!(
                            "manifest declares `{}` but its content is absent",
                            artifact.path
                        ))
                    })?,
                path => {
                    return Err(WebBundleError::Materialization(format!(
                        "manifest declares unsupported generated artifact `{path}`"
                    )));
                }
            };
            if bytes.len() as u64 != artifact.bytes
                || hex::encode(Sha256::digest(bytes.as_ref())) != artifact.sha256
            {
                return Err(WebBundleError::Materialization(format!(
                    "generated artifact `{}` does not match its manifest metadata",
                    artifact.path
                )));
            }
            push(&artifact.path, bytes)?;
        }
        if let Some(runtime) = &self.manifest.browser_runtime {
            if runtime.protocol != WEB_ACTOR_RUNTIME_PROTOCOL
                || runtime.protocol_version != WEB_ACTOR_RUNTIME_VERSION
            {
                return Err(WebBundleError::Materialization(
                    "browser actor runtime protocol metadata is unsupported".to_owned(),
                ));
            }
            for artifact in &runtime.artifacts {
                let source = WEB_ACTOR_RUNTIME
                    .iter()
                    .find_map(|(path, source)| (*path == artifact.path).then_some(*source))
                    .ok_or_else(|| {
                        WebBundleError::Materialization(format!(
                            "manifest declares unsupported browser runtime artifact `{}`",
                            artifact.path
                        ))
                    })?;
                if source.len() as u64 != artifact.bytes
                    || hex::encode(Sha256::digest(source.as_bytes())) != artifact.sha256
                {
                    return Err(WebBundleError::Materialization(format!(
                        "browser runtime artifact `{}` does not match its manifest metadata",
                        artifact.path
                    )));
                }
                push(&artifact.path, Arc::from(source.as_bytes()))?;
            }
        }
        push(MANIFEST_FILE, Arc::from(self.manifest_json()?))?;
        // Render bundles ship a compiler-emitted host page so the directory is
        // directly openable (WebGPU shader.wgsl, with a wasm per-pixel fallback),
        // plus the one shipped render runtime module that page imports.
        if self.manifest.layout.mode == WebBundleMode::Render {
            push(
                RENDER_RUNTIME_JS_FILE,
                Arc::from(RENDER_RUNTIME_JS.as_bytes()),
            )?;
            push(RENDER_INDEX_FILE, Arc::from(RENDER_RUNTIME_HTML.as_bytes()))?;
        }
        Ok(files)
    }

    /// Return the compiler-owned browser actor modules for generators that
    /// embed a `WebBundle` inside a larger application-specific artifact set.
    /// The returned paths remain bundle-relative (`runtime/*.js`) and have
    /// already passed the same manifest/hash checks as atomic publication.
    pub fn browser_runtime_files(&self) -> Result<Vec<WebBundleFile>, WebBundleError> {
        let runtime_paths = self
            .manifest
            .browser_runtime
            .as_ref()
            .map(|runtime| {
                runtime
                    .artifacts
                    .iter()
                    .map(|artifact| artifact.path.as_str())
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        Ok(self
            .materialized_files()?
            .into_iter()
            .filter(|file| runtime_paths.contains(file.path()))
            .collect())
    }

    /// Publish a complete new bundle directory with one filesystem rename.
    /// Existing destinations are rejected so readers never observe a missing
    /// or mixed-version directory. A future CLI can publish versioned paths and
    /// atomically switch its own pointer when replacement semantics are needed.
    pub fn write_atomic(&self, destination: impl AsRef<Path>) -> Result<(), WebBundleError> {
        let destination = destination.as_ref();
        if destination.exists() {
            return Err(WebBundleError::DestinationExists(destination.to_path_buf()));
        }
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let leaf = destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("bundle");
        let staging = parent.join(format!(
            ".{leaf}.fe-staging-{}-{}",
            std::process::id(),
            NEXT_STAGING_ID.fetch_add(1, Ordering::Relaxed)
        ));

        let result = (|| {
            fs::create_dir(&staging)?;
            for file in self.materialized_files()? {
                let path = staging.join(file.path());
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                write_synced(&path, file.bytes())?;
            }
            fs::rename(&staging, destination)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }
}

fn validate_materialized_path(path: &str) -> Result<(), WebBundleError> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(WebBundleError::Materialization(format!(
            "artifact path `{}` is not a safe bundle-relative path",
            path.display()
        )));
    }
    Ok(())
}

fn generated_canonical_adapters(
    interface: Option<&CanonicalInterfaceManifest>,
) -> Result<(Option<String>, Option<String>, Vec<WebGeneratedArtifact>), WebBundleError> {
    let Some(interface) = interface else {
        return Ok((None, None, Vec::new()));
    };
    let manifest_json = serde_json::to_string(interface)
        .map_err(|error| WebBundleError::Manifest(error.to_string()))?;
    let interface_js = format!(
        "{CANONICAL_INTERFACE_JS}\n\
         export const canonicalInterfaceManifest = Object.freeze({manifest_json});\n\
         export const compiledCanonicalInterface = \
         compileCanonicalInterfaceManifest(canonicalInterfaceManifest);\n\
         export function createInterfaceCaller(exports) {{\n  \
         return createCanonicalInterfaceCaller(compiledCanonicalInterface, exports);\n}}\n\
         export function compileActorAdapter() {{\n  \
         return compileCanonicalActorAdapter(canonicalInterfaceManifest, compiledCanonicalInterface);\n}}\n\
         export function createActorAdapter(exports, options) {{\n  \
         return createCanonicalActorAdapter(canonicalInterfaceManifest, compiledCanonicalInterface, exports, options);\n}}\n\
         export function createHostEffectAdapter(handlers, options) {{\n  \
         return createCanonicalHostEffectAdapter(canonicalInterfaceManifest, compiledCanonicalInterface, handlers, options);\n}}\n"
    );
    let interface_d_ts = canonical_interface_declarations(interface)?;
    let artifact = |path: &str, content: &str| WebGeneratedArtifact {
        path: path.to_owned(),
        bytes: content.len() as u64,
        sha256: hex::encode(Sha256::digest(content.as_bytes())),
    };
    let artifacts = vec![
        artifact(INTERFACE_JS_FILE, &interface_js),
        artifact(INTERFACE_D_TS_FILE, &interface_d_ts),
    ];
    Ok((Some(interface_js), Some(interface_d_ts), artifacts))
}

fn generated_browser_runtime(enabled: bool) -> Option<WebBrowserRuntimeManifest> {
    enabled.then(|| WebBrowserRuntimeManifest {
        protocol: WEB_ACTOR_RUNTIME_PROTOCOL.to_owned(),
        protocol_version: WEB_ACTOR_RUNTIME_VERSION,
        artifacts: WEB_ACTOR_RUNTIME
            .iter()
            .map(|(path, source)| WebGeneratedArtifact {
                path: (*path).to_owned(),
                bytes: source.len() as u64,
                sha256: hex::encode(Sha256::digest(source.as_bytes())),
            })
            .collect(),
    })
}

fn canonical_type_name(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn canonical_interface_declarations(
    interface: &CanonicalInterfaceManifest,
) -> Result<String, WebBundleError> {
    fn ty(layout: &crate::CanonicalLayout, indent: usize) -> String {
        match &layout.shape {
            crate::CanonicalShape::Bool => "boolean".to_owned(),
            crate::CanonicalShape::U8
            | crate::CanonicalShape::I32
            | crate::CanonicalShape::U32
            | crate::CanonicalShape::F32 => "number".to_owned(),
            crate::CanonicalShape::I64 | crate::CanonicalShape::U64 => "bigint".to_owned(),
            crate::CanonicalShape::Bytes { .. } => "Uint8Array".to_owned(),
            crate::CanonicalShape::String { .. } => "string".to_owned(),
            crate::CanonicalShape::List { element, .. } => match element {
                crate::CanonicalListElement::U32 => "Uint32Array".to_owned(),
                crate::CanonicalListElement::F32 => "Float32Array".to_owned(),
            },
            crate::CanonicalShape::Record { fields } => {
                let padding = " ".repeat(indent);
                let field_padding = " ".repeat(indent + 2);
                let fields = fields
                    .iter()
                    .map(|field| {
                        format!(
                            "{field_padding}{}: {};",
                            field.name,
                            ty(&field.layout, indent + 2)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{{\n{fields}\n{padding}}}")
            }
            crate::CanonicalShape::Variant { variants, .. } => variants
                .iter()
                .map(|variant| {
                    let padding = " ".repeat(indent);
                    let field_padding = " ".repeat(indent + 2);
                    let mut fields =
                        vec![format!("{field_padding}readonly tag: {:?};", variant.name)];
                    fields.extend(variant.fields.iter().map(|field| {
                        format!(
                            "{field_padding}{}: {};",
                            field.name,
                            ty(&field.layout, indent + 2)
                        )
                    }));
                    format!("{{\n{}\n{padding}}}", fields.join("\n"))
                })
                .collect::<Vec<_>>()
                .join(" | "),
        }
    }

    let mut output = String::from(
        "export declare const canonicalInterfaceManifest: Readonly<object>;\n\
         export declare const compiledCanonicalInterface: Readonly<object>;\n\n",
    );
    let mut type_names = std::collections::BTreeMap::new();
    for lane in &interface.lanes {
        let name = canonical_type_name(&lane.name);
        if let Some(previous) = type_names.insert(name.clone(), lane.name.clone()) {
            return Err(WebBundleError::Manifest(format!(
                "canonical lanes `{previous}` and `{}` collide as generated TypeScript name `{name}`",
                lane.name
            )));
        }
        output.push_str(&format!(
            "export type {name}Request = {};\n\
             export type {name}Response = {};\n\n",
            ty(&lane.request, 0),
            ty(&lane.response, 0),
        ));
    }
    output.push_str("export interface CanonicalInterfaceCaller {\n");
    for lane in &interface.lanes {
        let name = canonical_type_name(&lane.name);
        output.push_str(&format!(
            "  call(lane: {:?}, value: {name}Request): Promise<{name}Response>;\n",
            lane.name
        ));
    }
    output.push_str(
        "}\n\nexport declare function createInterfaceCaller(\n  \
         exports: WebAssembly.Exports,\n): CanonicalInterfaceCaller;\n",
    );
    output.push_str(
        "\nexport interface CanonicalActorRequest<Lane extends string, Payload> {\n  \
         lane: Lane;\n  payload: Payload;\n}\n\
         export interface CanonicalActorContext {\n  \
         readonly signal: AbortSignal;\n}\n\
         export interface CanonicalActorShape {\n  \
         readonly requestSchema: Readonly<Record<string, (value: unknown) => void>>;\n  \
         readonly resultSchema: Readonly<Record<string, (value: unknown) => void>>;\n  \
         transferRequest(value: unknown, request: { lane: string }): ArrayBuffer[];\n  \
         transferResult(value: unknown, request: { lane: string }): ArrayBuffer[];\n\
         }\n\
         export interface CanonicalActorAdapter extends CanonicalActorShape {\n",
    );
    for lane in &interface.lanes {
        let name = canonical_type_name(&lane.name);
        output.push_str(&format!(
            "  dispatch(request: CanonicalActorRequest<{:?}, {name}Request>, context?: CanonicalActorContext): Promise<{name}Response>;\n",
            lane.name
        ));
    }
    output.push_str(
        "}\n\n\
         export interface CanonicalHostEffectHandlers {\n",
    );
    for lane in &interface.lanes {
        let name = canonical_type_name(&lane.name);
        output.push_str(&format!(
            "  {:?}?: (request: {name}Request, context: CanonicalActorContext) => {name}Response | PromiseLike<{name}Response>;\n",
            lane.name
        ));
    }
    output.push_str(
        "}\n\
         export declare function compileActorAdapter(): CanonicalActorShape;\n\
         export declare function createActorAdapter(\n  \
         exports: WebAssembly.Exports,\n  \
         options?: { maxPendingPerLane?: number },\n\
         ): CanonicalActorAdapter;\n\
         export declare function createHostEffectAdapter(\n  \
         handlers: CanonicalHostEffectHandlers,\n  \
         options?: { maxPendingPerLane?: number },\n\
         ): CanonicalActorAdapter;\n",
    );
    Ok(output)
}

fn verify_canonical_candidate(
    wasm: &[u8],
    candidate: Option<CanonicalInterfaceManifest>,
    status: &mut WebCanonicalStatus,
) -> Result<Option<CanonicalInterfaceManifest>, WebBundleError> {
    let Some(interface) = candidate else {
        return Ok(None);
    };
    match verify_canonical_wasm_abi(wasm, &interface) {
        Ok(()) => {
            status.embedded = true;
            Ok(Some(interface))
        }
        Err(error) if status.policy == WebCanonicalPolicy::Optional => {
            status.omission_reason = Some(format!(
                "emitted canonical ABI verification failed: {error}"
            ));
            Ok(None)
        }
        Err(error) => Err(WebBundleError::CanonicalRequired(format!(
            "emitted canonical ABI verification failed: {error}"
        ))),
    }
}

impl WebLayout {
    fn from_spirv(layout: &SpirvLayout) -> Result<Self, WebBundleError> {
        let mode = match layout.mode {
            LayoutMode::Render => WebBundleMode::Render,
            LayoutMode::Grid => WebBundleMode::Grid,
            other => return Err(WebBundleError::UnexpectedLayout(format!("{other:?}"))),
        };
        let word = match layout.word {
            WordKind::U32 => "u32",
            WordKind::I64 => "i64",
        };
        Ok(Self {
            entry_point: layout.entry_point.clone(),
            mode,
            workgroup_size: layout.workgroup_size,
            word: word.to_string(),
            word_bytes: layout.word.width_bytes(),
            bindings: layout
                .bindings
                .iter()
                .map(|binding| WebBinding {
                    group: binding.group,
                    binding: binding.binding,
                    name: binding.name.clone(),
                    access: match binding.access {
                        Access::Read => WebBindingAccess::Read,
                        Access::ReadWrite => WebBindingAccess::ReadWrite,
                    },
                    role: match binding.role {
                        Role::Input => WebBindingRole::Input,
                        Role::Output => WebBindingRole::Output,
                    },
                    stride: binding.stride,
                    span: binding.span,
                    members: binding
                        .members
                        .iter()
                        .map(|member| WebBindingMember {
                            // Names/docs are projected from the actor field
                            // declarations after the layout is built (protocol
                            // v5, `project_actor_field_metadata`); the SPIR-V
                            // layout itself carries no source-level names.
                            name: String::new(),
                            doc: None,
                            arg_index: member.arg_index,
                            offset: member.offset,
                            width: member.width,
                            scalar: scalar_kind(member.scalar),
                        })
                        .collect(),
                })
                .collect(),
            builtin_inputs: layout
                .builtin_inputs
                .iter()
                .map(|input| WebBuiltinInput {
                    arg_index: input.arg_index,
                    source: match input.source {
                        SpirvBuiltinSource::GlobalInvocationIdX => {
                            WebBuiltinSource::GlobalInvocationIdX
                        }
                        SpirvBuiltinSource::GlobalInvocationIdY => {
                            WebBuiltinSource::GlobalInvocationIdY
                        }
                        SpirvBuiltinSource::FragmentPositionX => {
                            WebBuiltinSource::FragmentPositionX
                        }
                        SpirvBuiltinSource::FragmentPositionY => {
                            WebBuiltinSource::FragmentPositionY
                        }
                    },
                    scalar: scalar_kind(input.scalar),
                })
                .collect(),
            result: layout.result.map(|result| WebResult {
                group: result.group,
                binding: result.binding,
                offset: result.offset,
                width: result.width,
            }),
            vertex_entry: layout.vertex_entry.clone(),
            fragment_entry: layout.fragment_entry.clone(),
            color_target_format: layout.color_target_format.clone(),
        })
    }
}

fn scalar_kind(kind: SpirvScalarKind) -> WebScalarKind {
    match kind {
        SpirvScalarKind::I1 => WebScalarKind::I1,
        SpirvScalarKind::I32 => WebScalarKind::I32,
        SpirvScalarKind::U32 => WebScalarKind::U32,
        SpirvScalarKind::I64 => WebScalarKind::I64,
        SpirvScalarKind::F32 => WebScalarKind::F32,
    }
}

fn validate_browser_wgsl(wgsl: &str) -> Result<(), WebBundleError> {
    let module = naga::front::wgsl::parse_str(wgsl)
        .map_err(|error| WebBundleError::WgslParse(error.emit_to_string(wgsl)))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .map_err(|error| WebBundleError::WgslValidation(error.to_string()))?;
    Ok(())
}

fn normalize_generated_text(source: &str) -> String {
    let mut normalized = source
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    if source.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), WebBundleError> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

#[derive(Debug)]
pub enum WebBundleError {
    Lower(String),
    Wasm(String),
    WasmValidation(String),
    MissingWgsl,
    WgslParse(String),
    WgslValidation(String),
    UnexpectedLayout(String),
    CanonicalRequired(String),
    Manifest(String),
    Materialization(String),
    /// The `--entry`/`--mode` could not be derived from (or contradict) the
    /// module's `actor` declaration.
    EntryDerivation(String),
    DestinationExists(PathBuf),
    Io(io::Error),
}

impl fmt::Display for WebBundleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lower(error) => write!(f, "web bundle lowering failed: {error}"),
            Self::Wasm(error) => write!(f, "web bundle Wasm emission failed: {error}"),
            Self::WasmValidation(error) => write!(f, "web bundle Wasm validation failed: {error}"),
            Self::MissingWgsl => write!(f, "SPIR-V backend did not emit WGSL"),
            Self::WgslParse(error) => write!(f, "emitted WGSL did not reparse: {error}"),
            Self::WgslValidation(error) => {
                write!(f, "emitted WGSL failed browser-profile validation: {error}")
            }
            Self::UnexpectedLayout(mode) => {
                write!(
                    f,
                    "web bundle received unsupported SPIR-V layout mode `{mode}`"
                )
            }
            Self::CanonicalRequired(error) => {
                write!(f, "required canonical interface is unavailable: {error}")
            }
            Self::Manifest(error) => write!(f, "web bundle manifest serialization failed: {error}"),
            Self::Materialization(error) => {
                write!(f, "web bundle materialization failed: {error}")
            }
            Self::EntryDerivation(error) => {
                write!(f, "web entry derivation failed: {error}")
            }
            Self::DestinationExists(path) => {
                write!(
                    f,
                    "web bundle destination already exists: {}",
                    path.display()
                )
            }
            Self::Io(error) => write!(f, "web bundle I/O failed: {error}"),
        }
    }
}

impl std::error::Error for WebBundleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for WebBundleError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::InputDb;
    use url::Url;

    const SOURCE: &str = r#"
pub fn ignored(x: u32, y: u32) -> u32 {
    1 + x + y
}

pub fn shade(x: u32, y: u32) -> u32 {
    4278190080 + x * 65536 + y * 256
}
"#;

    const CANONICAL_SOURCE: &str = r#"
struct Request { value: u32 }
struct Response { value: u32 }
struct VerifyRequest { sample: i32 }
struct VerifyResponse { accepted: bool }

pub fn update(request: Request) -> Response {
    Response { value: request.value + 1 }
}

pub fn verify(request: VerifyRequest) -> VerifyResponse {
    VerifyResponse { accepted: request.sample == 7 }
}

pub fn shade(x: u32, y: u32) -> u32 {
    x + y
}
"#;

    fn wasm_exports(wasm: &[u8]) -> Vec<String> {
        let mut exports = Vec::new();
        for payload in wasmparser::Parser::new(0).parse_all(wasm) {
            if let wasmparser::Payload::ExportSection(reader) = payload.unwrap() {
                exports.extend(
                    reader
                        .into_iter()
                        .map(|export| export.unwrap().name.to_owned()),
                );
            }
        }
        exports
    }

    fn compile(mode: WebBundleMode) -> WebBundle {
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///web_bundle.fe").unwrap();
        db.workspace()
            .touch(&mut db, url.clone(), Some(SOURCE.to_string()));
        let file = db.workspace().get(&db, &url).unwrap();
        let options = match mode {
            WebBundleMode::Render => {
                WebBuildOptions::render("shade", Some("web_bundle.fe".to_string()))
            }
            WebBundleMode::Grid => {
                WebBuildOptions::grid("shade", [8, 4, 1], Some("web_bundle.fe".to_string()))
            }
        };
        WebBundle::compile(&db, db.top_mod(file), options).unwrap()
    }

    fn compile_canonical_bundle() -> WebBundle {
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///web_bundle_canonical_runtime.fe").unwrap();
        db.workspace()
            .touch(&mut db, url.clone(), Some(CANONICAL_SOURCE.to_string()));
        let file = db.workspace().get(&db, &url).unwrap();
        WebBundle::compile(
            &db,
            db.top_mod(file),
            WebBuildOptions::render("shade", None)
                .with_canonical_entries(["verify", "update"])
                .with_canonical_policy(WebCanonicalPolicy::Required),
        )
        .unwrap()
    }

    #[test]
    fn render_bundle_is_valid_typed_and_deterministic() {
        let first = compile(WebBundleMode::Render);
        let second = compile(WebBundleMode::Render);
        assert_eq!(first, second);
        assert_eq!(first.manifest.protocol, WEB_BUNDLE_PROTOCOL);
        assert_eq!(first.manifest.protocol_version, WEB_BUNDLE_PROTOCOL_VERSION);
        assert_eq!(first.manifest.source_entry, "shade");
        assert_eq!(first.manifest.layout.mode, WebBundleMode::Render);
        assert_eq!(
            first.manifest.canonical_status.policy,
            WebCanonicalPolicy::Disabled
        );
        assert!(first.manifest.canonical_interface.is_none());
        assert!(first.manifest.artifacts.canonical_adapters.is_empty());
        assert!(first.manifest.browser_runtime.is_none());
        assert!(first.interface_js.is_none());
        assert!(first.interface_d_ts.is_none());
        let exports = wasm_exports(&first.wasm);
        assert!(!exports.iter().any(|name| name == "fe_cabi_alloc"));
        assert!(!exports.iter().any(|name| name == "fe_cabi_reset"));
        assert!(first.manifest.layout.vertex_entry.is_some());
        assert!(first.manifest.layout.fragment_entry.is_some());
        assert!(
            first.wgsl.lines().all(|line| line == line.trim_end()),
            "materialized WebBundle text must not contain trailing whitespace"
        );
        let decoded: WebBundleManifest =
            serde_json::from_slice(&first.manifest_json().unwrap()).unwrap();
        assert_eq!(decoded, first.manifest);

        // V3 adds generated adapter metadata and V4 adds the compiler-owned
        // browser actor runtime. A compiler tool may still inspect a V2
        // manifest, but the retained version keeps that boundary explicit.
        let mut legacy = serde_json::to_value(&first.manifest).unwrap();
        legacy["protocol_version"] = serde_json::json!(2);
        legacy["artifacts"]
            .as_object_mut()
            .unwrap()
            .remove("canonical_adapters");
        legacy.as_object_mut().unwrap().remove("browser_runtime");
        let legacy: WebBundleManifest = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.protocol_version, 2);
        assert!(legacy.artifacts.canonical_adapters.is_empty());
        assert!(legacy.browser_runtime.is_none());
    }

    #[test]
    fn canonical_policy_is_fail_closed_and_records_optional_omission() {
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///web_bundle_canonical.fe").unwrap();
        db.workspace()
            .touch(&mut db, url.clone(), Some(CANONICAL_SOURCE.to_string()));
        let file = db.workspace().get(&db, &url).unwrap();
        let top_mod = db.top_mod(file);
        let candidate = CanonicalInterfaceManifest::build(vec![
            canonical_lane_decl_from_entry(&db, top_mod, "update", "update").unwrap(),
        ])
        .unwrap();
        let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "update").unwrap();
        let lane = candidate.lanes[0].clone();
        let wasm = compile_runtime_package_wasm_with_options(
            &db,
            &package,
            WasmCompileOptions::default().with_canonical_lane(lane),
        )
        .unwrap()
        .bytes;
        let mut required_status = WebCanonicalStatus {
            policy: WebCanonicalPolicy::Required,
            embedded: false,
            omission_reason: None,
        };
        let verified =
            verify_canonical_candidate(&wasm, Some(candidate), &mut required_status).unwrap();
        assert!(verified.is_some());
        assert!(required_status.embedded);
        assert!(required_status.omission_reason.is_none());
        let mut exports = wasm_exports(&wasm);
        exports.sort();
        assert_eq!(
            exports,
            ["fe_cabi_alloc", "fe_cabi_reset", "fe_cabi_update", "memory"],
            "canonical wrapper and arena own the host ABI; the typed Fe lane stays private",
        );

        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &wasm).unwrap();
        let mut store = wasmtime::Store::new(&engine, ());
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        let alloc = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
            .unwrap();
        let update = instance
            .get_typed_func::<i32, i32>(&mut store, "fe_cabi_update")
            .unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();
        // Deliberately leave the arena cursor misaligned before the wrapper's
        // MemAllocDynamic allocation.
        assert_eq!(alloc.call(&mut store, (1, 1)).unwrap(), 1024);
        memory.write(&mut store, 64, &41_u32.to_le_bytes()).unwrap();
        let response_ptr = update.call(&mut store, 64).unwrap();
        assert_eq!(response_ptr % 4, 0);
        let mut response = [0; 4];
        memory
            .read(&store, response_ptr as usize, &mut response)
            .unwrap();
        assert_eq!(u32::from_le_bytes(response), 42);

        let required = WebBundle::compile(
            &db,
            top_mod,
            WebBuildOptions::render("shade", None)
                .with_canonical_entries(["verify", "update", "verify"])
                .with_canonical_policy(WebCanonicalPolicy::Required),
        )
        .unwrap();
        assert!(required.manifest.canonical_interface.is_some());
        assert!(required.manifest.canonical_status.embedded);
        let interface_js = required.interface_js.as_ref().unwrap();
        let interface_d_ts = required.interface_d_ts.as_ref().unwrap();
        assert!(interface_js.contains("createInterfaceCaller"));
        assert!(interface_js.contains("compileActorAdapter"));
        assert!(interface_js.contains("createActorAdapter"));
        assert!(interface_js.contains("createHostEffectAdapter"));
        assert!(interface_js.contains("createCanonicalHostEffectAdapter"));
        assert!(interface_js.contains("FE_ACTOR_SUPERSEDED"));
        assert!(interface_js.contains("canonicalInterfaceManifest"));
        assert!(interface_d_ts.contains("export type UpdateRequest"));
        assert!(interface_d_ts.contains("export type UpdateResponse"));
        assert!(interface_d_ts.contains("export type VerifyRequest"));
        assert!(interface_d_ts.contains("export type VerifyResponse"));
        assert!(interface_d_ts.contains(
            "dispatch(request: CanonicalActorRequest<\"update\", UpdateRequest>, \
             context?: CanonicalActorContext): \
             Promise<UpdateResponse>"
        ));
        assert!(interface_d_ts.contains(
            "dispatch(request: CanonicalActorRequest<\"verify\", VerifyRequest>, \
             context?: CanonicalActorContext): \
             Promise<VerifyResponse>"
        ));
        assert!(interface_d_ts.contains(
            "\"verify\"?: (request: VerifyRequest, context: CanonicalActorContext) => \
             VerifyResponse | \
             PromiseLike<VerifyResponse>"
        ));
        assert!(interface_d_ts.contains(
            "\"update\"?: (request: UpdateRequest, context: CanonicalActorContext) => \
             UpdateResponse | \
             PromiseLike<UpdateResponse>"
        ));
        assert!(interface_d_ts.contains("createHostEffectAdapter"));
        let interface = required.manifest.canonical_interface.as_ref().unwrap();
        assert_eq!(
            interface
                .lanes
                .iter()
                .map(|lane| lane.name.as_str())
                .collect::<Vec<_>>(),
            ["verify", "update"],
            "repeatable canonical entries preserve first occurrence order"
        );
        let mut exports = wasm_exports(&required.wasm);
        exports.sort();
        assert_eq!(
            exports,
            [
                "fe_cabi_alloc",
                "fe_cabi_reset",
                "fe_cabi_update",
                "fe_cabi_verify",
                "memory",
            ],
            "only canonical wrappers, arena, and memory belong to the actor Wasm ABI",
        );
        let module = wasmtime::Module::new(&engine, &required.wasm).unwrap();
        let mut store = wasmtime::Store::new(&engine, ());
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        let alloc = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
            .unwrap();
        let reset = instance
            .get_typed_func::<(), ()>(&mut store, "fe_cabi_reset")
            .unwrap();
        let verify = instance
            .get_typed_func::<i32, i32>(&mut store, "fe_cabi_verify")
            .unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();
        for (sample, expected) in [(7_i32, 1_u8), (8_i32, 0_u8)] {
            reset.call(&mut store, ()).unwrap();
            let request = alloc.call(&mut store, (4, 4)).unwrap();
            memory
                .write(&mut store, request as usize, &sample.to_le_bytes())
                .unwrap();
            let response = verify.call(&mut store, request).unwrap();
            let mut accepted = [0_u8; 1];
            memory
                .read(&store, response as usize, &mut accepted)
                .unwrap();
            assert_eq!(accepted[0], expected, "canonical bool response");
        }
        assert_eq!(
            required
                .manifest
                .artifacts
                .canonical_adapters
                .iter()
                .map(|artifact| artifact.path.as_str())
                .collect::<Vec<_>>(),
            [INTERFACE_JS_FILE, INTERFACE_D_TS_FILE]
        );
        let runtime = required
            .manifest
            .browser_runtime
            .as_ref()
            .expect("canonical WebBundle packages its browser actor runtime");
        assert_eq!(runtime.protocol, WEB_ACTOR_RUNTIME_PROTOCOL);
        assert_eq!(runtime.protocol_version, WEB_ACTOR_RUNTIME_VERSION);
        assert_eq!(runtime.artifacts.len(), WEB_ACTOR_RUNTIME.len());
        assert_eq!(
            runtime
                .artifacts
                .iter()
                .map(|artifact| artifact.path.as_str())
                .collect::<Vec<_>>(),
            WEB_ACTOR_RUNTIME
                .iter()
                .map(|(path, _)| *path)
                .collect::<Vec<_>>()
        );
        for (artifact, content) in required
            .manifest
            .artifacts
            .canonical_adapters
            .iter()
            .zip([interface_js.as_bytes(), interface_d_ts.as_bytes()])
        {
            assert_eq!(artifact.bytes, content.len() as u64);
            assert_eq!(artifact.sha256, hex::encode(Sha256::digest(content)));
        }
        let materialized = required.materialized_files().unwrap();
        assert_eq!(
            materialized
                .iter()
                .map(WebBundleFile::path)
                .collect::<Vec<_>>(),
            [
                WASM_FILE,
                WGSL_FILE,
                INTERFACE_JS_FILE,
                INTERFACE_D_TS_FILE,
                "runtime/actor-coordinator.js",
                "runtime/actor-endpoint.js",
                "runtime/actor-router.js",
                "runtime/gpu-actor.js",
                "runtime/message-port-actor.js",
                "runtime/module-worker-actor.js",
                "runtime/worker-host.js",
                "runtime/actor-client.js",
                MANIFEST_FILE,
                RENDER_RUNTIME_JS_FILE,
                RENDER_INDEX_FILE,
            ]
        );
        assert_eq!(
            materialized
                .iter()
                .find(|file| file.path() == INTERFACE_JS_FILE)
                .unwrap()
                .bytes(),
            interface_js.as_bytes()
        );
        for (path, source) in WEB_ACTOR_RUNTIME {
            assert_eq!(
                materialized
                    .iter()
                    .find(|file| file.path() == *path)
                    .unwrap()
                    .bytes(),
                source.as_bytes()
            );
        }
        let actor_router = WEB_ACTOR_RUNTIME
            .iter()
            .find_map(|(path, source)| (*path == "runtime/actor-router.js").then_some(*source))
            .unwrap();
        assert!(actor_router.contains("FE_ACTOR_CONFLICTING_CAPABILITY_OWNER"));
        assert!(actor_router.contains("FE_ACTOR_CONFLICTING_CAPABILITY_CLAIM"));
        assert!(actor_router.contains("FE_ACTOR_CONFLICTING_LANE_DESCRIPTORS"));
        let worker_host = WEB_ACTOR_RUNTIME
            .iter()
            .find_map(|(path, source)| (*path == "runtime/worker-host.js").then_some(*source))
            .unwrap();
        assert!(worker_host.contains("createCanonicalIntentRouter"));
        assert!(worker_host.contains("adapter.intents"));
        assert!(!worker_host.contains("lanes: ["));
        assert!(!worker_host.contains("[\"render\", \"verify\"]"));
        let actor_client = WEB_ACTOR_RUNTIME
            .iter()
            .find_map(|(path, source)| (*path == "runtime/actor-client.js").then_some(*source))
            .unwrap();
        assert!(actor_client.contains("createCanonicalBrowserActor"));
        assert!(actor_client.contains("createCanonicalMainThreadGpuBroker"));
        assert!(actor_client.contains("createCanonicalModuleWorkerActor"));
        assert!(!actor_client.contains("actorEnvelope"));
        let destination = std::env::temp_dir().join(format!(
            "fe-canonical-adapter-test-{}-{}",
            std::process::id(),
            NEXT_STAGING_ID.fetch_add(1, Ordering::Relaxed)
        ));
        required.write_atomic(&destination).unwrap();
        assert_eq!(
            fs::read(destination.join(INTERFACE_JS_FILE)).unwrap(),
            interface_js.as_bytes()
        );
        assert_eq!(
            fs::read(destination.join(INTERFACE_D_TS_FILE)).unwrap(),
            interface_d_ts.as_bytes()
        );
        fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    fn optional_policy_never_embeds_an_unverified_candidate() {
        let candidate = crate::CanonicalInterfaceManifest::build(vec![crate::CanonicalLaneDecl {
            name: "update".to_owned(),
            export: Some("update".to_owned()),
            request: crate::CanonicalType::Record(vec![crate::CanonicalField::new(
                "value",
                crate::CanonicalType::U32,
            )]),
            response: crate::CanonicalType::Record(vec![crate::CanonicalField::new(
                "value",
                crate::CanonicalType::U32,
            )]),
            intent: crate::CanonicalLaneIntent::default(),
        }])
        .unwrap();
        let wasm_without_abi = b"\0asm\x01\0\0\0";
        let mut optional = WebCanonicalStatus {
            policy: WebCanonicalPolicy::Optional,
            embedded: false,
            omission_reason: None,
        };
        assert!(
            verify_canonical_candidate(wasm_without_abi, Some(candidate.clone()), &mut optional)
                .unwrap()
                .is_none()
        );
        assert!(!optional.embedded);
        assert!(
            optional
                .omission_reason
                .as_deref()
                .unwrap()
                .contains("missing exported memory")
        );
        let (js, declarations, artifacts) = generated_canonical_adapters(None).unwrap();
        assert!(js.is_none());
        assert!(declarations.is_none());
        assert!(artifacts.is_empty());

        let mut required = WebCanonicalStatus {
            policy: WebCanonicalPolicy::Required,
            embedded: false,
            omission_reason: None,
        };
        let error = verify_canonical_candidate(wasm_without_abi, Some(candidate), &mut required)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("required canonical interface is unavailable"),
            "{error}"
        );
    }

    #[test]
    fn generated_type_name_collisions_fail_closed() {
        let record = || {
            crate::CanonicalType::Record(vec![crate::CanonicalField::new(
                "value",
                crate::CanonicalType::U32,
            )])
        };
        let interface = crate::CanonicalInterfaceManifest::build(vec![
            crate::CanonicalLaneDecl {
                name: "foo_bar".to_owned(),
                export: Some("fe_cabi_foo_bar".to_owned()),
                request: record(),
                response: record(),
                intent: crate::CanonicalLaneIntent::default(),
            },
            crate::CanonicalLaneDecl {
                name: "foo__bar".to_owned(),
                export: Some("fe_cabi_foo__bar".to_owned()),
                request: record(),
                response: record(),
                intent: crate::CanonicalLaneIntent::default(),
            },
        ])
        .unwrap();
        let error = generated_canonical_adapters(Some(&interface)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("collide as generated TypeScript name `FooBar`"),
            "{error}"
        );
    }

    #[test]
    fn generated_typescript_uses_element_specific_bounded_list_views() {
        let interface = crate::CanonicalInterfaceManifest::build(vec![crate::CanonicalLaneDecl {
            name: "weights".to_owned(),
            export: Some("fe_cabi_weights".to_owned()),
            request: crate::CanonicalType::List {
                element: crate::CanonicalListElement::U32,
                max: 8,
            },
            response: crate::CanonicalType::List {
                element: crate::CanonicalListElement::F32,
                max: 8,
            },
            intent: crate::CanonicalLaneIntent::default(),
        }])
        .unwrap();
        let declarations = canonical_interface_declarations(&interface).unwrap();
        assert!(declarations.contains("export type WeightsRequest = Uint32Array;"));
        assert!(declarations.contains("export type WeightsResponse = Float32Array;"));
    }

    #[test]
    fn requested_entry_is_required_and_not_chosen_by_source_order() {
        let bundle = compile(WebBundleMode::Render);
        let mut exports = Vec::new();
        for payload in wasmparser::Parser::new(0).parse_all(&bundle.wasm) {
            if let wasmparser::Payload::ExportSection(reader) = payload.unwrap() {
                exports.extend(
                    reader
                        .into_iter()
                        .map(|export| export.unwrap().name.to_string()),
                );
            }
        }
        assert!(exports.iter().any(|name| name == "shade"), "{exports:?}");
        assert!(
            !exports.iter().any(|name| name == "ignored"),
            "unselected public root leaked into the bundle: {exports:?}"
        );

        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///web_bundle_missing_entry.fe").unwrap();
        db.workspace()
            .touch(&mut db, url.clone(), Some(SOURCE.to_string()));
        let file = db.workspace().get(&db, &url).unwrap();
        let error = WebBundle::compile(
            &db,
            db.top_mod(file),
            WebBuildOptions::render("missing", None),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("requested web entry `missing` was not found"),
            "{error}"
        );
    }

    #[test]
    fn grid_bundle_preserves_requested_workgroup_and_publishes_atomically() {
        let bundle = compile(WebBundleMode::Grid);
        assert_eq!(bundle.manifest.layout.mode, WebBundleMode::Grid);
        assert_eq!(bundle.manifest.layout.workgroup_size, [8, 4, 1]);

        let root = std::env::temp_dir().join(format!(
            "fe-web-bundle-test-{}-{}",
            std::process::id(),
            NEXT_STAGING_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let destination = root.join("bundle");
        let materialized = bundle.materialized_files().unwrap();
        bundle.write_atomic(&destination).unwrap();
        let mut disk_paths = fs::read_dir(&destination)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        disk_paths.sort();
        let mut materialized_paths = materialized
            .iter()
            .map(|file| file.path().to_owned())
            .collect::<Vec<_>>();
        materialized_paths.sort();
        assert_eq!(disk_paths, materialized_paths);
        for file in &materialized {
            assert_eq!(
                fs::read(destination.join(file.path())).unwrap(),
                file.bytes(),
                "disk publication diverged for {}",
                file.path()
            );
        }
        let disk_manifest: WebBundleManifest =
            serde_json::from_slice(&fs::read(destination.join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(disk_manifest, bundle.manifest);
        assert!(matches!(
            bundle.write_atomic(&destination),
            Err(WebBundleError::DestinationExists(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn materialization_rejects_manifest_content_drift() {
        let mut bundle = compile(WebBundleMode::Render);
        bundle.manifest.artifacts.wasm = "../module.wasm".to_owned();
        let error = bundle.materialized_files().unwrap_err().to_string();
        assert!(error.contains("safe bundle-relative path"), "{error}");

        let mut bundle = compile(WebBundleMode::Render);
        bundle.manifest.artifacts.wgsl = bundle.manifest.artifacts.wasm.clone();
        let error = bundle.materialized_files().unwrap_err().to_string();
        assert!(error.contains("duplicate artifact path"), "{error}");

        let mut bundle = compile(WebBundleMode::Render);
        bundle.manifest.artifacts.wasm_bytes += 1;
        let error = bundle.materialized_files().unwrap_err().to_string();
        assert!(error.contains("manifest byte length"), "{error}");

        let canonical = compile_canonical_bundle();
        let mut bundle = canonical.clone();
        bundle
            .manifest
            .browser_runtime
            .as_mut()
            .unwrap()
            .protocol_version += 1;
        let error = bundle.materialized_files().unwrap_err().to_string();
        assert!(error.contains("runtime protocol metadata"), "{error}");

        let mut bundle = canonical.clone();
        bundle.manifest.browser_runtime.as_mut().unwrap().artifacts[0].sha256 = "00".repeat(32);
        let error = bundle.materialized_files().unwrap_err().to_string();
        assert!(
            error.contains("does not match its manifest metadata"),
            "{error}"
        );

        let mut bundle = canonical;
        bundle.manifest.browser_runtime.as_mut().unwrap().artifacts[0].path =
            "runtime/unknown.js".to_owned();
        let error = bundle.materialized_files().unwrap_err().to_string();
        assert!(
            error.contains("unsupported browser runtime artifact"),
            "{error}"
        );
    }

    #[test]
    fn declarations_emit_discriminated_tagged_variant_types() {
        let manifest = CanonicalInterfaceManifest::build(vec![crate::CanonicalLaneDecl {
            name: "deliver".to_owned(),
            export: None,
            request: crate::CanonicalType::Variant(vec![
                crate::CanonicalVariant {
                    name: "empty".to_owned(),
                    fields: vec![],
                },
                crate::CanonicalVariant {
                    name: "data".to_owned(),
                    fields: vec![
                        crate::CanonicalField::new("code", crate::CanonicalType::U8),
                        crate::CanonicalField::new("payload", crate::CanonicalType::Bytes),
                    ],
                },
            ]),
            response: crate::CanonicalType::Record(vec![crate::CanonicalField::new(
                "accepted",
                crate::CanonicalType::Bool,
            )]),
            intent: crate::CanonicalLaneIntent {
                execution: crate::CanonicalExecution::HostEffect,
                placement: crate::CanonicalPlacement::Worker,
                capabilities: vec![],
            },
        }])
        .unwrap();
        let declarations = canonical_interface_declarations(&manifest).unwrap();
        assert!(declarations.contains("readonly tag: \"empty\";"));
        assert!(declarations.contains("readonly tag: \"data\";"));
        assert!(declarations.contains("payload: Uint8Array;"));
        assert!(declarations.contains("export type DeliverRequest"));
    }
}
