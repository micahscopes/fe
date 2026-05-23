use common::{
    facts::{TypedFactSet, origin_graph_facts},
    origin::{OriginExportKey, OriginExportKind, OriginExportOwnerKey, OriginKey, OriginLinkKind},
};
use cranelift_entity::EntityRef;
use hir::origin::SemanticOrigin;
use salsa::Update;

use crate::{
    db::MirDb,
    instance::RuntimeInstance,
    runtime::{RBlockId, RuntimeBody, RuntimeCodeRegion, RuntimePackage},
};

/// Index of a statement inside one MIR runtime block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Update)]
pub struct RuntimeStmtIndex(u32);

impl RuntimeStmtIndex {
    pub const fn from_u32(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Block-local MIR statement site.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Update)]
pub struct RuntimeStmtSite {
    block: RBlockId,
    stmt: RuntimeStmtIndex,
}

impl RuntimeStmtSite {
    pub const fn new(block: RBlockId, stmt: RuntimeStmtIndex) -> Self {
        Self { block, stmt }
    }

    pub const fn block(self) -> RBlockId {
        self.block
    }

    pub const fn stmt(self) -> RuntimeStmtIndex {
        self.stmt
    }

    fn export_local_key(self) -> String {
        format!("block:{}:stmt:{}", self.block.index(), self.stmt.index())
    }
}

impl common::origin::OriginExportLocalKey for RuntimeStmtSite {
    fn to_export_local_key(&self) -> String {
        (*self).export_local_key()
    }
}

/// Block-local MIR terminator site.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Update)]
pub struct RuntimeTerminatorSite {
    block: RBlockId,
}

impl RuntimeTerminatorSite {
    pub const fn new(block: RBlockId) -> Self {
        Self { block }
    }

    pub const fn block(self) -> RBlockId {
        self.block
    }

    fn export_local_key(self) -> String {
        format!("block:{}:terminator", self.block.index())
    }
}

impl common::origin::OriginExportLocalKey for RuntimeTerminatorSite {
    fn to_export_local_key(&self) -> String {
        (*self).export_local_key()
    }
}

common::define_origin_key_type! {
    /// Origin key for a MIR runtime statement. The statement site is only
    /// meaningful inside its owning runtime instance.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
    pub struct RuntimeStmtOrigin<'db> {
        owner: RuntimeInstance<'db> => instance,
        local: RuntimeStmtSite => site
    }
}

common::define_origin_key_type! {
    /// Origin key for a MIR runtime terminator. Terminators are block-local and
    /// only meaningful inside their owning runtime instance.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
    pub struct RuntimeTerminatorOrigin<'db> {
        owner: RuntimeInstance<'db> => instance,
        local: RuntimeTerminatorSite => site
    }
}

impl<'db> RuntimeTerminatorOrigin<'db> {
    pub const fn for_block(instance: RuntimeInstance<'db>, block: RBlockId) -> Self {
        Self::new(instance, RuntimeTerminatorSite::new(block))
    }

    pub fn block(self) -> RBlockId {
        self.site().block()
    }
}

/// Origin key for a runtime code region. This gives bytecode/debug exporters an
/// owner-aware handle before PC ranges are introduced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub struct RuntimeCodeRegionOrigin<'db> {
    key: OriginKey<RuntimeCodeRegion<'db>, ()>,
}

impl<'db> RuntimeCodeRegionOrigin<'db> {
    pub const fn new(region: RuntimeCodeRegion<'db>) -> Self {
        Self {
            key: OriginKey::new(region, ()),
        }
    }

    pub fn region(self) -> RuntimeCodeRegion<'db> {
        self.key.into_parts().0
    }
}

pub trait RuntimeOriginOwnerKey: OriginExportOwnerKey {}

pub fn runtime_stmt_export_key<K: RuntimeOriginOwnerKey + ?Sized>(
    origin: RuntimeStmtOrigin<'_>,
    stable_instance_key: &K,
) -> OriginExportKey {
    OriginExportKey::new(
        OriginExportKind::RuntimeStmt,
        stable_instance_key,
        &origin.site(),
    )
}

pub fn runtime_terminator_export_key<K: RuntimeOriginOwnerKey + ?Sized>(
    origin: RuntimeTerminatorOrigin<'_>,
    stable_instance_key: &K,
) -> OriginExportKey {
    OriginExportKey::new(
        OriginExportKind::RuntimeTerminator,
        stable_instance_key,
        &origin.site(),
    )
}

common::define_origin_owner_key! {
    pub struct RuntimeCodeRegionOwnerKey;
}

common::define_origin_local_key! {
    pub struct RuntimeCodeRegionLocalKey;
}

common::define_origin_string_key! {
    /// Stable runtime function symbol used to label runtime package origin bodies.
    pub struct RuntimePackageBodySymbol;
}

