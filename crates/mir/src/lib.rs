pub mod db;
pub mod instance;
pub mod origin;
pub mod runtime;
pub mod verify;

pub use db::MirDb;
pub use instance::{RuntimeInstance, RuntimeInstanceKey, get_or_build_runtime_instance};
pub use origin::{
    RuntimeBodyOrigins, RuntimeCodeRegionOrigin, RuntimeCodeRegionOwnerKey, RuntimeOriginFactGraph,
    RuntimeOriginFactNode, RuntimeOriginFactOwnerKeys, RuntimeOriginFactRuntimeOwnerKey,
    RuntimeOriginFactSemanticOwnerKey, RuntimeOriginFactSyntheticLocalKey,
    RuntimeOriginFactTargetKey, RuntimeOriginGraph, RuntimeOriginNode, RuntimeOriginOwnerKey,
    RuntimeOriginSource, RuntimePackageBodyOrigins, RuntimePackageBodySymbol,
    RuntimePackageOrigins, RuntimeStmtIndex, RuntimeStmtOrigin, RuntimeStmtOriginRecord,
    RuntimeStmtSite, RuntimeTerminatorOrigin, RuntimeTerminatorOriginRecord, RuntimeTerminatorSite,
    runtime_code_region_export_key, runtime_origin_fact_node_export_key,
    runtime_package_origin_fact_graph, runtime_package_origin_facts, runtime_package_origins,
    runtime_stmt_export_key, runtime_terminator_export_key,
};
pub use runtime::{
    AddressSpaceKind, ArrayLayout, BorrowAccess, BorrowTransportSet, ConstNode, ConstRegion,
    ConstRegionId, ConstScalar, EnumLayout, EnumVariantLayout, IntrinsicArithBinOp, Layout,
    LayoutId, LowerError, LoweredRuntimeBody, PlaceElem, PlaceRoot, RBlock, RBlockId, RExpr,
    RLocal, RLocalId, RStmt, RTerminator, RValueId, RefKind, RefView, ResolvedCodeRegion,
    ResolvedPlaceElem, ResolvedPlaceRootKind, ResolvedRuntimePlace, RuntimeBody,
    RuntimeBoundarySpec, RuntimeBuiltin, RuntimeCallEdge, RuntimeCarrier, RuntimeClass,
    RuntimeCodeRegion, RuntimeCodeRegionKey, RuntimeEmbed, RuntimeFunction, RuntimeFunctionOwner,
    RuntimeInlineHint, RuntimeInterfaceSignature, RuntimeLinkage, RuntimeLocalRoot, RuntimeObject,
    RuntimePackage, RuntimeParam, RuntimePlace, RuntimeProgramView, RuntimeReturnPlan,
    RuntimeSection, RuntimeSectionName, RuntimeSectionRef, RuntimeSyntheticSpec, SaturatingBinOp,
    ScalarClass, ScalarRepr, ScalarRole, StructLayout, VariantId, array_elem_size_bytes,
    build_runtime_package, build_test_runtime_package, enum_tag_size_bytes,
    enum_variant_field_offset_bytes, format_runtime_body, format_runtime_body_excerpt,
    format_runtime_package, format_runtime_verify_failure, layout_size_bytes,
    runtime_instance_stable_key, runtime_instance_symbol_key, serialize_const_region_bytes,
    struct_field_offset_bytes,
};
pub use verify::{
    VerifyError, resolve_runtime_place, resolve_runtime_place_address_class, verify_const_region,
    verify_runtime_body, verify_runtime_package,
};
