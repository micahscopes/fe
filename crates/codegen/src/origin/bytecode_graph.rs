use common::origin::OriginLinkKind;

use super::{
    bytecode_origins::{BytecodeOriginRecord, BytecodeOriginSource},
    codegen_graph::{CodegenOriginGraph, CodegenOriginNode},
    sonatina_post_opt::{
        push_sonatina_backend_prepared_origin_record, push_sonatina_post_opt_origin_record,
    },
};

pub(super) fn push_bytecode_origin_record<'db>(
    graph: &mut CodegenOriginGraph,
    record: &BytecodeOriginRecord<'db>,
) {
    let pc = CodegenOriginNode::BytecodePc(record.origin().clone());
    match record.source() {
        BytecodeOriginSource::SonatinaPostOpt(post_opt) => {
            let post_opt_node = push_sonatina_post_opt_origin_record(graph, post_opt);
            graph.push(post_opt_node, pc, OriginLinkKind::Lowered);
        }
        BytecodeOriginSource::SonatinaBackendPrepared(backend_prepared) => {
            let backend_prepared_node =
                push_sonatina_backend_prepared_origin_record(graph, backend_prepared);
            graph.push(backend_prepared_node, pc, OriginLinkKind::Lowered);
        }
        BytecodeOriginSource::Unmapped(reason) => graph.push(
            CodegenOriginNode::BytecodeUnmapped(reason),
            pc,
            OriginLinkKind::Synthetic,
        ),
    }
}
