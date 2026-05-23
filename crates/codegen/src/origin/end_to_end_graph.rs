use common::{
    facts::{TypedFactSet, try_origin_graph_facts},
    origin::{OriginExportKey, OriginExportKind, OriginExportLocalKey, OriginLinkKind},
};
use hir::origin::SemanticOrigin;
use mir::{
    RuntimeOriginOwnerKey, RuntimeOriginSource, RuntimePackageOrigins, RuntimeStmtOrigin,
    RuntimeTerminatorOrigin, runtime_stmt_export_key, runtime_terminator_export_key,
};
use sonatina_ir::module::FuncRef;

use super::function_keys::{SonatinaFunctionKeyMap, collect_sonatina_function_keys};
use super::{
    BytecodeOriginRecord, BytecodeOriginSource, BytecodePcOrigin, BytecodeUnmappedReason,
    MissingSonatinaFunctionKey, SonatinaBackendPreparedOriginSource, SonatinaFunctionExportKey,
    SonatinaFunctionOrigins, SonatinaInstOrigin, SonatinaOriginSource, SonatinaPackageOrigins,
    SonatinaPostOptOriginSource, SonatinaSyntheticOrigin, bytecode_pc_export_key,
    bytecode_unmapped_export_key, sonatina_inst_export_key, sonatina_synthetic_export_key,
};

common::define_origin_owner_key! {
    pub struct EndToEndSemanticOwnerKey;
}

impl hir::origin::SemanticOriginOwnerKey for EndToEndSemanticOwnerKey {}

common::define_origin_owner_key! {
    pub struct EndToEndRuntimeOwnerKey;
}

impl RuntimeOriginOwnerKey for EndToEndRuntimeOwnerKey {}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EndToEndOriginOwnerKeys {
    semantic: EndToEndSemanticOwnerKey,
    runtime: EndToEndRuntimeOwnerKey,
}

impl EndToEndOriginOwnerKeys {
    fn new(semantic: EndToEndSemanticOwnerKey, runtime: EndToEndRuntimeOwnerKey) -> Self {
        Self { semantic, runtime }
    }

    pub fn for_function(function_key: &SonatinaFunctionExportKey) -> Self {
        Self::new(
            EndToEndSemanticOwnerKey::new(function_key.as_str()),
            EndToEndRuntimeOwnerKey::new(function_key.as_str()),
        )
    }

    pub fn semantic(&self) -> &EndToEndSemanticOwnerKey {
        &self.semantic
    }

    pub fn runtime(&self) -> &EndToEndRuntimeOwnerKey {
        &self.runtime
    }
}

common::define_origin_local_key! {
    pub struct EndToEndRuntimeSyntheticLocalKey;
}

impl EndToEndRuntimeSyntheticLocalKey {
    pub fn for_stmt_site(site: mir::RuntimeStmtSite) -> Self {
        Self::new(site.to_export_local_key())
    }

    pub fn for_terminator(block: mir::RBlockId) -> Self {
        Self::new(mir::RuntimeTerminatorSite::new(block).to_export_local_key())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EndToEndOriginNode<'db> {
    Semantic {
        origin: SemanticOrigin<'db>,
        owner_key: EndToEndSemanticOwnerKey,
    },
    RuntimeSynthetic {
        owner_key: EndToEndRuntimeOwnerKey,
        local_key: EndToEndRuntimeSyntheticLocalKey,
    },
    RuntimeStmt {
        origin: RuntimeStmtOrigin<'db>,
        owner_key: EndToEndRuntimeOwnerKey,
    },
    RuntimeTerminator {
        origin: RuntimeTerminatorOrigin<'db>,
        owner_key: EndToEndRuntimeOwnerKey,
    },
    SonatinaInst(SonatinaInstOrigin),
    SonatinaSynthetic(SonatinaSyntheticOrigin),
    BytecodeUnmapped(BytecodeUnmappedReason),
    BytecodePc(BytecodePcOrigin),
}

common::define_origin_graph_type! {
    pub struct EndToEndOriginGraph<'db>(EndToEndOriginNode<'db>);
}

pub fn end_to_end_origin_graph_facts(
    graph: &EndToEndOriginGraph<'_>,
    mut stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
) -> Result<TypedFactSet, MissingSonatinaFunctionKey> {
    let function_keys = collect_end_to_end_graph_function_keys(graph, &mut stable_function_key)?;
    end_to_end_origin_graph_facts_with_function_keys(graph, &function_keys)
}

pub(super) fn end_to_end_origin_graph_facts_with_function_keys(
    graph: &EndToEndOriginGraph<'_>,
    function_keys: &SonatinaFunctionKeyMap,
) -> Result<TypedFactSet, MissingSonatinaFunctionKey> {
    try_origin_graph_facts(graph.as_origin_graph(), |node| {
        end_to_end_origin_node_export_key_checked(node, function_keys)
    })
}

