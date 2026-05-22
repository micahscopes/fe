use fe_mir::{
    RuntimeOriginFactNode, RuntimeOriginFactOwnerKeys, RuntimeOriginFactRuntimeOwnerKey,
    RuntimeOriginFactSemanticOwnerKey, RuntimeOriginFactSyntheticLocalKey,
    RuntimeOriginFactTargetKey, RuntimePackageBodySymbol, RuntimePackageOrigins, RuntimeStmtOrigin,
    RuntimeTerminatorOrigin, runtime_package_origin_facts, runtime_stmt_export_key,
    runtime_terminator_export_key,
};

fn semantic_owner_is_not_a_runtime_stmt_owner<'db>(
    origin: RuntimeStmtOrigin<'db>,
    owner_key: RuntimeOriginFactSemanticOwnerKey,
) {
    let _ = RuntimeOriginFactNode::Stmt { origin, owner_key };
}

fn semantic_owner_is_not_a_runtime_terminator_owner<'db>(
    origin: RuntimeTerminatorOrigin<'db>,
    owner_key: RuntimeOriginFactSemanticOwnerKey,
) {
    let _ = RuntimeOriginFactNode::Terminator { origin, owner_key };
}

fn semantic_owner_is_not_a_synthetic_owner(owner_key: RuntimeOriginFactSemanticOwnerKey) {
    let _ = RuntimeOriginFactNode::Synthetic {
        owner_key,
        local_key: RuntimeOriginFactSyntheticLocalKey::new("block:0:stmt:0"),
    };
}

fn runtime_owner_is_not_a_semantic_owner(owner_key: RuntimeOriginFactRuntimeOwnerKey) {
    expects_semantic_owner(owner_key);
}

fn owner_key_bundle_rejects_swapped_keys(
    semantic: RuntimeOriginFactSemanticOwnerKey,
    runtime: RuntimeOriginFactRuntimeOwnerKey,
) {
    let _ = RuntimeOriginFactOwnerKeys::new(runtime, semantic);
}

fn runtime_fact_owner_keys_reject_raw_target_labels(symbol: RuntimePackageBodySymbol) {
    let _ = RuntimeOriginFactOwnerKeys::for_body("contract:Foo", &symbol);
}

fn runtime_fact_owner_keys_reject_raw_body_symbols(target: RuntimeOriginFactTargetKey) {
    let _ = RuntimeOriginFactOwnerKeys::for_body(&target, "runtime_main");
}

fn runtime_fact_export_callback_rejects_raw_strings<'db>(origins: &RuntimePackageOrigins<'db>) {
    let _ = runtime_package_origin_facts(origins, |_| "runtime:test".to_string());
}

fn runtime_export_helpers_reject_raw_owner_strings<'db>(
    stmt_origin: RuntimeStmtOrigin<'db>,
    terminator_origin: RuntimeTerminatorOrigin<'db>,
) {
    let _ = runtime_stmt_export_key(stmt_origin, "runtime:test");
    let _ = runtime_terminator_export_key(terminator_origin, "runtime:test");
}

fn expects_semantic_owner(_: RuntimeOriginFactSemanticOwnerKey) {}

fn main() {}
