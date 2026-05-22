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

    pub fn export_local_key(self) -> String {
        format!("block:{}:stmt:{}", self.block.index(), self.stmt.index())
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
        local: RBlockId => block
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

    pub fn key(self) -> OriginKey<RuntimeCodeRegion<'db>, ()> {
        self.key
    }
}

pub trait RuntimeOriginOwnerKey: OriginExportOwnerKey {}

pub fn runtime_stmt_export_key<K: RuntimeOriginOwnerKey + ?Sized>(
    origin: RuntimeStmtOrigin<'_>,
    stable_instance_key: &K,
) -> OriginExportKey {
    OriginExportKey::new(
        OriginExportKind::RuntimeStmt,
        stable_instance_key.as_str(),
        origin.site().export_local_key(),
    )
}

pub fn runtime_terminator_export_key<K: RuntimeOriginOwnerKey + ?Sized>(
    origin: RuntimeTerminatorOrigin<'_>,
    stable_instance_key: &K,
) -> OriginExportKey {
    OriginExportKey::new(
        OriginExportKind::RuntimeTerminator,
        stable_instance_key.as_str(),
        runtime_terminator_local_key(origin.block()),
    )
}

pub fn runtime_terminator_local_key(block: RBlockId) -> String {
    format!("block:{}:terminator", block.index())
}

