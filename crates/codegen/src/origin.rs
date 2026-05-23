mod bytecode_coverage;
mod bytecode_graph;
mod bytecode_keys;
mod bytecode_origins;
mod codegen_graph;
mod end_to_end_graph;
mod frontend_labels;
mod function_keys;
mod sonatina_post_opt;
mod sonatina_pre_opt;
mod source_resolution;

pub use bytecode_coverage::BytecodeOriginCoverage;
pub use bytecode_keys::{
    BytecodeObjectKey, BytecodePcOrigin, BytecodePcOwnerKey, BytecodePcRange, BytecodeSectionKey,
    BytecodeSectionNameKey, BytecodeUnmappedOwnerKey, BytecodeUnmappedReason,
    bytecode_pc_export_key, bytecode_unmapped_export_key,
};
#[cfg(test)]
use bytecode_origins::bytecode_source_from_pc_entry;
pub use bytecode_origins::{
    BytecodeOriginRecord, BytecodeOriginSource, BytecodePackageOrigins, BytecodePackageOriginsError,
};
pub use codegen_graph::{
    CodegenOriginGraph, CodegenOriginNode, codegen_origin_graph_facts,
    codegen_origin_node_export_key,
};
pub use end_to_end_graph::{
    EndToEndOriginGraph, EndToEndOriginNode, EndToEndOriginOwnerKeys, EndToEndRuntimeOwnerKey,
    EndToEndRuntimeSyntheticLocalKey, EndToEndSemanticOwnerKey, end_to_end_origin_graph_facts,
    end_to_end_origin_node_export_key,
};
pub use frontend_labels::{FrontendOriginLabel, FrontendOriginLabelMap};
pub use function_keys::{MissingSonatinaFunctionKey, SonatinaFunctionExportKey};
pub use sonatina_post_opt::{
    SonatinaBackendPreparedOriginRecord, SonatinaBackendPreparedOriginSource,
    SonatinaPostOptFunctionOrigins, SonatinaPostOptOriginCoverage, SonatinaPostOptOriginRecord,
    SonatinaPostOptOriginSource, SonatinaPostOptPackageOrigins, SonatinaPreOptSnapshotLossReason,
    SonatinaPreOptSnapshotLossRecord,
};
pub use sonatina_pre_opt::{
    SonatinaFunctionOrigins, SonatinaInstLocal, SonatinaInstOrigin, SonatinaInstOriginRecord,
    SonatinaInstStage, SonatinaOriginCoverage, SonatinaOriginGraph, SonatinaOriginNode,
    SonatinaOriginSource, SonatinaPackageOrigins, SonatinaSyntheticOrigin,
    SonatinaSyntheticOwnerKey, SonatinaUnmappedReason, sonatina_inst_export_key,
    sonatina_synthetic_export_key,
};
pub use source_resolution::{BytecodeSourceResolution, BytecodeSourceResolutionResult};

#[cfg(test)]
mod tests;
