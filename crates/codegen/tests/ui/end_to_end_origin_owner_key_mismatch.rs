use fe_codegen::origin::{
    EndToEndOriginNode, EndToEndOriginOwnerKeys, EndToEndRuntimeOwnerKey,
    EndToEndRuntimeSyntheticLocalKey, EndToEndSemanticOwnerKey,
};
use mir::{RuntimeStmtOrigin, RuntimeTerminatorOrigin};

fn semantic_owner_is_not_a_runtime_stmt_owner<'db>(
    origin: RuntimeStmtOrigin<'db>,
    owner_key: EndToEndSemanticOwnerKey,
) {
    let _ = EndToEndOriginNode::RuntimeStmt { origin, owner_key };
}

fn semantic_owner_is_not_a_runtime_terminator_owner<'db>(
    origin: RuntimeTerminatorOrigin<'db>,
    owner_key: EndToEndSemanticOwnerKey,
) {
    let _ = EndToEndOriginNode::RuntimeTerminator { origin, owner_key };
}

fn semantic_owner_is_not_a_runtime_synthetic_owner(owner_key: EndToEndSemanticOwnerKey) {
    let _ = EndToEndOriginNode::RuntimeSynthetic {
        owner_key,
        local_key: EndToEndRuntimeSyntheticLocalKey::new("block:0:stmt:0"),
    };
}

fn runtime_owner_is_not_a_semantic_owner(owner_key: EndToEndRuntimeOwnerKey) {
    expects_semantic_owner(owner_key);
}

fn end_to_end_owner_keys_reject_raw_function_keys() {
    let _ = EndToEndOriginOwnerKeys::for_function("sonatina:func:test");
}

fn expects_semantic_owner(_: EndToEndSemanticOwnerKey) {}

fn main() {}