pub fn end_to_end_origin_node_export_key(
    node: &EndToEndOriginNode<'_>,
    mut stable_function_key: impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
) -> Option<OriginExportKey> {
    match node {
        EndToEndOriginNode::Semantic { origin, owner_key } => Some(origin.export_key(owner_key)),
        EndToEndOriginNode::RuntimeSynthetic {
            owner_key,
            local_key,
        } => Some(OriginExportKey::new(
            OriginExportKind::RuntimeSynthetic,
            owner_key,
            local_key,
        )),
        EndToEndOriginNode::RuntimeStmt { origin, owner_key } => {
            Some(runtime_stmt_export_key(*origin, owner_key))
        }
        EndToEndOriginNode::RuntimeTerminator { origin, owner_key } => {
            Some(runtime_terminator_export_key(*origin, owner_key))
        }
        EndToEndOriginNode::SonatinaInst(origin) => stable_function_key(origin.function())
            .map(|function_key| sonatina_inst_export_key(*origin, &function_key)),
        EndToEndOriginNode::SonatinaSynthetic(origin) => {
            Some(sonatina_synthetic_export_key(*origin))
        }
        EndToEndOriginNode::BytecodeUnmapped(reason) => Some(bytecode_unmapped_export_key(*reason)),
        EndToEndOriginNode::BytecodePc(origin) => Some(bytecode_pc_export_key(origin.clone())),
    }
}

fn end_to_end_origin_node_export_key_checked(
    node: &EndToEndOriginNode<'_>,
    function_keys: &SonatinaFunctionKeyMap,
) -> Result<OriginExportKey, MissingSonatinaFunctionKey> {
    match node {
        EndToEndOriginNode::Semantic { origin, owner_key } => Ok(origin.export_key(owner_key)),
        EndToEndOriginNode::RuntimeSynthetic {
            owner_key,
            local_key,
        } => Ok(OriginExportKey::new(
            OriginExportKind::RuntimeSynthetic,
            owner_key,
            local_key,
        )),
        EndToEndOriginNode::RuntimeStmt { origin, owner_key } => {
            Ok(runtime_stmt_export_key(*origin, owner_key))
        }
        EndToEndOriginNode::RuntimeTerminator { origin, owner_key } => {
            Ok(runtime_terminator_export_key(*origin, owner_key))
        }
        EndToEndOriginNode::SonatinaInst(origin) => {
            let function_key = function_keys.get(origin.function())?;
            Ok(sonatina_inst_export_key(*origin, function_key))
        }
        EndToEndOriginNode::SonatinaSynthetic(origin) => Ok(sonatina_synthetic_export_key(*origin)),
        EndToEndOriginNode::BytecodeUnmapped(reason) => Ok(bytecode_unmapped_export_key(*reason)),
        EndToEndOriginNode::BytecodePc(origin) => Ok(bytecode_pc_export_key(origin.clone())),
    }
}

pub(super) fn push_bytecode_end_to_end_origin_record<'db>(
    graph: &mut EndToEndOriginGraph<'db>,
    record: &BytecodeOriginRecord<'db>,
) {
    let pc = EndToEndOriginNode::BytecodePc(record.origin().clone());
    match record.source() {
        BytecodeOriginSource::SonatinaPostOpt(post_opt) => {
            let post_opt_node = EndToEndOriginNode::SonatinaInst(post_opt.origin());
            match post_opt.source() {
                SonatinaPostOptOriginSource::SameInstId(pre_opt) => graph.push(
                    EndToEndOriginNode::SonatinaInst(pre_opt.origin()),
                    post_opt_node.clone(),
                    OriginLinkKind::Alias,
                ),
                SonatinaPostOptOriginSource::CreatedOrUnmatchedAfterPreOptSnapshot => graph.push(
                    EndToEndOriginNode::SonatinaSynthetic(
                        SonatinaSyntheticOrigin::PostPreOptSnapshotGap,
                    ),
                    post_opt_node.clone(),
                    OriginLinkKind::Synthetic,
                ),
            }
            graph.push(post_opt_node, pc, OriginLinkKind::Lowered);
        }
        BytecodeOriginSource::SonatinaBackendPrepared(backend_prepared) => {
            let backend_prepared_node = EndToEndOriginNode::SonatinaInst(backend_prepared.origin());
            match backend_prepared.source() {
                SonatinaBackendPreparedOriginSource::MissingPostOptSnapshotRecord => graph.push(
                    EndToEndOriginNode::SonatinaSynthetic(
                        SonatinaSyntheticOrigin::PostPreOptSnapshotGap,
                    ),
                    backend_prepared_node.clone(),
                    OriginLinkKind::Synthetic,
                ),
            }
            graph.push(backend_prepared_node, pc, OriginLinkKind::Lowered);
        }
        BytecodeOriginSource::Unmapped(reason) => graph.push(
            EndToEndOriginNode::BytecodeUnmapped(reason),
            pc,
            OriginLinkKind::Synthetic,
        ),
    }
}

