mod backend;
pub mod canonical_interface;
pub mod capstone_evidence;
pub mod dispatch;
mod function_symbols;
pub mod guest_callbacks;
mod layout;
pub mod resumable_tasks;
mod runtime_package;
mod sonatina;
mod test_output;
#[cfg(feature = "spirv-backend")]
mod web_bundle;

pub use backend::{
    Backend, BackendError, BackendKind, BackendOutput, OptLevel, SonatinaBackend, SpirvBackend,
    WasmBackend, layout_for,
};
pub use canonical_interface::{
    CANONICAL_INTERFACE_PROTOCOL, CANONICAL_INTERFACE_VERSION, CanonicalAbi, CanonicalCapability,
    CanonicalCapabilityRequirement, CanonicalEndianness, CanonicalExecution, CanonicalField,
    CanonicalFieldLayout, CanonicalInterfaceError, CanonicalInterfaceManifest, CanonicalLane,
    CanonicalLaneDecl, CanonicalLaneIntent, CanonicalLayout, CanonicalListElement,
    CanonicalPlacement, CanonicalShape, CanonicalType, CanonicalVariant, CanonicalVariantLayout,
    canonical_lane_decl_from_entry, canonical_type_from_semantic, verify_canonical_wasm_abi,
};
pub use dispatch::DispatchKind;
pub use layout::{
    DISCRIMINANT_SIZE_BYTES, EVM_LAYOUT, Endianness, TargetDataLayout, WASM_LAYOUT, WORD_SIZE_BYTES,
};
pub use sonatina::{
    LowerError, SonatinaContractBytecode, SonatinaTestOptions, WasmCompileOptions,
    compile_runtime_package_wasm_with_options, emit_ingot_sonatina_bytecode,
    emit_ingot_sonatina_ir, emit_ingot_sonatina_ir_optimized, emit_module_sonatina_bytecode,
    emit_module_sonatina_ir, emit_module_sonatina_ir_optimized, emit_runtime_package_sonatina_ir,
    emit_runtime_package_sonatina_ir_optimized, emit_test_ingot_sonatina,
    emit_test_module_sonatina, validate_module_sonatina_ir,
};
#[cfg(all(
    feature = "native-backend",
    not(target_arch = "wasm32"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use sonatina::{NativeI32EntryArtifact, compile_runtime_package_native_i32_entry};
#[cfg(feature = "spirv-backend")]
pub use sonatina::{
    compile_runtime_package_spirv, compile_runtime_package_spirv_grid,
    compile_runtime_package_spirv_render, compile_runtime_package_spirv_with_workgroup,
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
    WEB_BUNDLE_PROTOCOL_VERSION, WebArtifactManifest, WebBinding, WebBindingAccess,
    WebBindingMember, WebBindingRole, WebBrowserRuntimeManifest, WebBuildOptions, WebBuiltinInput,
    WebBuiltinSource, WebBundle, WebBundleError, WebBundleFile, WebBundleManifest, WebBundleMode,
    WebCanonicalPolicy, WebCanonicalStatus, WebGeneratedArtifact, WebLayout, WebProvenance,
    WebResult, WebScalarKind, actor_web_entry, resolve_web_entry,
};
