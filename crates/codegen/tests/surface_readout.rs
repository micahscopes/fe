#![cfg(feature = "spirv-backend")]
use common::InputDb;
use fe_codegen::{WebBuildOptions, WebBundle};
use hir::hir_def::HirIngot;

#[test]
fn readouts_observe_initialized_actor_state_without_joining_input_derivation() {
    for (fixture, accepted) in [
        ("actor_surface_readout", true),
        ("actor_surface_readout_no_owner", false),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture)
            .canonicalize()
            .unwrap();
        let url = url::Url::from_directory_path(path).unwrap();
        let mut db = driver::DriverDataBase::default();
        assert!(!driver::init_ingot(&mut db, &url));
        let top = db
            .workspace()
            .containing_ingot(&db, url)
            .unwrap()
            .root_mod(&db);
        let diagnostics = db.run_on_top_mod(top).format_diags(&db);
        assert!(diagnostics.is_empty(), "{diagnostics}");
        let result = WebBundle::compile(&db, top, WebBuildOptions::render("shade", None));
        if accepted {
            let bundle = result.unwrap();
            let surface = bundle.manifest.surface.unwrap();
            assert_eq!(surface.params.len(), 2);
            let output = &surface.params[1];
            assert_eq!(output.name, "resolved");
            assert_eq!(output.source.as_deref(), Some("state"));
            assert_eq!(output.init, None);
            assert_eq!(output.presentation.as_ref().unwrap().widget, "output");
        } else {
            let error = result
                .err()
                .expect("a readout must not invent missing actor state");
            assert!(
                error.to_string().contains("require an InitialState"),
                "{error}"
            );
        }
    }
}