pub fn runtime_code_region_export_key(
    origin: RuntimeCodeRegionOrigin<'_>,
    stable_region_key: &RuntimeCodeRegionOwnerKey,
) -> OriginExportKey {
    let _ = origin.region();
    OriginExportKey::new(
        OriginExportKind::RuntimeCodeRegion,
        stable_region_key,
        &RuntimeCodeRegionLocalKey::new("region"),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeOriginSource<'db> {
    Semantic(SemanticOrigin<'db>),
    Synthetic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub struct RuntimeStmtOriginRecord<'db> {
    origin: RuntimeStmtOrigin<'db>,
    source: RuntimeOriginSource<'db>,
}

impl<'db> RuntimeStmtOriginRecord<'db> {
    pub const fn new(origin: RuntimeStmtOrigin<'db>, source: RuntimeOriginSource<'db>) -> Self {
        Self { origin, source }
    }

    pub const fn origin(self) -> RuntimeStmtOrigin<'db> {
        self.origin
    }

    pub const fn source(self) -> RuntimeOriginSource<'db> {
        self.source
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub struct RuntimeTerminatorOriginRecord<'db> {
    origin: RuntimeTerminatorOrigin<'db>,
    source: RuntimeOriginSource<'db>,
}

impl<'db> RuntimeTerminatorOriginRecord<'db> {
    pub const fn new(
        origin: RuntimeTerminatorOrigin<'db>,
        source: RuntimeOriginSource<'db>,
    ) -> Self {
        Self { origin, source }
    }

    pub const fn origin(self) -> RuntimeTerminatorOrigin<'db> {
        self.origin
    }

    pub const fn source(self) -> RuntimeOriginSource<'db> {
        self.source
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct RuntimeBodyOrigins<'db> {
    stmt_origins: Vec<RuntimeStmtOriginRecord<'db>>,
    terminator_origins: Vec<RuntimeTerminatorOriginRecord<'db>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct RuntimePackageBodyOrigins<'db> {
    symbol: RuntimePackageBodySymbol,
    instance: RuntimeInstance<'db>,
    origins: RuntimeBodyOrigins<'db>,
}

impl<'db> RuntimePackageBodyOrigins<'db> {
    pub fn new(
        symbol: RuntimePackageBodySymbol,
        instance: RuntimeInstance<'db>,
        origins: RuntimeBodyOrigins<'db>,
    ) -> Self {
        Self {
            symbol,
            instance,
            origins,
        }
    }

    pub fn symbol(&self) -> &str {
        self.symbol.as_str()
    }

    pub fn symbol_key(&self) -> &RuntimePackageBodySymbol {
        &self.symbol
    }

    pub fn instance(&self) -> RuntimeInstance<'db> {
        self.instance
    }

    pub fn origins(&self) -> &RuntimeBodyOrigins<'db> {
        &self.origins
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct RuntimePackageOrigins<'db> {
    bodies: Vec<RuntimePackageBodyOrigins<'db>>,
}

impl<'db> RuntimePackageOrigins<'db> {
    pub fn new(mut bodies: Vec<RuntimePackageBodyOrigins<'db>>) -> Self {
        bodies.sort_by(|lhs, rhs| lhs.symbol_key().cmp(rhs.symbol_key()));
        for (index, body) in bodies.iter().enumerate() {
            assert!(
                !bodies[..index]
                    .iter()
                    .any(|previous| previous.instance() == body.instance()),
                "runtime package origins cannot contain the same runtime instance more than once"
            );
            assert!(
                !bodies[..index]
                    .iter()
                    .any(|previous| previous.symbol_key() == body.symbol_key()),
                "runtime package origins cannot contain the same runtime body symbol more than once"
            );
        }
        Self { bodies }
    }

    pub fn bodies(&self) -> &[RuntimePackageBodyOrigins<'db>] {
        &self.bodies
    }

    pub fn body_for_instance(
        &self,
        instance: RuntimeInstance<'db>,
    ) -> Option<&RuntimePackageBodyOrigins<'db>> {
        self.bodies.iter().find(|body| body.instance() == instance)
    }

    pub fn stmt_origin(
        &self,
        origin: RuntimeStmtOrigin<'db>,
    ) -> Option<RuntimeStmtOriginRecord<'db>> {
        self.bodies
            .iter()
            .find(|body| body.instance() == origin.instance())
            .and_then(|body| {
                body.origins()
                    .stmt_origin(origin.site().block(), origin.site().stmt())
            })
            .filter(|record| record.origin() == origin)
    }

    pub fn terminator_origin(
        &self,
        origin: RuntimeTerminatorOrigin<'db>,
    ) -> Option<RuntimeTerminatorOriginRecord<'db>> {
        self.bodies
            .iter()
            .find(|body| body.instance() == origin.instance())
            .and_then(|body| body.origins().terminator_origin(origin.block()))
            .filter(|record| record.origin() == origin)
    }
}

