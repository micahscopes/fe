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

use common::InputDb;
use compiler_db::DriverDataBase;
use hir::analysis::{
    semantic::{ViewParam, ViewParamKind, project_view_surface},
    ty::{
        adt_def::AdtRef,
        const_ty::{ConstTyData, EvaluatedConstTy},
        trait_resolution::PredicateListId,
        ty_def::{PrimTy, TyBase, TyData, TyId},
        ty_lower::lower_hir_ty,
    },
};
use hir::hir_def::{
    FieldParent, Func, GenericArg, GpuControl, GpuDispatch, GpuDraw, GpuResource, GpuStage,
    HirIngot, Partial, PathId, TopLevelMod, TypeKind, Visibility,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sonatina_codegen::isa::spirv::{
    Access, LayoutMode, Role, SpirvBuiltinArgument, SpirvBuiltinSource, SpirvExternalResource,
    SpirvLayout, SpirvResourceElement, SpirvResourceField, SpirvScalarKind, WordKind,
};

use crate::actor_semantics::{SemanticActor, nominal_attrs, resolve_metadata_ty, semantic_actors};
use crate::browser_actor_runtime::{
    BROWSER_ACTOR_RUNTIME_FILES, BROWSER_ACTOR_RUNTIME_PROTOCOL, BROWSER_ACTOR_RUNTIME_VERSION,
};
use crate::resident_actor::{
    StructuredChildActorArtifact, behavior_is_scoped_task, compile_scoped_task_support,
};
use crate::sonatina::{
    WasmCompileOptions, compile_runtime_package_spirv_authored_raster,
    compile_runtime_package_spirv_compute_with_interface, compile_runtime_package_spirv_grid,
    compile_runtime_package_spirv_render, compile_runtime_package_spirv_render_with_resources,
    compile_runtime_package_wasm_with_options,
};
use crate::{
    CanonicalField, CanonicalInterfaceManifest, CanonicalType, CanonicalVariant, WasmTaskAdapter,
    canonical_lane_decl_from_entry, canonical_lane_decls_from_actor,
    canonical_lane_decls_from_module, canonical_type_from_semantic, emit_canonical_interface_js,
    materialize_scoped_task_package, verify_canonical_wasm_abi,
};

pub const WEB_BUNDLE_PROTOCOL: &str = "fe-web-bundle";
pub const WEB_BUNDLE_PROTOCOL_VERSION: u32 = 6;
pub const WEB_ACTOR_RUNTIME_PROTOCOL: &str = BROWSER_ACTOR_RUNTIME_PROTOCOL;
pub const WEB_ACTOR_RUNTIME_VERSION: u32 = BROWSER_ACTOR_RUNTIME_VERSION;

const WASM_FILE: &str = "module.wasm";
const WGSL_FILE: &str = "shader.wgsl";
const PASS_DIR: &str = "passes";
const MANIFEST_FILE: &str = "manifest.json";
const INTERFACE_JS_FILE: &str = "interface.js";
const INTERFACE_D_TS_FILE: &str = "interface.d.ts";
/// Manifest-free fixed host discovery point for a typed Fe surface transition.
/// The authored behavior may have any ordinary Fe name; Wasm publication
/// aliases it to this versioned ABI identity after semantic shape validation.
const TYPED_SURFACE_TRANSITION_EXPORT: &str = "fe_surface_transition_v2";
/// Fixed discovery point for a resident transition whose presentation policy
/// is selected by `SurfaceScheduling<P>`. The ABI deliberately does not encode
/// a policy name: `P` and its ordinary Fe implementation own those semantics.
const TYPED_SURFACE_SCHEDULED_EXPORT: &str = "fe_surface_transition_scheduled_v1";
/// Scalar leaves in the fixed v2 `SurfaceEvent`: ten browser gesture/extent
/// facts followed by the typed browser/presentation identity, direct-parameter
/// index, and proposed value.
const TYPED_SURFACE_EVENT_FIELDS: usize = 13;
/// Fixed companion ABI for seeding or explicitly replacing the private state
/// of a resident scheduled actor. It takes the complete non-resource state in
/// declaration order and returns nothing. Frame transitions never receive
/// those values back from the browser.
const TYPED_SURFACE_STATE_REPLACE_EXPORT: &str = "fe_surface_state_replace_v1";
/// Manifest-free fixed host discovery point for a GPU actor's Fe-authored
/// complete-state initializer. The selected behavior is identified by the
/// nominal `InitialState` role; its source name remains application vocabulary.
const SURFACE_INITIALIZER_EXPORT: &str = "fe_surface_initialize_v1";
/// Fixed binary discovery point for a resident Fe presentation policy. The
/// policy's authored behavior name and private state never enter the manifest.
const TYPED_SURFACE_SCHEDULE_EXPORT: &str = "fe_surface_schedule_v2";
/// Fixed binary discovery point for a Fe backing-quality policy. The browser
/// supplies raw viewport/device facts and realizes the returned integral
/// extent; the selected policy type and authored function name remain private.
const TYPED_SURFACE_QUALITY_EXPORT: &str = "fe_surface_quality_v1";
/// Fixed discovery point for the actor-level Fe shared-device policy. Its
/// private supervision state remains resident in generated Wasm.
const TYPED_SURFACE_RECOVERY_EXPORT: &str = "fe_surface_recovery_v1";
/// Compiler-emitted host page for render bundles. It reads `manifest.json` and
/// drives the two lowerings of the render kernel it describes: `shader.wgsl`
/// via WebGPU, with a per-pixel `module.wasm` fallback. Emitted verbatim next to
/// the bundle so `--mode render` output is directly openable, not hand-authored.
const RENDER_INDEX_FILE: &str = "index.html";
const RENDER_RUNTIME_HTML: &str = include_str!("../assets/render-runtime/index.html");
const RENDER_SCOPED_TASK_OPTION_MARKER: &str = "/* FE_SCOPED_TASK_OPTION */";
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
const WEB_ACTOR_RUNTIME: &[(&str, &str)] = BROWSER_ACTOR_RUNTIME_FILES;
static NEXT_STAGING_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebBundleMode {
    Render,
    Grid,
    Compute,
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
    /// Authored Fe program inputs (including the ingot manifest), filled by
    /// filesystem-aware frontends such as `fe web`. Bundle lowering itself is
    /// deliberately filesystem-blind, so direct API callers may leave this
    /// empty while retaining the ownership contract below.
    #[serde(default)]
    pub authored_sources: Vec<WebSourceProvenance>,
    /// Authored inputs which are not Fe. A published render manifest records
    /// its containing HTML document here; canonical-gallery validation rejects
    /// JavaScript/Rust/WGSL/Wasm/generated-manifest inputs while still honestly
    /// admitting the hand-authored HTML/CSS composition until `WebPage` lands.
    #[serde(default)]
    pub non_fe_authored_sources: Vec<WebSourceProvenance>,
    /// Artifact classes produced by the Fe compiler for this bundle. This is
    /// ownership metadata, not a second path/size inventory (the concrete
    /// files remain in `artifacts` and `passes`).
    #[serde(default)]
    pub generated_artifacts: Vec<WebGeneratedArtifactKind>,
    /// Application responsibilities whose implementation is authored in Fe.
    #[serde(default)]
    pub fe_responsibilities: Vec<WebFeResponsibility>,
    /// The one fixed, versioned, demo-blind browser host. Static `fe web build`
    /// output names the contract; HTML publication additionally pins its exact
    /// content-addressed artifact.
    #[serde(default)]
    pub fixed_host: WebFixedHostProvenance,
}

impl WebProvenance {
    pub fn new(source_id: Option<String>) -> Self {
        Self {
            compiler: "fe".to_string(),
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            source_id,
            authored_sources: Vec::new(),
            non_fe_authored_sources: Vec::new(),
            generated_artifacts: Vec::new(),
            fe_responsibilities: Vec::new(),
            fixed_host: WebFixedHostProvenance::render_runtime(),
        }
    }

    fn with_bundle_shape(
        mut self,
        has_wasm: bool,
        has_surface: bool,
        has_control: bool,
        has_pass_graph: bool,
        has_fe_schedule: bool,
        has_fe_quality: bool,
        has_fe_recovery: bool,
    ) -> Self {
        self.generated_artifacts = vec![
            WebGeneratedArtifactKind::Manifest,
            WebGeneratedArtifactKind::Wgsl,
        ];
        if has_wasm {
            self.generated_artifacts
                .push(WebGeneratedArtifactKind::Wasm);
        }
        self.generated_artifacts.sort();
        self.fe_responsibilities = vec![WebFeResponsibility::GpuProgram];
        if has_surface {
            self.fe_responsibilities
                .push(WebFeResponsibility::SurfaceDeclaration);
        }
        if has_control {
            self.fe_responsibilities
                .push(WebFeResponsibility::ControlTransition);
        }
        if has_fe_schedule {
            self.fe_responsibilities
                .push(WebFeResponsibility::SchedulingPolicy);
            self.fe_responsibilities
                .push(WebFeResponsibility::ResidentActorState);
            self.fixed_host
                .responsibilities
                .retain(|role| *role != WebHostResponsibility::PresentationScheduler);
            self.fixed_host
                .responsibilities
                .push(WebHostResponsibility::PresentationClock);
            self.fixed_host.responsibilities.sort();
        }
        if has_fe_quality {
            self.fe_responsibilities
                .push(WebFeResponsibility::BackingQualityPolicy);
            self.fixed_host
                .responsibilities
                .retain(|role| *role != WebHostResponsibility::BackingStorePolicy);
        }
        if has_fe_recovery {
            self.fe_responsibilities
                .push(WebFeResponsibility::DeviceRecoveryPolicy);
        }
        if has_pass_graph {
            self.fe_responsibilities
                .push(WebFeResponsibility::GpuPassGraph);
        }
        self.fe_responsibilities.sort();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSourceProvenance {
    /// Stable logical identity, normally relative to the sketches directory or
    /// the published HTML document. Never an ambient absolute build path.
    pub id: String,
    pub sha256: String,
    pub kind: WebAuthoredSourceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebAuthoredSourceKind {
    Fe,
    FeManifest,
    Html,
    Css,
    JavaScript,
    Rust,
    Wgsl,
    Wasm,
    Json,
    Asset,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebGeneratedArtifactKind {
    Manifest,
    Wasm,
    Wgsl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebFeResponsibility {
    GpuProgram,
    GpuPassGraph,
    SurfaceDeclaration,
    ControlTransition,
    SchedulingPolicy,
    ResidentActorState,
    BackingQualityPolicy,
    DeviceRecoveryPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebHostResponsibility {
    DomSurface,
    InputTransport,
    PresentationScheduler,
    PresentationClock,
    /// Legacy/default selection of the canvas backing extent. A typed
    /// `SurfaceQuality<P>` policy removes this responsibility from the host.
    BackingStorePolicy,
    /// Standards/device facts (CSS geometry, DPR, pointer media query, and GPU
    /// limits) supplied without application-specific interpretation.
    DeviceCapabilityFacts,
    WebGpuExecutor,
    Lifecycle,
    WasmLoader,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebFixedHostProvenance {
    pub name: String,
    pub contract: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<WebGeneratedArtifact>,
    #[serde(default)]
    pub responsibilities: Vec<WebHostResponsibility>,
}

impl WebFixedHostProvenance {
    fn render_runtime() -> Self {
        Self {
            name: "fe-render-runtime".to_owned(),
            contract: "fixed_versioned_demo_blind_browser_host".to_owned(),
            artifact: None,
            responsibilities: vec![
                WebHostResponsibility::DomSurface,
                WebHostResponsibility::InputTransport,
                WebHostResponsibility::PresentationScheduler,
                WebHostResponsibility::BackingStorePolicy,
                WebHostResponsibility::DeviceCapabilityFacts,
                WebHostResponsibility::WebGpuExecutor,
                WebHostResponsibility::Lifecycle,
                WebHostResponsibility::WasmLoader,
            ],
        }
    }
}

impl Default for WebFixedHostProvenance {
    fn default() -> Self {
        // Additive decoding of a legacy manifest must not manufacture a host
        // claim the producer never made. New bundles call `render_runtime()`
        // explicitly; absence remains honestly unknown.
        Self {
            name: String::new(),
            contract: String::new(),
            artifact: None,
            responsibilities: Vec::new(),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebActorProgram {
    pub actor: String,
    pub stages: Vec<WebActorStage>,
    pub resources: Vec<WebActorResource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebActorStage {
    pub source_entry: String,
    pub kind: WebActorStageKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebActorStageKind {
    Compute {
        workgroup_size: [u32; 3],
        dispatch: [u32; 3],
        repeat: u32,
        cycle: Option<WebActorPassCycle>,
        invocation_context: bool,
    },
    /// Authored vertex behavior. The payload tree is derived from the role's
    /// nominal `V` and retained only as compiler state; it is not serialized
    /// into the transitional render manifest.
    Vertex {
        varying: CanonicalType,
        vertex_count: u32,
    },
    Fragment,
    /// Authored fragment behavior paired with `Vertex` by exact nominal
    /// payload identity before this serializable-free descriptor is produced.
    RasterFragment {
        varying: CanonicalType,
    },
}

/// One consecutive actor pass body repeated as a unit. `group` is retained
/// only while compiling the actor graph; the transport receives a compact
/// compiler-assigned group number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebActorPassCycle {
    pub group: String,
    pub repeat: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebActorResource {
    pub field_index: u32,
    pub name: String,
    pub length: u32,
    pub element: WebActorResourceElement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebActorResourceElement {
    U32,
    Record {
        fields: Vec<WebActorResourceField>,
        span: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebActorResourceField {
    pub name: String,
    pub offset: u32,
}

fn actor_is_gpu_program(db: &DriverDataBase, actor: &SemanticActor<'_>) -> bool {
    actor
        .state
        .actor_placement(db)
        .data(db)
        .iter()
        .filter_map(|role| role.key_path.to_opt())
        .filter_map(|path| resolve_metadata_ty(db, path, actor.state.scope()))
        .filter_map(|ty| nominal_attrs(db, ty))
        .any(|attrs| attrs.is_gpu_program(db))
}

fn behavior_stage(db: &DriverDataBase, behavior: hir::hir_def::Func<'_>) -> Option<GpuStage> {
    behavior_stage_role(db, behavior).map(|(stage, _)| stage)
}

fn behavior_stage_role<'db>(
    db: &'db DriverDataBase,
    behavior: hir::hir_def::Func<'db>,
) -> Option<(GpuStage, PathId<'db>)> {
    behavior
        .actor_roles(db)
        .data(db)
        .iter()
        .filter_map(|role| role.key_path.to_opt())
        .find_map(|path| {
            let attrs = nominal_attrs(db, resolve_metadata_ty(db, path, behavior.scope())?)?;
            Some((attrs.gpu_stage(db)?, path))
        })
}

fn nested_type_path_db<'db>(db: &'db DriverDataBase, arg: &GenericArg<'db>) -> Option<PathId<'db>> {
    let GenericArg::Type(arg) = arg else {
        return None;
    };
    let TypeKind::Path(Partial::Present(path)) = arg.ty.to_opt()?.data(db) else {
        return None;
    };
    Some(*path)
}

fn compute_stage_shape(
    db: &DriverDataBase,
    behavior: hir::hir_def::Func<'_>,
    role_path: PathId<'_>,
) -> Result<([u32; 3], [u32; 3], u32, Option<WebActorPassCycle>), WebBundleError> {
    let args = role_path.generic_args(db).data(db);
    let [workgroup_arg, dispatch_arg] = args.as_slice() else {
        return Err(WebBundleError::EntryDerivation(
            "compute-stage role requires workgroup and dispatch type arguments".to_owned(),
        ));
    };
    let workgroup_path = nested_type_path_db(db, workgroup_arg).ok_or_else(|| {
        WebBundleError::EntryDerivation(
            "compute-stage workgroup argument must be a nominal type".to_owned(),
        )
    })?;
    let dispatch_path = nested_type_path_db(db, dispatch_arg).ok_or_else(|| {
        WebBundleError::EntryDerivation(
            "compute-stage dispatch argument must be a nominal type".to_owned(),
        )
    })?;
    let workgroup_ty =
        resolve_metadata_ty(db, workgroup_path, behavior.scope()).ok_or_else(|| {
            WebBundleError::EntryDerivation(
                "compute-stage workgroup type did not resolve".to_owned(),
            )
        })?;
    let workgroup_attrs = nominal_attrs(db, workgroup_ty).ok_or_else(|| {
        WebBundleError::EntryDerivation(
            "compute-stage workgroup argument is not an attributed nominal type".to_owned(),
        )
    })?;
    if !workgroup_attrs.is_gpu_workgroup(db) {
        return Err(WebBundleError::EntryDerivation(
            "compute-stage workgroup type lacks `#[gpu_workgroup]`".to_owned(),
        ));
    }
    let dispatch_ty =
        resolve_metadata_ty(db, dispatch_path, behavior.scope()).ok_or_else(|| {
            WebBundleError::EntryDerivation(
                "compute-stage dispatch type did not resolve".to_owned(),
            )
        })?;
    let dispatch_attrs = nominal_attrs(db, dispatch_ty).ok_or_else(|| {
        WebBundleError::EntryDerivation(
            "compute-stage dispatch argument is not an attributed nominal type".to_owned(),
        )
    })?;
    dispatch_attrs.gpu_dispatch(db).ok_or_else(|| {
        WebBundleError::EntryDerivation(
            "compute-stage dispatch type must carry an attributed GPU dispatch policy"
                .to_owned(),
        )
    })?;
    let workgroup_size = semantic_const_triplet(db, workgroup_ty).ok_or_else(|| {
        WebBundleError::EntryDerivation(
            "GPU workgroup dimensions must be three concrete u32-sized constants".to_owned(),
        )
    })?;
    let (dispatch, repeat, cycle) = compute_dispatch_shape(db, dispatch_ty)?;
    if workgroup_size.contains(&0) || dispatch.contains(&0) || repeat == 0 {
        return Err(WebBundleError::EntryDerivation(
            "GPU workgroup, fixed dispatch dimensions, and repeat count must be nonzero".to_owned(),
        ));
    }
    if repeat > 65_535 {
        return Err(WebBundleError::EntryDerivation(
            "repeated dispatch count exceeds the portable 65535-command envelope".to_owned(),
        ));
    }
    Ok((workgroup_size, dispatch, repeat, cycle))
}

fn compute_dispatch_shape(
    db: &DriverDataBase,
    dispatch_ty: TyId<'_>,
) -> Result<([u32; 3], u32, Option<WebActorPassCycle>), WebBundleError> {
    let attrs = nominal_attrs(db, dispatch_ty).ok_or_else(|| {
        WebBundleError::EntryDerivation(
            "compute-stage dispatch policy is not an attributed nominal type".to_owned(),
        )
    })?;
    match attrs.gpu_dispatch(db) {
        Some(GpuDispatch::Fixed) => {
            let dispatch = semantic_const_triplet(db, dispatch_ty).ok_or_else(|| {
                WebBundleError::EntryDerivation(
                    "fixed dispatch dimensions must be three concrete u32-sized constants"
                        .to_owned(),
                )
            })?;
            Ok((dispatch, 1, None))
        }
        Some(GpuDispatch::Repeated) => {
            let dispatch = semantic_const_triplet(db, dispatch_ty).ok_or_else(|| {
                WebBundleError::EntryDerivation(
                    "repeated dispatch dimensions must be three concrete u32-sized constants"
                        .to_owned(),
                )
            })?;
            let repeat = dispatch_ty
                .generic_args(db)
                .get(3)
                .and_then(|count| semantic_const_u32(db, *count))
                .ok_or_else(|| {
                    WebBundleError::EntryDerivation(
                        "repeated dispatch requires a fourth concrete u32-sized repeat count"
                            .to_owned(),
                    )
                })?;
            Ok((dispatch, repeat, None))
        }
        Some(GpuDispatch::Cycled) => {
            let [group, inner, count, ..] = dispatch_ty.generic_args(db) else {
                return Err(WebBundleError::EntryDerivation(
                    "cycled dispatch requires a group type, inner dispatch policy, and cycle count"
                        .to_owned(),
                ));
            };
            if matches!(group.data(db), TyData::ConstTy(_)) {
                return Err(WebBundleError::EntryDerivation(
                    "cycled dispatch group must be a nominal Fe type".to_owned(),
                ));
            }
            let cycles = semantic_const_u32(db, *count).ok_or_else(|| {
                WebBundleError::EntryDerivation(
                    "cycled dispatch count must be a concrete u32-sized integer".to_owned(),
                )
            })?;
            if cycles == 0 || cycles > 65_535 {
                return Err(WebBundleError::EntryDerivation(
                    "cycled dispatch count must be between 1 and 65535".to_owned(),
                ));
            }
            let (dispatch, repeat, nested) = compute_dispatch_shape(db, *inner)?;
            if nested.is_some() {
                return Err(WebBundleError::EntryDerivation(
                    "nested actor pass cycles are not yet supported".to_owned(),
                ));
            }
            Ok((
                dispatch,
                repeat,
                Some(WebActorPassCycle {
                    group: group.pretty_print(db).to_string(),
                    repeat: cycles,
                }),
            ))
        }
        None => Err(WebBundleError::EntryDerivation(
            "compute-stage dispatch type lacks a recognized GPU dispatch attribute".to_owned(),
        )),
    }
}

fn compute_invocation_context(
    db: &DriverDataBase,
    behavior: hir::hir_def::Func<'_>,
) -> Result<bool, WebBundleError> {
    let args = behavior.arg_tys(db);
    let marked = args
        .iter()
        .enumerate()
        .filter_map(|(index, ty)| {
            nominal_attrs(db, *ty.skip_binder())
                .is_some_and(|attrs| attrs.is_gpu_compute_invocation(db))
                .then_some(index)
        })
        .collect::<Vec<_>>();
    let context_index = match marked.as_slice() {
        [] => return Ok(false),
        [index] => *index,
        _ => {
            return Err(WebBundleError::EntryDerivation(
                "compute behavior may take at most one `#[gpu_compute_invocation]` context"
                    .to_owned(),
            ));
        }
    };
    if context_index != 0 {
        return Err(WebBundleError::EntryDerivation(
            "`#[gpu_compute_invocation]` context must be the compute behavior's first argument"
                .to_owned(),
        ));
    }

    let context = *args[context_index].skip_binder();
    let context = context.as_view(db).unwrap_or(context);
    let fields = context.field_types(db);
    let [global, local, workgroup, workgroups, local_index] = fields.as_slice() else {
        return Err(WebBundleError::EntryDerivation(
            "`#[gpu_compute_invocation]` must contain four three-axis records followed by one `u32` local index"
                .to_owned(),
        ));
    };
    for axis in [global, local, workgroup, workgroups] {
        let axis = axis.as_view(db).unwrap_or(*axis);
        let components = axis.field_types(db);
        if components.len() != 3
            || components
                .into_iter()
                .any(|component| !is_primitive(db, component, PrimTy::U32))
        {
            return Err(WebBundleError::EntryDerivation(
                "each `#[gpu_compute_invocation]` axis record must contain exactly three `u32` fields"
                    .to_owned(),
            ));
        }
    }
    if !is_primitive(db, *local_index, PrimTy::U32) {
        return Err(WebBundleError::EntryDerivation(
            "`#[gpu_compute_invocation]` local index must be exactly `u32`".to_owned(),
        ));
    }
    Ok(true)
}

fn compute_invocation_builtin_arguments() -> Vec<SpirvBuiltinArgument> {
    use SpirvBuiltinSource as Source;
    [
        Source::GlobalInvocationIdX,
        Source::GlobalInvocationIdY,
        Source::GlobalInvocationIdZ,
        Source::LocalInvocationIdX,
        Source::LocalInvocationIdY,
        Source::LocalInvocationIdZ,
        Source::WorkgroupIdX,
        Source::WorkgroupIdY,
        Source::WorkgroupIdZ,
        Source::NumWorkgroupsX,
        Source::NumWorkgroupsY,
        Source::NumWorkgroupsZ,
        Source::LocalInvocationIndex,
    ]
    .into_iter()
    .enumerate()
    .map(|(arg_index, source)| SpirvBuiltinArgument {
        arg_index: arg_index as u32,
        source,
    })
    .collect()
}

fn role_payload_ty<'db>(
    db: &'db DriverDataBase,
    behavior: hir::hir_def::Func<'db>,
    role_path: PathId<'db>,
    stage_name: &str,
) -> Result<TyId<'db>, WebBundleError> {
    let role_ty = resolve_metadata_ty(db, role_path, behavior.scope()).ok_or_else(|| {
        WebBundleError::EntryDerivation(format!("{stage_name}-stage role did not resolve"))
    })?;
    let [payload, ..] = role_ty.generic_args(db) else {
        return Err(WebBundleError::EntryDerivation(format!(
            "{stage_name}-stage role requires one varying payload type"
        )));
    };
    Ok(*payload)
}

fn raster_draw_count(
    db: &DriverDataBase,
    behavior: hir::hir_def::Func<'_>,
    role_path: PathId<'_>,
) -> Result<u32, WebBundleError> {
    let role_ty = resolve_metadata_ty(db, role_path, behavior.scope()).ok_or_else(|| {
        WebBundleError::EntryDerivation("vertex-stage role did not resolve".to_owned())
    })?;
    let [_, draw, ..] = role_ty.generic_args(db) else {
        return Err(WebBundleError::EntryDerivation(
            "vertex-stage role requires a Fe-authored draw policy".to_owned(),
        ));
    };
    let attrs = nominal_attrs(db, *draw).ok_or_else(|| {
        WebBundleError::EntryDerivation(
            "vertex-stage draw policy is not an attributed nominal type".to_owned(),
        )
    })?;
    if attrs.gpu_draw(db) != Some(GpuDraw::TriangleList) {
        return Err(WebBundleError::EntryDerivation(
            "authored raster currently requires `#[gpu_draw(triangle_list)]`".to_owned(),
        ));
    }
    let [count, ..] = draw.generic_args(db) else {
        return Err(WebBundleError::EntryDerivation(
            "triangle-list draw policy requires one concrete vertex count".to_owned(),
        ));
    };
    let count = semantic_const_u32(db, *count).ok_or_else(|| {
        WebBundleError::EntryDerivation(
            "triangle-list vertex count must be a concrete u32-sized integer".to_owned(),
        )
    })?;
    if count == 0 {
        return Err(WebBundleError::EntryDerivation(
            "triangle-list vertex count must be nonzero".to_owned(),
        ));
    }
    Ok(count)
}

fn is_primitive(db: &DriverDataBase, ty: TyId<'_>, primitive: PrimTy) -> bool {
    let ty = ty.as_view(db).unwrap_or(ty);
    matches!(
        ty.base_ty(db).data(db),
        TyData::TyBase(TyBase::Prim(found)) if *found == primitive
    )
}

fn raster_vertex_stage<'db>(
    db: &'db DriverDataBase,
    behavior: hir::hir_def::Func<'db>,
    role_path: PathId<'db>,
) -> Result<(WebActorStageKind, TyId<'db>), WebBundleError> {
    let payload = role_payload_ty(db, behavior, role_path, "vertex")?;
    let vertex_count = raster_draw_count(db, behavior, role_path)?;
    let args = behavior.arg_tys(db);
    let Some(vertex_index) = args.first() else {
        return Err(WebBundleError::EntryDerivation(
            "authored vertex behavior must take one `u32` vertex-index context before actor state"
                .to_owned(),
        ));
    };
    if !is_primitive(db, *vertex_index.skip_binder(), PrimTy::U32) {
        return Err(WebBundleError::EntryDerivation(
            "authored vertex behavior's first context argument must be exactly `u32`".to_owned(),
        ));
    }
    let output = behavior.return_ty(db);
    if !nominal_attrs(db, output).is_some_and(|attrs| attrs.is_gpu_vertex_output(db)) {
        return Err(WebBundleError::EntryDerivation(
            "authored vertex behavior must return the nominal `#[gpu_vertex_output]` record"
                .to_owned(),
        ));
    }
    let output_fields = output.field_types(db);
    let [position, returned_payload] = output_fields.as_slice() else {
        return Err(WebBundleError::EntryDerivation(
            "`#[gpu_vertex_output]` must contain exactly clip position and varying payload fields"
                .to_owned(),
        ));
    };
    if !nominal_attrs(db, *position).is_some_and(|attrs| attrs.is_gpu_clip_position(db))
        || position.field_types(db).len() != 4
        || position
            .field_types(db)
            .into_iter()
            .any(|field| !is_primitive(db, field, PrimTy::F32))
    {
        return Err(WebBundleError::EntryDerivation(
            "authored vertex output position must be the four-f32 nominal `#[gpu_clip_position]` record"
                .to_owned(),
        ));
    }
    if returned_payload != &payload {
        return Err(WebBundleError::EntryDerivation(
            "authored vertex role payload differs from its returned varying payload".to_owned(),
        ));
    }
    let varying = canonical_type_from_semantic(db, payload, "raster_varying")
        .map_err(|error| WebBundleError::EntryDerivation(error.to_string()))?;
    if !matches!(varying, CanonicalType::Record(_)) {
        return Err(WebBundleError::EntryDerivation(
            "authored raster varying payload must be a nominal record".to_owned(),
        ));
    }
    fn all_f32(ty: &CanonicalType) -> bool {
        match ty {
            CanonicalType::F32 => true,
            CanonicalType::Record(fields) => fields.iter().all(|field| all_f32(&field.ty)),
            _ => false,
        }
    }
    if !all_f32(&varying) {
        return Err(WebBundleError::EntryDerivation(
            "authored raster varying payload currently supports recursively nested f32 records only"
                .to_owned(),
        ));
    }
    Ok((
        WebActorStageKind::Vertex {
            varying,
            vertex_count,
        },
        payload,
    ))
}

fn raster_fragment_stage<'db>(
    db: &'db DriverDataBase,
    behavior: hir::hir_def::Func<'db>,
    role_path: PathId<'db>,
) -> Result<(WebActorStageKind, TyId<'db>), WebBundleError> {
    let payload = role_payload_ty(db, behavior, role_path, "raster fragment")?;
    let args = behavior.arg_tys(db);
    let Some(varying_arg) = args.first() else {
        return Err(WebBundleError::EntryDerivation(
            "authored raster fragment behavior must take its varying payload before actor state"
                .to_owned(),
        ));
    };
    let varying_arg = *varying_arg.skip_binder();
    let varying_arg = varying_arg.as_view(db).unwrap_or(varying_arg);
    if varying_arg != payload {
        return Err(WebBundleError::EntryDerivation(
            "authored raster fragment role payload differs from its varying argument".to_owned(),
        ));
    }
    if !is_primitive(db, behavior.return_ty(db), PrimTy::U32)
        && !is_primitive(db, behavior.return_ty(db), PrimTy::I32)
    {
        return Err(WebBundleError::EntryDerivation(
            "authored raster fragment behavior must return a packed `u32` or `i32` color"
                .to_owned(),
        ));
    }
    let varying = canonical_type_from_semantic(db, payload, "raster_varying")
        .map_err(|error| WebBundleError::EntryDerivation(error.to_string()))?;
    Ok((WebActorStageKind::RasterFragment { varying }, payload))
}

fn semantic_const_u32(db: &DriverDataBase, ty: TyId<'_>) -> Option<u32> {
    let TyData::ConstTy(value) = ty.data(db) else {
        return None;
    };
    let evaluated = value.evaluate(db, None);
    let ConstTyData::Evaluated(EvaluatedConstTy::LitInt(value), _) = evaluated.data(db) else {
        return None;
    };
    u32::try_from(value.data(db).clone()).ok()
}

fn semantic_const_triplet(db: &DriverDataBase, ty: TyId<'_>) -> Option<[u32; 3]> {
    let [x, y, z, ..] = ty.generic_args(db) else {
        return None;
    };
    Some([
        semantic_const_u32(db, *x)?,
        semantic_const_u32(db, *y)?,
        semantic_const_u32(db, *z)?,
    ])
}

fn resource_element(
    db: &DriverDataBase,
    ty: TyId<'_>,
    path: &str,
) -> Result<WebActorResourceElement, WebBundleError> {
    let ty = ty.as_view(db).unwrap_or(ty);
    if matches!(
        ty.base_ty(db).data(db),
        TyData::TyBase(TyBase::Prim(PrimTy::U32))
    ) {
        return Ok(WebActorResourceElement::U32);
    }
    let adt = ty.adt_def(db).ok_or_else(|| {
        WebBundleError::EntryDerivation(format!(
            "resource `{path}` element must be `u32` or a POD record"
        ))
    })?;
    let AdtRef::Struct(struct_) = adt.adt_ref(db) else {
        return Err(WebBundleError::EntryDerivation(format!(
            "resource `{path}` element must be `u32` or a POD record"
        )));
    };
    let field_views = FieldParent::Struct(struct_).fields(db).collect::<Vec<_>>();
    let field_tys = ty.field_types(db);
    if field_views.is_empty() || field_views.len() != field_tys.len() {
        return Err(WebBundleError::EntryDerivation(format!(
            "resource `{path}` POD record has no fields or inconsistent semantic metadata"
        )));
    }
    let mut fields = Vec::with_capacity(field_views.len());
    for (index, (field, field_ty)) in field_views.into_iter().zip(field_tys).enumerate() {
        if !matches!(
            field_ty.base_ty(db).data(db),
            TyData::TyBase(TyBase::Prim(PrimTy::U32))
        ) {
            return Err(WebBundleError::EntryDerivation(format!(
                "resource `{path}` POD field {} must be exactly `u32`",
                index
            )));
        }
        let name = field
            .name(db)
            .map(|name| name.data(db).to_string())
            .ok_or_else(|| {
                WebBundleError::EntryDerivation(format!(
                    "resource `{path}` POD fields must be named"
                ))
            })?;
        fields.push(WebActorResourceField {
            name,
            offset: u32::try_from(index).unwrap() * 4,
        });
    }
    let span = u32::try_from(fields.len()).unwrap() * 4;
    Ok(WebActorResourceElement::Record { fields, span })
}

fn actor_resources(
    db: &DriverDataBase,
    actor: &SemanticActor<'_>,
) -> Result<Vec<WebActorResource>, WebBundleError> {
    let assumptions = PredicateListId::empty_list(db);
    let mut resources = Vec::new();
    for (field_index, field) in actor.state.hir_fields(db).data(db).iter().enumerate() {
        let Some(type_ref) = field.type_ref().to_opt() else {
            continue;
        };
        let ty = lower_hir_ty(db, type_ref, actor.state.scope(), assumptions);
        let Some(attrs) = nominal_attrs(db, ty) else {
            continue;
        };
        if attrs.gpu_resource(db) != Some(GpuResource::Storage) {
            continue;
        }
        let [element_ty, length_ty, ..] = ty.generic_args(db) else {
            return Err(WebBundleError::EntryDerivation(
                "storage resource type requires element and length arguments".to_owned(),
            ));
        };
        let name = field
            .name
            .to_opt()
            .map(|name| name.data(db).to_string())
            .ok_or_else(|| {
                WebBundleError::EntryDerivation(
                    "actor storage resource fields must be named".to_owned(),
                )
            })?;
        let length = semantic_const_u32(db, *length_ty).ok_or_else(|| {
            WebBundleError::EntryDerivation(format!(
                "storage resource `{name}` length must be a concrete u32-sized integer"
            ))
        })?;
        if length == 0 {
            return Err(WebBundleError::EntryDerivation(format!(
                "storage resource `{name}` length must be nonzero"
            )));
        }
        resources.push(WebActorResource {
            field_index: u32::try_from(field_index).unwrap(),
            element: resource_element(db, *element_ty, &name)?,
            name,
            length,
        });
    }
    Ok(resources)
}

pub fn actor_gpu_program(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
) -> Result<Option<WebActorProgram>, WebBundleError> {
    let actors = semantic_actors(db, top_mod);
    let gpu_actors = actors
        .iter()
        .filter(|actor| actor_is_gpu_program(db, actor))
        .collect::<Vec<_>>();
    let actor = match gpu_actors.as_slice() {
        [] => return Ok(None),
        [actor] => *actor,
        _ => {
            return Err(WebBundleError::EntryDerivation(format!(
                "module declares {} attributed GPU-program actors; exactly one is required",
                gpu_actors.len()
            )));
        }
    };
    let actor_name = actor
        .state
        .name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
        .ok_or_else(|| {
            WebBundleError::EntryDerivation(
                "attributed GPU-program actor has no resolvable name".to_owned(),
            )
        })?;
    let mut stages = Vec::new();
    // Kept in lockstep with `stages` only while resolving the semantic actor.
    // The serializable-free program descriptor retains the canonical varying
    // shape, while exact nominal identity is checked here before it is erased.
    let mut raster_payloads = Vec::new();
    for behavior in &actor.behaviors {
        let Some((stage, role_path)) = behavior_stage_role(db, *behavior) else {
            continue;
        };
        let source_entry = behavior
            .name(db)
            .to_opt()
            .map(|name| name.data(db).to_string())
            .ok_or_else(|| {
                WebBundleError::EntryDerivation(
                    "attributed GPU-stage behavior has no resolvable name".to_owned(),
                )
            })?;
        let (kind, raster_payload) = match stage {
            GpuStage::Vertex => {
                let (kind, payload) = raster_vertex_stage(db, *behavior, role_path)?;
                (kind, Some(payload))
            }
            GpuStage::Fragment => (WebActorStageKind::Fragment, None),
            GpuStage::RasterFragment => {
                let (kind, payload) = raster_fragment_stage(db, *behavior, role_path)?;
                (kind, Some(payload))
            }
            GpuStage::Compute => {
                let (workgroup_size, dispatch, repeat, cycle) =
                    compute_stage_shape(db, *behavior, role_path)?;
                let invocation_context = compute_invocation_context(db, *behavior)?;
                (
                    WebActorStageKind::Compute {
                        workgroup_size,
                        dispatch,
                        repeat,
                        cycle,
                        invocation_context,
                    },
                    None,
                )
            }
        };
        stages.push(WebActorStage { source_entry, kind });
        raster_payloads.push(raster_payload);
    }

    // A raster pass is one adjacent vertex/fragment pair. This grammar admits
    // any ordered mixture of compute, fullscreen, and raster passes without a
    // second pass manifest or application-named pipeline table. In particular,
    // a small raster pass may overlay a fullscreen distance field while both
    // stages continue to consume the same Fe actor state.
    let mut index = 0;
    while index < stages.len() {
        match &stages[index].kind {
            WebActorStageKind::Vertex {
                varying: vertex, ..
            } => {
                let Some(next) = stages.get(index + 1) else {
                    return Err(WebBundleError::EntryDerivation(
                        "authored raster vertex behavior must be followed by its paired raster-fragment behavior"
                            .to_owned(),
                    ));
                };
                let WebActorStageKind::RasterFragment { varying: fragment } = &next.kind else {
                    return Err(WebBundleError::EntryDerivation(
                        "authored raster vertex behavior must be followed by its paired raster-fragment behavior"
                            .to_owned(),
                    ));
                };
                if raster_payloads[index] != raster_payloads[index + 1] {
                    return Err(WebBundleError::EntryDerivation(
                        "authored raster vertex and fragment behaviors use different varying payload types"
                            .to_owned(),
                    ));
                }
                debug_assert_eq!(vertex, fragment);
                index += 2;
            }
            WebActorStageKind::RasterFragment { .. } => {
                return Err(WebBundleError::EntryDerivation(
                    "authored raster fragment behavior must immediately follow its paired vertex behavior"
                        .to_owned(),
                ));
            }
            WebActorStageKind::Compute { .. } | WebActorStageKind::Fragment => index += 1,
        }
    }
    validate_actor_pass_cycles(&stages)?;
    Ok(Some(WebActorProgram {
        actor: actor_name,
        stages,
        resources: actor_resources(db, actor)?,
    }))
}

fn validate_actor_pass_cycles(stages: &[WebActorStage]) -> Result<(), WebBundleError> {
    let mut completed = std::collections::HashSet::new();
    let mut active: Option<&str> = None;
    let mut active_repeat = 0;
    for stage in stages {
        let cycle = match &stage.kind {
            WebActorStageKind::Compute { cycle, .. } => cycle.as_ref(),
            _ => None,
        };
        match cycle {
            Some(cycle) if active == Some(cycle.group.as_str()) => {
                if active_repeat != cycle.repeat {
                    return Err(WebBundleError::EntryDerivation(format!(
                        "actor pass cycle `{}` uses inconsistent repeat counts",
                        cycle.group
                    )));
                }
            }
            Some(cycle) => {
                if let Some(previous) = active.take() {
                    completed.insert(previous.to_owned());
                }
                if completed.contains(&cycle.group) {
                    return Err(WebBundleError::EntryDerivation(format!(
                        "actor pass cycle `{}` must occupy one consecutive stage range",
                        cycle.group
                    )));
                }
                active = Some(cycle.group.as_str());
                active_repeat = cycle.repeat;
            }
            None => {
                if let Some(previous) = active.take() {
                    completed.insert(previous.to_owned());
                }
            }
        }
    }
    Ok(())
}

fn behavior_surface_control_kind(
    db: &DriverDataBase,
    behavior: hir::hir_def::Func<'_>,
) -> Option<GpuControl> {
    behavior
        .actor_roles(db)
        .data(db)
        .iter()
        .filter_map(|role| role.key_path.to_opt())
        .filter_map(|path| resolve_metadata_ty(db, path, behavior.scope()))
        .filter_map(|ty| nominal_attrs(db, ty))
        .find_map(|attrs| attrs.gpu_control(db))
}

fn behavior_is_surface_control(db: &DriverDataBase, behavior: hir::hir_def::Func<'_>) -> bool {
    matches!(
        behavior_surface_control_kind(db, behavior),
        Some(GpuControl::Surface | GpuControl::TypedSurface)
    )
}

fn behavior_surface_policy_ty<'db>(
    db: &'db DriverDataBase,
    behavior: Func<'db>,
) -> Option<TyId<'db>> {
    behavior
        .actor_roles(db)
        .data(db)
        .iter()
        .filter_map(|role| role.key_path.to_opt())
        .filter_map(|path| resolve_metadata_ty(db, path, behavior.scope()))
        .find_map(|ty| {
            let attrs = nominal_attrs(db, ty)?;
            if attrs.gpu_control(db) != Some(GpuControl::SurfaceSchedule) {
                return None;
            }
            let [policy_ty] = ty.generic_args(db) else {
                return None;
            };
            Some(*policy_ty)
        })
}

fn actor_surface_quality_policy_tys<'db>(
    db: &'db DriverDataBase,
    actor: &SemanticActor<'db>,
) -> Vec<TyId<'db>> {
    actor
        .state
        .actor_placement(db)
        .data(db)
        .iter()
        .filter_map(|role| role.key_path.to_opt())
        .filter_map(|path| resolve_metadata_ty(db, path, actor.state.scope()))
        .filter_map(|ty| {
            let attrs = nominal_attrs(db, ty)?;
            if attrs.gpu_control(db) != Some(GpuControl::SurfaceQuality) {
                return None;
            }
            let [policy_ty] = ty.generic_args(db) else {
                return None;
            };
            Some(*policy_ty)
        })
        .collect()
}

fn actor_surface_recovery_policy_tys<'db>(
    db: &'db DriverDataBase,
    actor: &SemanticActor<'db>,
) -> Vec<TyId<'db>> {
    actor
        .state
        .actor_placement(db)
        .data(db)
        .iter()
        .filter_map(|role| role.key_path.to_opt())
        .filter_map(|path| resolve_metadata_ty(db, path, actor.state.scope()))
        .filter_map(|ty| {
            let attrs = nominal_attrs(db, ty)?;
            if attrs.gpu_control(db) != Some(GpuControl::SurfaceRecovery) {
                return None;
            }
            let [policy_ty] = ty.generic_args(db) else {
                return None;
            };
            Some(*policy_ty)
        })
        .collect()
}

fn gpu_actor_name_for_entry(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    source_entry: &str,
) -> Option<String> {
    semantic_actors(db, top_mod)
        .into_iter()
        .find(|actor| {
            actor_is_gpu_program(db, actor)
                && actor.behaviors.iter().any(|behavior| {
                    behavior
                        .name(db)
                        .to_opt()
                        .is_some_and(|name| name.data(db) == source_entry)
                })
        })?
        .state
        .name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
}

/// Select every actor-scoped task belonging to the GPU actor that owns the
/// page-facing render entry. A self-receiving task consumes the same complete
/// non-resource state snapshot as the render transition. Resource-bearing
/// self tasks stay fail-closed until the Wasm host can preserve opaque GPU
/// authorities across the task boundary.
fn render_actor_scoped_task_entries(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    source_entry: &str,
    resource_field_indices: &[u32],
) -> Result<Vec<String>, WebBundleError> {
    let Some(actor_name) = gpu_actor_name_for_entry(db, top_mod, source_entry) else {
        return Ok(Vec::new());
    };
    let actors = semantic_actors(db, top_mod);
    let actor = actors
        .iter()
        .find(|actor| {
            actor
                .state
                .name(db)
                .to_opt()
                .is_some_and(|name| name.data(db) == &actor_name)
        })
        .ok_or_else(|| {
            WebBundleError::EntryDerivation(format!(
                "GPU actor `{actor_name}` has no semantic state declaration"
            ))
        })?;
    let state_fields = actor.state.hir_fields(db).data(db);
    let assumptions = PredicateListId::empty_list(db);
    let mut entries = Vec::new();
    for task in actor
        .behaviors
        .iter()
        .copied()
        .filter(|behavior| behavior_is_scoped_task(db, *behavior))
    {
        let name = task
            .name(db)
            .to_opt()
            .map(|name| name.data(db).to_owned())
            .ok_or_else(|| {
                WebBundleError::EntryDerivation(format!(
                    "GPU actor `{actor_name}` has an unnamed scoped task"
                ))
            })?;
        let task_args = task.arg_tys(db);
        if task_args.is_empty() {
            entries.push(name);
            continue;
        }
        if !resource_field_indices.is_empty() {
            return Err(WebBundleError::EntryDerivation(format!(
                "GPU actor `{actor_name}` scoped task `{name}` receives self while the actor owns GPU resources; opaque resource custody is not yet available to Wasm tasks"
            )));
        }
        if task_args.len() != state_fields.len() {
            return Err(WebBundleError::EntryDerivation(format!(
                "GPU actor `{actor_name}` scoped task `{name}` must be self-less or take self as exactly {} flattened actor-state arguments; found {}",
                state_fields.len(),
                task_args.len(),
            )));
        }
        for (index, (field, actual)) in state_fields.iter().zip(&task_args).enumerate() {
            let type_ref = field.type_ref().to_opt().ok_or_else(|| {
                WebBundleError::EntryDerivation(format!(
                    "GPU actor `{actor_name}` state field {index} has no resolved type"
                ))
            })?;
            let expected = lower_hir_ty(db, type_ref, actor.state.scope(), assumptions);
            let expected =
                canonical_type_from_semantic(db, expected, &format!("render_actor_state.{index}"))
                    .map_err(|error| WebBundleError::EntryDerivation(error.to_string()))?;
            let actual = canonical_type_from_semantic(
                db,
                *actual.skip_binder(),
                &format!("render_scoped_task_state.{index}"),
            )
            .map_err(|error| WebBundleError::EntryDerivation(error.to_string()))?;
            if actual != expected {
                return Err(WebBundleError::EntryDerivation(format!(
                    "GPU actor `{actor_name}` scoped task `{name}` state argument {index} differs from the actor field: expected {expected:?}, got {actual:?}"
                )));
            }
        }
        entries.push(name);
    }
    Ok(entries)
}

/// The render entry and mode derived from a module's unique GPU-program actor,
/// or `None` when the module declares no such actor (the pre-actor flag path).
///
/// This resolves the actor's placement and behavior role types, then consumes
/// their nominal GPU attributes. Source spellings and imported aliases do not
/// participate in classification.
pub fn actor_web_entry(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
) -> Result<Option<(String, WebBundleMode)>, WebBundleError> {
    let actors = semantic_actors(db, top_mod);
    let gpu_actors: Vec<&SemanticActor<'_>> = actors
        .iter()
        .filter(|actor| actor_is_gpu_program(db, actor))
        .collect();
    let actor = match gpu_actors.as_slice() {
        [] => return Ok(None),
        [actor] => *actor,
        _ => {
            return Err(WebBundleError::EntryDerivation(format!(
                "module declares {} attributed GPU-program actors; the web entry cannot be derived from more than one",
                gpu_actors.len()
            )));
        }
    };

    let fullscreen_behaviors: Vec<hir::hir_def::Func<'_>> = actor
        .behaviors
        .iter()
        .copied()
        .filter(|behavior| matches!(behavior_stage(db, *behavior), Some(GpuStage::Fragment)))
        .collect();
    if fullscreen_behaviors.len() > 1 {
        return Err(WebBundleError::EntryDerivation(format!(
            "GPU-program actor `{}` declares {} fullscreen fragment-stage behaviors; a render program has at most one",
            actor
                .state
                .name(db)
                .to_opt()
                .map(|name| name.data(db))
                .map_or("<unnamed>", |name| name.as_str()),
            fullscreen_behaviors.len()
        )));
    }
    // A unique fullscreen behavior remains the page-facing surface entry even
    // when later authored raster pairs overlay it. Pure raster actors retain
    // their unique raster-fragment entry. Pass order itself stays in Fe source.
    let raster_fragment_behaviors: Vec<hir::hir_def::Func<'_>> = actor
        .behaviors
        .iter()
        .copied()
        .filter(|behavior| {
            matches!(
                behavior_stage(db, *behavior),
                Some(GpuStage::RasterFragment)
            )
        })
        .collect();
    let entry = match fullscreen_behaviors.as_slice() {
        [behavior] => Some(*behavior),
        [] => match raster_fragment_behaviors.as_slice() {
            [behavior] => Some(*behavior),
            [] => None,
            _ => {
                return Err(WebBundleError::EntryDerivation(format!(
                    "GPU-program actor `{}` declares {} raster fragment-stage behaviors without one fullscreen surface entry",
                    actor
                        .state
                        .name(db)
                        .to_opt()
                        .map(|name| name.data(db))
                        .map_or("<unnamed>", |name| name.as_str()),
                    raster_fragment_behaviors.len()
                )));
            }
        },
        _ => unreachable!("multiple fullscreen behaviors rejected above"),
    };
    match entry {
        Some(behavior) => Ok(Some((
            behavior
                .name(db)
                .to_opt()
                .map(|name| name.data(db).to_string())
                .ok_or_else(|| {
                    WebBundleError::EntryDerivation(
                        "attributed fragment behavior has no resolvable name".to_owned(),
                    )
                })?,
            WebBundleMode::Render,
        ))),
        None => Err(WebBundleError::EntryDerivation(format!(
            "GPU-program actor `{}` has no behavior carrying `#[gpu_stage(fragment)]` or `#[gpu_stage(raster_fragment)]`",
            actor
                .state
                .name(db)
                .to_opt()
                .map(|name| name.data(db))
                .map_or("<unnamed>", |name| name.as_str())
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
#[derive(Debug, Clone, PartialEq, Eq)]
struct ActorStateLeafMetadata {
    name: String,
    path: Vec<String>,
    doc: Option<String>,
}

fn append_actor_state_leaves(
    ty: &CanonicalType,
    field_path: &[String],
    doc: &Option<String>,
    leaves: &mut Vec<ActorStateLeafMetadata>,
) -> Result<(), WebBundleError> {
    let field_name = field_path
        .last()
        .expect("actor state leaves always have a field path");
    match ty {
        CanonicalType::Record(fields) => {
            for field in fields {
                let mut nested_path = field_path.to_vec();
                nested_path.push(field.name.clone());
                append_actor_state_leaves(&field.ty, &nested_path, doc, leaves)?;
            }
        }
        CanonicalType::Bool
        | CanonicalType::U8
        | CanonicalType::I32
        | CanonicalType::U32
        | CanonicalType::I64
        | CanonicalType::U64
        | CanonicalType::F32 => leaves.push(ActorStateLeafMetadata {
            name: field_name.to_owned(),
            path: field_path.to_vec(),
            doc: doc.clone(),
        }),
        CanonicalType::Variant(variants)
            if !variants.is_empty() && variants.iter().all(|variant| variant.fields.is_empty()) =>
        {
            leaves.push(ActorStateLeafMetadata {
                name: field_name.to_owned(),
                path: field_path.to_vec(),
                doc: doc.clone(),
            })
        }
        CanonicalType::Variant(_) => {
            return Err(WebBundleError::SurfaceProjection(format!(
                "GPU actor state field `{field_name}` has a payload enum, which cannot be flattened into one uniform member"
            )));
        }
        CanonicalType::Bytes | CanonicalType::String | CanonicalType::List { .. } => {
            return Err(WebBundleError::SurfaceProjection(format!(
                "GPU actor state field `{field_name}` has non-scalar browser state `{ty:?}`"
            )));
        }
    }
    Ok(())
}

/// Keep the familiar leaf name whenever it is already unique. When nested
/// value types repeat names such as `x`, derive the shortest unique semantic
/// suffix (`origin.x`, `right.x`, ...) instead of rejecting the Fe structure
/// or forcing authors to flatten it by hand.
fn qualify_repeated_actor_leaf_names(
    actor_name: &str,
    leaves: &mut [ActorStateLeafMetadata],
) -> Result<(), WebBundleError> {
    for index in 0..leaves.len() {
        let path_len = leaves[index].path.len();
        let mut resolved = None;
        for depth in 1..=path_len {
            let candidate = leaves[index].path[path_len - depth..].join(".");
            let unique = leaves.iter().enumerate().all(|(other_index, other)| {
                if other_index == index || other.path.len() < depth {
                    return true;
                }
                other.path[other.path.len() - depth..].join(".") != candidate
            });
            if unique {
                resolved = Some(candidate);
                break;
            }
        }
        leaves[index].name = resolved.ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "GPU actor `{actor_name}` recursively flattens indistinguishable state path `{}`",
                leaves[index].path.join("."),
            ))
        })?;
    }
    Ok(())
}

/// Resolve an actor's complete non-resource state from semantic field types.
/// The structural declaration contributes source names/docs; recursive record
/// leaves come from the nominal Fe types rather than a parallel binding table.
fn actor_state_shape(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    source_entry: &str,
    resource_field_indices: &[u32],
) -> Result<Option<(CanonicalType, Vec<ActorStateLeafMetadata>)>, WebBundleError> {
    let Some(actor_name) = gpu_actor_name_for_entry(db, top_mod, source_entry) else {
        return Ok(None);
    };
    let declaration = hir::lower::module_actor_decls(db, top_mod)
        .into_iter()
        .find(|actor| actor.name == actor_name)
        .ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "GPU actor `{actor_name}` has no structural declaration"
            ))
        })?;
    let actors = semantic_actors(db, top_mod);
    let actor = actors
        .iter()
        .find(|actor| {
            actor
                .state
                .name(db)
                .to_opt()
                .is_some_and(|name| name.data(db) == &actor_name)
        })
        .ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "GPU actor `{actor_name}` has no semantic state declaration"
            ))
        })?;
    let semantic_fields = actor.state.hir_fields(db).data(db);
    if semantic_fields.len() != declaration.fields.len() {
        return Err(WebBundleError::SurfaceProjection(format!(
            "GPU actor `{actor_name}` has inconsistent structural and semantic state fields"
        )));
    }

    let assumptions = PredicateListId::empty_list(db);
    let mut fields = Vec::new();
    let mut leaves = Vec::new();
    for (index, (field, semantic_field)) in
        declaration.fields.iter().zip(semantic_fields).enumerate()
    {
        if resource_field_indices.contains(&(index as u32)) {
            continue;
        }
        let type_ref = semantic_field.type_ref().to_opt().ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "GPU actor `{actor_name}` state field `{}` has no resolved type",
                field.name
            ))
        })?;
        let ty = lower_hir_ty(db, type_ref, actor.state.scope(), assumptions);
        let canonical =
            canonical_type_from_semantic(db, ty, &format!("surface_state.{}", field.name))
                .map_err(|error| WebBundleError::SurfaceProjection(error.to_string()))?;
        append_actor_state_leaves(
            &canonical,
            std::slice::from_ref(&field.name),
            &field.doc,
            &mut leaves,
        )?;
        fields.push(CanonicalField::new(field.name.clone(), canonical));
    }
    qualify_repeated_actor_leaf_names(&actor_name, &mut leaves)?;
    Ok(Some((CanonicalType::Record(fields), leaves)))
}

