use std::path::Path;

use dir_test::{Fixture, dir_test};
use fe_hir::test_db::HirAnalysisTestDb;
use fe_hir::{
    core::semantic::reference::{HasReferences, ReferenceView, Target},
    hir_def::ItemKind,
};
use test_utils::snap_test;

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/test_files/body_references",
    glob: "*.fe"
)]
fn body_references(fixture: Fixture<&str>) {
    let mut db = HirAnalysisTestDb::default();
    let path = Path::new(fixture.path());
    let file_name = path.file_name().and_then(|file| file.to_str()).unwrap();
    let file = db.new_stand_alone(file_name.into(), fixture.content());
    let (top_mod, mut prop_formatter) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    // Find all functions and collect their body references
    let scope_graph = top_mod.scope_graph(&db);
    for item in scope_graph.items_dfs(&db) {
        if let ItemKind::Func(func) = item
            && let Some(body) = func.body(&db)
        {
            let refs = body.references(&db);
            for r in refs {
                match r {
                    ReferenceView::Path(pv) => {
                        // Get the path name for display
                        let path_name = pv
                            .path
                            .ident(&db)
                            .to_opt()
                            .map_or_else(|| "<complex>".to_string(), |id| id.data(&db).to_string());

                        // Try to resolve the target
                        let resolution = pv.target(&db);
                        let target_desc = match resolution.first() {
                            Some(Target::Scope(scope)) => scope.kind_name().to_string(),
                            Some(Target::Local { .. }) => "local".to_string(),
                            None => "unresolved".to_string(),
                        };

                        let annotation = format!("{path_name} -> {target_desc}");
                        prop_formatter.push_prop(top_mod, pv.span(), annotation);
                    }
                    ReferenceView::FieldAccess(fv) => {
                        let resolution = fv.target(&db);
                        let target_desc = match resolution.first() {
                            Some(Target::Scope(scope)) => format!("field -> {}", scope.kind_name()),
                            _ => "field access".to_string(),
                        };
                        prop_formatter.push_prop(top_mod, fv.span(), target_desc);
                    }
                    ReferenceView::MethodCall(mv) => {
                        let resolution = mv.target(&db);
                        let target_desc = match resolution.first() {
                            Some(Target::Scope(scope)) => {
                                format!("method -> {}", scope.kind_name())
                            }
                            _ => "method call".to_string(),
                        };
                        prop_formatter.push_prop(top_mod, mv.span(), target_desc);
                    }
                    ReferenceView::UsePath(_) => {
                        // Use paths are handled at item level, not body level
                    }
                }
            }
        }
    }

    let res = prop_formatter.finish(&db);
    snap_test!(res, fixture.path());
}

#[test]
fn reference_views_read_accepted_trait_choices() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "reference_decisions.fe".into(),
        "trait Get { fn get(self) -> u256 }\nstruct S {}\nimpl Get for S { fn get(self) -> u256 { 1 } }\nfn method() -> u256 { S {}.get() }\nfn path() { let f = <S as Get>::get }\nfn unrelated() {}\n",
    );
    let (module, _) = db.top_mod(file);
    db.assert_no_diags(module);
    let unrelated = fe_hir::test_db::find_func(&db, module, "unrelated");
    let (_, other_body) = fe_hir::analysis::ty::ty_check::check_func_body(&db, unrelated);
    for name in ["method", "path"] {
        let function = fe_hir::test_db::find_func(&db, module, name);
        let (_, typed) = fe_hir::analysis::ty::ty_check::check_func_body(&db, function);
        let body = function.body(&db).unwrap();
        if name == "method" {
            let call_answer = body
                .call_sites(&db)
                .iter()
                .find_map(|call| {
                    assert!(call.decision(other_body).is_none());
                    call.decision(typed)
                })
                .expect("whole-call accepted answer");
            assert_eq!(
                call_answer.data(&db).goal().pretty_print(&db, true),
                "S: Get"
            );
        }
        let mut found = false;
        for reference in body.references(&db) {
            let answer = match reference {
                ReferenceView::MethodCall(view) if name == "method" => {
                    assert!(view.decision(other_body).is_none());
                    view.decision(typed)
                }
                ReferenceView::Path(view) if name == "path" => {
                    assert!(view.decision(other_body).is_none());
                    view.decision(typed)
                }
                _ => None,
            };
            if let Some(answer) = answer {
                assert_eq!(answer.data(&db).goal().pretty_print(&db, true), "S: Get");
                assert!(matches!(
                    answer.data(&db).answer_origin(&db),
                    fe_hir::analysis::ty::trait_def::ImplementorOrigin::Hir(_)
                ));
                found = true;
            }
        }
        assert!(found, "{name} view did not read its accepted answer");
    }
}
