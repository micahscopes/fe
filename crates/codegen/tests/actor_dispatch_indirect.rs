//! GPU-written dispatch commands retain exact resource identity across cycles.
use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WebBuildOptions, WebBundle, WebBufferUsage};
use hir::hir_def::HirIngot;
use url::Url;

#[test]
fn indirect_dispatch_preserves_command_identity_without_shader_binding() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/actor_dispatch_indirect");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let ingot = db.workspace().containing_ingot(&db, url).unwrap();
    let top = ingot.root_mod(&db);
    let diagnostics = db.run_on_ingot(ingot).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let bundle = WebBundle::compile(&db, top, WebBuildOptions::compute("work", None))
        .expect("typed indirect compute bundle");
    let passes = &bundle.manifest.passes;
    assert_eq!(passes.len(), 2);
    assert!(passes[0].dispatch_indirect.is_none());
    let command = passes[1].dispatch_indirect.as_ref().unwrap();
    assert!(passes[1].dispatch.is_none());
    assert_eq!(command.resource, "command");
    assert_eq!(command.offset, 0);
    assert_eq!(passes[1].cycle.as_ref().unwrap().repeat, 2);
    assert!(passes[1].layout.bindings.iter().all(|b| b.name != "command"));
    let resource = bundle.manifest.resources.iter().find(|r| r.name == "command").unwrap();
    assert_eq!(resource.buffer_usage, [WebBufferUsage::Storage, WebBufferUsage::Indirect]);
}

#[test]
fn indirect_dispatch_rejects_invalid_command_contracts() {
    let original = include_str!("fixtures/actor_dispatch_indirect/src/lib.fe");
    let cases = [
        (original.replace("struct Round {}", "struct Round {}\nstruct Missing {}")
            .replace("IndirectDispatch<DispatchIndirectBuffer<ActiveWork>>", "IndirectDispatch<DispatchIndirectBuffer<Missing>>"),
            "has no field of that exact resource type"),
        (original.replace("output: StorageBuffer", "duplicate: DispatchIndirectBuffer<ActiveWork>,\n    output: StorageBuffer"),
            "actor fields have that exact resource type"),
        (original.replace("DispatchIndirectBuffer", "DrawIndirectBuffer"),
            "must contain exactly 3 `u32` words"),
        (original.replace("if i.global.x<64", "self.command.store(index:0, value:0)\n        if i.global.x<64"),
            "must not also be bound by its consuming shader"),
    ];
    for (source, expected) in cases {
        let temp = tempfile::tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")).unwrap();
        std::fs::create_dir(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("fe.toml"),
            include_str!("fixtures/actor_dispatch_indirect/fe.toml")).unwrap();
        std::fs::write(temp.path().join("src/lib.fe"), source).unwrap();
        let url = Url::from_directory_path(temp.path()).unwrap();
        let mut db = DriverDataBase::default();
        assert!(!driver::init_ingot(&mut db, &url));
        let ingot = db.workspace().containing_ingot(&db, url).unwrap();
        let diagnostics = db.run_on_ingot(ingot).format_diags(&db);
        assert!(diagnostics.is_empty(), "{diagnostics}");
        let error = WebBundle::compile(&db, ingot.root_mod(&db), WebBuildOptions::compute("work", None))
            .err().expect("invalid command contract must fail");
        assert!(error.to_string().contains(expected), "expected {expected}; got {error}");
    }
}