fn behavior_is_actor_initializer(db: &DriverDataBase, behavior: Func<'_>) -> bool {
    behavior
        .actor_roles(db)
        .data(db)
        .iter()
        .filter_map(|role| role.key_path.to_opt())
        .filter_map(|path| resolve_metadata_ty(db, path, behavior.scope()))
        .filter_map(|ty| nominal_attrs(db, ty))
        .any(|attrs| attrs.is_actor_initializer(db))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SurfaceInitializerContract {
    source_entry: String,
    results: Vec<WebControlWasmType>,
}

/// Select an optional GPU-state initializer by its nominal Fe role and prove
/// that it returns the actor's complete nested state. This is compile-only
/// metadata; the browser discovers only the fixed Wasm export.
fn surface_initializer_contract(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    source_entry: &str,
    resource_field_indices: &[u32],
) -> Result<Option<SurfaceInitializerContract>, WebBundleError> {
    let Some(actor_name) = gpu_actor_name_for_entry(db, top_mod, source_entry) else {
        return Ok(None);
    };
    let actors = semantic_actors(db, top_mod);
    let actor = actors
        .iter()
        .find(|actor| {
            actor_is_gpu_program(db, actor)
                && actor
                    .state
                    .name(db)
                    .to_opt()
                    .is_some_and(|name| name.data(db) == &actor_name)
        })
        .ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "GPU actor `{actor_name}` has no semantic declaration"
            ))
        })?;
    let initializers = actor
        .behaviors
        .iter()
        .copied()
        .filter(|behavior| behavior_is_actor_initializer(db, *behavior))
        .collect::<Vec<_>>();
    let initializer = match initializers.as_slice() {
        [] => return Ok(None),
        [initializer] => *initializer,
        _ => {
            return Err(WebBundleError::SurfaceProjection(format!(
                "GPU actor `{actor_name}` declares {} complete-state initializers; at most one is allowed",
                initializers.len()
            )));
        }
    };
    let initializer_name = initializer
        .name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
        .ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "GPU actor `{actor_name}` has an unnamed state initializer"
            ))
        })?;
    if !initializer.arg_tys(db).is_empty() {
        return Err(WebBundleError::SurfaceProjection(format!(
            "GPU state initializer `{initializer_name}` must be self-less and take no arguments"
        )));
    }
    if !resource_field_indices.is_empty() {
        return Err(WebBundleError::SurfaceProjection(format!(
            "GPU state initializer `{initializer_name}` cannot initialize external resource handles"
        )));
    }
    let (state, _) = actor_state_shape(db, top_mod, source_entry, resource_field_indices)?
        .expect("initializer containing actor must have state shape");
    let initialized =
        canonical_type_from_semantic(db, initializer.return_ty(db), "initial_surface_state")
            .map_err(|error| WebBundleError::SurfaceProjection(error.to_string()))?;
    if initialized != state {
        return Err(WebBundleError::SurfaceProjection(format!(
            "GPU state initializer `{initializer_name}` must return complete actor state: expected {state:?}, got {initialized:?}"
        )));
    }
    let mut results = Vec::new();
    append_canonical_wasm_types(&initialized, &mut results, "initial_surface_state")?;
    Ok(Some(SurfaceInitializerContract {
        source_entry: initializer_name,
        results,
    }))
}