pub(super) fn push_selected_runtime_and_sonatina_origins<'db>(
    graph: &mut EndToEndOriginGraph<'db>,
    sonatina_origins: &SonatinaPackageOrigins<'db>,
    runtime_origins: &RuntimePackageOrigins<'db>,
    function_keys: &SonatinaFunctionKeyMap,
) {
    for function_origins in sonatina_origins.functions() {
        let Some(owner_key) = function_keys.get_optional(function_origins.function()) else {
            continue;
        };
        let owner_keys = EndToEndOriginOwnerKeys::for_function(owner_key);
        push_runtime_source_origins(
            graph,
            runtime_origins.body_for_instance(function_origins.runtime_instance()),
            &owner_keys,
        );
        push_sonatina_preopt_origins(graph, function_origins, &owner_keys);
    }
}

fn push_runtime_source_origins<'db>(
    graph: &mut EndToEndOriginGraph<'db>,
    body: Option<&mir::RuntimePackageBodyOrigins<'db>>,
    owner_keys: &EndToEndOriginOwnerKeys,
) {
    let Some(body) = body else {
        return;
    };
    let semantic_owner_key = owner_keys.semantic().clone();
    let runtime_owner_key = owner_keys.runtime().clone();

    for record in body.origins().stmt_origins() {
        let local_key = EndToEndRuntimeSyntheticLocalKey::for_stmt_site(record.origin().site());
        let target = EndToEndOriginNode::RuntimeStmt {
            origin: record.origin(),
            owner_key: runtime_owner_key.clone(),
        };
        push_runtime_source_link(
            graph,
            record.source(),
            semantic_owner_key.clone(),
            runtime_owner_key.clone(),
            local_key,
            target,
        );
    }

    for record in body.origins().terminator_origins() {
        let local_key = EndToEndRuntimeSyntheticLocalKey::for_terminator(record.origin().block());
        let target = EndToEndOriginNode::RuntimeTerminator {
            origin: record.origin(),
            owner_key: runtime_owner_key.clone(),
        };
        push_runtime_source_link(
            graph,
            record.source(),
            semantic_owner_key.clone(),
            runtime_owner_key.clone(),
            local_key,
            target,
        );
    }
}

fn push_runtime_source_link<'db>(
    graph: &mut EndToEndOriginGraph<'db>,
    source: RuntimeOriginSource<'db>,
    semantic_owner_key: EndToEndSemanticOwnerKey,
    runtime_owner_key: EndToEndRuntimeOwnerKey,
    local_key: EndToEndRuntimeSyntheticLocalKey,
    target: EndToEndOriginNode<'db>,
) {
    match source {
        RuntimeOriginSource::Semantic(origin) => graph.push(
            EndToEndOriginNode::Semantic {
                origin,
                owner_key: semantic_owner_key,
            },
            target,
            OriginLinkKind::Lowered,
        ),
        RuntimeOriginSource::Synthetic => graph.push(
            EndToEndOriginNode::RuntimeSynthetic {
                owner_key: runtime_owner_key,
                local_key,
            },
            target,
            OriginLinkKind::Synthetic,
        ),
    }
}

fn push_sonatina_preopt_origins<'db>(
    graph: &mut EndToEndOriginGraph<'db>,
    function_origins: &SonatinaFunctionOrigins<'db>,
    owner_keys: &EndToEndOriginOwnerKeys,
) {
    let runtime_owner_key = owner_keys.runtime().clone();
    for record in function_origins.records() {
        let target = EndToEndOriginNode::SonatinaInst(record.origin());
        match record.source() {
            SonatinaOriginSource::RuntimeStmt(origin) => graph.push(
                EndToEndOriginNode::RuntimeStmt {
                    origin,
                    owner_key: runtime_owner_key.clone(),
                },
                target,
                OriginLinkKind::Lowered,
            ),
            SonatinaOriginSource::RuntimeTerminator(origin) => graph.push(
                EndToEndOriginNode::RuntimeTerminator {
                    origin,
                    owner_key: runtime_owner_key.clone(),
                },
                target,
                OriginLinkKind::Lowered,
            ),
            SonatinaOriginSource::Synthetic(origin) => graph.push(
                EndToEndOriginNode::SonatinaSynthetic(origin),
                target,
                OriginLinkKind::Synthetic,
            ),
            SonatinaOriginSource::Unmapped(_) => {}
        }
    }
}

pub(super) fn collect_end_to_end_graph_function_keys(
    graph: &EndToEndOriginGraph<'_>,
    stable_function_key: &mut impl FnMut(FuncRef) -> Option<SonatinaFunctionExportKey>,
) -> Result<SonatinaFunctionKeyMap, MissingSonatinaFunctionKey> {
    collect_sonatina_function_keys(graph.links(), stable_function_key, end_to_end_node_function)
}

fn end_to_end_node_function(node: &EndToEndOriginNode<'_>) -> Option<FuncRef> {
    match node {
        EndToEndOriginNode::SonatinaInst(origin) => Some(origin.function()),
        EndToEndOriginNode::Semantic { .. }
        | EndToEndOriginNode::RuntimeSynthetic { .. }
        | EndToEndOriginNode::RuntimeStmt { .. }
        | EndToEndOriginNode::RuntimeTerminator { .. }
        | EndToEndOriginNode::SonatinaSynthetic(_)
        | EndToEndOriginNode::BytecodeUnmapped(_)
        | EndToEndOriginNode::BytecodePc(_) => None,
    }
}
