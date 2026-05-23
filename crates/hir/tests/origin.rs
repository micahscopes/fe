use common::origin::{OriginExportKey, OriginExportKind, OriginLinkKind};
use cranelift_entity::EntityRef;
use fe_hir::{
    analysis::{
        semantic::{SemOrigin, identity_semantic_instance_key},
        ty::{ty_check::BodyOwner, ty_def::TyId},
    },
    hir_def::{Body, ExprId, Func, StmtId, TopLevelMod},
    origin::{
        HirExprOrigin, HirOriginBodyOwnerKey, HirOriginGraph, HirOriginNode, HirStmtOrigin,
        SemanticOrigin, SemanticOriginInstanceOwnerKey,
    },
    span::LazySpan,
    test_db::HirAnalysisTestDb,
};

fn origin_key(kind: OriginExportKind, owner: &str, local: impl Into<String>) -> OriginExportKey {
    OriginExportKey::try_from_raw_parts(kind, owner, local.into()).unwrap()
}

fn find_func<'db>(db: &'db HirAnalysisTestDb, top_mod: TopLevelMod<'db>, name: &str) -> Func<'db> {
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

fn body_for<'db>(db: &'db HirAnalysisTestDb, top_mod: TopLevelMod<'db>, name: &str) -> Body<'db> {
    find_func(db, top_mod, name)
        .body(db)
        .unwrap_or_else(|| panic!("function `{name}` should have a body"))
}

fn first_expr<'db>(db: &'db HirAnalysisTestDb, body: Body<'db>) -> ExprId {
    body.exprs(db)
        .keys()
        .next()
        .expect("body should contain an expression")
}

#[test]
fn hir_expr_origin_includes_body_owner() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "origin_keys.fe".into(),
        r#"
fn a() -> u256 {
    let x: u256 = 1
    x
}

fn b() -> u256 {
    let x: u256 = 2
    x
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
    let a_body = body_for(&db, top_mod, "a");
    let b_body = body_for(&db, top_mod, "b");
    let same_local_expr = ExprId::from_u32(0);

    let first = HirExprOrigin::new(a_body, same_local_expr);
    let second = HirExprOrigin::new(b_body, same_local_expr);

    assert_ne!(first, second);
}

#[test]
fn hir_origin_node_distinguishes_exprs_from_stmts() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "origin_keys.fe".into(),
        r#"
fn a() -> u256 {
    let x: u256 = 1
    x
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
    let body = body_for(&db, top_mod, "a");
    let same_local = body
        .stmts(&db)
        .keys()
        .next()
        .expect("body should contain a statement")
        .index() as u32;

    let expr = HirOriginNode::Expr(HirExprOrigin::new(body, ExprId::from_u32(same_local)));
    let stmt = HirOriginNode::Stmt(HirStmtOrigin::new(body, StmtId::from_u32(same_local)));

    assert_ne!(expr, stmt);
}

#[test]
fn hir_origin_graph_uses_typed_hir_nodes() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "origin_graph.fe".into(),
        r#"
fn a() -> u256 {
    let x: u256 = 1
    x
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
    let body = body_for(&db, top_mod, "a");
    let stmt_id = body
        .stmts(&db)
        .keys()
        .next()
        .expect("body should contain a statement");

    let stmt = HirOriginNode::Stmt(HirStmtOrigin::new(body, stmt_id));
    let expr = HirOriginNode::Expr(HirExprOrigin::new(body, ExprId::from_u32(0)));
    let mut graph = HirOriginGraph::new();
    graph.push(expr, stmt, OriginLinkKind::Expanded);

    let link = graph
        .links()
        .first()
        .expect("origin graph should have a link");

    assert_eq!(link.from(), &expr);
    assert_eq!(link.to(), &stmt);
    assert_eq!(link.kind(), OriginLinkKind::Expanded);
}

#[test]
fn hir_origin_export_keys_include_kind_owner_and_local_identity() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "origin_export_keys.fe".into(),
        r#"
