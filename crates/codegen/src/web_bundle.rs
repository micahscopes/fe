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
    sync::atomic::{AtomicU64, Ordering},
};

use driver::DriverDataBase;
use hir::hir_def::TopLevelMod;
use serde::{Deserialize, Serialize};
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
pub const WEB_BUNDLE_PROTOCOL_VERSION: u32 = 2;

const WASM_FILE: &str = "module.wasm";
const WGSL_FILE: &str = "shader.wgsl";
const MANIFEST_FILE: &str = "manifest.json";
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
    /// Optional message-lane entry when it differs from the GPU kernel.
    pub canonical_entry: Option<String>,
}

impl WebBuildOptions {
    pub fn render(source_entry: impl Into<String>, source_id: Option<String>) -> Self {
        Self {
            source_entry: source_entry.into(),
            mode: WebBundleMode::Render,
            workgroup_size: [0, 0, 0],
            provenance: WebProvenance::new(source_id),
            canonical_policy: WebCanonicalPolicy::Disabled,
            canonical_entry: None,
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
            canonical_entry: None,
        }
    }

    pub fn with_canonical_policy(mut self, policy: WebCanonicalPolicy) -> Self {
        self.canonical_policy = policy;
        self
    }

    pub fn with_canonical_entry(mut self, entry: impl Into<String>) -> Self {
        self.canonical_entry = Some(entry.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebArtifactManifest {
    pub wasm: String,
    pub wasm_bytes: u64,
    pub wgsl: String,
    pub wgsl_bytes: u64,
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
}

impl WebBundle {
    /// Compile Wasm and browser-profile WGSL from the same resolved module.
    /// The canonical message lane may be a different public entry from the GPU
    /// kernel, in which case each backend gets an explicitly selected
    /// single-root package. Validation is part of construction: an invalid
    /// target can never be represented as a `WebBundle`.
    pub fn compile(
        db: &DriverDataBase,
        top_mod: TopLevelMod<'_>,
        options: WebBuildOptions,
    ) -> Result<Self, WebBundleError> {
        let canonical_entry = options
            .canonical_entry
            .as_deref()
            .unwrap_or(&options.source_entry);
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
                let derived =
                    canonical_lane_decl_from_entry(db, top_mod, canonical_entry, canonical_entry)
                        .and_then(|lane| CanonicalInterfaceManifest::build(vec![lane]));
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
        let wasm_package = mir::build_wasm_runtime_package_for_entry(db, top_mod, canonical_entry)
            .map_err(|error| WebBundleError::Lower(error.to_string()))?;

        let wasm_options = match options.canonical_policy {
            WebCanonicalPolicy::Disabled => WasmCompileOptions::default(),
            WebCanonicalPolicy::Optional | WebCanonicalPolicy::Required => canonical_candidate
                .as_ref()
                .and_then(|interface| interface.lanes.first())
                .cloned()
                .map(|lane| WasmCompileOptions::default().with_canonical_lane(lane))
                .unwrap_or_else(|| WasmCompileOptions::default().with_canonical_arena()),
        };
        let wasm = compile_runtime_package_wasm_with_options(db, &wasm_package, wasm_options)
            .map_err(|error| WebBundleError::Lower(error.to_string()))?
            .bytes;
        wasmparser::validate(&wasm)
            .map_err(|error| WebBundleError::WasmValidation(error.to_string()))?;
        let canonical_interface =
            verify_canonical_candidate(&wasm, canonical_candidate, &mut canonical_status)?;

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
        let wgsl = artifact.wgsl.ok_or(WebBundleError::MissingWgsl)?;
        validate_browser_wgsl(&wgsl)?;
        let layout = WebLayout::from_spirv(&artifact.layout)?;

        let manifest = WebBundleManifest {
            protocol: WEB_BUNDLE_PROTOCOL.to_string(),
            protocol_version: WEB_BUNDLE_PROTOCOL_VERSION,
            source_entry: options.source_entry,
            artifacts: WebArtifactManifest {
                wasm: WASM_FILE.to_string(),
                wasm_bytes: wasm.len() as u64,
                wgsl: WGSL_FILE.to_string(),
                wgsl_bytes: wgsl.len() as u64,
            },
            layout,
            provenance: options.provenance,
            canonical_interface,
            canonical_status,
        };
        Ok(Self {
            wasm,
            wgsl,
            manifest,
        })
    }

    pub fn manifest_json(&self) -> Result<Vec<u8>, WebBundleError> {
        let mut json = serde_json::to_vec_pretty(&self.manifest)
            .map_err(|error| WebBundleError::Manifest(error.to_string()))?;
        json.push(b'\n');
        Ok(json)
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
            write_synced(&staging.join(WASM_FILE), &self.wasm)?;
            write_synced(&staging.join(WGSL_FILE), self.wgsl.as_bytes())?;
            write_synced(&staging.join(MANIFEST_FILE), &self.manifest_json()?)?;
            fs::rename(&staging, destination)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }
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

pub fn update(request: Request) -> Response {
    Response { value: request.value + 1 }
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
        let exports = wasm_exports(&first.wasm);
        assert!(!exports.iter().any(|name| name == "fe_cabi_alloc"));
        assert!(!exports.iter().any(|name| name == "fe_cabi_reset"));
        assert!(first.manifest.layout.vertex_entry.is_some());
        assert!(first.manifest.layout.fragment_entry.is_some());
        let decoded: WebBundleManifest =
            serde_json::from_slice(&first.manifest_json().unwrap()).unwrap();
        assert_eq!(decoded, first.manifest);
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
        let exports = wasm_exports(&wasm);
        assert!(exports.iter().any(|name| name == "fe_cabi_alloc"));
        assert!(exports.iter().any(|name| name == "fe_cabi_reset"));
        assert!(exports.iter().any(|name| name == "fe_cabi_update"));

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
                .with_canonical_entry("update")
                .with_canonical_policy(WebCanonicalPolicy::Required),
        )
        .unwrap();
        assert!(required.manifest.canonical_interface.is_some());
        assert!(required.manifest.canonical_status.embedded);
    }

    #[test]
    fn optional_policy_never_embeds_an_unverified_candidate() {
        let candidate = crate::CanonicalInterfaceManifest::build(vec![crate::CanonicalLaneDecl {
            name: "update".to_owned(),
            export: "update".to_owned(),
            request: crate::CanonicalType::Record(vec![crate::CanonicalField::new(
                "value",
                crate::CanonicalType::U32,
            )]),
            response: crate::CanonicalType::Record(vec![crate::CanonicalField::new(
                "value",
                crate::CanonicalType::U32,
            )]),
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
        bundle.write_atomic(&destination).unwrap();
        assert_eq!(fs::read(destination.join(WASM_FILE)).unwrap(), bundle.wasm);
        assert_eq!(
            fs::read_to_string(destination.join(WGSL_FILE)).unwrap(),
            bundle.wgsl
        );
        let disk_manifest: WebBundleManifest =
            serde_json::from_slice(&fs::read(destination.join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(disk_manifest, bundle.manifest);
        assert!(matches!(
            bundle.write_atomic(&destination),
            Err(WebBundleError::DestinationExists(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
