use hir::origin::SemanticOrigin;
use salsa::Update;

use crate::{
    db::MirDb,
    instance::RuntimeInstance,
    origin::{
        RuntimePackageBodySymbol, RuntimeStmtIndex, RuntimeStmtOrigin, RuntimeStmtSite,
        RuntimeTerminatorOrigin,
    },
    runtime::{RBlockId, RuntimeBody, RuntimePackage},
};

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
