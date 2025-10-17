use fe_hir::{lower, hir_def::ItemKind};

mod test_db;
use test_db::HirAnalysisTestDb;

#[test]
fn test_body_owner_func() {
    let mut db = HirAnalysisTestDb::default();

    let file = db.new_stand_alone(
        "test.fe".into(),
        r#"
fn foo() {
    let x = 1
}
"#,
    );

    let top_mod = lower::map_file_to_mod(&db, file);

    // Get the function
    let func = top_mod
        .all_funcs(&db)
        .iter()
        .find(|f| {
            if let Some(name) = f.name(&db).to_opt() {
                name.data(&db) == "foo"
            } else {
                false
            }
        })
        .expect("should find foo function");

    // Get the body from the function
    let body = func.body(&db).expect("foo should have a body");

    // Navigate back to the owner function
    let owner_func = body.owner_func(&db).expect("body should have an owner func");

    // Verify it's the same function
    assert_eq!(*func, owner_func);
}

#[test]
fn test_body_owner_const() {
    let mut db = HirAnalysisTestDb::default();

    let file = db.new_stand_alone(
        "test.fe".into(),
        r#"
const FOO: i32 = 42
"#,
    );

    let top_mod = lower::map_file_to_mod(&db, file);

    // Get the const
    let const_item = top_mod
        .all_items(&db)
        .iter()
        .find_map(|item| {
            if let ItemKind::Const(c) = item {
                if let Some(name) = c.name(&db).to_opt() {
                    if name.data(&db) == "FOO" {
                        return Some(*c);
                    }
                }
            }
            None
        })
        .expect("should find FOO const");

    // Get the body from the const
    let body = const_item.body(&db).to_opt().expect("FOO should have a body");

    // Navigate back to the owner const
    let owner_const = body.owner_const(&db).expect("body should have an owner const");

    // Verify it's the same const
    assert_eq!(const_item, owner_const);
}

#[test]
fn test_context_rich_wrappers() {
    let mut db = HirAnalysisTestDb::default();

    let file = db.new_stand_alone(
        "test.fe".into(),
        r#"
fn bar() {
    let x = 1
    let y = 2
}
"#,
    );

    let top_mod = lower::map_file_to_mod(&db, file);

    // Get the function
    let func = top_mod
        .all_funcs(&db)
        .iter()
        .find(|f| {
            if let Some(name) = f.name(&db).to_opt() {
                name.data(&db) == "bar"
            } else {
                false
            }
        })
        .expect("should find bar function");

    // Get the body
    let body = func.body(&db).expect("bar should have a body");

    // Get the body's root expression ID
    let root_expr_id = body.expr(&db);

    // Create a context-rich wrapper for the expression
    let expr_wrapper = body.wrap_expr(root_expr_id);

    // Test that the wrapper carries context
    assert_eq!(expr_wrapper.body(), body);
    assert_eq!(expr_wrapper.id(), root_expr_id);

    // Test upward navigation: expr → body → func
    let func_from_expr = expr_wrapper
        .containing_func(&db)
        .expect("expr should know its containing function");
    assert_eq!(*func, func_from_expr);

    // Test scope access (currently delegates to body scope)
    let scope = expr_wrapper.scope(&db);
    assert_eq!(scope, body.scope());

    // Test data access through wrapper
    let expr_data = expr_wrapper.data(&db);
    assert!(matches!(expr_data, fe_hir::hir_def::Partial::Present(fe_hir::hir_def::expr::ExprDescription::Block(_))));
}
