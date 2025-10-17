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
