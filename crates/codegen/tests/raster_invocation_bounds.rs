//! A direct draw's declared counts constrain only its physical invocation IDs.
//! Indirect draw contents and actor data are not compile-time promises.
use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WebBuildOptions, WebBundle};
use url::Url;

#[test]
fn raster_invocation_bounds_prove_only_declared_direct_indices() {
    let direct = include_str!("fixtures/actor_raster_instanced/src/lib.fe");
    let indirect = include_str!("fixtures/actor_raster_indirect/src/lib.fe");
    let cases = [
        ("direct_instance", direct.replace("instance_value(instance_index)", "instance_value(instance_index + 1)"), true),
        ("direct_vertex", direct.replace("if vertex_index == 1", "if vertex_index + 1 == 2"), true),
        ("underflow", direct.replace("instance_value(instance_index)", "instance_value(instance_index - 1)"), false),
        ("overflow", direct.replace("instance_value(instance_index)", "instance_value(instance_index + 4294967295)"), false),
        ("actor_data", direct.replace("tint: f32,", "tint: f32, unbounded:u32,")
            .replace("instance_value(instance_index)", "instance_value(self.unbounded + 1)"), false),
        ("indirect", indirect.replace("if instance_index == 0", "if instance_index + 1 == 1"), false),
    ];
    for (name, source, succeeds) in cases {
        let mut db = DriverDataBase::default();
        let url = Url::parse(&format!("file:///raster_bounds_{name}.fe")).unwrap();
        db.workspace().touch(&mut db, url.clone(), Some(source));
        let file = db.workspace().get(&db, &url).unwrap();
        let top = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top).format_diags(&db);
        assert!(diagnostics.is_empty(), "{name}: {diagnostics}");
        let result = WebBundle::compile(&db, top, WebBuildOptions::render("shade", None));
        if succeeds {
            let bundle = result.unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!(!bundle.wgsl.is_empty());
            let raster = bundle.manifest.passes.iter().find(|pass| pass.draw_vertices.is_some()).unwrap();
            assert_eq!(raster.draw_vertices, Some(3));
            assert_eq!(raster.draw_instances, Some(4));
        } else {
            let error = result.err().expect("unproved arithmetic must retain its safety failure").to_string();
            assert!(error.contains("trap"), "{name}: {error}");
        }
    }
}
