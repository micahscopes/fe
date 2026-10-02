use std::path::Path;

use common::{InputDb, dependencies::DependencyArguments};
use dir_test::{Fixture, dir_test};
use fe_driver::DriverDataBase;
use hir::analysis::{
    semantic::{GenericSubst, eval_body_owner_const_with_args},
    ty::ty_check::BodyOwner,
};
use url::Url;

fn add_file(db: &mut DriverDataBase, root: &Path, relative: &str) -> Url {
    let path = root.join(relative);
    let url = Url::from_file_path(&path).expect("fixture file path is absolute");
    let source = std::fs::read_to_string(&path).expect("fixture file");
    db.workspace().update(db, url.clone(), source);
    url
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/generated_provider_scope/project/src", glob: "lib.fe")]
fn provider_paths_keep_their_ingot(fixture: Fixture<&str>) {
    let project = Path::new(fixture.path())
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let policy = project.parent().unwrap().join("policy");
    let project_url = Url::from_directory_path(project).expect("project path");
    let policy_url = Url::from_directory_path(&policy).expect("policy path");
    let mut db = DriverDataBase::default();
    add_file(&mut db, project, "fe.toml");
    add_file(&mut db, &policy, "fe.toml");
    add_file(&mut db, &policy, "src/lib.fe");
    let owner_url = add_file(&mut db, project, "src/lib.fe");
    db.dependency_graph().add_dependency(
        &mut db,
        &project_url,
        &policy_url,
        "policy".into(),
        DependencyArguments::default(),
    );
    let output = fe_driver::elaboration::prepare_target(&db, &project_url)
        .unwrap()
        .expect("generation request");
    let file = output.workspace().get(&output, &owner_url).unwrap();
    for name in ["answer", "answer_shadow"] {
        let function = output
            .top_mod(file)
            .all_funcs(&output)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(&output)
                    .to_opt()
                    .is_some_and(|ident| ident.data(&output) == name)
            })
            .expect("answer function");
        let result = eval_body_owner_const_with_args(
            &output,
            BodyOwner::Func(function),
            GenericSubst::for_body_owner(&output, BodyOwner::Func(function), vec![]),
            vec![],
        )
        .into_ready()
        .expect("generated answer evaluates")
        .pretty_print(&output);
        assert_eq!(result, "43", "{name}");
    }
}
