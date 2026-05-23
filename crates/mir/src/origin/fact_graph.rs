use common::{
    facts::{TypedFactSet, origin_graph_facts},
    origin::{OriginExportKey, OriginExportKind, OriginLinkKind},
};
use hir::origin::SemanticOrigin;

use crate::{
    origin::{
        RuntimeOriginOwnerKey, RuntimeOriginSource, RuntimePackageBodyOrigins,
        RuntimePackageBodySymbol, RuntimePackageOrigins, RuntimeStmtOrigin, RuntimeStmtSite,
        RuntimeTerminatorOrigin, RuntimeTerminatorSite, runtime_stmt_export_key,
        runtime_terminator_export_key,
    },
    runtime::RBlockId,
};

common::define_origin_owner_key! {
    pub struct RuntimeOriginFactSemanticOwnerKey;
}

impl hir::origin::SemanticOriginOwnerKey for RuntimeOriginFactSemanticOwnerKey {}

common::define_origin_owner_key! {
    pub struct RuntimeOriginFactRuntimeOwnerKey;
}

impl RuntimeOriginOwnerKey for RuntimeOriginFactRuntimeOwnerKey {}

common::define_origin_string_key! {
    /// Stable target label used to derive runtime origin fact owner namespaces.
    pub struct RuntimeOriginFactTargetKey;
}

common::define_origin_local_key! {
    pub struct RuntimeOriginFactSyntheticLocalKey;
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RuntimeOriginFactOwnerKeys {
    semantic: RuntimeOriginFactSemanticOwnerKey,
    runtime: RuntimeOriginFactRuntimeOwnerKey,
}

impl RuntimeOriginFactOwnerKeys {
    pub const fn new(
        semantic: RuntimeOriginFactSemanticOwnerKey,
        runtime: RuntimeOriginFactRuntimeOwnerKey,
    ) -> Self {
        Self { semantic, runtime }
    }

    pub fn for_body(
        target: &RuntimeOriginFactTargetKey,
        symbol: &RuntimePackageBodySymbol,
    ) -> Self {
        Self::new(
            RuntimeOriginFactSemanticOwnerKey::new(format!(
                "target:{}:semantic:{}",
                target.as_str(),
                symbol.as_str()
            )),
            RuntimeOriginFactRuntimeOwnerKey::new(format!(
                "target:{}:runtime:{}",
                target.as_str(),
                symbol.as_str()
            )),
        )
    }

    pub fn semantic(&self) -> &RuntimeOriginFactSemanticOwnerKey {
        &self.semantic
    }

    pub fn runtime(&self) -> &RuntimeOriginFactRuntimeOwnerKey {
        &self.runtime
    }
}

impl RuntimeOriginFactSyntheticLocalKey {
    pub fn for_stmt_site(site: RuntimeStmtSite) -> Self {
        Self::new(site.export_local_key())
    }

    pub fn for_terminator(block: RBlockId) -> Self {
        Self::new(RuntimeTerminatorSite::new(block).export_local_key())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RuntimeOriginFactNode<'db> {
    Semantic {
        origin: SemanticOrigin<'db>,
        owner_key: RuntimeOriginFactSemanticOwnerKey,
    },
    Synthetic {
        owner_key: RuntimeOriginFactRuntimeOwnerKey,
        local_key: RuntimeOriginFactSyntheticLocalKey,
    },
    Stmt {
        origin: RuntimeStmtOrigin<'db>,
        owner_key: RuntimeOriginFactRuntimeOwnerKey,
    },
    Terminator {
        origin: RuntimeTerminatorOrigin<'db>,
        owner_key: RuntimeOriginFactRuntimeOwnerKey,
    },
}

common::define_origin_graph_type! {
    pub struct RuntimeOriginFactGraph<'db>(RuntimeOriginFactNode<'db>);
}

pub fn runtime_package_origin_fact_graph<'db>(
    origins: &RuntimePackageOrigins<'db>,
    mut owner_keys: impl FnMut(&RuntimePackageBodyOrigins<'db>) -> RuntimeOriginFactOwnerKeys,
) -> RuntimeOriginFactGraph<'db> {
    let mut graph = RuntimeOriginFactGraph::new();

    for body in origins.bodies() {
        let owner_keys = owner_keys(body);
        let runtime_owner_key = owner_keys.runtime().clone();
        let semantic_owner_key = owner_keys.semantic().clone();

        for record in body.origins().stmt_origins() {
            let target = RuntimeOriginFactNode::Stmt {
                origin: record.origin(),
                owner_key: runtime_owner_key.clone(),
            };
            push_runtime_fact_source_link(
                &mut graph,
                record.source(),
                semantic_owner_key.clone(),
                runtime_owner_key.clone(),
                RuntimeOriginFactSyntheticLocalKey::for_stmt_site(record.origin().site()),
                target,
            );
        }

        for record in body.origins().terminator_origins() {
            let target = RuntimeOriginFactNode::Terminator {
                origin: record.origin(),
                owner_key: runtime_owner_key.clone(),
            };
            push_runtime_fact_source_link(
                &mut graph,
                record.source(),
                semantic_owner_key.clone(),
                runtime_owner_key.clone(),
                RuntimeOriginFactSyntheticLocalKey::for_terminator(record.origin().block()),
                target,
            );
        }
    }

    graph
}

pub fn runtime_package_origin_facts<'db>(
    origins: &RuntimePackageOrigins<'db>,
    owner_keys: impl FnMut(&RuntimePackageBodyOrigins<'db>) -> RuntimeOriginFactOwnerKeys,
) -> TypedFactSet {
    let graph = runtime_package_origin_fact_graph(origins, owner_keys);
    origin_graph_facts(graph.as_origin_graph(), runtime_origin_fact_node_export_key)
}

pub fn runtime_origin_fact_node_export_key(node: &RuntimeOriginFactNode<'_>) -> OriginExportKey {
    match node {
        RuntimeOriginFactNode::Semantic { origin, owner_key } => origin.export_key(owner_key),
        RuntimeOriginFactNode::Synthetic {
            owner_key,
            local_key,
        } => OriginExportKey::new(OriginExportKind::RuntimeSynthetic, owner_key, local_key),
        RuntimeOriginFactNode::Stmt { origin, owner_key } => {
            runtime_stmt_export_key(*origin, owner_key)
        }
        RuntimeOriginFactNode::Terminator { origin, owner_key } => {
            runtime_terminator_export_key(*origin, owner_key)
        }
    }
}

fn push_runtime_fact_source_link<'db>(
    graph: &mut RuntimeOriginFactGraph<'db>,
    source: RuntimeOriginSource<'db>,
    semantic_owner_key: RuntimeOriginFactSemanticOwnerKey,
    runtime_owner_key: RuntimeOriginFactRuntimeOwnerKey,
    local_key: RuntimeOriginFactSyntheticLocalKey,
    target: RuntimeOriginFactNode<'db>,
) {
    match source {
        RuntimeOriginSource::Semantic(origin) => graph.push(
            RuntimeOriginFactNode::Semantic {
                origin,
                owner_key: semantic_owner_key,
            },
            target,
            OriginLinkKind::Lowered,
        ),
        RuntimeOriginSource::Synthetic => graph.push(
            RuntimeOriginFactNode::Synthetic {
                owner_key: runtime_owner_key,
                local_key,
            },
            target,
            OriginLinkKind::Synthetic,
        ),
    }
}
