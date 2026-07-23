pub mod actor_manifest;
mod backend;
pub mod canonical_interface;
pub mod dispatch;
mod function_symbols;
mod layout;
mod runtime_package;
mod sonatina;
mod test_output;
#[cfg(feature = "spirv-backend")]
mod web_bundle;

pub use actor_manifest::{
    ACTOR_PROTOCOL, ACTOR_PROTOCOL_VERSION, ActorLaneSpec, ActorManifestError, ActorRecordField,
    ActorScalar, actor_manifest_from_wasm_exports,
};
pub use backend::{
    Backend, BackendError, BackendKind, BackendOutput, OptLevel, SonatinaBackend, SpirvBackend,
    WasmBackend, layout_for,
};
pub use canonical_interface::{
    CANONICAL_INTERFACE_PROTOCOL, CANONICAL_INTERFACE_VERSION, CanonicalAbi, CanonicalEndianness,
    CanonicalField, CanonicalFieldLayout, CanonicalInterfaceError, CanonicalInterfaceManifest,
    CanonicalLane, CanonicalLaneDecl, CanonicalLayout, CanonicalShape, CanonicalType,
    canonical_lane_decl_from_entry, canonical_type_from_semantic,
};
pub use dispatch::DispatchKind;
pub use layout::{
    DISCRIMINANT_SIZE_BYTES, EVM_LAYOUT, Endianness, TargetDataLayout, WASM_LAYOUT, WORD_SIZE_BYTES,
};
pub use sonatina::{
    LowerError, SonatinaContractBytecode, SonatinaTestOptions, emit_ingot_sonatina_bytecode,
    emit_ingot_sonatina_ir, emit_ingot_sonatina_ir_optimized, emit_module_sonatina_bytecode,
    emit_module_sonatina_ir, emit_module_sonatina_ir_optimized, emit_runtime_package_sonatina_ir,
    emit_runtime_package_sonatina_ir_optimized, emit_test_ingot_sonatina,
    emit_test_module_sonatina, validate_module_sonatina_ir,
};
#[cfg(feature = "spirv-backend")]
pub use sonatina::{
    compile_runtime_package_spirv, compile_runtime_package_spirv_grid,
    compile_runtime_package_spirv_render, compile_runtime_package_spirv_with_workgroup,
};
pub use test_output::{ExpectedRevert, TestMetadata, TestModuleOutput, parse_expected_revert};
#[cfg(feature = "spirv-backend")]
pub use web_bundle::{
    WEB_BUNDLE_PROTOCOL, WEB_BUNDLE_PROTOCOL_VERSION, WebArtifactManifest, WebBinding,
    WebBindingAccess, WebBindingMember, WebBindingRole, WebBuildOptions, WebBuiltinInput,
    WebBuiltinSource, WebBundle, WebBundleError, WebBundleManifest, WebBundleMode, WebLayout,
    WebProvenance, WebResult, WebScalarKind,
};
