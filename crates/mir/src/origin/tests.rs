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
    RuntimePackageBodyOrigins, RuntimePackageBodySymbol, RuntimePackageOrigins, RuntimeStmtIndex,
    RuntimeStmtOrigin, RuntimeStmtSite, RuntimeTerminatorOrigin, RuntimeTerminatorSite,
    runtime_origin_fact_node_export_key, runtime_package_origin_facts, runtime_package_origins,
    runtime_stmt_export_key, runtime_terminator_export_key,
};
use crate::{
    RBlockId,
    instance::{RuntimeInstanceKey, RuntimeInstanceSource, get_or_build_runtime_instance},
    runtime::build_runtime_package,
};

fn origin_key(kind: OriginExportKind, owner: &str, local: &str) -> OriginExportKey {
    OriginExportKey::try_from_raw_parts(kind, owner, local).unwrap()
}

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
fn runtime_terminator_site_is_a_typed_local_key() {
    let first = RuntimeTerminatorSite::new(RBlockId::from_u32(0));
    let second = RuntimeTerminatorSite::new(RBlockId::from_u32(1));

    assert_ne!(first, second);
    assert_eq!(second.block(), RBlockId::from_u32(1));
    assert_eq!(second.export_local_key(), "block:1:terminator");
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
        RuntimeOriginNode::Terminator(RuntimeTerminatorOrigin::for_block(instance, block));

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
    let origin = RuntimeTerminatorOrigin::for_block(instance, RBlockId::from_u32(0));
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
#[should_panic(
    expected = "runtime package origins cannot contain the same runtime body symbol more than once"
)]
fn runtime_package_origins_reject_duplicate_body_symbols() {
    let mut db = DriverDataBase::default();
    let file_url = Url::parse("file:///duplicate_runtime_package_symbol.fe").unwrap();
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
"#
            .to_string(),
        ),
    );
    let top_mod = db.top_mod(file);
    let first_instance = runtime_instance_for_func(&db, find_func(&db, top_mod, "helper_a"));
    let second_instance = runtime_instance_for_func(&db, find_func(&db, top_mod, "helper_b"));

    RuntimePackageOrigins::new(vec![
        RuntimePackageBodyOrigins::new(
            RuntimePackageBodySymbol::new("same"),
            first_instance,
            RuntimeBodyOrigins::new(),
        ),
        RuntimePackageBodyOrigins::new(
            RuntimePackageBodySymbol::new("same"),
            second_instance,
            RuntimeBodyOrigins::new(),
        ),
    ]);
}

#[test]
fn runtime_package_origins_constructor_orders_bodies_by_symbol() {
    let mut db = DriverDataBase::default();
    let file_url = Url::parse("file:///runtime_package_origin_constructor_order.fe").unwrap();
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
"#
            .to_string(),
        ),
    );
    let top_mod = db.top_mod(file);
    let first_instance = runtime_instance_for_func(&db, find_func(&db, top_mod, "helper_a"));
    let second_instance = runtime_instance_for_func(&db, find_func(&db, top_mod, "helper_b"));

    let origins = RuntimePackageOrigins::new(vec![
        RuntimePackageBodyOrigins::new(
            RuntimePackageBodySymbol::new("z_second"),
            first_instance,
            RuntimeBodyOrigins::new(),
        ),
        RuntimePackageBodyOrigins::new(
            RuntimePackageBodySymbol::new("a_first"),
            second_instance,
            RuntimeBodyOrigins::new(),
        ),
    ]);

    assert_eq!(
        origins
            .bodies()
            .iter()
            .map(RuntimePackageBodyOrigins::symbol)
            .collect::<Vec<_>>(),
        vec!["a_first", "z_second"]
    );
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
        RuntimeTerminatorOrigin::for_block(instance, block),
        &owner_key,
    );

    assert_ne!(stmt_key, terminator_key);
    assert_eq!(
        stmt_key,
        origin_key(
            OriginExportKind::RuntimeStmt,
            "runtime:test",
            "block:3:stmt:5"
        )
    );
    assert_eq!(
        terminator_key,
        origin_key(
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
        origin_key(
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
        RuntimeOriginNode::Terminator(RuntimeTerminatorOrigin::for_block(instance, block));
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