fn with_surface_initializer(
    options: WasmCompileOptions,
    contract: &SurfaceInitializerContract,
) -> WasmCompileOptions {
    options.with_export_alias(&contract.source_entry, SURFACE_INITIALIZER_EXPORT)
}

fn verify_surface_initializer_export(
    wasm: &[u8],
    contract: &SurfaceInitializerContract,
) -> Result<(), WebBundleError> {
    let (params, results) = wasm_export_signature(wasm, SURFACE_INITIALIZER_EXPORT).ok_or_else(
        || {
            WebBundleError::SurfaceProjection(format!(
                "GPU state initializer `{}` has no fixed Wasm export `{SURFACE_INITIALIZER_EXPORT}`",
                contract.source_entry
            ))
        },
    )?;
    if !params.is_empty() || results != contract.results {
        return Err(WebBundleError::SurfaceProjection(format!(
            "GPU state initializer `{}` has measured Wasm signature {params:?} -> {results:?}; expected [] -> {:?} from the complete Fe actor state",
            contract.source_entry, contract.results
        )));
    }
    Ok(())
}

fn project_actor_field_metadata(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    source_entry: &str,
    layout: &mut WebLayout,
    resource_field_indices: &[u32],
) -> Result<(), WebBundleError> {
    let Some((_, fields)) = actor_state_shape(db, top_mod, source_entry, resource_field_indices)?
    else {
        return Ok(());
    };
    let mut members = layout
        .bindings
        .iter()
        .enumerate()
        .filter(|(_, binding)| binding.role == WebBindingRole::Input)
        .flat_map(|(binding_index, binding)| {
            binding
                .members
                .iter()
                .enumerate()
                .map(move |(member_index, member)| (member.arg_index, binding_index, member_index))
        })
        .collect::<Vec<_>>();
    members.sort_by_key(|(arg_index, _, _)| *arg_index);
    if members.len() != fields.len() {
        return Err(WebBundleError::SurfaceProjection(format!(
            "GPU actor `{source_entry}` has {} recursively flattened state leaves but its shader layout has {} input members",
            fields.len(),
            members.len()
        )));
    }
    for ((_, binding_index, member_index), field) in members.into_iter().zip(fields) {
        let member = &mut layout.bindings[binding_index].members[member_index];
        member.name = field.name;
        member.doc = field.doc;
    }
    Ok(())
}

/// The reserved const behavior a render actor declares to project its
/// interactive surface (design 1.2).
const VIEW_BEHAVIOR: &str = "view";

/// The reserved gesture-argument name vocabulary an `UpdateSurface` behavior's
/// own declared (pre-flatten) params must draw from, kept IDENTICAL to the
/// `mandel_view_ctl.fe` / `clifford_ctl.fe` fixtures' established convention.
/// Anything else is a projection error (an unrecognized gesture arg name),
/// never a silent guess.
fn gesture_arg_source(name: &str) -> Option<WebControlArgSource> {
    match name {
        "dx" => Some(WebControlArgSource::Drag {
            axis: "x".to_string(),
        }),
        "dy" => Some(WebControlArgSource::Drag {
            axis: "y".to_string(),
        }),
        "dzoom" => Some(WebControlArgSource::Wheel),
        "mx" => Some(WebControlArgSource::Pointer {
            axis: "x".to_string(),
        }),
        "my" => Some(WebControlArgSource::Pointer {
            axis: "y".to_string(),
        }),
        _ => None,
    }
}

