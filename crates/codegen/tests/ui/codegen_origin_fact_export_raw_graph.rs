use common::origin::OriginGraph;
use fe_codegen::origin::{CodegenOriginNode, codegen_origin_graph_facts};

fn main() {
    let raw_graph = OriginGraph::<CodegenOriginNode>::new();
    let _ = codegen_origin_graph_facts(&raw_graph, |_| None);
}