common::define_origin_owner_key! {
    pub struct RuntimeCodeRegionOwnerKey;
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
        stable_region_key.as_str(),
        "region",
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
    pub fn new(bodies: Vec<RuntimePackageBodyOrigins<'db>>) -> Self {
        for (index, body) in bodies.iter().enumerate() {
            assert!(
                !bodies[..index]
                    .iter()
                    .any(|previous| previous.instance() == body.instance()),
                "runtime package origins cannot contain the same runtime instance more than once"
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
    let mut bodies = package
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
    bodies.sort_by(|lhs, rhs| lhs.symbol().cmp(rhs.symbol()));
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
                RuntimeTerminatorOrigin::new(instance, block),
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

common::define_origin_string_key! {
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
        Self::new(runtime_terminator_local_key(block))
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
        } => OriginExportKey::new(
            OriginExportKind::RuntimeSynthetic,
            owner_key.as_str(),
            local_key.as_str(),
        ),
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
mod tests {
    use common::{
        InputDb,
        origin::{OriginExportKey, OriginExportKind, OriginLinkKind},
    };
    use driver::DriverDataBase;
    use hir::{
        analysis::{
            semantic::{get_or_build_semantic_instance, root_semantic_instance_key},
            ty::ty_check::BodyOwner,
        },
        hir_def::{Func, TopLevelMod},
    };
    use url::Url;

    use super::{
        RuntimeBodyOrigins, RuntimeOriginFactNode, RuntimeOriginFactOwnerKeys,
        RuntimeOriginFactRuntimeOwnerKey, RuntimeOriginFactSyntheticLocalKey,
        RuntimeOriginFactTargetKey, RuntimeOriginGraph, RuntimeOriginNode, RuntimeOriginSource,
        RuntimePackageBodyOrigins, RuntimePackageBodySymbol, RuntimePackageOrigins,
        RuntimeStmtIndex, RuntimeStmtOrigin, RuntimeStmtSite, RuntimeTerminatorOrigin,
        runtime_origin_fact_node_export_key, runtime_package_origin_facts, runtime_package_origins,
        runtime_stmt_export_key, runtime_terminator_export_key,
    };
    use crate::{
        RBlockId,
        instance::{RuntimeInstanceKey, RuntimeInstanceSource, get_or_build_runtime_instance},
        runtime::build_runtime_package,
    };

    fn find_func<'db>(db: &'db DriverDataBase, top_mod: TopLevelMod<'db>, name: &str) -> Func<'db> {
        top_mod
            .all_funcs(db)
            .iter()
            .copied()
            .find(|func| {
                func.name(db)
                    .to_opt()
                    .is_some_and(|ident| ident.data(db) == name)
            })
            .unwrap_or_else(|| panic!("missing function `{name}`"))
    }

    fn runtime_instance_for_func<'db>(
        db: &'db DriverDataBase,
        func: Func<'db>,
    ) -> crate::RuntimeInstance<'db> {
        let semantic_key = root_semantic_instance_key(db, BodyOwner::Func(func))
            .expect("fixture function should have a root semantic instance key");
        let semantic = get_or_build_semantic_instance(db, semantic_key);
        let runtime_key =
            RuntimeInstanceKey::new(db, RuntimeInstanceSource::Semantic(semantic), Vec::new());
        get_or_build_runtime_instance(db, runtime_key)
    }

    #[test]
    fn runtime_stmt_site_includes_block_and_statement_index() {
        let first = RuntimeStmtSite::new(RBlockId::from_u32(0), RuntimeStmtIndex::from_u32(0));
        let second = RuntimeStmtSite::new(RBlockId::from_u32(1), RuntimeStmtIndex::from_u32(0));
        let third = RuntimeStmtSite::new(RBlockId::from_u32(0), RuntimeStmtIndex::from_u32(1));

        assert_ne!(first, second);
        assert_ne!(first, third);
        assert_eq!(second.export_local_key(), "block:1:stmt:0");
    }

    #[test]
    fn runtime_stmt_origin_includes_runtime_instance_owner() {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse("file:///origin_runtime_keys.fe").unwrap();
        let file = db.workspace().touch(
            &mut db,
            file_url,
            Some(
                r#"
fn helper_a() -> u256 {
    1
}

fn helper_b() -> u256 {
    2
}

fn test_origin_keys() {
    let a: u256 = helper_a()
    let b: u256 = helper_b()
}
"#
                .to_string(),
            ),
        );
        let top_mod = db.top_mod(file);
        let first_instance = runtime_instance_for_func(&db, find_func(&db, top_mod, "helper_a"));
        let second_instance = runtime_instance_for_func(&db, find_func(&db, top_mod, "helper_b"));

        let site = RuntimeStmtSite::new(RBlockId::from_u32(0), RuntimeStmtIndex::from_u32(0));
        let first = RuntimeStmtOrigin::new(first_instance, site);
        let second = RuntimeStmtOrigin::new(second_instance, site);

        assert_ne!(first, second);
    }

    #[test]
    fn runtime_origin_node_distinguishes_statements_from_terminators() {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse("file:///origin_runtime_node_keys.fe").unwrap();
        let file = db.workspace().touch(
            &mut db,
            file_url,
            Some(
                r#"
fn test_origin_keys() -> u256 {
    1
}
"#
                .to_string(),
            ),
        );
        let top_mod = db.top_mod(file);
        let instance = runtime_instance_for_func(&db, find_func(&db, top_mod, "test_origin_keys"));
        let block = RBlockId::from_u32(0);
        let stmt_site = RuntimeStmtSite::new(block, RuntimeStmtIndex::from_u32(0));

        let stmt = RuntimeOriginNode::Stmt(RuntimeStmtOrigin::new(instance, stmt_site));
        let terminator =
            RuntimeOriginNode::Terminator(RuntimeTerminatorOrigin::new(instance, block));

        assert_ne!(stmt, terminator);
    }

    #[test]
    #[should_panic(expected = "runtime statement origin recorded more than once")]
    fn runtime_body_origins_reject_duplicate_statement_sites() {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse("file:///duplicate_runtime_stmt_origin.fe").unwrap();
        let file = db.workspace().touch(
            &mut db,
            file_url,
            Some(
                r#"
fn test_origin_keys() -> u256 {
    1
}
"#
                .to_string(),
            ),
        );
        let top_mod = db.top_mod(file);
        let instance = runtime_instance_for_func(&db, find_func(&db, top_mod, "test_origin_keys"));
        let site = RuntimeStmtSite::new(RBlockId::from_u32(0), RuntimeStmtIndex::from_u32(0));
        let origin = RuntimeStmtOrigin::new(instance, site);
        let mut origins = RuntimeBodyOrigins::new();

        origins.push_stmt(origin, RuntimeOriginSource::Synthetic);
        origins.push_stmt(origin, RuntimeOriginSource::Synthetic);
    }

    #[test]
    #[should_panic(expected = "runtime terminator origin recorded more than once")]
    fn runtime_body_origins_reject_duplicate_terminator_blocks() {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse("file:///duplicate_runtime_terminator_origin.fe").unwrap();
        let file = db.workspace().touch(
            &mut db,
            file_url,
            Some(
                r#"
fn test_origin_keys() -> u256 {
    1
}
"#
                .to_string(),
            ),
        );
        let top_mod = db.top_mod(file);
        let instance = runtime_instance_for_func(&db, find_func(&db, top_mod, "test_origin_keys"));
        let origin = RuntimeTerminatorOrigin::new(instance, RBlockId::from_u32(0));
        let mut origins = RuntimeBodyOrigins::new();

        origins.push_terminator(origin, RuntimeOriginSource::Synthetic);
        origins.push_terminator(origin, RuntimeOriginSource::Synthetic);
    }

    #[test]
    #[should_panic(
        expected = "runtime package origins cannot contain the same runtime instance more than once"
    )]
    fn runtime_package_origins_reject_duplicate_instances() {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse("file:///duplicate_runtime_package_origin.fe").unwrap();
        let file = db.workspace().touch(
            &mut db,
            file_url,
            Some(
                r#"
fn test_origin_keys() -> u256 {
    1
}
"#
                .to_string(),
            ),
        );
        let top_mod = db.top_mod(file);
        let instance = runtime_instance_for_func(&db, find_func(&db, top_mod, "test_origin_keys"));

        RuntimePackageOrigins::new(vec![
            RuntimePackageBodyOrigins::new(
                RuntimePackageBodySymbol::new("first"),
                instance,
                RuntimeBodyOrigins::new(),
            ),
            RuntimePackageBodyOrigins::new(
                RuntimePackageBodySymbol::new("second"),
                instance,
                RuntimeBodyOrigins::new(),
            ),
        ]);
    }

    #[test]
    #[should_panic(expected = "origin string key must not be empty")]
    fn runtime_package_body_symbols_reject_empty_strings() {
        let _ = RuntimePackageBodySymbol::new("");
    }

    #[test]
    fn runtime_origin_export_keys_include_kind_owner_and_local_identity() {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse("file:///origin_runtime_export_keys.fe").unwrap();
        let file = db.workspace().touch(
            &mut db,
            file_url,
            Some(
                r#"
fn test_origin_keys() -> u256 {
    1
}
"#
                .to_string(),
            ),
        );
        let top_mod = db.top_mod(file);
        let instance = runtime_instance_for_func(&db, find_func(&db, top_mod, "test_origin_keys"));
        let block = RBlockId::from_u32(3);
        let site = RuntimeStmtSite::new(block, RuntimeStmtIndex::from_u32(5));
        let owner_key = RuntimeOriginFactRuntimeOwnerKey::new("runtime:test");

        let stmt_key = runtime_stmt_export_key(RuntimeStmtOrigin::new(instance, site), &owner_key);
        let terminator_key = runtime_terminator_export_key(
            RuntimeTerminatorOrigin::new(instance, block),
            &owner_key,
        );

        assert_ne!(stmt_key, terminator_key);
        assert_eq!(
            stmt_key,
            OriginExportKey::new(
                OriginExportKind::RuntimeStmt,
                "runtime:test",
                "block:3:stmt:5"
            )
        );
        assert_eq!(
            terminator_key,
            OriginExportKey::new(
                OriginExportKind::RuntimeTerminator,
                "runtime:test",
                "block:3:terminator"
            )
        );
    }

    #[test]
    fn runtime_synthetic_fact_export_uses_typed_runtime_owner_and_local_keys() {
        let node = RuntimeOriginFactNode::Synthetic {
            owner_key: RuntimeOriginFactRuntimeOwnerKey::new("runtime:test"),
            local_key: RuntimeOriginFactSyntheticLocalKey::new("block:0:stmt:0"),
        };

        assert_eq!(
            runtime_origin_fact_node_export_key(&node),
            OriginExportKey::new(
                OriginExportKind::RuntimeSynthetic,
                "runtime:test",
                "block:0:stmt:0"
            )
        );
    }

    #[test]
    fn runtime_origin_fact_owner_keys_are_derived_from_typed_target_and_body_symbol() {
        let target = RuntimeOriginFactTargetKey::new("contract:Foo");
        let symbol = RuntimePackageBodySymbol::new("runtime_main");
        let keys = RuntimeOriginFactOwnerKeys::for_body(&target, &symbol);

        assert_eq!(
            keys.semantic().as_str(),
            "target:contract:Foo:semantic:runtime_main"
        );
        assert_eq!(
            keys.runtime().as_str(),
            "target:contract:Foo:runtime:runtime_main"
        );
    }

    #[test]
    fn runtime_body_origins_are_cached_complete_and_typed() {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse("file:///runtime_body_origins.fe").unwrap();
        let file = db.workspace().touch(
            &mut db,
            file_url,
            Some(
                r#"
fn test_origin_keys() -> u256 {
    let x: u256 = 1
    x
}
"#
                .to_string(),
            ),
        );
        let top_mod = db.top_mod(file);
        let instance = runtime_instance_for_func(&db, find_func(&db, top_mod, "test_origin_keys"));
        let body = instance.body(&db);

        let first = instance.origins(&db);
        let second = instance.origins(&db);

        assert_eq!(first, second);
        assert!(first.is_complete_for_body(&body));
        assert!(
            first
                .stmt_origins()
                .iter()
                .any(|record| matches!(record.source(), RuntimeOriginSource::Semantic(_)))
        );
        assert!(
            first
                .terminator_origin(RBlockId::from_u32(0))
                .is_some_and(|record| matches!(record.source(), RuntimeOriginSource::Semantic(_)))
        );

        let synthetic = RuntimeBodyOrigins::synthetic_for_body(instance, &body);
        assert!(synthetic.is_complete_for_body(&body));
        assert!(
            synthetic
                .stmt_origins()
                .iter()
                .all(|record| matches!(record.source(), RuntimeOriginSource::Synthetic))
        );
        assert!(
            synthetic
                .terminator_origins()
                .iter()
                .all(|record| matches!(record.source(), RuntimeOriginSource::Synthetic))
        );
    }

    #[test]
    fn runtime_package_origins_are_cached_and_deterministically_ordered() {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse("file:///runtime_package_origins.fe").unwrap();
        let file = db.workspace().touch(
            &mut db,
            file_url,
            Some(
                r#"
fn helper() -> u256 {
    1
}

fn main() {
    let x: u256 = helper()
}
"#
                .to_string(),
            ),
        );
        let top_mod = db.top_mod(file);
        let package = build_runtime_package(&db, top_mod).expect("package should build");

        let first = runtime_package_origins(&db, package);
        let second = runtime_package_origins(&db, package);

        assert_eq!(first, second);
        assert!(!first.bodies().is_empty());
        assert!(
            first
                .bodies()
                .windows(2)
                .all(|window| window[0].symbol() <= window[1].symbol())
        );
        assert!(first.bodies().iter().all(|body| {
            body.origins()
                .is_complete_for_body(&body.instance().body(&db))
        }));
    }

    #[test]
    fn runtime_origin_graph_uses_typed_runtime_nodes() {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse("file:///origin_runtime_graph_keys.fe").unwrap();
        let file = db.workspace().touch(
            &mut db,
            file_url,
            Some(
                r#"
fn test_origin_keys() -> u256 {
    1
}
"#
                .to_string(),
            ),
        );
        let top_mod = db.top_mod(file);
        let instance = runtime_instance_for_func(&db, find_func(&db, top_mod, "test_origin_keys"));
        let block = RBlockId::from_u32(0);
        let stmt_site = RuntimeStmtSite::new(block, RuntimeStmtIndex::from_u32(0));
        let stmt = RuntimeOriginNode::Stmt(RuntimeStmtOrigin::new(instance, stmt_site));
        let terminator =
            RuntimeOriginNode::Terminator(RuntimeTerminatorOrigin::new(instance, block));
        let mut graph = RuntimeOriginGraph::new();

        graph.push(stmt, terminator, OriginLinkKind::Lowered);

        let link = graph
            .links()
            .first()
            .expect("origin graph should have a link");

        assert_eq!(link.from(), &stmt);
        assert_eq!(link.to(), &terminator);
        assert_eq!(link.kind(), OriginLinkKind::Lowered);
    }

    #[test]
    fn runtime_package_origin_facts_export_semantic_to_runtime_links() {
        let mut db = DriverDataBase::default();
        let file_url = Url::parse("file:///runtime_package_origin_facts.fe").unwrap();
        let file = db.workspace().touch(
            &mut db,
            file_url,
            Some(
                r#"
fn test_origin_facts() -> u256 {
    let x: u256 = 1
    x
}
"#
                .to_string(),
            ),
        );
        let top_mod = db.top_mod(file);
        let package = build_runtime_package(&db, top_mod).expect("package should build");
        let origins = runtime_package_origins(&db, package);

        let facts = runtime_package_origin_facts(&origins, |body| {
            RuntimeOriginFactOwnerKeys::for_body(
                &RuntimeOriginFactTargetKey::new("runtime_package_origin_facts"),
                body.symbol_key(),
            )
        });

        assert!(facts.origin_nodes().any(|node| {
            node.key().kind() == OriginExportKind::Semantic
                && node
                    .key()
                    .owner_key()
                    .starts_with("target:runtime_package_origin_facts:semantic:")
        }));
        assert!(facts.origin_nodes().any(|node| {
            matches!(
                node.key().kind(),
                OriginExportKind::RuntimeStmt | OriginExportKind::RuntimeTerminator
            ) && node
                .key()
                .owner_key()
                .starts_with("target:runtime_package_origin_facts:runtime:")
        }));
        assert!(
            facts
                .origin_links()
                .any(|link| link.kind() == OriginLinkKind::Lowered)
        );
    }
}