/// Finds the render actor's (at most one) `UpdateSurface`-marked behavior and
/// returns its export NAME, or `Ok(None)` when the actor declares none (the
/// non-interactive path: today's sketches, byte-stable). Errors naming the
/// actor when more than one is declared (a render program has at most one
/// control behavior, mirroring `actor_web_entry`'s `FragmentSurface` rule).
fn actor_update_export_name(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    source_entry: &str,
) -> Result<Option<String>, WebBundleError> {
    let actors = semantic_actors(db, top_mod);
    let Some(actor) = actors.iter().find(|actor| {
        actor_is_gpu_program(db, actor)
            && actor.behaviors.iter().any(|behavior| {
                behavior
                    .name(db)
                    .to_opt()
                    .is_some_and(|name| name.data(db) == source_entry)
            })
    }) else {
        return Ok(None);
    };
    let update_behaviors: Vec<hir::hir_def::Func<'_>> = actor
        .behaviors
        .iter()
        .copied()
        .filter(|behavior| behavior_is_surface_control(db, *behavior))
        .collect();
    match update_behaviors.as_slice() {
        [] => Ok(None),
        [behavior] => Ok(behavior
            .name(db)
            .to_opt()
            .map(|name| name.data(db).to_string())),
        _ => Err(WebBundleError::SurfaceProjection(format!(
            "actor `{}` declares {} surface-control behaviors; a render program has at most one",
            actor
                .state
                .name(db)
                .to_opt()
                .map(|name| name.data(db))
                .map_or("<unnamed>", |name| name.as_str()),
            update_behaviors.len()
        ))),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TypedSurfaceTransitionContract {
    params: Vec<WebControlWasmType>,
    results: Vec<WebControlWasmType>,
    event_tag_limits: Vec<(usize, u32)>,
    state_tag_limits: Vec<(usize, u32)>,
    coalesce_tag_field: usize,
    coalesce_tag_variant: u32,
    actor_param_is_resource: Vec<bool>,
    scheduled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TypedSurfaceScheduleContract {
    event_fields: usize,
    state_fields: usize,
    decision_fields: usize,
    event_tag_limits: Vec<(usize, u32)>,
    state_tag_limits: Vec<(usize, u32)>,
}

/// An ordinary Fe policy function selected solely from the nominal
/// `SurfaceScheduling<P>` capability and its structurally unique inherent
/// implementation. No behavior/function name participates in selection.
#[derive(Debug, Clone)]
struct ResolvedSurfaceSchedulePolicy<'db> {
    func: Func<'db>,
    event_first: bool,
    contract: TypedSurfaceScheduleContract,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TypedSurfaceQualityContract {
    fact_fields: usize,
    decision_fields: usize,
}

/// An ordinary Fe policy selected solely from the actor's nominal
/// `SurfaceQuality<P>` capability. It is a pure, stateless sibling of the
/// resident scheduling policy and never becomes manifest data.
#[derive(Debug, Clone)]
struct ResolvedSurfaceQualityPolicy<'db> {
    func: Func<'db>,
    contract: TypedSurfaceQualityContract,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TypedSurfaceRecoveryContract {
    event_fields: usize,
    state_fields: usize,
    decision_fields: usize,
    event_tag_limits: Vec<(usize, u32)>,
    state_tag_limits: Vec<(usize, u32)>,
}

#[derive(Debug, Clone)]
struct ResolvedSurfaceRecoveryPolicy<'db> {
    func: Func<'db>,
    event_first: bool,
    contract: TypedSurfaceRecoveryContract,
}

fn typed_surface_transition_export(contract: &TypedSurfaceTransitionContract) -> &'static str {
    if contract.scheduled {
        TYPED_SURFACE_SCHEDULED_EXPORT
    } else {
        TYPED_SURFACE_TRANSITION_EXPORT
    }
}

fn with_typed_surface_export(
    options: WasmCompileOptions,
    source: &str,
    contract: &TypedSurfaceTransitionContract,
) -> WasmCompileOptions {
    if contract.scheduled {
        options.with_surface_frame(
            source,
            TYPED_SURFACE_SCHEDULED_EXPORT,
            TYPED_SURFACE_STATE_REPLACE_EXPORT,
            contract.event_tag_limits.clone(),
            contract.state_tag_limits.clone(),
            contract.coalesce_tag_field,
            contract.coalesce_tag_variant,
            contract.actor_param_is_resource.clone(),
        )
    } else {
        options.with_export_alias(source, TYPED_SURFACE_TRANSITION_EXPORT)
    }
}

fn typed_surface_wasm_signature(
    contract: &TypedSurfaceTransitionContract,
) -> (Vec<WebControlWasmType>, Vec<WebControlWasmType>) {
    let params = if contract.scheduled {
        // Raw SurfaceEvent records live in exported linear memory. The fixed
        // wrapper receives `(pointer, count)` followed only by inert external
        // resource slots. Complete non-resource state is resident in private
        // Wasm globals and is not couriered per frame.
        let mut params = vec![WebControlWasmType::I32, WebControlWasmType::I32];
        params.extend(
            contract.params[TYPED_SURFACE_EVENT_FIELDS..]
                .iter()
                .zip(&contract.actor_param_is_resource)
                .filter_map(|(ty, is_resource)| is_resource.then_some(*ty)),
        );
        params
    } else {
        contract.params.clone()
    };
    (params, contract.results.clone())
}

fn canonical_surface_event_kind_type() -> CanonicalType {
    CanonicalType::Variant(
        [
            "gesture",
            "param_edit",
            "animation_frame",
            "gpu_complete",
            "visible",
            "hidden",
            "device_lost",
            "device_recovered",
            "pointer_down",
            "pointer_move",
            "pointer_up",
        ]
        .into_iter()
        .map(|name| CanonicalVariant {
            name: name.to_owned(),
            fields: Vec::new(),
        })
        .collect(),
    )
}

fn canonical_surface_event_type() -> CanonicalType {
    CanonicalType::Record(vec![
        CanonicalField::new("pointer_x", CanonicalType::F32),
        CanonicalField::new("pointer_y", CanonicalType::F32),
        CanonicalField::new("delta_x", CanonicalType::F32),
        CanonicalField::new("delta_y", CanonicalType::F32),
        CanonicalField::new("wheel_delta", CanonicalType::F32),
        CanonicalField::new("wheel_mode", CanonicalType::U32),
        CanonicalField::new("buttons", CanonicalType::U32),
        CanonicalField::new("timestamp", CanonicalType::F32),
        CanonicalField::new("width", CanonicalType::F32),
        CanonicalField::new("height", CanonicalType::F32),
        CanonicalField::new("event_kind", canonical_surface_event_kind_type()),
        CanonicalField::new("param_index", CanonicalType::U32),
        CanonicalField::new("param_value", CanonicalType::F32),
    ])
}

fn canonical_surface_schedule_event_type() -> CanonicalType {
    CanonicalType::Record(vec![
        CanonicalField::new("kind", canonical_surface_event_kind_type()),
        CanonicalField::new("timestamp", CanonicalType::F32),
        CanonicalField::new("pending_events", CanonicalType::U32),
    ])
}

fn canonical_gpu_device_loss_reason_type() -> CanonicalType {
    CanonicalType::Variant(
        ["not_lost", "unknown", "destroyed"]
            .into_iter()
            .map(|name| CanonicalVariant {
                name: name.to_owned(),
                fields: Vec::new(),
            })
            .collect(),
    )
}

fn canonical_gpu_device_event_kind_type() -> CanonicalType {
    CanonicalType::Variant(
        ["unknown", "available", "lost", "unavailable"]
            .into_iter()
            .map(|name| CanonicalVariant {
                name: name.to_owned(),
                fields: Vec::new(),
            })
            .collect(),
    )
}

fn canonical_surface_schedule_state_type() -> CanonicalType {
    CanonicalType::Record(vec![
        CanonicalField::new("presenting", CanonicalType::Bool),
        CanonicalField::new("visible", CanonicalType::Bool),
        CanonicalField::new("device_lost", CanonicalType::Bool),
        CanonicalField::new("last_presented_at", CanonicalType::F32),
        CanonicalField::new("deadline", CanonicalType::F32),
        CanonicalField::new("observed_inputs", CanonicalType::U32),
    ])
}

fn canonical_surface_queue_action_type() -> CanonicalType {
    CanonicalType::Variant(
        ["retain", "keep_latest", "drop"]
            .into_iter()
            .map(|name| CanonicalVariant {
                name: name.to_owned(),
                fields: Vec::new(),
            })
            .collect(),
    )
}

fn canonical_surface_recovery_action_type() -> CanonicalType {
    CanonicalType::Variant(
        [
            "no_action",
            "retry_device",
            "degrade_to_wasm",
            "fail_surface",
        ]
        .into_iter()
        .map(|name| CanonicalVariant {
            name: name.to_owned(),
            fields: Vec::new(),
        })
        .collect(),
    )
}

fn canonical_surface_recovery_event_type() -> CanonicalType {
    CanonicalType::Record(vec![
        CanonicalField::new("kind", canonical_gpu_device_event_kind_type()),
        CanonicalField::new("reason", canonical_gpu_device_loss_reason_type()),
        CanonicalField::new("device_required", CanonicalType::Bool),
        CanonicalField::new("software_fallback", CanonicalType::Bool),
        CanonicalField::new("generation", CanonicalType::U32),
    ])
}

fn canonical_surface_recovery_state_type() -> CanonicalType {
    CanonicalType::Record(vec![
        CanonicalField::new("device_lost", CanonicalType::Bool),
        CanonicalField::new("attempts", CanonicalType::U32),
    ])
}

fn canonical_surface_recovery_step_type() -> CanonicalType {
    CanonicalType::Record(vec![
        CanonicalField::new("state", canonical_surface_recovery_state_type()),
        CanonicalField::new("action", canonical_surface_recovery_action_type()),
    ])
}

fn canonical_surface_schedule_step_type() -> CanonicalType {
    CanonicalType::Record(vec![
        CanonicalField::new("state", canonical_surface_schedule_state_type()),
        CanonicalField::new("present", CanonicalType::Bool),
        CanonicalField::new("request_frame", CanonicalType::Bool),
        CanonicalField::new("queue", canonical_surface_queue_action_type()),
    ])
}

fn canonical_surface_quality_facts_type() -> CanonicalType {
    CanonicalType::Record(vec![
        CanonicalField::new("css_width", CanonicalType::F32),
        CanonicalField::new("css_height", CanonicalType::F32),
        CanonicalField::new("device_pixel_ratio", CanonicalType::F32),
        CanonicalField::new("declared_width", CanonicalType::F32),
        CanonicalField::new("declared_height", CanonicalType::F32),
        CanonicalField::new("max_texture_dimension_2d", CanonicalType::F32),
        CanonicalField::new("coarse_pointer", CanonicalType::Bool),
        CanonicalField::new("gpu_available", CanonicalType::Bool),
    ])
}

fn canonical_surface_backing_extent_type() -> CanonicalType {
    CanonicalType::Record(vec![
        CanonicalField::new("width", CanonicalType::F32),
        CanonicalField::new("height", CanonicalType::F32),
    ])
}

fn append_canonical_wasm_types(
    ty: &CanonicalType,
    output: &mut Vec<WebControlWasmType>,
    path: &str,
) -> Result<(), WebBundleError> {
    match ty {
        CanonicalType::Bool | CanonicalType::U8 | CanonicalType::I32 | CanonicalType::U32 => {
            output.push(WebControlWasmType::I32)
        }
        CanonicalType::I64 | CanonicalType::U64 => output.push(WebControlWasmType::I64),
        CanonicalType::F32 => output.push(WebControlWasmType::F32),
        CanonicalType::Record(fields) => {
            for field in fields {
                append_canonical_wasm_types(&field.ty, output, &format!("{path}.{}", field.name))?;
            }
        }
        CanonicalType::Variant(variants)
            if !variants.is_empty() && variants.iter().all(|variant| variant.fields.is_empty()) =>
        {
            output.push(WebControlWasmType::I32)
        }
        CanonicalType::Bytes
        | CanonicalType::String
        | CanonicalType::List { .. }
        | CanonicalType::Variant(_) => {
            return Err(WebBundleError::SurfaceProjection(format!(
                "typed surface ABI `{path}` must be a closed scalar record, got {ty:?}"
            )));
        }
    }
    Ok(())
}

/// Return the flattened scalar leaf count while deriving fieldless-enum bounds
/// from the resolved Fe type. This is boundary validation metadata, not an
/// application manifest: it stays inside the generated Wasm wrapper.
fn surface_scalar_tag_limits(
    ty: &CanonicalType,
    path: &str,
    offset: usize,
    output: &mut Vec<(usize, u32)>,
) -> Result<usize, WebBundleError> {
    match ty {
        CanonicalType::Bool
        | CanonicalType::U8
        | CanonicalType::I32
        | CanonicalType::U32
        | CanonicalType::I64
        | CanonicalType::U64
        | CanonicalType::F32 => Ok(1),
        CanonicalType::Record(fields) => {
            let mut count = 0usize;
            for field in fields {
                count += surface_scalar_tag_limits(
                    &field.ty,
                    &format!("{path}.{}", field.name),
                    offset + count,
                    output,
                )?;
            }
            Ok(count)
        }
        CanonicalType::Variant(variants)
            if !variants.is_empty() && variants.iter().all(|variant| variant.fields.is_empty()) =>
        {
            let limit = u32::try_from(variants.len()).map_err(|_| {
                WebBundleError::SurfaceProjection(format!(
                    "typed surface ABI `{path}` has too many fieldless enum variants"
                ))
            })?;
            output.push((offset, limit));
            Ok(1)
        }
        _ => Err(WebBundleError::SurfaceProjection(format!(
            "typed surface ABI `{path}` must be a closed scalar record, got {ty:?}"
        ))),
    }
}

/// Derive the homogeneous pointer-motion batch key from the validated fixed
/// `SurfaceEvent` shape. The enum ordinal comes from Fe's resolved variant
/// order; neither an example nor the browser host supplies a numeric table.
fn surface_event_coalesce_key(event: &CanonicalType) -> Result<(usize, u32), WebBundleError> {
    let CanonicalType::Record(fields) = event else {
        return Err(WebBundleError::SurfaceProjection(
            "typed SurfaceEvent must be a record".to_owned(),
        ));
    };
    let mut offset = 0usize;
    for field in fields {
        if field.name == "event_kind" {
            let CanonicalType::Variant(variants) = &field.ty else {
                return Err(WebBundleError::SurfaceProjection(
                    "typed SurfaceEvent.event_kind must be a fieldless enum".to_owned(),
                ));
            };
            let variant = variants
                .iter()
                .position(|variant| variant.name == "pointer_move")
                .ok_or_else(|| {
                    WebBundleError::SurfaceProjection(
                        "typed SurfaceEventKind is missing its pointer_move variant".to_owned(),
                    )
                })?;
            return Ok((offset, variant as u32));
        }
        offset += surface_scalar_tag_limits(&field.ty, &field.name, offset, &mut Vec::new())?;
    }
    Err(WebBundleError::SurfaceProjection(
        "typed SurfaceEvent is missing its event_kind field".to_owned(),
    ))
}

/// Resolve and validate the manifest-free typed surface-transition contract.
/// `Ok(None)` is the legacy `UpdateSurface` lane. A typed role fails closed on
/// any mismatch: nominal event marker, complete event shape, complete
/// non-resource state response, or scalar Wasm transport layout.
fn typed_surface_transition_contract(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    source_entry: &str,
    control_export: &str,
    resource_field_indices: &[u32],
) -> Result<Option<TypedSurfaceTransitionContract>, WebBundleError> {
    let actors = semantic_actors(db, top_mod);
    let actor = actors
        .iter()
        .find(|actor| {
            actor_is_gpu_program(db, actor)
                && actor.behaviors.iter().any(|behavior| {
                    behavior
                        .name(db)
                        .to_opt()
                        .is_some_and(|name| name.data(db) == source_entry)
                })
        })
        .ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "typed control export `{control_export}` has no containing GPU actor"
            ))
        })?;
    let behavior = actor
        .behaviors
        .iter()
        .copied()
        .find(|behavior| {
            behavior
                .name(db)
                .to_opt()
                .is_some_and(|name| name.data(db) == control_export)
        })
        .ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "typed control export `{control_export}` was not found semantically"
            ))
        })?;
    let scheduled = behavior_surface_policy_ty(db, behavior).is_some();
    if behavior_surface_control_kind(db, behavior) != Some(GpuControl::TypedSurface) {
        if scheduled {
            return Err(WebBundleError::SurfaceProjection(format!(
                "surface behavior `{control_export}` may declare a GPU schedule only with the typed SurfaceTransition role"
            )));
        }
        return Ok(None);
    }

    let actor_name = actor
        .state
        .name(db)
        .to_opt()
        .map(|name| name.data(db).to_string())
        .unwrap_or_else(|| "<unnamed>".to_owned());
    let decl = hir::lower::module_actor_decls(db, top_mod)
        .into_iter()
        .find(|decl| decl.name == actor_name)
        .ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "typed surface actor `{actor_name}` has no structural declaration"
            ))
        })?;
    let arg_tys = behavior.arg_tys(db);
    let expected_args = 1 + decl.fields.len();
    if arg_tys.len() != expected_args {
        return Err(WebBundleError::SurfaceProjection(format!(
            "typed surface behavior `{control_export}` must take exactly one SurfaceEvent context record before {} actor fields; found {} semantic arguments",
            decl.fields.len(),
            arg_tys.len()
        )));
    }
    let event_ty = *arg_tys[0].skip_binder();
    if !nominal_attrs(db, event_ty).is_some_and(|attrs| attrs.is_web_surface_event(db)) {
        return Err(WebBundleError::SurfaceProjection(format!(
            "typed surface behavior `{control_export}` must take the nominal #[web_surface_event] record"
        )));
    }
    let event = canonical_type_from_semantic(db, event_ty, "surface_event")
        .map_err(|error| WebBundleError::SurfaceProjection(error.to_string()))?;
    let expected_event = canonical_surface_event_type();
    if event != expected_event {
        return Err(WebBundleError::SurfaceProjection(format!(
            "typed surface behavior `{control_export}` event shape differs from the fixed SurfaceEvent ABI: expected {expected_event:?}, got {event:?}"
        )));
    }

    let mut state_fields = Vec::new();
    for (index, (field, ty)) in decl.fields.iter().zip(&arg_tys[1..]).enumerate() {
        if resource_field_indices.contains(&(index as u32)) {
            continue;
        }
        let ty = canonical_type_from_semantic(
            db,
            *ty.skip_binder(),
            &format!("surface_state.{}", field.name),
        )
        .map_err(|error| WebBundleError::SurfaceProjection(error.to_string()))?;
        state_fields.push(CanonicalField::new(field.name.clone(), ty));
    }
    let expected_state = CanonicalType::Record(state_fields);
    let returned_state =
        canonical_type_from_semantic(db, behavior.return_ty(db), "surface_state_response")
            .map_err(|error| WebBundleError::SurfaceProjection(error.to_string()))?;
    if returned_state != expected_state {
        return Err(WebBundleError::SurfaceProjection(format!(
            "typed surface behavior `{control_export}` must return the actor's complete non-resource state record in declaration order: expected {expected_state:?}, got {returned_state:?}"
        )));
    }

    let mut params = Vec::new();
    let mut event_tag_limits = Vec::new();
    let mut actor_param_is_resource = Vec::new();
    append_canonical_wasm_types(&event, &mut params, "surface_event")?;
    let event_fields =
        surface_scalar_tag_limits(&event, "surface_event", 0, &mut event_tag_limits)?;
    if event_fields != TYPED_SURFACE_EVENT_FIELDS {
        return Err(WebBundleError::SurfaceProjection(format!(
            "typed surface event flattened to {event_fields} scalar leaves; expected {TYPED_SURFACE_EVENT_FIELDS}"
        )));
    }
    let (coalesce_tag_field, coalesce_tag_variant) = surface_event_coalesce_key(&event)?;
    for (index, ty) in arg_tys[1..].iter().enumerate() {
        if resource_field_indices.contains(&(index as u32)) {
            // GPU resource values are inert handles in the control-only Wasm
            // lane. The fixed host supplies zero; no resource operation is
            // available to this transition.
            params.push(WebControlWasmType::I64);
            actor_param_is_resource.push(true);
            continue;
        }
        let ty = canonical_type_from_semantic(
            db,
            *ty.skip_binder(),
            &format!("surface_state.{}", decl.fields[index].name),
        )
        .map_err(|error| WebBundleError::SurfaceProjection(error.to_string()))?;
        let before = params.len();
        append_canonical_wasm_types(&ty, &mut params, "surface_state")?;
        actor_param_is_resource.extend(std::iter::repeat_n(false, params.len() - before));
    }
    let mut results = Vec::new();
    append_canonical_wasm_types(&returned_state, &mut results, "surface_state_response")?;
    let mut state_tag_limits = Vec::new();
    let state_fields = surface_scalar_tag_limits(
        &returned_state,
        "surface_state_response",
        0,
        &mut state_tag_limits,
    )?;
    if state_fields != results.len() {
        return Err(WebBundleError::SurfaceProjection(format!(
            "typed surface state flattened inconsistently: canonical shape has {state_fields} scalar leaves but Wasm ABI has {}",
            results.len()
        )));
    }
    Ok(Some(TypedSurfaceTransitionContract {
        params,
        results,
        event_tag_limits,
        state_tag_limits,
        coalesce_tag_field,
        coalesce_tag_variant,
        actor_param_is_resource,
        scheduled,
    }))
}

fn surface_schedule_arg_order(db: &DriverDataBase, func: Func<'_>) -> Option<bool> {
    let arg_tys = func.arg_tys(db);
    if arg_tys.len() != 2
        || !nominal_attrs(db, func.return_ty(db))
            .is_some_and(|attrs| attrs.is_web_surface_schedule_step(db))
    {
        return None;
    }
    let first = *arg_tys[0].skip_binder();
    let second = *arg_tys[1].skip_binder();
    let first_attrs = nominal_attrs(db, first)?;
    let second_attrs = nominal_attrs(db, second)?;
    if first_attrs.is_web_surface_schedule_event(db)
        && second_attrs.is_web_surface_schedule_state(db)
    {
        Some(true)
    } else if first_attrs.is_web_surface_schedule_state(db)
        && second_attrs.is_web_surface_schedule_event(db)
    {
        Some(false)
    } else {
        None
    }
}

/// Resolve a policy from the typed capability on the application transition.
/// Its nominal policy type identifies an inherent `impl`; exactly one public
/// ordinary Fe function in that impl must have the schedule event/state/step
/// shape. Function names and argument labels are deliberately irrelevant.
fn resolve_surface_schedule_policy<'db>(
    db: &'db DriverDataBase,
    top_mod: TopLevelMod<'db>,
    source_entry: &str,
    control_export: &str,
) -> Result<Option<ResolvedSurfaceSchedulePolicy<'db>>, WebBundleError> {
    let actors = semantic_actors(db, top_mod);
    let actor = actors
        .iter()
        .find(|actor| {
            actor_is_gpu_program(db, actor)
                && actor.behaviors.iter().any(|behavior| {
                    behavior
                        .name(db)
                        .to_opt()
                        .is_some_and(|name| name.data(db) == source_entry)
                })
        })
        .ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "typed control export `{control_export}` has no containing GPU actor"
            ))
        })?;
    let behavior = actor
        .behaviors
        .iter()
        .copied()
        .find(|behavior| {
            behavior
                .name(db)
                .to_opt()
                .is_some_and(|name| name.data(db) == control_export)
        })
        .ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "typed control export `{control_export}` was not found semantically"
            ))
        })?;
    let Some(policy_ty) = behavior_surface_policy_ty(db, behavior) else {
        return Ok(None);
    };
    let separate_schedule_behaviors = actor
        .behaviors
        .iter()
        .copied()
        .filter(|candidate| {
            behavior_surface_control_kind(db, *candidate) == Some(GpuControl::SurfaceSchedule)
        })
        .count();
    if separate_schedule_behaviors != 0 {
        return Err(WebBundleError::SurfaceProjection(format!(
            "surface actor `{source_entry}` declares {separate_schedule_behaviors} redundant surface-schedule behavior(s); select the policy only through SurfaceScheduling<P> on its SurfaceTransition"
        )));
    }

    let policy_name = policy_ty.pretty_print(db);
    let policy_ingot = policy_ty.ingot(db).ok_or_else(|| {
        WebBundleError::SurfaceProjection(format!(
            "SurfaceScheduling policy `{policy_name}` is not owned by a resolvable Fe ingot"
        ))
    })?;
    let candidates = policy_ingot
        .all_impls(db)
        .iter()
        .copied()
        .filter(|impl_| impl_.admissible_inherent_impl_ty(db) == Some(policy_ty))
        .flat_map(|impl_| impl_.funcs(db))
        .filter(|func| {
            func.vis(db) == Visibility::Public && !func.is_extern(db) && func.body(db).is_some()
        })
        .filter_map(|func| {
            surface_schedule_arg_order(db, func).map(|event_first| (func, event_first))
        })
        .collect::<Vec<_>>();
    let (func, event_first) = match candidates.as_slice() {
        [(func, event_first)] => (*func, *event_first),
        [] => {
            return Err(WebBundleError::SurfaceProjection(format!(
                "SurfaceScheduling policy `{policy_name}` has no unique public Fe implementation with nominal SurfaceScheduleEvent/SurfaceScheduleState -> SurfaceScheduleStep shape"
            )));
        }
        _ => {
            return Err(WebBundleError::SurfaceProjection(format!(
                "SurfaceScheduling policy `{policy_name}` has {} structurally matching Fe implementations; exactly one is required",
                candidates.len()
            )));
        }
    };
    let contract = typed_surface_schedule_contract(db, func, event_first, &policy_name)?;
    Ok(Some(ResolvedSurfaceSchedulePolicy {
        func,
        event_first,
        contract,
    }))
}