fn a() -> u256 {
    let x: u256 = 1
    x
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
    let body = body_for(&db, top_mod, "a");
    let local = 7;
    let owner_key = HirOriginBodyOwnerKey::new("body:a");

    let expr_key = HirExprOrigin::new(body, ExprId::from_u32(local)).export_key(&owner_key);
    let stmt_key = HirStmtOrigin::new(body, StmtId::from_u32(local)).export_key(&owner_key);

    assert_ne!(expr_key, stmt_key);
    assert_eq!(
        expr_key,
        origin_key(OriginExportKind::HirExpr, "body:a", "7")
    );
    assert_eq!(
        stmt_key,
        origin_key(OriginExportKind::HirStmt, "body:a", "7")
    );
}

#[test]
fn hir_origin_source_helpers_delegate_to_lazy_span() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "origin_source_span.fe".into(),
        r#"
fn a() -> u256 {
    let x: u256 = 1
    x
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
    let body = body_for(&db, top_mod, "a");
    let expr = first_expr(&db, body);
    let stmt = body
        .stmts(&db)
        .keys()
        .next()
        .expect("body should contain a statement");
    let expr_origin = HirExprOrigin::new(body, expr);
    let stmt_origin = HirStmtOrigin::new(body, stmt);

    assert_eq!(
        expr_origin.resolve_source_span(&db),
        expr.span(body).resolve(&db)
    );
    assert_eq!(
        stmt_origin.resolve_source_span(&db),
        stmt.span(body).resolve(&db)
    );
}

#[test]
fn semantic_origin_includes_semantic_instance_owner() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "semantic_origin_keys.fe".into(),
        r#"
fn generic<T>(value: T) -> T {
    value
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
    let generic = find_func(&db, top_mod, "generic");
    let owner = BodyOwner::Func(generic);
    let body = body_for(&db, top_mod, "generic");
    let local_expr = body
        .exprs(&db)
        .keys()
        .next()
        .expect("body should contain an expression");
    let first_key = identity_semantic_instance_key(&db, owner);
    let second_key = fe_hir::analysis::semantic::SemanticInstanceKey::new(
        &db,
        owner,
        fe_hir::analysis::semantic::GenericSubst::new(&db, vec![TyId::u256(&db)]),
        fe_hir::analysis::semantic::EffectProviderSubst::empty(&db),
        fe_hir::analysis::semantic::ImplEnv::empty(&db, owner.scope()),
    );

    let first = SemanticOrigin::new(first_key, SemOrigin::Expr(local_expr));
    let second = SemanticOrigin::new(second_key, SemOrigin::Expr(local_expr));
    let owner_key = SemanticOriginInstanceOwnerKey::new("semantic:generic:u256");

    assert_ne!(first, second);
    assert_eq!(
        second.export_key(&owner_key),
        origin_key(
            OriginExportKind::Semantic,
            "semantic:generic:u256",
            format!("expr:{}", local_expr.index())
        )
    );
}

#[test]
fn hir_origin_node_can_carry_semantic_origins_without_erasing_kind() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "semantic_origin_node_keys.fe".into(),
        r#"
fn a() -> u256 {
    let x: u256 = 1
    x
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
    let func = find_func(&db, top_mod, "a");
    let body = body_for(&db, top_mod, "a");
    let expr = body
        .exprs(&db)
        .keys()
        .next()
        .expect("body should contain an expression");
    let semantic_key = identity_semantic_instance_key(&db, BodyOwner::Func(func));

    let hir = HirOriginNode::Expr(HirExprOrigin::new(body, expr));
    let semantic =
        HirOriginNode::Semantic(SemanticOrigin::new(semantic_key, SemOrigin::Expr(expr)));

    assert_ne!(hir, semantic);
}

#[test]
fn semantic_origin_source_helper_delegates_to_hir_lazy_span() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "semantic_origin_source_span.fe".into(),
        r#"
fn a() -> u256 {
    let x: u256 = 1
    x
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
    let func = find_func(&db, top_mod, "a");
    let body = body_for(&db, top_mod, "a");
    let expr = first_expr(&db, body);
    let semantic_key = identity_semantic_instance_key(&db, BodyOwner::Func(func));
    let semantic = SemanticOrigin::new(semantic_key, SemOrigin::Expr(expr));

    assert_eq!(
        semantic.resolve_source_span(&db),
        expr.span(body).resolve(&db)
    );
}
