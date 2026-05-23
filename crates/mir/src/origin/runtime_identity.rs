use common::origin::{OriginExportKey, OriginExportKind, OriginExportOwnerKey, OriginKey};
use cranelift_entity::EntityRef;
use salsa::Update;

use crate::{
    instance::RuntimeInstance,
    runtime::{RBlockId, RuntimeCodeRegion},
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

    pub(super) fn export_local_key(self) -> String {
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

    pub(super) fn export_local_key(self) -> String {
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