fn surface_quality_function_shape(db: &DriverDataBase, func: Func<'_>) -> bool {
    let arg_tys = func.arg_tys(db);
    arg_tys.len() == 1
        && nominal_attrs(db, *arg_tys[0].skip_binder())
            .is_some_and(|attrs| attrs.is_web_surface_quality_facts(db))
        && nominal_attrs(db, func.return_ty(db))
            .is_some_and(|attrs| attrs.is_web_surface_backing_extent(db))
}

/// Resolve the backing policy selected on the GPU actor itself. Actor-level
/// placement keeps the policy available to pure compute/render graphs which
/// have no application transition and therefore no control behavior to adorn.
fn resolve_surface_quality_policy<'db>(
    db: &'db DriverDataBase,
    top_mod: TopLevelMod<'db>,
    source_entry: &str,
) -> Result<Option<ResolvedSurfaceQualityPolicy<'db>>, WebBundleError> {
    let actors = semantic_actors(db, top_mod);
    let Some(actor) = actors.iter().find(|actor| {
        actor_is_gpu_program(db, actor)
            && actor.behaviors.iter().any(|behavior| {
                behavior
                    .name(db)
                    .to_opt()
                    .is_some_and(|name| name.data(db) == source_entry)
            })
    }) else {
        return Ok(None);
    };
    let policies = actor_surface_quality_policy_tys(db, actor);
    let policy_ty = match policies.as_slice() {
        [] => return Ok(None),
        [policy] => *policy,
        _ => {
            return Err(WebBundleError::SurfaceProjection(format!(
                "surface actor `{source_entry}` selects {} SurfaceQuality policies; exactly one is allowed",
                policies.len()
            )));
        }
    };
    let policy_name = policy_ty.pretty_print(db);
    let policy_ingot = policy_ty.ingot(db).ok_or_else(|| {
        WebBundleError::SurfaceProjection(format!(
            "SurfaceQuality policy `{policy_name}` is not owned by a resolvable Fe ingot"
        ))
    })?;
    let candidates = policy_ingot
        .all_impls(db)
        .iter()
        .copied()
        .filter(|impl_| impl_.admissible_inherent_impl_ty(db) == Some(policy_ty))
        .flat_map(|impl_| impl_.funcs(db))
        .filter(|func| {
            func.vis(db) == Visibility::Public
                && !func.is_extern(db)
                && func.body(db).is_some()
                && surface_quality_function_shape(db, *func)
        })
        .collect::<Vec<_>>();
    let func = match candidates.as_slice() {
        [func] => *func,
        [] => {
            return Err(WebBundleError::SurfaceProjection(format!(
                "SurfaceQuality policy `{policy_name}` has no unique public Fe implementation with nominal SurfaceQualityFacts -> SurfaceBackingExtent shape"
            )));
        }
        _ => {
            return Err(WebBundleError::SurfaceProjection(format!(
                "SurfaceQuality policy `{policy_name}` has {} structurally matching Fe implementations; exactly one is required",
                candidates.len()
            )));
        }
    };
    let contract = typed_surface_quality_contract(db, func, &policy_name)?;
    Ok(Some(ResolvedSurfaceQualityPolicy { func, contract }))
}

fn typed_surface_quality_contract(
    db: &DriverDataBase,
    func: Func<'_>,
    policy_name: &str,
) -> Result<TypedSurfaceQualityContract, WebBundleError> {
    let arg_tys = func.arg_tys(db);
    if arg_tys.len() != 1 {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceQuality policy `{policy_name}` must take exactly one SurfaceQualityFacts record; found {} semantic arguments",
            arg_tys.len()
        )));
    }
    let facts_ty = *arg_tys[0].skip_binder();
    if !nominal_attrs(db, facts_ty).is_some_and(|attrs| attrs.is_web_surface_quality_facts(db)) {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceQuality policy `{policy_name}` must take the nominal #[web_surface_quality_facts] record"
        )));
    }
    let result_ty = func.return_ty(db);
    if !nominal_attrs(db, result_ty).is_some_and(|attrs| attrs.is_web_surface_backing_extent(db)) {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceQuality policy `{policy_name}` must return the nominal #[web_surface_backing_extent] record"
        )));
    }
    let facts = canonical_type_from_semantic(db, facts_ty, "surface_quality_facts")
        .map_err(|error| WebBundleError::SurfaceProjection(error.to_string()))?;
    let extent = canonical_type_from_semantic(db, result_ty, "surface_backing_extent")
        .map_err(|error| WebBundleError::SurfaceProjection(error.to_string()))?;
    let expected_facts = canonical_surface_quality_facts_type();
    let expected_extent = canonical_surface_backing_extent_type();
    if facts != expected_facts || extent != expected_extent {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceQuality policy `{policy_name}` differs from the fixed typed quality ABI: expected {expected_facts:?} -> {expected_extent:?}; got {facts:?} -> {extent:?}"
        )));
    }
    let mut fact_tag_limits = Vec::new();
    let fact_fields =
        surface_scalar_tag_limits(&facts, "surface_quality_facts", 0, &mut fact_tag_limits)?;
    let mut extent_tag_limits = Vec::new();
    let decision_fields =
        surface_scalar_tag_limits(&extent, "surface_backing_extent", 0, &mut extent_tag_limits)?;
    if fact_fields != 8 || decision_fields != 2 {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceQuality policy `{policy_name}` must flatten to 8 fact and 2 extent leaves; got {fact_fields} and {decision_fields}"
        )));
    }
    Ok(TypedSurfaceQualityContract {
        fact_fields,
        decision_fields,
    })
}

fn surface_recovery_arg_order(db: &DriverDataBase, func: Func<'_>) -> Option<bool> {
    let arg_tys = func.arg_tys(db);
    if arg_tys.len() != 2
        || !nominal_attrs(db, func.return_ty(db))
            .is_some_and(|attrs| attrs.is_web_surface_recovery_step(db))
    {
        return None;
    }
    let first = *arg_tys[0].skip_binder();
    let second = *arg_tys[1].skip_binder();
    let first_attrs = nominal_attrs(db, first)?;
    let second_attrs = nominal_attrs(db, second)?;
    if first_attrs.is_web_surface_recovery_event(db)
        && second_attrs.is_web_surface_recovery_state(db)
    {
        Some(true)
    } else if first_attrs.is_web_surface_recovery_state(db)
        && second_attrs.is_web_surface_recovery_event(db)
    {
        Some(false)
    } else {
        None
    }
}

fn resolve_surface_recovery_policy<'db>(
    db: &'db DriverDataBase,
    top_mod: TopLevelMod<'db>,
    source_entry: &str,
) -> Result<Option<ResolvedSurfaceRecoveryPolicy<'db>>, WebBundleError> {
    let actors = semantic_actors(db, top_mod);
    let Some(actor) = actors.iter().find(|actor| {
        actor_is_gpu_program(db, actor)
            && actor.behaviors.iter().any(|behavior| {
                behavior
                    .name(db)
                    .to_opt()
                    .is_some_and(|name| name.data(db) == source_entry)
            })
    }) else {
        return Ok(None);
    };
    let policies = actor_surface_recovery_policy_tys(db, actor);
    let policy_ty = match policies.as_slice() {
        [] => return Ok(None),
        [policy] => *policy,
        _ => {
            return Err(WebBundleError::SurfaceProjection(format!(
                "surface actor `{source_entry}` selects {} SurfaceRecovery policies; exactly one is allowed",
                policies.len()
            )));
        }
    };
    let policy_name = policy_ty.pretty_print(db);
    let policy_ingot = policy_ty.ingot(db).ok_or_else(|| {
        WebBundleError::SurfaceProjection(format!(
            "SurfaceRecovery policy `{policy_name}` is not owned by a resolvable Fe ingot"
        ))
    })?;
    let candidates = policy_ingot
        .all_impls(db)
        .iter()
        .copied()
        .filter(|impl_| impl_.admissible_inherent_impl_ty(db) == Some(policy_ty))
        .flat_map(|impl_| impl_.funcs(db))
        .filter(|func| {
            func.vis(db) == Visibility::Public && !func.is_extern(db) && func.body(db).is_some()
        })
        .filter_map(|func| {
            surface_recovery_arg_order(db, func).map(|event_first| (func, event_first))
        })
        .collect::<Vec<_>>();
    let (func, event_first) = match candidates.as_slice() {
        [(func, event_first)] => (*func, *event_first),
        [] => {
            return Err(WebBundleError::SurfaceProjection(format!(
                "SurfaceRecovery policy `{policy_name}` has no unique public Fe implementation with nominal SurfaceRecoveryEvent/SurfaceRecoveryState -> SurfaceRecoveryStep shape"
            )));
        }
        _ => {
            return Err(WebBundleError::SurfaceProjection(format!(
                "SurfaceRecovery policy `{policy_name}` has {} structurally matching Fe implementations; exactly one is required",
                candidates.len()
            )));
        }
    };
    let contract = typed_surface_recovery_contract(db, func, event_first, &policy_name)?;
    Ok(Some(ResolvedSurfaceRecoveryPolicy {
        func,
        event_first,
        contract,
    }))
}

fn typed_surface_recovery_contract(
    db: &DriverDataBase,
    func: Func<'_>,
    event_first: bool,
    policy_name: &str,
) -> Result<TypedSurfaceRecoveryContract, WebBundleError> {
    let arg_tys = func.arg_tys(db);
    if arg_tys.len() != 2 {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceRecovery policy `{policy_name}` must take exactly SurfaceRecoveryEvent and SurfaceRecoveryState; found {} semantic arguments",
            arg_tys.len()
        )));
    }
    let (event_ty, state_ty) = if event_first {
        (*arg_tys[0].skip_binder(), *arg_tys[1].skip_binder())
    } else {
        (*arg_tys[1].skip_binder(), *arg_tys[0].skip_binder())
    };
    if !nominal_attrs(db, event_ty).is_some_and(|attrs| attrs.is_web_surface_recovery_event(db)) {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceRecovery policy `{policy_name}` must take the nominal #[web_surface_recovery_event] record"
        )));
    }
    if !nominal_attrs(db, state_ty).is_some_and(|attrs| attrs.is_web_surface_recovery_state(db)) {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceRecovery policy `{policy_name}` must take the nominal #[web_surface_recovery_state] record"
        )));
    }
    let result_ty = func.return_ty(db);
    if !nominal_attrs(db, result_ty).is_some_and(|attrs| attrs.is_web_surface_recovery_step(db)) {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceRecovery policy `{policy_name}` must return the nominal #[web_surface_recovery_step] record"
        )));
    }

    let event = canonical_type_from_semantic(db, event_ty, "surface_recovery_event")
        .map_err(|error| WebBundleError::SurfaceProjection(error.to_string()))?;
    let state = canonical_type_from_semantic(db, state_ty, "surface_recovery_state")
        .map_err(|error| WebBundleError::SurfaceProjection(error.to_string()))?;
    let step = canonical_type_from_semantic(db, result_ty, "surface_recovery_step")
        .map_err(|error| WebBundleError::SurfaceProjection(error.to_string()))?;
    let expected_event = canonical_surface_recovery_event_type();
    let expected_state = canonical_surface_recovery_state_type();
    let expected_step = canonical_surface_recovery_step_type();
    if event != expected_event || state != expected_state || step != expected_step {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceRecovery policy `{policy_name}` differs from the fixed typed recovery ABI: expected {expected_event:?}, {expected_state:?} -> {expected_step:?}; got {event:?}, {state:?} -> {step:?}"
        )));
    }

    let mut event_tag_limits = Vec::new();
    let event_fields =
        surface_scalar_tag_limits(&event, "surface_recovery_event", 0, &mut event_tag_limits)?;
    let mut state_tag_limits = Vec::new();
    let state_fields =
        surface_scalar_tag_limits(&state, "surface_recovery_state", 0, &mut state_tag_limits)?;
    let mut step_tag_limits = Vec::new();
    let step_fields =
        surface_scalar_tag_limits(&step, "surface_recovery_step", 0, &mut step_tag_limits)?;
    let decision_fields = step_fields.checked_sub(state_fields).ok_or_else(|| {
        WebBundleError::SurfaceProjection(format!(
            "SurfaceRecovery policy `{policy_name}` reply is shorter than its resident state"
        ))
    })?;
    if event_fields != 5 || state_fields != 2 || decision_fields != 1 {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceRecovery policy `{policy_name}` must flatten to 5 event, 2 state, and 1 decision leaves; got {event_fields}, {state_fields}, and {decision_fields}"
        )));
    }
    Ok(TypedSurfaceRecoveryContract {
        event_fields,
        state_fields,
        decision_fields,
        event_tag_limits,
        state_tag_limits,
    })
}

/// Compile the exact ordinary Fe recovery policy structurally selected by a
/// GPU actor for native differential execution. The actor's render entry is
/// used only to select its nominal `SurfaceRecovery<P>` capability; the JIT
/// artifact contains the selected Fe policy and its transitive callees, not a
/// parallel Rust implementation of the policy.
#[cfg(all(
    feature = "native-backend",
    not(target_arch = "wasm32"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub fn compile_native_surface_recovery_policy(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    source_entry: &str,
) -> Result<crate::NativeSurfaceRecoveryArtifact, WebBundleError> {
    let policy = resolve_surface_recovery_policy(db, top_mod, source_entry)?.ok_or_else(|| {
        WebBundleError::SurfaceProjection(format!(
            "surface actor `{source_entry}` has no SurfaceRecovery<P> policy"
        ))
    })?;
    let package = mir::build_wasm_runtime_package_for_entries_with_internal_funcs(
        db,
        top_mod,
        &[],
        std::slice::from_ref(&policy.func),
    )
    .map_err(|error| WebBundleError::Lower(error.to_string()))?;
    let symbol = mir::runtime_package_symbol_for_func(db, package, policy.func)
        .map_err(|error| WebBundleError::Lower(error.to_string()))?;
    crate::sonatina::compile_runtime_package_native_surface_recovery(
        db,
        &package,
        &symbol,
        policy.event_first,
    )
    .map_err(|error| WebBundleError::Lower(error.to_string()))
}

/// Compile the exact ordinary Fe scheduling policy structurally selected by a
/// GPU actor for native differential execution. The caller names only the
/// actor's render entry; its control behavior, nominal policy type, unique
/// implementation, argument order, and emitted symbol are all compiler-
/// derived through the same path used by [`WebBundle::compile`].
#[cfg(all(
    feature = "native-backend",
    not(target_arch = "wasm32"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub fn compile_native_surface_schedule_policy(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    source_entry: &str,
) -> Result<crate::NativeSurfaceScheduleArtifact, WebBundleError> {
    let control_export = actor_update_export_name(db, top_mod, source_entry)?.ok_or_else(|| {
        WebBundleError::SurfaceProjection(format!(
            "surface actor `{source_entry}` has no typed control behavior"
        ))
    })?;
    let policy = resolve_surface_schedule_policy(db, top_mod, source_entry, &control_export)?
        .ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "surface actor `{source_entry}` has no SurfaceScheduling<P> policy"
            ))
        })?;
    let package = mir::build_wasm_runtime_package_for_entries_with_internal_funcs(
        db,
        top_mod,
        std::slice::from_ref(&control_export),
        std::slice::from_ref(&policy.func),
    )
    .map_err(|error| WebBundleError::Lower(error.to_string()))?;
    let symbol = mir::runtime_package_symbol_for_func(db, package, policy.func)
        .map_err(|error| WebBundleError::Lower(error.to_string()))?;
    crate::sonatina::compile_runtime_package_native_surface_schedule(
        db,
        &package,
        &symbol,
        policy.event_first,
    )
    .map_err(|error| WebBundleError::Lower(error.to_string()))
}

/// Validate the selected Fe policy's nominal records, then retain only scalar
/// counts and enum bounds for target-neutral wrapper lowering. Nothing is
/// projected into the render manifest or interpreted by the browser.
fn typed_surface_schedule_contract(
    db: &DriverDataBase,
    func: Func<'_>,
    event_first: bool,
    policy_name: &str,
) -> Result<TypedSurfaceScheduleContract, WebBundleError> {
    let arg_tys = func.arg_tys(db);
    if arg_tys.len() != 2 {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceScheduling policy `{policy_name}` must take exactly SurfaceScheduleEvent and SurfaceScheduleState; found {} semantic arguments",
            arg_tys.len()
        )));
    }
    let (event_ty, state_ty) = if event_first {
        (*arg_tys[0].skip_binder(), *arg_tys[1].skip_binder())
    } else {
        (*arg_tys[1].skip_binder(), *arg_tys[0].skip_binder())
    };
    if !nominal_attrs(db, event_ty).is_some_and(|attrs| attrs.is_web_surface_schedule_event(db)) {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceScheduling policy `{policy_name}` must take the nominal #[web_surface_schedule_event] record"
        )));
    }
    if !nominal_attrs(db, state_ty).is_some_and(|attrs| attrs.is_web_surface_schedule_state(db)) {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceScheduling policy `{policy_name}` must take the nominal #[web_surface_schedule_state] record"
        )));
    }
    let result_ty = func.return_ty(db);
    if !nominal_attrs(db, result_ty).is_some_and(|attrs| attrs.is_web_surface_schedule_step(db)) {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceScheduling policy `{policy_name}` must return the nominal #[web_surface_schedule_step] record"
        )));
    }

    let event = canonical_type_from_semantic(db, event_ty, "surface_schedule_event")
        .map_err(|error| WebBundleError::SurfaceProjection(error.to_string()))?;
    let state = canonical_type_from_semantic(db, state_ty, "surface_schedule_state")
        .map_err(|error| WebBundleError::SurfaceProjection(error.to_string()))?;
    let step = canonical_type_from_semantic(db, result_ty, "surface_schedule_step")
        .map_err(|error| WebBundleError::SurfaceProjection(error.to_string()))?;
    let expected_event = canonical_surface_schedule_event_type();
    let expected_state = canonical_surface_schedule_state_type();
    let expected_step = canonical_surface_schedule_step_type();
    if event != expected_event || state != expected_state || step != expected_step {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceScheduling policy `{policy_name}` differs from the fixed typed policy ABI: expected {expected_event:?}, {expected_state:?} -> {expected_step:?}; got {event:?}, {state:?} -> {step:?}"
        )));
    }

    let mut event_tag_limits = Vec::new();
    let event_fields =
        surface_scalar_tag_limits(&event, "surface_schedule_event", 0, &mut event_tag_limits)?;
    let mut state_tag_limits = Vec::new();
    let state_fields =
        surface_scalar_tag_limits(&state, "surface_schedule_state", 0, &mut state_tag_limits)?;
    let mut step_tag_limits = Vec::new();
    let step_fields =
        surface_scalar_tag_limits(&step, "surface_schedule_step", 0, &mut step_tag_limits)?;
    let decision_fields = step_fields.checked_sub(state_fields).ok_or_else(|| {
        WebBundleError::SurfaceProjection(format!(
            "SurfaceScheduling policy `{policy_name}` reply is shorter than its resident state"
        ))
    })?;
    if event_fields != 3 || state_fields != 6 || decision_fields != 3 {
        return Err(WebBundleError::SurfaceProjection(format!(
            "SurfaceScheduling policy `{policy_name}` must flatten to 3 event, 6 state, and 3 decision leaves; got {event_fields}, {state_fields}, and {decision_fields}"
        )));
    }
    Ok(TypedSurfaceScheduleContract {
        event_fields,
        state_fields,
        decision_fields,
        event_tag_limits,
        state_tag_limits,
    })
}

fn with_typed_surface_schedule(
    options: WasmCompileOptions,
    source: &str,
    event_first: bool,
    contract: &TypedSurfaceScheduleContract,
) -> WasmCompileOptions {
    options.with_resident_policy(
        source,
        TYPED_SURFACE_SCHEDULE_EXPORT,
        event_first,
        contract.event_fields,
        contract.state_fields,
        contract.decision_fields,
        contract.event_tag_limits.clone(),
        contract.state_tag_limits.clone(),
    )
}

fn with_typed_surface_quality(
    options: WasmCompileOptions,
    source: &str,
    contract: &TypedSurfaceQualityContract,
) -> WasmCompileOptions {
    options.with_resident_policy(
        source,
        TYPED_SURFACE_QUALITY_EXPORT,
        true,
        contract.fact_fields,
        0,
        contract.decision_fields,
        Vec::new(),
        Vec::new(),
    )
}

fn with_typed_surface_recovery(
    options: WasmCompileOptions,
    source: &str,
    event_first: bool,
    contract: &TypedSurfaceRecoveryContract,
) -> WasmCompileOptions {
    options.with_resident_policy(
        source,
        TYPED_SURFACE_RECOVERY_EXPORT,
        event_first,
        contract.event_fields,
        contract.state_fields,
        contract.decision_fields,
        contract.event_tag_limits.clone(),
        contract.state_tag_limits.clone(),
    )
}

