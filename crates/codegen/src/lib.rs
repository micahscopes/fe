mod actor_semantics;
mod backend;
mod browser_actor_runtime;
pub mod canonical_interface;
pub mod capstone_evidence;
pub mod dispatch;
mod function_symbols;
pub mod guest_callbacks;
mod layout;
mod page_projection;
mod resident_actor;
mod runtime_package;
mod scoped_task_package;
mod sonatina;
mod test_output;
#[cfg(feature = "spirv-backend")]
mod web_bundle;

pub use backend::{
    Backend, BackendError, BackendKind, BackendOutput, OptLevel, SonatinaBackend, SpirvBackend,
    WasmBackend, layout_for,
};
pub use browser_actor_runtime::{
    BROWSER_ACTOR_RUNTIME_PROTOCOL, BROWSER_ACTOR_RUNTIME_VERSION, browser_actor_runtime_files,
};
pub use canonical_interface::{
    CANONICAL_INTERFACE_PROTOCOL, CANONICAL_INTERFACE_VERSION, CanonicalAbi, CanonicalCapability,
    CanonicalCapabilityRequirement, CanonicalEndianness, CanonicalExecution, CanonicalField,
    CanonicalFieldLayout, CanonicalInterfaceError, CanonicalInterfaceManifest, CanonicalLane,
    CanonicalLaneDecl, CanonicalLaneIntent, CanonicalLayout, CanonicalListElement,
    CanonicalPlacement, CanonicalShape, CanonicalType, CanonicalVariant, CanonicalVariantLayout,
    canonical_lane_decl_from_entry, canonical_lane_decls_from_actor,
    canonical_lane_decls_from_module, canonical_type_from_semantic, emit_canonical_interface_js,
    verify_canonical_wasm_abi,
};
pub use dispatch::DispatchKind;
pub use layout::{
    DISCRIMINANT_SIZE_BYTES, EVM_LAYOUT, Endianness, TargetDataLayout, WASM_LAYOUT, WORD_SIZE_BYTES,
};
pub use page_projection::{
    ComponentProjection, PageAttributeKind, PageElement, PageProjection, PageProjectionError,
    PageProjectionOp, ProjectedPageAttribute, ProjectedPageComponent, ProjectedPageRender,
    project_component, project_page,
};
pub use resident_actor::{
    RESIDENT_ACTOR_INITIALIZE_EXPORT, RESIDENT_ACTOR_PROJECT_EXPORT,
    RESIDENT_ACTOR_STATE_REPLACE_EXPORT, RESIDENT_ACTOR_TRANSITION_EXPORT, ResidentActorArtifact,
    ResidentActorContract, ResidentActorError, StructuredChildActorArtifact,
    StructuredChildScopeImports, compile_resident_actor, compile_resident_actor_with_optimization,
    resident_actor_contract,
};
pub use scoped_task_package::{
    ScopedTaskPackage, ScopedTaskPackageError, ScopedTaskPackageFile,
    materialize_scoped_task_package,
};
#[cfg(all(
    feature = "native-backend",
    not(target_arch = "wasm32"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use sonatina::{
    GRID_LOOP_NATIVE_ENTRY_ARITY, MERKLE_ROOT_NATIVE_ENTRY_ARITY, MERKLE8_ROOT_NATIVE_ENTRY_ARITY,
    NativeGpuDeviceEventKind, NativeGpuDeviceLossReason, NativeGridLoopEntryArtifact,
    NativeI32EntryArtifact, NativeMerkle8RootEntryArtifact, NativeMerkleRootEntryArtifact,
    NativeSurfaceEvent, NativeSurfaceEventKind, NativeSurfaceQueueAction,
    NativeSurfaceRecoveryAction, NativeSurfaceRecoveryArtifact, NativeSurfaceRecoveryEvent,
    NativeSurfaceRecoveryState, NativeSurfaceRecoveryStep, NativeSurfaceScheduleArtifact,
    NativeSurfaceScheduleState, NativeSurfaceScheduleStep, NativeSurfaceTransition4F32Artifact,
    compile_runtime_package_native_grid_loop_entry, compile_runtime_package_native_i32_entry,
    compile_runtime_package_native_merkle_root_entry,
    compile_runtime_package_native_merkle8_root_entry,
    compile_runtime_package_native_surface_transition4_f32,
};
pub use sonatina::{
    HOST_COMPLETION_RUNTIME_JS, LowerError, MATERIALIZED_TASK_RUNTIME_JS, SonatinaContractBytecode,
    SonatinaTestOptions, WasmCompileOptions, WasmTaskAdapter, WasmTaskContinuation,
    WasmTaskDelivery, WasmTaskRange, WasmTaskScalar, compile_runtime_package_wasm_with_options,
    emit_ingot_sonatina_bytecode, emit_ingot_sonatina_ir, emit_ingot_sonatina_ir_optimized,
    emit_materialized_task_adapter_js, emit_module_sonatina_bytecode, emit_module_sonatina_ir,
    emit_module_sonatina_ir_optimized, emit_runtime_package_sonatina_ir,
    emit_runtime_package_sonatina_ir_optimized, emit_test_ingot_sonatina,
    emit_test_module_sonatina, materialized_task_adapters, validate_module_sonatina_ir,
};
#[cfg(feature = "spirv-backend")]
pub use sonatina::{
    compile_render_wgsl, compile_runtime_package_spirv,
    compile_runtime_package_spirv_authored_raster,
    compile_runtime_package_spirv_compute_with_resources, compile_runtime_package_spirv_grid,
    compile_runtime_package_spirv_render, compile_runtime_package_spirv_render_with_resources,
    compile_runtime_package_spirv_with_workgroup,
};
pub use test_output::{ExpectedRevert, TestMetadata, TestModuleOutput, parse_expected_revert};

/// Embeds a CTFE-oriented ingot as a standalone root-module source artifact.
///
/// A true dependency ingot may expose public const helpers without making
/// them runtime exports. Source composition loses that module boundary, so
/// standalone publication removes only callable const-helper visibility while
/// preserving public types, type functions, and runtime functions.
pub fn standalone_ctfe_ingot_source(source: &str) -> String {
    source.replace("pub const fn ", "const fn ")
}
#[cfg(feature = "spirv-backend")]
pub use web_bundle::{
    WEB_ACTOR_RUNTIME_PROTOCOL, WEB_ACTOR_RUNTIME_VERSION, WEB_BUNDLE_PROTOCOL,
    WEB_BUNDLE_PROTOCOL_VERSION, WebActorProgram, WebActorResource, WebActorResourceElement,
    WebActorResourceField, WebActorStage, WebActorStageKind, WebArtifactManifest,
    WebAuthoredSourceKind, WebBinding, WebBindingAccess, WebBindingMember, WebBindingRole,
    WebBrowserRuntimeManifest, WebBuildOptions, WebBuiltinInput, WebBuiltinSource, WebBundle,
    WebBundleError, WebBundleFile, WebBundleManifest, WebBundleMode, WebCanonicalPolicy,
    WebCanonicalStatus, WebControl, WebControlArgSource, WebControlWasmType, WebFeResponsibility,
    WebFixedHostProvenance, WebGeneratedArtifact, WebGeneratedArtifactKind, WebHostResponsibility,
    WebLayout, WebPass, WebPassShader, WebProvenance, WebResource, WebResult, WebScalarKind,
    WebSourceProvenance, actor_gpu_program, actor_web_entry, render_runtime_js, resolve_web_entry,
};
#[cfg(all(
    feature = "spirv-backend",
    feature = "native-backend",
    not(target_arch = "wasm32"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use web_bundle::{
    compile_native_surface_recovery_policy, compile_native_surface_schedule_policy,
};