#[salsa::tracked]
pub fn runtime_package_origins<'db>(
    db: &'db dyn MirDb,
    package: RuntimePackage<'db>,
) -> RuntimePackageOrigins<'db> {
    let bodies = package
        .functions(db)
        .iter()
        .map(|function| {
            let instance = function.instance(db);
            RuntimePackageBodyOrigins::new(
                RuntimePackageBodySymbol::new(function.symbol(db)),
                instance,
                instance.origins(db),
            )
        })
        .collect::<Vec<_>>();
    RuntimePackageOrigins::new(bodies)
}

impl<'db> Default for RuntimeBodyOrigins<'db> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'db> RuntimeBodyOrigins<'db> {
    pub const fn new() -> Self {
        Self {
            stmt_origins: Vec::new(),
            terminator_origins: Vec::new(),
        }
    }

    pub fn synthetic_for_body(instance: RuntimeInstance<'db>, body: &RuntimeBody<'db>) -> Self {
        let mut origins = Self::new();
        for (block_idx, runtime_block) in body.blocks.iter().enumerate() {
            let block = RBlockId::from_u32(block_idx as u32);
            for stmt_idx in 0..runtime_block.stmts.len() {
                origins.push_stmt(
                    RuntimeStmtOrigin::new(
                        instance,
                        RuntimeStmtSite::new(block, RuntimeStmtIndex::from_u32(stmt_idx as u32)),
                    ),
                    RuntimeOriginSource::Synthetic,
                );
            }
            origins.push_terminator(
                RuntimeTerminatorOrigin::for_block(instance, block),
                RuntimeOriginSource::Synthetic,
            );
        }
        origins
    }

    pub fn push_stmt(&mut self, origin: RuntimeStmtOrigin<'db>, source: RuntimeOriginSource<'db>) {
        assert!(
            self.stmt_origin(origin.site().block(), origin.site().stmt())
                .is_none(),
            "runtime statement origin recorded more than once"
        );
        self.stmt_origins
            .push(RuntimeStmtOriginRecord::new(origin, source));
    }

    pub fn push_terminator(
        &mut self,
        origin: RuntimeTerminatorOrigin<'db>,
        source: RuntimeOriginSource<'db>,
    ) {
        assert!(
            self.terminator_origin(origin.block()).is_none(),
            "runtime terminator origin recorded more than once"
        );
        self.terminator_origins
            .push(RuntimeTerminatorOriginRecord::new(origin, source));
    }

    pub fn stmt_origins(&self) -> &[RuntimeStmtOriginRecord<'db>] {
        &self.stmt_origins
    }

    pub fn terminator_origins(&self) -> &[RuntimeTerminatorOriginRecord<'db>] {
        &self.terminator_origins
    }

    pub fn stmt_origin(
        &self,
        block: RBlockId,
        stmt: RuntimeStmtIndex,
    ) -> Option<RuntimeStmtOriginRecord<'db>> {
        self.stmt_origins
            .iter()
            .copied()
            .find(|record| record.origin().site() == RuntimeStmtSite::new(block, stmt))
    }

    pub fn terminator_origin(&self, block: RBlockId) -> Option<RuntimeTerminatorOriginRecord<'db>> {
        self.terminator_origins
            .iter()
            .copied()
            .find(|record| record.origin().block() == block)
    }

    pub fn is_complete_for_body(&self, body: &RuntimeBody<'db>) -> bool {
        let stmt_count: usize = body.blocks.iter().map(|block| block.stmts.len()).sum();
        if self.stmt_origins.len() != stmt_count
            || self.terminator_origins.len() != body.blocks.len()
        {
            return false;
        }

        for (block_idx, block) in body.blocks.iter().enumerate() {
            let block_id = RBlockId::from_u32(block_idx as u32);
            for stmt_idx in 0..block.stmts.len() {
                if self
                    .stmt_origin(block_id, RuntimeStmtIndex::from_u32(stmt_idx as u32))
                    .is_none()
                {
                    return false;
                }
            }
            if self.terminator_origin(block_id).is_none() {
                return false;
            }
        }
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeOriginNode<'db> {
    Stmt(RuntimeStmtOrigin<'db>),
    Terminator(RuntimeTerminatorOrigin<'db>),
    CodeRegion(RuntimeCodeRegionOrigin<'db>),
}

common::define_origin_graph_type! {
    pub struct RuntimeOriginGraph<'db>(RuntimeOriginNode<'db>);
}

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

#[cfg(test)]
mod tests;