fn validate_surface_schedule_pair(
    transition: Option<&TypedSurfaceTransitionContract>,
    policy: Option<&ResolvedSurfaceSchedulePolicy<'_>>,
    source_entry: &str,
) -> Result<(), WebBundleError> {
    match (
        transition.is_some_and(|contract| contract.scheduled),
        policy,
    ) {
        (true, Some(_)) => Ok(()),
        (true, None) => Err(WebBundleError::SurfaceProjection(format!(
            "surface actor `{source_entry}` selects SurfaceScheduling<P> but P's Fe policy implementation could not be derived"
        ))),
        (false, Some(_)) => Err(WebBundleError::SurfaceProjection(format!(
            "surface actor `{source_entry}` derived a scheduling policy without selecting it on its typed SurfaceTransition"
        ))),
        (false, None) => Ok(()),
    }
}

/// Projects the render actor's `UpdateSurface`-marked behavior (already named
/// by `actor_update_export_name`) into the manifest `control` section (R3
/// param gestures): the compiled wasm export name, its positional argument
/// sources (gesture deltas or current state, in the EXACT order the export
/// expects them), and which leading state fields (declaration order) its
/// reply feeds back. Reconciles against the ACTUAL compiled wasm export
/// signature (wasmparser), never assumed, mirroring `project_surface`'s own
/// "measured, not assumed" reconciliation against the lowered layout.
fn project_control(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    source_entry: &str,
    control_export: &str,
    wasm: &[u8],
    resource_field_indices: &[u32],
) -> Result<Option<WebControl>, WebBundleError> {
    let decls = hir::lower::module_actor_decls(db, top_mod);
    let actor_name = gpu_actor_name_for_entry(db, top_mod, source_entry).ok_or_else(|| {
        WebBundleError::SurfaceProjection(format!(
            "control export `{control_export}` was named but its actor could not be re-found"
        ))
    })?;
    let actor = decls
        .iter()
        .find(|actor| actor.name == actor_name)
        .ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "control export `{control_export}` was named but its actor could not be re-found"
            ))
        })?;
    let behavior = actor
        .behaviors
        .iter()
        .find(|behavior| behavior.name == control_export)
        .ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "actor `{}`: update behavior `{control_export}` was named but not found among its behaviors",
                actor.name
            ))
        })?;

    let state_field_names: Vec<&str> = actor
        .fields
        .iter()
        .enumerate()
        .filter(|(index, _)| !resource_field_indices.contains(&(*index as u32)))
        .map(|(_, field)| field)
        .map(|field| field.name.as_str())
        .collect();
    let typed_contract = typed_surface_transition_contract(
        db,
        top_mod,
        source_entry,
        control_export,
        resource_field_indices,
    )?;
    let wasm_export = typed_contract
        .as_ref()
        .map(typed_surface_transition_export)
        .unwrap_or(control_export);
    let (param_types, result_types) = wasm_export_signature(wasm, wasm_export).ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "actor `{}`: update behavior `{control_export}` has no matching wasm export `{wasm_export}`",
                actor.name
            ))
        })?;
    if let Some(contract) = typed_contract {
        let (expected_params, expected_results) = typed_surface_wasm_signature(&contract);
        if param_types != expected_params || result_types != expected_results {
            return Err(WebBundleError::SurfaceProjection(format!(
                "actor `{}`: typed surface export `{wasm_export}` has measured Wasm signature {param_types:?} -> {result_types:?}; expected {expected_params:?} -> {expected_results:?} from the resolved Fe records and schedule",
                actor.name
            )));
        }
        if contract.scheduled {
            let (replace_params, replace_results) =
                wasm_export_signature(wasm, TYPED_SURFACE_STATE_REPLACE_EXPORT).ok_or_else(
                    || {
                        WebBundleError::SurfaceProjection(format!(
                            "actor `{}`: resident typed surface has no state replacement export `{TYPED_SURFACE_STATE_REPLACE_EXPORT}`",
                            actor.name
                        ))
                    },
                )?;
            if replace_params != contract.results || !replace_results.is_empty() {
                return Err(WebBundleError::SurfaceProjection(format!(
                    "actor `{}`: resident state replacement export has measured Wasm signature {replace_params:?} -> {replace_results:?}; expected {:?} -> [] from the complete Fe state record",
                    actor.name, contract.results
                )));
            }
        }
        return Ok(None);
    }
    let param_count = param_types.len();
    let result_count = result_types.len();
    let expected_param_count = behavior.context_params.len() + actor.fields.len();
    if param_count != expected_param_count {
        return Err(WebBundleError::SurfaceProjection(format!(
            "actor `{}`: update export `{control_export}` takes {param_count} wasm params but the \
             behavior's gesture args ({}) + actor fields ({}) = {expected_param_count}",
            actor.name,
            behavior.context_params.len(),
            actor.fields.len(),
        )));
    }
    if result_count == 0 || result_count > state_field_names.len() {
        return Err(WebBundleError::SurfaceProjection(format!(
            "actor `{}`: update export `{control_export}` returns {result_count} values; expected \
             1..={} (a leading subset of the actor's state fields, declaration order)",
            actor.name,
            state_field_names.len()
        )));
    }
    if result_types.iter().any(|ty| *ty != WebControlWasmType::F32) {
        return Err(WebBundleError::SurfaceProjection(format!(
            "actor `{}`: update export `{control_export}` results must all be f32, got {result_types:?}",
            actor.name
        )));
    }

    let mut args = Vec::with_capacity(expected_param_count);
    for (index, name) in behavior.context_params.iter().enumerate() {
        if param_types[index] != WebControlWasmType::F32 {
            return Err(WebBundleError::SurfaceProjection(format!(
                "actor `{}`: gesture argument `{name}` must lower as f32, got {:?}",
                actor.name, param_types[index]
            )));
        }
        let source = gesture_arg_source(name).ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "actor `{}`: update behavior `{control_export}` has an unrecognized gesture arg \
                 `{name}` (expected one of dx, dy, dzoom, mx, my)",
                actor.name
            ))
        })?;
        args.push(source);
    }
    for (index, field) in actor.fields.iter().enumerate() {
        let name = field.name.clone();
        let wasm_type = param_types[behavior.context_params.len() + index];
        if resource_field_indices.contains(&(index as u32)) {
            if !matches!(wasm_type, WebControlWasmType::I32 | WebControlWasmType::I64) {
                return Err(WebBundleError::SurfaceProjection(format!(
                    "actor `{}`: resource control argument `{name}` must lower as an integer handle, got {wasm_type:?}",
                    actor.name
                )));
            }
            args.push(WebControlArgSource::Resource { name, wasm_type });
        } else {
            if wasm_type != WebControlWasmType::F32 {
                return Err(WebBundleError::SurfaceProjection(format!(
                    "actor `{}`: state control argument `{name}` must lower as f32, got {wasm_type:?}",
                    actor.name
                )));
            }
            args.push(WebControlArgSource::State { name });
        }
    }
    let result: Vec<String> = state_field_names[..result_count]
        .iter()
        .map(|name| (*name).to_string())
        .collect();

    Ok(Some(WebControl {
        export: control_export.to_string(),
        args,
        result,
    }))
}

/// The (param count, result count) of a named function export in a compiled
/// wasm module, measured with `wasmparser` (never assumed). `None` when the
/// export is absent or not a function.
/// The names of every FUNCTION export of a wasm module, in section order.
/// Used to assert that the manifest `source_entry` is actually resolvable by
/// the browser render runtime before the bundle is written.
fn wasm_function_export_names(wasm: &[u8]) -> Vec<String> {
    use wasmparser::{ExternalKind, Payload};

    let mut names = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let Ok(Payload::ExportSection(reader)) = payload {
            for export in reader {
                let Ok(export) = export else { continue };
                if matches!(export.kind, ExternalKind::Func) {
                    names.push(export.name.to_owned());
                }
            }
        }
    }
    names
}

fn wasm_export_signature(
    wasm: &[u8],
    export_name: &str,
) -> Option<(Vec<WebControlWasmType>, Vec<WebControlWasmType>)> {
    use wasmparser::{ExternalKind, Payload, TypeRef};

    let mut func_sigs: Vec<(Vec<WebControlWasmType>, Vec<WebControlWasmType>)> = Vec::new();
    let mut func_type_indices: Vec<u32> = Vec::new();
    let mut imported_func_count: u32 = 0;
    let mut export_func_index: Option<u32> = None;

    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        match payload.ok()? {
            Payload::TypeSection(reader) => {
                for rec in reader {
                    for sub in rec.ok()?.into_types() {
                        let ft = sub.unwrap_func();
                        let convert = |ty| match ty {
                            wasmparser::ValType::I32 => Some(WebControlWasmType::I32),
                            wasmparser::ValType::I64 => Some(WebControlWasmType::I64),
                            wasmparser::ValType::F32 => Some(WebControlWasmType::F32),
                            wasmparser::ValType::F64 => Some(WebControlWasmType::F64),
                            _ => None,
                        };
                        func_sigs.push((
                            ft.params()
                                .iter()
                                .copied()
                                .map(convert)
                                .collect::<Option<Vec<_>>>()?,
                            ft.results()
                                .iter()
                                .copied()
                                .map(convert)
                                .collect::<Option<Vec<_>>>()?,
                        ));
                    }
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    if let TypeRef::Func(_) = import.ok()?.ty {
                        imported_func_count += 1;
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                for tyidx in reader {
                    func_type_indices.push(tyidx.ok()?);
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.ok()?;
                    if export.name == export_name && matches!(export.kind, ExternalKind::Func) {
                        export_func_index = Some(export.index);
                    }
                }
            }
            _ => {}
        }
    }

    let fidx = export_func_index?;
    if fidx < imported_func_count {
        return None;
    }
    let defined = (fidx - imported_func_count) as usize;
    let tyidx = *func_type_indices.get(defined)? as usize;
    func_sigs.get(tyidx).cloned()
}

/// Projects the render actor's `const fn view()` behavior into the manifest
/// `surface` section (protocol v5, R1b; design decision 2). CTFE-evaluates the
/// behavior to a value (via the semantic const machine), walks it, and
/// Reconciles params-record field names against recursively flattened actor
/// state and lowered uniform members. Normally all three sets must match. When
/// the actor also has a nominal `InitialState` behavior, that Fe computation
/// supplies complete defaults and `view()` may deliberately expose a named
/// interactive subset.
///
/// Returns `Ok(None)` for a render entry whose actor declares no `view()`
/// behavior (the v4-compatible path: legacy non-actor bundles, and sketches not
/// yet migrated). There is deliberately no fabricated fallback: a real
/// evaluation gap surfaces as a `SurfaceProjection` error, never a guess.
fn project_surface(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    source_entry: &str,
    layout: &WebLayout,
    resource_field_indices: &[u32],
) -> Result<Option<WebSurface>, WebBundleError> {
    let decls = hir::lower::module_actor_decls(db, top_mod);
    let Some(actor_name) = gpu_actor_name_for_entry(db, top_mod, source_entry) else {
        return Ok(None);
    };
    let Some(actor) = decls.iter().find(|actor| actor.name == actor_name) else {
        return Ok(None);
    };
    // Recognize the reserved `view()` behavior structurally.
    if !actor
        .behaviors
        .iter()
        .any(|behavior| behavior.name == VIEW_BEHAVIOR)
    {
        return Ok(None);
    }

    // The desugared `view` free function (same module, named `view`).
    let view_func = top_mod
        .all_funcs(db)
        .iter()
        .copied()
        .find(|func| {
            func.top_mod(db) == top_mod
                && func
                    .name(db)
                    .to_opt()
                    .is_some_and(|name| name.data(db) == VIEW_BEHAVIOR)
        })
        .ok_or_else(|| {
            WebBundleError::SurfaceProjection(format!(
                "actor `{}` declares a `view()` behavior but its lowered function was not found",
                actor.name
            ))
        })?;

    let view = project_view_surface(db, view_func).map_err(|error| {
        WebBundleError::SurfaceProjection(format!("actor `{}`: {error}", actor.name))
    })?;
    let (_, state_leaves) = actor_state_shape(db, top_mod, source_entry, resource_field_indices)?
        .expect("view-containing GPU actor must have a semantic state shape");

    // The uniform binding members, in arg_index order (== actor field order via
    // the R0 anchor); their names were projected in L0.
    let mut members: Vec<&WebBindingMember> = layout
        .bindings
        .iter()
        .filter(|binding| binding.role == WebBindingRole::Input)
        .flat_map(|binding| binding.members.iter())
        .collect();
    members.sort_by_key(|member| member.arg_index);

    // Reconcile view params <-> recursively flattened actor state <-> binding
    // members by name. An authored complete-state initializer allows `view()`
    // to expose only the interactive subset; without one, retain the strict
    // field-complete rule so no state default is fabricated.
    let view_names: Vec<&str> = view
        .params
        .iter()
        .map(|param| param.name.as_str())
        .collect();
    let field_names: Vec<&str> = state_leaves
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    let member_names: Vec<&str> = members.iter().map(|member| member.name.as_str()).collect();
    let mismatch = || {
        WebBundleError::SurfaceProjection(format!(
            "actor `{}`: the `view()` params {view_names:?} do not reconcile with the recursively flattened actor state fields {field_names:?} and lowered uniform members {member_names:?} (without InitialState all three sets must match; with InitialState every view param must be a unique state leaf)",
            actor.name
        ))
    };
    let has_initializer =
        surface_initializer_contract(db, top_mod, source_entry, resource_field_indices)?.is_some();
    if (!has_initializer
        && (view.params.len() != members.len() || view.params.len() != field_names.len()))
        || view.params.len() > members.len()
    {
        return Err(mismatch());
    }

    // Emit params in member (arg_index) order so `surface.params[i]` aligns with
    // `members[i]`, matching each member name to exactly one `view()` param.
    let mut params = Vec::with_capacity(members.len());
    for view_param in &view.params {
        if !field_names.contains(&view_param.name.as_str()) {
            return Err(mismatch());
        }
        let Some(member) = members
            .iter()
            .copied()
            .find(|member| member.name == view_param.name)
        else {
            return Err(mismatch());
        };
        params.push(web_surface_param(view_param, member)?);
    }

    Ok(Some(WebSurface {
        extent: WebExtent {
            width: view.extent_width,
            height: view.extent_height,
            dpr: "auto".to_string(),
            filter: "smooth".to_string(),
        },
        pipeline: WebPipeline {
            kind: if layout.vertex_entry.is_some() {
                "authored_raster".to_string()
            } else {
                "fullscreen_fragment".to_string()
            },
        },
        params,
        state: WebSurfaceState {
            kind: "params".to_string(),
        },
        activate: "pointer".to_string(),
    }))
}

/// Builds one manifest param from an evaluated `view()` param and the uniform
/// member it binds. Range sanity is checked here, at the projection boundary
/// (ranges are data), with a structured diagnostic.
fn web_surface_param(
    view_param: &ViewParam,
    member: &WebBindingMember,
) -> Result<WebSurfaceParam, WebBundleError> {
    let kind = view_param.kind;
    let (min, max, init, visible) = if kind.is_extent() {
        // Extent-bound: the runtime writes the live canvas size into the member.
        (None, None, None, false)
    } else if matches!(kind, ViewParamKind::Fixed) {
        // Projected constant: carried into the uniform record, no control.
        (None, None, Some(view_param.init), false)
    } else {
        if view_param.min >= view_param.max {
            return Err(WebBundleError::SurfaceProjection(format!(
                "param `{}`: min ({}) must be less than max ({})",
                view_param.name, view_param.min, view_param.max
            )));
        }
        if view_param.init < view_param.min || view_param.init > view_param.max {
            return Err(WebBundleError::SurfaceProjection(format!(
                "param `{}`: init ({}) must be within [{}, {}]",
                view_param.name, view_param.init, view_param.min, view_param.max
            )));
        }
        (
            Some(view_param.min),
            Some(view_param.max),
            Some(view_param.init),
            true,
        )
    };
    Ok(WebSurfaceParam {
        name: member.name.clone(),
        doc: member.doc.clone(),
        kind: kind.as_str().to_string(),
        min,
        max,
        init,
        visible,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebArtifactManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm_bytes: Option<u64>,
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
    Resource,
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
    GlobalInvocationIdZ,
    LocalInvocationIdX,
    LocalInvocationIdY,
    LocalInvocationIdZ,
    WorkgroupIdX,
    WorkgroupIdY,
    WorkgroupIdZ,
    NumWorkgroupsX,
    NumWorkgroupsY,
    NumWorkgroupsZ,
    LocalInvocationIndex,
    FragmentPositionX,
    FragmentPositionY,
    VertexIndex,
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
pub struct WebResource {
    pub group: u32,
    pub binding: u32,
    pub name: String,
    pub length: u32,
    pub stride: u32,
    pub span: u32,
    pub element: WebActorResourceElement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebPass {
    pub source_entry: String,
    pub shader: String,
    pub shader_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatch: Option<[u32; 3]>,
    /// Number of strictly ordered submissions of one compiled compute stage.
    /// This is derived from an Fe dispatch-policy type. A legacy or ordinary
    /// fixed pass decodes as one.
    #[serde(default = "one_u32", skip_serializing_if = "is_one_u32")]
    pub repeat: u32,
    /// Consecutive passes with the same compiler-assigned group form one
    /// ordered body that is repeated as a unit. The originating group identity
    /// and count come from Fe's `CycledDispatch` policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle: Option<WebPassCycle>,
    /// Compiler-derived non-indexed draw count. This transitional transport is
    /// consumed by the fixed host; the source of truth is the Fe draw-policy
    /// type (`TriangleList<N>`), never page JavaScript.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw_vertices: Option<u32>,
    pub layout: WebLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebPassCycle {
    pub group: u32,
    pub repeat: u32,
}

const fn one_u32() -> u32 {
    1
}

fn is_one_u32(value: &u32) -> bool {
    *value == 1
}

// The manifest carries floating-point param ranges/inits in its `surface`
// section (protocol v5, R1b), so it can no longer be `Eq`; `PartialEq` is
// retained for the round-trip tests. Nothing keys a manifest by hash/equality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebBundleManifest {
    pub protocol: String,
    pub protocol_version: u32,
    pub source_entry: String,
    pub artifacts: WebArtifactManifest,
    pub layout: WebLayout,
    #[serde(default)]
    pub resources: Vec<WebResource>,
    #[serde(default)]
    pub passes: Vec<WebPass>,
    /// The CTFE-projected `view()` surface (extent + per-param range/init/kind),
    /// added in protocol v5 (R1b). Absent for a render entry with no `view()`
    /// behavior (the v4-compatible path); serde-defaulted on read so v4
    /// manifests still parse structurally.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<WebSurface>,
    /// The projected `UpdateSurface` control behavior (R3 param gestures):
    /// the wasm export the runtime calls per raw gesture, its positional
    /// argument sources, and which state fields its reply writes back.
    /// `serde` default/omit so a non-interactive demo's manifest stays
    /// byte-identical to before this section existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<WebControl>,
    pub provenance: WebProvenance,
    pub canonical_interface: Option<CanonicalInterfaceManifest>,
    pub canonical_status: WebCanonicalStatus,
    /// Present exactly when a canonical browser interface is embedded. These
    /// compiler-owned modules implement its Worker/MessagePort and WebGPU
    /// actor transport; applications provide effect handlers, not wire glue.
    #[serde(default)]
    pub browser_runtime: Option<WebBrowserRuntimeManifest>,
}

/// The `surface` section (protocol v5): the render actor's interactive boundary,
/// CTFE-projected from its `const fn view()` behavior. Every value here traces
/// to the evaluated `Surface` record; nothing is guessed by the runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSurface {
    pub extent: WebExtent,
    pub pipeline: WebPipeline,
    pub params: Vec<WebSurfaceParam>,
    pub state: WebSurfaceState,
    /// When the surface goes live: `"pointer"` (on hover/focus/tap). The
    /// lifecycle vocabulary opens in R2.
    pub activate: String,
}

/// The dispatch/canvas extent in pixels, plus the presentation policy. Replaces
/// the page's `data-fe-width/height` attributes and `const RESOLUTION`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebExtent {
    pub width: u32,
    pub height: u32,
    /// Device-pixel-ratio policy: `"auto"` (backing store scales with DPR,
    /// honored in R2) for now.
    pub dpr: String,
    /// Canvas filtering: `"smooth"` or `"pixelated"` (per-surface; R2 lifts the
    /// runtime's global default).
    pub filter: String,
}

/// The render pipeline kind. An open vocabulary: `"fullscreen_fragment"` today;
/// `"mesh_gpu"` etc. join in later rungs without a manifest-format change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebPipeline {
    pub kind: String,
}

/// One projected interactive parameter. `min`/`max`/`init` are absent for a
/// non-slider kind (extent-bound or fixed). `doc` is reused from the L0 field
/// projection (the actor field's doc comment).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSurfaceParam {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// The kind vocabulary: `range` | `unit` | `angle` | `log` | `int` |
    /// `fixed` | `extent_x` | `extent_y` | `toggle`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub init: Option<f32>,
    /// Whether the runtime renders a control for this param. Omitted (defaults
    /// true) for ordinary sliders; `false` for extent-bound and fixed params.
    #[serde(
        default = "web_surface_param_visible_default",
        skip_serializing_if = "is_true"
    )]
    pub visible: bool,
}

fn web_surface_param_visible_default() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

/// The surface's held-state model. `"params"` = the uniform record IS the whole
/// state (all fragment sketches); `"record"` joins in R3 for message-driven
/// state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSurfaceState {
    pub kind: String,
}

/// The `control` section (R3 param gestures, protocol v5): the render actor's
/// gesture-driven state update, projected from its `UpdateSurface`-marked
/// behavior. Absent for a demo with no such behavior (the pre-R3-compatible
/// path). Every value here traces to the compiled wasm export's OWN measured
/// signature (`wasm_export_signature`) and the actor's structural field/
/// behavior declarations; nothing is guessed by the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebControl {
    /// The wasm export name the runtime calls once per raw gesture (e.g.
    /// `update_view`), taking `args` positionally and returning a tuple the
    /// runtime writes back by `result`'s names (native wasm multi-value).
    pub export: String,
    /// Positional argument sources, in the EXACT order the wasm export
    /// expects them.
    pub args: Vec<WebControlArgSource>,
    /// The reply tuple's positional mapping: each element's target state-field
    /// NAME (a leading, declaration-order subset of the actor's fields; e.g.
    /// `res` is read via `args` but not itself gesture-updated, so it is
    /// absent here).
    pub result: Vec<String>,
}

