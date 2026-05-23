use common::{
    facts::{TypedFactSet, try_origin_graph_facts},
    origin::OriginExportKey,
};
use sonatina_ir::module::FuncRef;

use super::function_keys::{SonatinaFunctionKeyMap, collect_sonatina_function_keys};
use super::{
    BytecodePcOrigin, BytecodeUnmappedReason, MissingSonatinaFunctionKey,
    SonatinaFunctionExportKey, SonatinaInstOrigin, SonatinaSyntheticOrigin, bytecode_pc_export_key,
    bytecode_unmapped_export_key, sonatina_inst_export_key, sonatina_synthetic_export_key,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CodegenOriginNode {
    SonatinaInst(SonatinaInstOrigin),
    SonatinaSynthetic(SonatinaSyntheticOrigin),
    BytecodeUnmapped(BytecodeUnmappedReason),
    BytecodePc(BytecodePcOrigin),
}

pub fn codegen_origin_node_export_key(
    node: &CodegenOriginNode,
    mut stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
) -> Option<OriginExportKey> {
    match node {
        CodegenOriginNode::SonatinaInst(origin) => stable_function_key(origin.function())
            .map(|function_key| sonatina_inst_export_key(*origin, &function_key)),
        CodegenOriginNode::SonatinaSynthetic(origin) => {
            Some(sonatina_synthetic_export_key(*origin))
        }
        CodegenOriginNode::BytecodeUnmapped(reason) => Some(bytecode_unmapped_export_key(*reason)),
        CodegenOriginNode::BytecodePc(origin) => Some(bytecode_pc_export_key(origin.clone())),
    }
}

pub fn codegen_origin_graph_facts(
    graph: &CodegenOriginGraph,
    mut stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
) -> Result<TypedFactSet, MissingSonatinaFunctionKey> {
    let function_keys = collect_codegen_graph_function_keys(graph, &mut stable_function_key)?;
    codegen_origin_graph_facts_with_function_keys(graph, &function_keys)
}

fn codegen_origin_graph_facts_with_function_keys(
    graph: &CodegenOriginGraph,
    function_keys: &SonatinaFunctionKeyMap,
) -> Result<TypedFactSet, MissingSonatinaFunctionKey> {
    try_origin_graph_facts(graph.as_origin_graph(), |node| {
        codegen_origin_node_export_key_checked(node, function_keys)
    })
}

fn collect_codegen_graph_function_keys(
    graph: &CodegenOriginGraph,
    stable_function_key: &mut impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
) -> Result<SonatinaFunctionKeyMap, MissingSonatinaFunctionKey> {
    collect_sonatina_function_keys(graph.links(), stable_function_key, codegen_node_function)
}

fn codegen_node_function(node: &CodegenOriginNode) -> Option<FuncRef> {
    match node {
        CodegenOriginNode::SonatinaInst(origin) => Some(origin.function()),
        CodegenOriginNode::SonatinaSynthetic(_)
        | CodegenOriginNode::BytecodeUnmapped(_)
        | CodegenOriginNode::BytecodePc(_) => None,
    }
}

fn codegen_origin_node_export_key_checked(
    node: &CodegenOriginNode,
    function_keys: &SonatinaFunctionKeyMap,
) -> Result<OriginExportKey, MissingSonatinaFunctionKey> {
    match node {
        CodegenOriginNode::SonatinaInst(origin) => {
            let function_key = function_keys.get(origin.function())?;
            Ok(sonatina_inst_export_key(*origin, function_key))
        }
        CodegenOriginNode::SonatinaSynthetic(origin) => Ok(sonatina_synthetic_export_key(*origin)),
        CodegenOriginNode::BytecodeUnmapped(reason) => Ok(bytecode_unmapped_export_key(*reason)),
        CodegenOriginNode::BytecodePc(origin) => Ok(bytecode_pc_export_key(origin.clone())),
    }
}

common::define_origin_graph_type! {
    pub struct CodegenOriginGraph(CodegenOriginNode);
}
