mod fact_graph;
mod package;
mod runtime_identity;

pub use fact_graph::{
    RuntimeOriginFactGraph, RuntimeOriginFactNode, RuntimeOriginFactOwnerKeys,
    RuntimeOriginFactRuntimeOwnerKey, RuntimeOriginFactSemanticOwnerKey,
    RuntimeOriginFactSyntheticLocalKey, RuntimeOriginFactTargetKey,
    runtime_origin_fact_node_export_key, runtime_package_origin_fact_graph,
    runtime_package_origin_facts,
};
pub use package::{
    RuntimeBodyOrigins, RuntimeOriginSource, RuntimePackageBodyOrigins, RuntimePackageOrigins,
    RuntimeStmtOriginRecord, RuntimeTerminatorOriginRecord, runtime_package_origins,
};
pub use runtime_identity::{
    RuntimeCodeRegionLocalKey, RuntimeCodeRegionOrigin, RuntimeCodeRegionOwnerKey,
    RuntimeOriginOwnerKey, RuntimePackageBodySymbol, RuntimeStmtIndex, RuntimeStmtOrigin,
    RuntimeStmtSite, RuntimeTerminatorOrigin, RuntimeTerminatorSite,
    runtime_code_region_export_key, runtime_stmt_export_key, runtime_terminator_export_key,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum RuntimeOriginNode<'db> {
    Stmt(RuntimeStmtOrigin<'db>),
    Terminator(RuntimeTerminatorOrigin<'db>),
    CodeRegion(RuntimeCodeRegionOrigin<'db>),
}

common::define_origin_graph_type! {
    pub struct RuntimeOriginGraph<'db>(RuntimeOriginNode<'db>);
}

#[cfg(test)]
mod tests;