/// Where one positional argument of a `control.export` call comes from. The
/// runtime accumulates raw pointer/wheel deltas and reads live state; it does
/// NOT compute pan sensitivity, a zoom curve, or a clamp -- that is exactly
/// what `control.export` (Fe) owns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum WebControlArgSource {
    /// The actor's CURRENT value of the state field named `name` (read from
    /// the live uniform vector by name, exactly like a `surface.params[]`
    /// member).
    State { name: String },
    /// An actor-owned GPU resource occupies a position in the desugared
    /// behavior ABI but is not browser scalar state. The control lane receives
    /// an inert zero handle; resource operations are not available to Wasm.
    Resource {
        name: String,
        wasm_type: WebControlWasmType,
    },
    /// Accumulated pointer movement while dragging (primary button held), in
    /// the SAME pixel frame as an `extent_x`/`extent_y` state field; `axis`
    /// is `"x"` or `"y"`.
    Drag { axis: String },
    /// One wheel gesture's notch direction: `Math.sign(deltaY)`, so -1 (in),
    /// 0 (unused; wheel events always carry a nonzero delta), or 1 (out).
    Wheel,
    /// The pointer's CURRENT position, in the same pixel frame as `Drag`
    /// (not normalized); `axis` is `"x"` or `"y"`.
    Pointer { axis: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebControlWasmType {
    I32,
    I64,
    F32,
    F64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebCanonicalStatus {
    pub policy: WebCanonicalPolicy,
    pub embedded: bool,
    pub omission_reason: Option<String>,
}

// `WebBundle` embeds the v6 manifest (which carries f32 surface ranges), so it
// is `PartialEq` but not `Eq`.
#[derive(Debug, Clone, PartialEq)]
pub struct WebBundle {
    pub wasm: Vec<u8>,
    pub wgsl: String,
    pub pass_wgsl: Vec<WebPassShader>,
    pub manifest: WebBundleManifest,
    pub interface_js: Option<String>,
    pub interface_d_ts: Option<String>,
    /// Compiler-derived continuation machines owned by the selected render
    /// actor. They are published as a manifest-free executable package.
    pub scoped_tasks: Vec<WasmTaskAdapter>,
    /// Separately compiled nominal child actors, selected from the typed
    /// mailbox and supervision operations in `scoped_tasks`.
    pub structured_children: Vec<StructuredChildActorArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebPassShader {
    pub path: String,
    pub source: String,
}

fn web_resource_manifest(resource: &WebActorResource, binding: u32) -> WebResource {
    let span = match &resource.element {
        WebActorResourceElement::U32 => 4,
        WebActorResourceElement::Record { span, .. } => *span,
    };
    WebResource {
        group: 0,
        binding,
        name: resource.name.clone(),
        length: resource.length,
        stride: span,
        span,
        element: resource.element.clone(),
    }
}

fn stage_external_resources(
    db: &DriverDataBase,
    package: &mir::RuntimePackage<'_>,
    entry: &str,
    resources: &[WebResource],
    access: Access,
    leading_context_leaves: Option<u32>,
) -> Result<Vec<SpirvExternalResource>, WebBundleError> {
    let functions = package
        .functions(db)
        .into_iter()
        .filter(|function| {
            function.linkage(db) == mir::RuntimeLinkage::Internal && function.symbol(db) == entry
        })
        .collect::<Vec<_>>();
    let [function] = functions.as_slice() else {
        return Err(WebBundleError::Lower(format!(
            "GPU stage `{entry}` must select exactly one public runtime function (found {})",
            functions.len()
        )));
    };
    let body = function.instance(db).body(db);
    let context_offset = leading_context_leaves
        .map(|leaves| leaves.saturating_sub(1))
        .unwrap_or(0);
    let arg_indices = body
        .signature
        .params
        .iter()
        .enumerate()
        .filter_map(|(index, param)| {
            let ty = body.local(param.local)?.semantic_ty;
            nominal_attrs(db, ty)
                .is_some_and(|attrs| attrs.gpu_resource(db) == Some(GpuResource::Storage))
                .then_some(index as u32 + context_offset)
        })
        .collect::<Vec<_>>();
    if arg_indices.len() != resources.len() {
        return Err(WebBundleError::EntryDerivation(format!(
            "GPU stage exposes {} attributed resource arguments but the actor declares {} resources",
            arg_indices.len(),
            resources.len()
        )));
    }
    Ok(resources
        .iter()
        .zip(arg_indices)
        .map(|(resource, arg_index)| SpirvExternalResource {
            arg_index,
            group: resource.group,
            binding: resource.binding,
            name: resource.name.clone(),
            access,
            element: match &resource.element {
                WebActorResourceElement::U32 => SpirvResourceElement::Scalar(SpirvScalarKind::U32),
                WebActorResourceElement::Record { fields, span } => SpirvResourceElement::Record {
                    fields: fields
                        .iter()
                        .map(|field| SpirvResourceField {
                            name: field.name.clone(),
                            scalar: SpirvScalarKind::U32,
                            offset: field.offset,
                        })
                        .collect(),
                    span: *span,
                },
            },
            stride: resource.stride,
            length: resource.length,
        })
        .collect())
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

/// Run one bounded compilation unit against an input-equivalent database whose
/// query lifetime ends with the unit. Salsa interned values intentionally live
/// for a database's lifetime; a large pass graph must therefore make physical
/// derivation and emission boundaries explicit instead of retaining every
/// stage's specialized HIR and MIR until the complete page bundle is finished.
fn with_isolated_compiler_database<T>(
    source_db: &DriverDataBase,
    source_mod: TopLevelMod<'_>,
    unit: &str,
    compile: impl for<'db> FnOnce(&'db DriverDataBase, TopLevelMod<'db>) -> Result<T, WebBundleError>,
) -> Result<T, WebBundleError> {
    let trace = std::env::var_os("FE_WEB_STAGE_TRACE").is_some()
        || std::env::var_os("FE_WASM_LOWER_TRACE").is_some();
    let started = std::time::Instant::now();
    if trace {
        eprintln!("[fe web compiler unit] begin unit={unit}");
    }
    let source_url = source_mod
        .source_file(source_db)
        .url(source_db)
        .ok_or_else(|| {
            WebBundleError::Lower("GPU actor source module has no compiler-owned URL".to_owned())
        })?;
    let stage_db = source_db.replicate_inputs();
    let stage_file = stage_db
        .workspace()
        .get(&stage_db, &source_url)
        .ok_or_else(|| {
            WebBundleError::Lower(format!(
                "isolated GPU stage database lost source `{source_url}`"
            ))
        })?;
    let stage_mod = stage_db.top_mod(stage_file);
    let result = compile(&stage_db, stage_mod);
    drop(stage_db);
    reclaim_completed_compiler_database();
    if trace {
        eprintln!(
            "[fe web compiler unit] end unit={unit}, elapsed_ms={}",
            started.elapsed().as_millis()
        );
    }
    result
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn reclaim_completed_compiler_database() {
    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> std::ffi::c_int;
    }
    unsafe {
        malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn reclaim_completed_compiler_database() {}

impl WebBundle {
    fn compile_actor_graph(
        db: &DriverDataBase,
        top_mod: TopLevelMod<'_>,
        options: WebBuildOptions,
        program: WebActorProgram,
    ) -> Result<Self, WebBundleError> {
        if options.mode != WebBundleMode::Render {
            return Err(WebBundleError::EntryDerivation(
                "a GPU actor pass graph must terminate in a fragment stage".to_owned(),
            ));
        }
        if options.canonical_policy != WebCanonicalPolicy::Disabled
            || !options.canonical_entries.is_empty()
        {
            return Err(WebBundleError::CanonicalRequired(
                "GPU actor pass graphs do not yet carry Wasm message lanes".to_owned(),
            ));
        }
        let control_export = actor_update_export_name(db, top_mod, &options.source_entry)?;
        let quality_policy = resolve_surface_quality_policy(db, top_mod, &options.source_entry)?;
        let recovery_policy = resolve_surface_recovery_policy(db, top_mod, &options.source_entry)?;
        let fragment_entries = program
            .stages
            .iter()
            .filter(|stage| {
                matches!(
                    stage.kind,
                    WebActorStageKind::Fragment | WebActorStageKind::RasterFragment { .. }
                )
            })
            .map(|stage| stage.source_entry.as_str())
            .collect::<Vec<_>>();
        if !fragment_entries.contains(&options.source_entry.as_str())
            || !matches!(
                program.stages.last().map(|stage| &stage.kind),
                Some(WebActorStageKind::Fragment | WebActorStageKind::RasterFragment { .. })
            )
        {
            return Err(WebBundleError::EntryDerivation(format!(
                "GPU actor pass graph must contain its derived fragment entry `{}` and end in a render stage",
                options.source_entry
            )));
        }
        let resources = program
            .resources
            .iter()
            .enumerate()
            .map(|(binding, resource)| web_resource_manifest(resource, binding as u32))
            .collect::<Vec<_>>();
        let resource_field_indices = program
            .resources
            .iter()
            .map(|resource| resource.field_index)
            .collect::<Vec<_>>();
        let mut passes = Vec::with_capacity(program.stages.len());
        let mut pass_wgsl = Vec::with_capacity(program.stages.len());
        // The top-level compatibility artifact/layout follows the derived
        // page-facing fragment entry. Ordered pass execution uses `passes`;
        // a later overlay must not silently replace the inspected primary
        // shader merely because it is last in source order.
        let mut primary_shader = None;
        let mut primary_layout = None;
        let mut cycle_groups = Vec::<String>::new();
        let mut index = 0;
        while index < program.stages.len() {
            let stage = &program.stages[index];
            if let WebActorStageKind::Vertex { vertex_count, .. } = &stage.kind {
                let Some(WebActorStage {
                    source_entry: fragment_entry,
                    kind: WebActorStageKind::RasterFragment { .. },
                }) = program.stages.get(index + 1)
                else {
                    return Err(WebBundleError::Lower(
                        "authored raster vertex behavior lost its adjacent fragment pair"
                            .to_owned(),
                    ));
                };
                if !resources.is_empty() {
                    return Err(WebBundleError::Lower(
                        "authored raster resources are not wired into both stages yet".to_owned(),
                    ));
                }
                let vertex_entry = &stage.source_entry;
                let artifact = with_isolated_compiler_database(
                    db,
                    top_mod,
                    fragment_entry,
                    |stage_db, stage_mod| {
                        let package = mir::build_wasm_runtime_package_for_entries(
                            stage_db,
                            stage_mod,
                            &[vertex_entry.clone(), fragment_entry.clone()],
                        )
                        .map_err(|error| WebBundleError::Lower(error.to_string()))?;
                        compile_runtime_package_spirv_authored_raster(
                            stage_db,
                            &package,
                            vertex_entry,
                            fragment_entry,
                        )
                        .map_err(|error| WebBundleError::Lower(error.to_string()))
                    },
                )?;
                let shader =
                    normalize_generated_text(&artifact.wgsl.ok_or(WebBundleError::MissingWgsl)?);
                validate_browser_wgsl(&shader)?;
                let mut layout = WebLayout::from_spirv(&artifact.layout)?;
                project_actor_field_metadata(
                    db,
                    top_mod,
                    fragment_entry,
                    &mut layout,
                    &resource_field_indices,
                )?;
                let path = format!("{PASS_DIR}/{index:03}-raster.wgsl");
                passes.push(WebPass {
                    source_entry: fragment_entry.clone(),
                    shader: path.clone(),
                    shader_bytes: shader.len() as u64,
                    dispatch: None,
                    repeat: 1,
                    cycle: None,
                    draw_vertices: Some(*vertex_count),
                    layout: layout.clone(),
                });
                pass_wgsl.push(WebPassShader {
                    path: path.clone(),
                    source: shader.clone(),
                });
                if fragment_entry == &options.source_entry {
                    primary_shader = Some((path, shader));
                    primary_layout = Some(layout);
                }
                index += 2;
                continue;
            }
            let (artifact, dispatch, repeat, cycle, kind) = match &stage.kind {
                WebActorStageKind::Compute {
                    workgroup_size,
                    dispatch,
                    repeat,
                    cycle,
                    invocation_context,
                } => {
                    let builtin_arguments = (*invocation_context)
                        .then(compute_invocation_builtin_arguments)
                        .unwrap_or_default();
                    let context_leaves = (!builtin_arguments.is_empty())
                        .then_some(u32::try_from(builtin_arguments.len()).unwrap());
                    let artifact = with_isolated_compiler_database(
                        db,
                        top_mod,
                        &stage.source_entry,
                        |stage_db, stage_mod| {
                            let package = mir::build_wasm_runtime_package_for_entry(
                                stage_db,
                                stage_mod,
                                &stage.source_entry,
                            )
                            .map_err(|error| WebBundleError::Lower(error.to_string()))?;
                            let external = stage_external_resources(
                                stage_db,
                                &package,
                                &stage.source_entry,
                                &resources,
                                Access::ReadWrite,
                                context_leaves,
                            )?;
                            compile_runtime_package_spirv_compute_with_interface(
                                stage_db,
                                &package,
                                *workgroup_size,
                                *dispatch,
                                &external,
                                &builtin_arguments,
                            )
                            .map_err(|error| WebBundleError::Lower(error.to_string()))
                        },
                    );
                    (
                        artifact,
                        Some(*dispatch),
                        *repeat,
                        cycle.clone(),
                        "compute",
                    )
                }
                WebActorStageKind::Fragment => {
                    let artifact = with_isolated_compiler_database(
                        db,
                        top_mod,
                        &stage.source_entry,
                        |stage_db, stage_mod| {
                            let package = mir::build_wasm_runtime_package_for_entry(
                                stage_db,
                                stage_mod,
                                &stage.source_entry,
                            )
                            .map_err(|error| WebBundleError::Lower(error.to_string()))?;
                            let external = stage_external_resources(
                                stage_db,
                                &package,
                                &stage.source_entry,
                                &resources,
                                Access::Read,
                                None,
                            )?;
                            compile_runtime_package_spirv_render_with_resources(
                                stage_db, &package, &external,
                            )
                            .map_err(|error| WebBundleError::Lower(error.to_string()))
                        },
                    );
                    (artifact, None, 1, None, "fragment")
                }
                WebActorStageKind::Vertex { .. } => unreachable!("handled as an adjacent pair"),
                WebActorStageKind::RasterFragment { .. } => {
                    unreachable!("actor construction rejects unpaired raster fragments")
                }
            };
            let artifact = artifact.map_err(|error| {
                WebBundleError::Lower(format!(
                    "GPU actor stage `{}` ({kind}) failed: {error}",
                    stage.source_entry
                ))
            })?;
            let shader =
                normalize_generated_text(&artifact.wgsl.ok_or(WebBundleError::MissingWgsl)?);
            validate_browser_wgsl(&shader)?;
            let mut layout = WebLayout::from_spirv(&artifact.layout)?;
            project_actor_field_metadata(
                db,
                top_mod,
                &stage.source_entry,
                &mut layout,
                &resource_field_indices,
            )?;
            let path = format!("{PASS_DIR}/{index:03}-{kind}.wgsl");
            let cycle = cycle.map(|cycle| {
                let group = cycle_groups
                    .iter()
                    .position(|group| group == &cycle.group)
                    .unwrap_or_else(|| {
                        cycle_groups.push(cycle.group);
                        cycle_groups.len() - 1
                    });
                WebPassCycle {
                    group: u32::try_from(group)
                        .expect("actor pass cycle group count fits in u32"),
                    repeat: cycle.repeat,
                }
            });
            passes.push(WebPass {
                source_entry: stage.source_entry.clone(),
                shader: path.clone(),
                shader_bytes: shader.len() as u64,
                dispatch,
                repeat,
                cycle,
                draw_vertices: None,
                layout: layout.clone(),
            });
            pass_wgsl.push(WebPassShader {
                path: path.clone(),
                source: shader.clone(),
            });
            if matches!(stage.kind, WebActorStageKind::Fragment)
                && stage.source_entry == options.source_entry
            {
                primary_shader = Some((path, shader));
                primary_layout = Some(layout);
            }
            index += 1;
        }
        let (final_path, wgsl) = primary_shader.ok_or_else(|| {
            WebBundleError::EntryDerivation(
                "GPU actor pass graph has no shader for its derived fragment entry".to_owned(),
            )
        })?;
        let layout = primary_layout.expect("primary fragment shader and layout are paired");
        let initializer = surface_initializer_contract(
            db,
            top_mod,
            &options.source_entry,
            &resource_field_indices,
        )?;
        let (wasm, control, has_fe_schedule) = if control_export.is_some()
            || initializer.is_some()
            || quality_policy.is_some()
            || recovery_policy.is_some()
        {
            // A pass graph remains GPU-only for all rendering and resource
            // work. Its optional Wasm artifact contains only Fe-authored state
            // initialization/control behavior; neither body is reimplemented
            // by the browser host.
            let typed_transition = control_export
                .as_deref()
                .map(|control_export| {
                    typed_surface_transition_contract(
                        db,
                        top_mod,
                        &options.source_entry,
                        control_export,
                        &resource_field_indices,
                    )
                })
                .transpose()?
                .flatten();
            let schedule_policy = control_export
                .as_deref()
                .map(|control_export| {
                    resolve_surface_schedule_policy(
                        db,
                        top_mod,
                        &options.source_entry,
                        control_export,
                    )
                })
                .transpose()?
                .flatten();
            validate_surface_schedule_pair(
                typed_transition.as_ref(),
                schedule_policy.as_ref(),
                &options.source_entry,
            )?;
            let mut control_entries = Vec::new();
            if let Some(control_export) = control_export.as_deref() {
                control_entries.push(control_export.to_owned());
            }
            if let Some(initializer) = initializer.as_ref() {
                control_entries.push(initializer.source_entry.clone());
            }
            let internal_funcs = schedule_policy
                .as_ref()
                .map(|policy| policy.func)
                .into_iter()
                .chain(quality_policy.as_ref().map(|policy| policy.func))
                .chain(recovery_policy.as_ref().map(|policy| policy.func))
                .collect::<Vec<_>>();
            let control_package = mir::build_wasm_runtime_package_for_entries_with_internal_funcs(
                db,
                top_mod,
                &control_entries,
                &internal_funcs,
            )
            .map_err(|error| WebBundleError::Lower(error.to_string()))?;
            let mut wasm_options = WasmCompileOptions::default().with_optimization();
            if let Some(contract) = typed_transition.as_ref() {
                wasm_options = with_typed_surface_export(
                    wasm_options,
                    control_export
                        .as_deref()
                        .expect("typed transition has a source export"),
                    contract,
                );
            }
            if let Some(initializer) = initializer.as_ref() {
                wasm_options = with_surface_initializer(wasm_options, initializer);
            }
            if let Some(policy) = schedule_policy.as_ref() {
                let policy_instance_key =
                    mir::runtime_package_instance_key_for_func(db, control_package, policy.func)
                        .map_err(|error| WebBundleError::Lower(error.to_string()))?;
                wasm_options = with_typed_surface_schedule(
                    wasm_options,
                    &policy_instance_key,
                    policy.event_first,
                    &policy.contract,
                );
            }
            if let Some(policy) = quality_policy.as_ref() {
                let policy_instance_key =
                    mir::runtime_package_instance_key_for_func(db, control_package, policy.func)
                        .map_err(|error| WebBundleError::Lower(error.to_string()))?;
                wasm_options = with_typed_surface_quality(
                    wasm_options,
                    &policy_instance_key,
                    &policy.contract,
                );
            }
            if let Some(policy) = recovery_policy.as_ref() {
                let policy_instance_key =
                    mir::runtime_package_instance_key_for_func(db, control_package, policy.func)
                        .map_err(|error| WebBundleError::Lower(error.to_string()))?;
                wasm_options = with_typed_surface_recovery(
                    wasm_options,
                    &policy_instance_key,
                    policy.event_first,
                    &policy.contract,
                );
            }
            let wasm =
                compile_runtime_package_wasm_with_options(db, &control_package, wasm_options)
                    .map_err(|error| WebBundleError::Lower(error.to_string()))?
                    .bytes;
            wasmparser::validate(&wasm)
                .map_err(|error| WebBundleError::WasmValidation(error.to_string()))?;
            if let Some(initializer) = initializer.as_ref() {
                verify_surface_initializer_export(&wasm, initializer)?;
            }
            let control = control_export
                .as_deref()
                .map(|control_export| {
                    project_control(
                        db,
                        top_mod,
                        &options.source_entry,
                        control_export,
                        &wasm,
                        &resource_field_indices,
                    )
                })
                .transpose()?
                .flatten();
            let has_fe_schedule = typed_transition
                .as_ref()
                .is_some_and(|contract| contract.scheduled);
            (wasm, control, has_fe_schedule)
        } else {
            (Vec::new(), None, false)
        };
        let surface = project_surface(
            db,
            top_mod,
            &options.source_entry,
            &layout,
            &resource_field_indices,
        )?;
        let provenance = options.provenance.with_bundle_shape(
            !wasm.is_empty(),
            surface.is_some(),
            control_export.is_some(),
            passes.len() > 1 || !resources.is_empty(),
            has_fe_schedule,
            quality_policy.is_some(),
            recovery_policy.is_some(),
        );
        let manifest = WebBundleManifest {
            protocol: WEB_BUNDLE_PROTOCOL.to_owned(),
            protocol_version: WEB_BUNDLE_PROTOCOL_VERSION,
            source_entry: options.source_entry,
            artifacts: WebArtifactManifest {
                wasm: (!wasm.is_empty()).then(|| WASM_FILE.to_owned()),
                wasm_bytes: (!wasm.is_empty()).then_some(wasm.len() as u64),
                wgsl: final_path,
                wgsl_bytes: wgsl.len() as u64,
                canonical_adapters: Vec::new(),
            },
            layout,
            resources,
            passes,
            surface,
            control,
            provenance,
            canonical_interface: None,
            canonical_status: WebCanonicalStatus {
                policy: WebCanonicalPolicy::Disabled,
                embedded: false,
                omission_reason: Some(
                    "GPU resource pass graph has no CPU fallback or canonical Wasm message lane"
                        .to_owned(),
                ),
            },
            browser_runtime: generated_browser_runtime(false),
        };
        Ok(Self {
            wasm,
            wgsl,
            pass_wgsl,
            manifest,
            interface_js: None,
            interface_d_ts: None,
            scoped_tasks: Vec::new(),
            structured_children: Vec::new(),
        })
    }

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
        let actor_program =
            with_isolated_compiler_database(db, top_mod, "<actor-program>", actor_gpu_program)?;
        if let Some(program) = actor_program
            && (!program.resources.is_empty()
                || program.stages.iter().any(|stage| {
                    matches!(
                        stage.kind,
                        WebActorStageKind::Compute { .. }
                            | WebActorStageKind::Vertex { .. }
                            | WebActorStageKind::RasterFragment { .. }
                    )
                }))
        {
            return Self::compile_actor_graph(db, top_mod, options, program);
        }
        // Structurally recognized like `view()`: an `UpdateSurface`-marked
        // behavior joins the wasm root set automatically (no caller opt-in),
        // so a demo with none stays byte-identical. It is a PLAIN multi-scalar
        // wasm export (native multi-value reply, R2.1), never a canonical
        // message lane (that machinery demands a single nominal request/
        // response record). This compatibility lane is admitted only under
        // `Disabled` canonical policy; combining it with `Optional`/`Required`
        // would need `control_export` routed around canonical-lane derivation
        // too, so that unsupported combination remains fail-closed.
        let control_export = actor_update_export_name(db, top_mod, &options.source_entry)?;
        let quality_policy = resolve_surface_quality_policy(db, top_mod, &options.source_entry)?;
        let recovery_policy = resolve_surface_recovery_policy(db, top_mod, &options.source_entry)?;
        let typed_transition = control_export
            .as_deref()
            .map(|export| {
                typed_surface_transition_contract(db, top_mod, &options.source_entry, export, &[])
            })
            .transpose()?
            .flatten();
        let schedule_policy = control_export
            .as_deref()
            .map(|export| {
                resolve_surface_schedule_policy(db, top_mod, &options.source_entry, export)
            })
            .transpose()?
            .flatten();
        validate_surface_schedule_pair(
            typed_transition.as_ref(),
            schedule_policy.as_ref(),
            &options.source_entry,
        )?;
        let initializer = surface_initializer_contract(db, top_mod, &options.source_entry, &[])?;
        let scoped_task_entries =
            render_actor_scoped_task_entries(db, top_mod, &options.source_entry, &[])?;
        // Canonical actor messages and the surface-control transition are two
        // different roles. Explicitly placed/capability-bearing Fe functions
        // form the public message interface; `navigate` remains a private
        // resident root even though it is compiled into the same Wasm module.
        // Keeping those sets distinct prevents a render actor's multi-field
        // transition from being mistaken for a one-request/one-response lane.
        //
        // `canonical_entries` is retained as a compatibility/tooling override
        // for unmarked fixtures. An ordinary actor build needs no parallel
        // Rust/CLI name list: its effect rows are the source of truth.
        let explicit_canonical_entries = !options.canonical_entries.is_empty();
        let (canonical_declarations, canonical_entries) = if explicit_canonical_entries {
            let mut entries = options.canonical_entries.clone();
            let mut seen = std::collections::BTreeSet::new();
            entries.retain(|entry| seen.insert(entry.clone()));
            let declarations = entries
                .iter()
                .map(|entry| canonical_lane_decl_from_entry(db, top_mod, entry, entry))
                .collect::<Result<Vec<_>, _>>();
            (declarations, entries)
        } else {
            let declarations = match gpu_actor_name_for_entry(db, top_mod, &options.source_entry) {
                Some(actor_name) => canonical_lane_decls_from_actor(db, top_mod, &actor_name),
                None => canonical_lane_decls_from_module(db, top_mod),
            };
            let entries = declarations
                .as_ref()
                .map(|declarations| {
                    declarations
                        .iter()
                        .map(|declaration| declaration.name.clone())
                        .collect()
                })
                .unwrap_or_default();
            (declarations, entries)
        };
        let has_declared_interface = match canonical_declarations.as_ref() {
            Ok(declarations) => !declarations.is_empty(),
            // A malformed explicitly marked lane must diagnose; it must not
            // make the interface disappear and fall back to an untyped build.
            Err(_) => !explicit_canonical_entries,
        };
        // A Fe-declared browser actor interface is fail-closed even when an
        // older caller left the opt-in policy at its default. The policy flag
        // controls only legacy/manual derivation; it cannot disable semantics
        // authored explicitly in Fe.
        let canonical_policy = if has_declared_interface && !explicit_canonical_entries {
            WebCanonicalPolicy::Required
        } else {
            options.canonical_policy
        };
        let (canonical_candidate, mut canonical_status) = match canonical_policy {
            WebCanonicalPolicy::Disabled => (
                None,
                WebCanonicalStatus {
                    policy: WebCanonicalPolicy::Disabled,
                    embedded: false,
                    omission_reason: Some("canonical interface was not requested".to_owned()),
                },
            ),
            policy @ (WebCanonicalPolicy::Optional | WebCanonicalPolicy::Required) => {
                let derived = canonical_declarations.and_then(CanonicalInterfaceManifest::build);
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
        // The render entry remains the module's CPU fallback and must be
        // rooted independently of public actor messages. Canonical wrappers
        // hide their authored lane entries, but they do not replace the
        // render runtime's direct pixel-kernel contract.
        let mut wasm_entries = vec![options.source_entry.clone()];
        wasm_entries.extend(match canonical_candidate.as_ref() {
            Some(interface) => canonical_entries
                .iter()
                .zip(&interface.lanes)
                .filter(|(_, lane)| lane.intent.execution == crate::CanonicalExecution::Wasm)
                .map(|(entry, _)| entry.clone())
                .collect::<Vec<_>>(),
            None => Vec::new(),
        });
        if let Some(control_export) = &control_export {
            wasm_entries.push(control_export.clone());
        }
        if let Some(initializer) = initializer.as_ref() {
            wasm_entries.push(initializer.source_entry.clone());
        }
        wasm_entries.extend(scoped_task_entries.iter().cloned());
        let mut seen_wasm_entries = std::collections::BTreeSet::new();
        wasm_entries.retain(|entry| seen_wasm_entries.insert(entry.clone()));
        if wasm_entries.is_empty() {
            return Err(WebBundleError::CanonicalRequired(
                "canonical bundle requires at least one executable Wasm lane".to_owned(),
            ));
        }
        let internal_funcs = schedule_policy
            .as_ref()
            .map(|policy| policy.func)
            .into_iter()
            .chain(quality_policy.as_ref().map(|policy| policy.func))
            .chain(recovery_policy.as_ref().map(|policy| policy.func))
            .collect::<Vec<_>>();
        let wasm_package = mir::build_wasm_runtime_package_for_entries_with_internal_funcs(
            db,
            top_mod,
            &wasm_entries,
            &internal_funcs,
        )
        .map_err(|error| WebBundleError::Lower(error.to_string()))?;

        let (scoped_tasks, structured_children) = if scoped_task_entries.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            compile_scoped_task_support(db, top_mod, wasm_package, scoped_task_entries.len(), true)
                .map_err(|error| WebBundleError::EntryDerivation(error.to_string()))?
        };

        let mut wasm_options = match canonical_policy {
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
        if !scoped_tasks.is_empty() {
            wasm_options = wasm_options
                .with_canonical_stack_memory(["fe_cabi_post_return"])
                .with_canonical_scoped_host_borrows();
        }
        if let Some(contract) = typed_transition.as_ref() {
            wasm_options = with_typed_surface_export(
                wasm_options,
                control_export
                    .as_deref()
                    .expect("typed transition has a source export"),
                contract,
            );
        }
        if let Some(initializer) = initializer.as_ref() {
            wasm_options = with_surface_initializer(wasm_options, initializer);
        }
        if let Some(policy) = schedule_policy.as_ref() {
            let policy_instance_key =
                mir::runtime_package_instance_key_for_func(db, wasm_package, policy.func)
                    .map_err(|error| WebBundleError::Lower(error.to_string()))?;
            wasm_options = with_typed_surface_schedule(
                wasm_options,
                &policy_instance_key,
                policy.event_first,
                &policy.contract,
            );
        }
        if let Some(policy) = quality_policy.as_ref() {
            let policy_instance_key =
                mir::runtime_package_instance_key_for_func(db, wasm_package, policy.func)
                    .map_err(|error| WebBundleError::Lower(error.to_string()))?;
            wasm_options =
                with_typed_surface_quality(wasm_options, &policy_instance_key, &policy.contract);
        }
        if let Some(policy) = recovery_policy.as_ref() {
            let policy_instance_key =
                mir::runtime_package_instance_key_for_func(db, wasm_package, policy.func)
                    .map_err(|error| WebBundleError::Lower(error.to_string()))?;
            wasm_options = with_typed_surface_recovery(
                wasm_options,
                &policy_instance_key,
                policy.event_first,
                &policy.contract,
            );
        }
        let wasm = compile_runtime_package_wasm_with_options(
            db,
            &wasm_package,
            wasm_options.with_optimization(),
        )
        .map_err(|error| WebBundleError::Lower(error.to_string()))?
        .bytes;
        wasmparser::validate(&wasm)
            .map_err(|error| WebBundleError::WasmValidation(error.to_string()))?;
        if let Some(initializer) = initializer.as_ref() {
            verify_surface_initializer_export(&wasm, initializer)?;
        }
        let control = control_export
            .as_deref()
            .map(|export| project_control(db, top_mod, &options.source_entry, export, &wasm, &[]))
            .transpose()?
            .flatten();
        let canonical_interface =
            verify_canonical_candidate(&wasm, canonical_candidate, &mut canonical_status)?;
        // The fixed render host always resolves the direct pixel entry. Actor
        // message wrappers are an additional interface, never a substitute.
        let function_exports = wasm_function_export_names(&wasm);
        if !function_exports
            .iter()
            .any(|name| name == &options.source_entry)
        {
            return Err(WebBundleError::EntryExportMismatch {
                source_entry: options.source_entry.clone(),
                exports: function_exports,
            });
        }
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
            WebBundleMode::Compute => {
                return Err(WebBundleError::EntryDerivation(
                    "standalone compute bundles require an attributed GPU actor pass graph"
                        .to_owned(),
                ));
            }
        }
        .map_err(|error| WebBundleError::Lower(error.to_string()))?;
        let wgsl = normalize_generated_text(&artifact.wgsl.ok_or(WebBundleError::MissingWgsl)?);
        validate_browser_wgsl(&wgsl)?;
        let mut layout = WebLayout::from_spirv(&artifact.layout)?;
        project_actor_field_metadata(db, top_mod, &options.source_entry, &mut layout, &[])?;
        let surface = project_surface(db, top_mod, &options.source_entry, &layout, &[])?;
        let passes = vec![WebPass {
            source_entry: options.source_entry.clone(),
            shader: WGSL_FILE.to_owned(),
            shader_bytes: wgsl.len() as u64,
            dispatch: None,
            repeat: 1,
            cycle: None,
            draw_vertices: None,
            layout: layout.clone(),
        }];

        let provenance = options.provenance.with_bundle_shape(
            !wasm.is_empty(),
            surface.is_some(),
            control_export.is_some(),
            false,
            typed_transition
                .as_ref()
                .is_some_and(|contract| contract.scheduled),
            quality_policy.is_some(),
            recovery_policy.is_some(),
        );
        let manifest = WebBundleManifest {
            protocol: WEB_BUNDLE_PROTOCOL.to_string(),
            protocol_version: WEB_BUNDLE_PROTOCOL_VERSION,
            source_entry: options.source_entry,
            artifacts: WebArtifactManifest {
                wasm: Some(WASM_FILE.to_string()),
                wasm_bytes: Some(wasm.len() as u64),
                wgsl: WGSL_FILE.to_string(),
                wgsl_bytes: wgsl.len() as u64,
                canonical_adapters,
            },
            layout,
            resources: Vec::new(),
            passes,
            surface,
            control,
            provenance,
            canonical_interface,
            canonical_status,
            browser_runtime,
        };
        Ok(Self {
            wasm,
            wgsl,
            pass_wgsl: Vec::new(),
            manifest,
            interface_js,
            interface_d_ts,
            scoped_tasks,
            structured_children,
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
        let scoped_task_package =
            materialize_scoped_task_package(&self.scoped_tasks, &self.structured_children)
                .map_err(|error| WebBundleError::Materialization(error.to_string()))?;
        let has_scoped_tasks = scoped_task_package.is_some();
        let scoped_task_file_count = scoped_task_package
            .as_ref()
            .map_or(0, |package| package.files.len());
        let mut files = Vec::with_capacity(
            3 + self.manifest.artifacts.canonical_adapters.len()
                + runtime_artifact_count
                + scoped_task_file_count,
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

        match (
            self.manifest.artifacts.wasm.as_deref(),
            self.manifest.artifacts.wasm_bytes,
        ) {
            (Some(path), Some(bytes)) if self.wasm.len() as u64 == bytes => {
                push(path, Arc::from(self.wasm.as_slice()))?;
            }
            (None, None) if self.wasm.is_empty() => {}
            (Some(path), Some(_)) => {
                return Err(WebBundleError::Materialization(format!(
                    "artifact `{path}` does not match its manifest byte length"
                )));
            }
            _ => {
                return Err(WebBundleError::Materialization(
                    "Wasm artifact path and byte length must either both be present or both be absent"
                        .to_owned(),
                ));
            }
        }
        if self.wgsl.len() as u64 != self.manifest.artifacts.wgsl_bytes {
            return Err(WebBundleError::Materialization(format!(
                "artifact `{}` does not match its manifest byte length",
                self.manifest.artifacts.wgsl
            )));
        }
        if self.pass_wgsl.is_empty() {
            push(
                &self.manifest.artifacts.wgsl,
                Arc::from(self.wgsl.as_bytes()),
            )?;
        } else {
            if self
                .pass_wgsl
                .iter()
                .find(|shader| shader.path == self.manifest.artifacts.wgsl)
                .is_none_or(|shader| shader.source != self.wgsl)
            {
                return Err(WebBundleError::Materialization(
                    "primary WGSL artifact does not identify the final pass shader".to_owned(),
                ));
            }
            for pass in &self.manifest.passes {
                let shader = self
                    .pass_wgsl
                    .iter()
                    .find(|shader| shader.path == pass.shader)
                    .ok_or_else(|| {
                        WebBundleError::Materialization(format!(
                            "manifest pass `{}` has no shader content",
                            pass.source_entry
                        ))
                    })?;
                if shader.source.len() as u64 != pass.shader_bytes {
                    return Err(WebBundleError::Materialization(format!(
                        "pass shader `{}` does not match its manifest byte length",
                        pass.shader
                    )));
                }
                push(&shader.path, Arc::from(shader.source.as_bytes()))?;
            }
        }
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
        if let Some(package) = scoped_task_package {
            for file in package.files {
                push(
                    &format!("tasks/{}", file.path),
                    Arc::from(file.bytes.into_boxed_slice()),
                )?;
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
            let task_option = if has_scoped_tasks {
                "scopedTasksUrl: \"./tasks/tasks.js\","
            } else {
                ""
            };
            let render_index =
                RENDER_RUNTIME_HTML.replace(RENDER_SCOPED_TASK_OPTION_MARKER, task_option);
            push(RENDER_INDEX_FILE, Arc::from(render_index.into_bytes()))?;
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
    let interface_js = emit_canonical_interface_js(interface)
        .map_err(|error| WebBundleError::Manifest(error.to_string()))?;
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
            LayoutMode::Compute => WebBundleMode::Compute,
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
                        Role::Resource => WebBindingRole::Resource,
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
                        SpirvBuiltinSource::GlobalInvocationIdZ => {
                            WebBuiltinSource::GlobalInvocationIdZ
                        }
                        SpirvBuiltinSource::LocalInvocationIdX => {
                            WebBuiltinSource::LocalInvocationIdX
                        }
                        SpirvBuiltinSource::LocalInvocationIdY => {
                            WebBuiltinSource::LocalInvocationIdY
                        }
                        SpirvBuiltinSource::LocalInvocationIdZ => {
                            WebBuiltinSource::LocalInvocationIdZ
                        }
                        SpirvBuiltinSource::WorkgroupIdX => WebBuiltinSource::WorkgroupIdX,
                        SpirvBuiltinSource::WorkgroupIdY => WebBuiltinSource::WorkgroupIdY,
                        SpirvBuiltinSource::WorkgroupIdZ => WebBuiltinSource::WorkgroupIdZ,
                        SpirvBuiltinSource::NumWorkgroupsX => WebBuiltinSource::NumWorkgroupsX,
                        SpirvBuiltinSource::NumWorkgroupsY => WebBuiltinSource::NumWorkgroupsY,
                        SpirvBuiltinSource::NumWorkgroupsZ => WebBuiltinSource::NumWorkgroupsZ,
                        SpirvBuiltinSource::LocalInvocationIndex => {
                            WebBuiltinSource::LocalInvocationIndex
                        }
                        SpirvBuiltinSource::FragmentPositionX => {
                            WebBuiltinSource::FragmentPositionX
                        }
                        SpirvBuiltinSource::FragmentPositionY => {
                            WebBuiltinSource::FragmentPositionY
                        }
                        SpirvBuiltinSource::VertexIndex => WebBuiltinSource::VertexIndex,
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
    /// The render actor's `view()` behavior could not be CTFE-projected into
    /// the manifest `surface` section, or the projected params fail to reconcile
    /// against the actor's state fields and the lowered uniform binding members.
    SurfaceProjection(String),
    /// The manifest `source_entry` is not present as a function export of the
    /// emitted wasm module. The browser render runtime resolves the render entry
    /// with `instance.exports[manifest.source_entry]`, so a mismatch fails the
    /// mount silently in the console; catching it here fails the build closed
    /// with the offending name and the actual export set.
    EntryExportMismatch {
        source_entry: String,
        exports: Vec<String>,
    },
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
            Self::SurfaceProjection(error) => {
                write!(f, "web surface projection failed: {error}")
            }
            Self::EntryExportMismatch {
                source_entry,
                exports,
            } => {
                write!(
                    f,
                    "manifest source_entry `{source_entry}` is not a wasm function export \
                     (the browser runtime resolves it via instance.exports[source_entry]); \
                     emitted function exports are [{}]",
                    exports.join(", ")
                )
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
            WebBundleMode::Compute => panic!("test helper does not build standalone compute"),
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
        assert_eq!(
            first.manifest.provenance.generated_artifacts,
            [
                WebGeneratedArtifactKind::Manifest,
                WebGeneratedArtifactKind::Wasm,
                WebGeneratedArtifactKind::Wgsl,
            ]
        );
        assert_eq!(
            first.manifest.provenance.fe_responsibilities,
            [WebFeResponsibility::GpuProgram]
        );
        assert_eq!(
            first.manifest.provenance.fixed_host.contract,
            "fixed_versioned_demo_blind_browser_host"
        );
        assert!(first.manifest.provenance.fixed_host.artifact.is_none());
        assert_eq!(
            first.manifest.provenance.fixed_host.responsibilities,
            [
                WebHostResponsibility::DomSurface,
                WebHostResponsibility::InputTransport,
                WebHostResponsibility::PresentationScheduler,
                WebHostResponsibility::BackingStorePolicy,
                WebHostResponsibility::DeviceCapabilityFacts,
                WebHostResponsibility::WebGpuExecutor,
                WebHostResponsibility::Lifecycle,
                WebHostResponsibility::WasmLoader,
            ]
        );
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

        let mut unattributed = serde_json::to_value(&first.manifest).unwrap();
        let provenance = unattributed["provenance"].as_object_mut().unwrap();
        for field in [
            "authored_sources",
            "non_fe_authored_sources",
            "generated_artifacts",
            "fe_responsibilities",
            "fixed_host",
        ] {
            provenance.remove(field);
        }
        let unattributed: WebBundleManifest = serde_json::from_value(unattributed).unwrap();
        assert!(unattributed.provenance.fixed_host.contract.is_empty());
        assert!(unattributed.provenance.generated_artifacts.is_empty());

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
                "shade",
            ],
            "the render fallback and canonical wrappers are public; authored message lanes stay private",
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
        let shade = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "shade")
            .unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();
        assert_eq!(shade.call(&mut store, (19, 23)).unwrap(), 42);
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
                "runtime/worker-host-core.js",
                "runtime/actor-client.js",
                "runtime/actor-client-core.js",
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
            .find_map(|(path, source)| (*path == "runtime/worker-host-core.js").then_some(*source))
            .unwrap();
        assert!(worker_host.contains("createCanonicalIntentRouter"));
        assert!(worker_host.contains("adapter.intents"));
        assert!(!worker_host.contains("lanes: ["));
        assert!(!worker_host.contains("[\"render\", \"verify\"]"));
        let actor_client = WEB_ACTOR_RUNTIME
            .iter()
            .find_map(|(path, source)| (*path == "runtime/actor-client-core.js").then_some(*source))
            .unwrap();
        assert!(actor_client.contains("createCanonicalBrowserActor"));
        assert!(actor_client.contains("createCanonicalBrowserWorkerScope"));
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
        bundle.manifest.artifacts.wasm = Some("../module.wasm".to_owned());
        let error = bundle.materialized_files().unwrap_err().to_string();
        assert!(error.contains("safe bundle-relative path"), "{error}");

        let mut bundle = compile(WebBundleMode::Render);
        bundle.manifest.artifacts.wgsl = bundle.manifest.artifacts.wasm.clone().unwrap();
        let error = bundle.materialized_files().unwrap_err().to_string();
        assert!(error.contains("duplicate artifact path"), "{error}");

        let mut bundle = compile(WebBundleMode::Render);
        *bundle.manifest.artifacts.wasm_bytes.as_mut().unwrap() += 1;
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
