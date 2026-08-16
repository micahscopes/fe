use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    PageAttributeKind, PageElement, PageProjectionOp, project_component, project_page,
};
use hir::hir_def::HirIngot;
use url::Url;

#[test]
fn repository_has_one_canonical_gallery_source() {
    let demos = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("demos")
        .canonicalize()
        .expect("repository demos directory");
    assert!(
        demos.join("gallery.html").is_file(),
        "the canonical Fe gallery source must remain demos/gallery.html"
    );
    assert!(
        !demos.join("gallery").exists(),
        "the retired Trunk gallery directory must not return"
    );

    let legacy_index = std::fs::read_to_string(demos.join("index.html"))
        .expect("legacy showcase landing page");
    assert!(
        !legacy_index.contains("copy-dir\" href=\"gallery")
            && !legacy_index.contains("href=\"gallery/\""),
        "the legacy landing page must not copy or link a second gallery lane"
    );
}

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
    assert_eq!(page.body.len(), 10);
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
    assert!(render.sequenced);
    assert_eq!(render.sequence, 0);
    assert_eq!(
        (
            render.wgsl_action,
            render.wasm_action,
            render.manifest_action
        ),
        (101, 102, 103)
    );
    let PageProjectionOp::Render(second_render) = &page.body[7] else {
        panic!("expected second render program")
    };
    assert!(second_render.sequenced);
    assert_eq!(second_render.sequence, 1);
    assert_eq!(second_render.source, "sketches/plasma");
    let PageProjectionOp::Component(component) = &page.body[8] else {
        panic!("expected component program")
    };
    assert_eq!(component.source, "sketches/todomvc/src/lib.fe");
    assert_eq!(component.mount, "todo-app");

    let component = project_component(&db, top_mod)
        .expect("component projection")
        .expect("selected component view");
    assert_eq!(component.actor, "TodoComponent");
    assert_eq!(component.source_entry, "view");
    assert_eq!(component.body.len(), 7);
    let PageProjectionOp::Attribute(local_for) = &component.body[1] else {
        panic!("expected component-local for attribute")
    };
    assert_eq!(local_for.kind, PageAttributeKind::LocalFor);
    assert_eq!(local_for.text, "toggle-all");
    let PageProjectionOp::Attribute(local_id) = &component.body[5] else {
        panic!("expected component-local id attribute")
    };
    assert_eq!(local_id.kind, PageAttributeKind::LocalId);
    assert_eq!(local_id.text, "toggle-all");
}
