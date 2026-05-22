use fe_codegen::origin::{
    CodegenOriginNode, SonatinaInstOrigin, codegen_origin_node_export_key,
    sonatina_inst_export_key,
};
use sonatina_ir::{InstId, module::FuncRef};

fn main() {
    let origin = SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), InstId::from_u32(7));

    let _ = sonatina_inst_export_key(origin, "sonatina:func:test");

    let node = CodegenOriginNode::SonatinaInst(origin);
    let _ = codegen_origin_node_export_key(&node, |_| Some("sonatina:func:test".to_string()));
}
