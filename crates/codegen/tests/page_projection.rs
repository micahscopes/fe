use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{PageAttributeKind, PageElement, PageProjectionOp, project_page};
use hir::hir_def::HirIngot;
use url::Url;

#[test]
fn role_selected_fe_page_projects_typed_structure_without_json() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/web_page_actor")
        .canonicalize()
        .expect("page fixture path");
    let url = Url::from_directory_path(path).expect("page fixture URL");
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let ingot = db.workspace().containing_ingot(&db, url).unwrap();
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "page diagnostics:\n{diagnostics}");

    let page = project_page(&db, top_mod)
        .expect("page projection")
        .expect("selected page");
    assert_eq!(page.actor, "GalleryPage");
    assert_eq!(page.source_entry, "compose");
    assert_eq!(page.title, "Fe page");
    assert_eq!(page.body.len(), 9);
    assert_eq!(page.body[0], PageProjectionOp::Open(PageElement::Main));
    let PageProjectionOp::Attribute(id) = &page.body[1] else {
        panic!("expected id attribute")
    };
    assert_eq!(id.kind, PageAttributeKind::Id);
    assert_eq!(id.text, "gallery");
    let PageProjectionOp::Attribute(hidden) = &page.body[2] else {
        panic!("expected hidden attribute")
    };
    assert_eq!(hidden.kind, PageAttributeKind::Hidden);
    assert!(hidden.enabled);
    assert_eq!(
        page.body[4],
        PageProjectionOp::Text("Hello <Fe>".to_owned())
    );
    let PageProjectionOp::Render(render) = &page.body[6] else {
        panic!("expected render program")
    };
    assert_eq!(render.source, "sketches/gradient");
    assert_eq!(
        (
            render.wgsl_action,
            render.wasm_action,
            render.manifest_action
        ),
        (101, 102, 103)
    );
    let PageProjectionOp::Component(component) = &page.body[7] else {
        panic!("expected component program")
    };
    assert_eq!(component.source, "sketches/todomvc/src/lib.fe");
    assert_eq!(component.mount, "todo-app");
}
