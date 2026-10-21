#![cfg(feature = "spirv-backend")]
use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WasmCompileOptions, WebBuildOptions, WebBundle, compile_runtime_package_wasm_with_options,
};
use hir::hir_def::HirIngot;

#[test]
fn generic_nested_controls_preserve_private_tails_and_resolve_uniform_offsets() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nested_param_binding_actor")
        .canonicalize()
        .unwrap();
    let url = url::Url::from_directory_path(path).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let top = db
        .workspace()
        .containing_ingot(&db, url)
        .unwrap()
        .root_mod(&db);
    let diagnostics = db.run_on_top_mod(top).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");

    let package = mir::build_wasm_runtime_package_for_entry(&db, top, "edit").unwrap();
    let artifact =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .unwrap();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &artifact.bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let edit = instance
        .get_typed_func::<i32, i32>(&mut store, "edit")
        .unwrap();
    for index in [0, 1, 2, 3, 4, 255] {
        assert_eq!(
            edit.call(&mut store, index).unwrap(),
            1,
            "event ordinal {index}"
        );
    }

    let bundle = WebBundle::compile(&db, top, WebBuildOptions::render("shade", None)).unwrap();
    let surface = bundle.manifest.surface.as_ref().unwrap();
    assert_eq!(surface.params.len(), 4);
    let members = bundle
        .manifest
        .layout
        .bindings
        .iter()
        .flat_map(|binding| &binding.members)
        .collect::<Vec<_>>();
    // Declaration order: ticks, zoom, center.re, center.im. Hidden tails and
    // hidden state still occupy their original uniform positions.
    let offsets = surface
        .params
        .iter()
        .map(|param| {
            members
                .iter()
                .find(|member| member.name == param.name)
                .unwrap()
                .offset
        })
        .collect::<Vec<_>>();
    assert_eq!(offsets, [56, 32, 0, 16]);
    assert_eq!(members.len(), 15);
    assert_eq!(members.iter().map(|m| m.offset + m.width).max(), Some(60));
    assert_eq!(surface.params[0].source.as_deref(), Some("state"));
    assert_eq!(surface.params[0].init, None);
}

#[test]
fn invalid_relative_projection_is_a_diagnostic_not_a_fallback() {
    for (replacement, expected) in [
        ("missing", "must resolve to exactly one actor scalar leaf"),
        ("tail", "must resolve to exactly one actor scalar leaf"),
        ("head..tail", "invalid relative projection"),
    ] {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/nested_param_binding_actor");
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("src")).unwrap();
        std::fs::copy(root.join("fe.toml"), temp.path().join("fe.toml")).unwrap();
        std::fs::copy(
            root.join("src/scalar.fe"),
            temp.path().join("src/scalar.fe"),
        )
        .unwrap();
        let source = std::fs::read_to_string(root.join("src/lib.fe"))
            .unwrap()
            .replace(
                ".projected(\"head\")",
                &format!(".projected(\"{replacement}\")"),
            );
        std::fs::write(temp.path().join("src/lib.fe"), source).unwrap();
        let url = url::Url::from_directory_path(temp.path()).unwrap();
        let mut db = DriverDataBase::default();
        assert!(!driver::init_ingot(&mut db, &url));
        let top = db
            .workspace()
            .containing_ingot(&db, url)
            .unwrap()
            .root_mod(&db);
        let diagnostics = db.run_on_top_mod(top).format_diags(&db);
        assert!(diagnostics.is_empty(), "{diagnostics}");
        let error = WebBundle::compile(&db, top, WebBuildOptions::render("shade", None))
            .err()
            .expect("invalid projection must not compile a bundle");
        assert!(
            error.to_string().contains(expected),
            "{replacement}: {error}"
        );
    }
}

#[test]
fn ambiguous_flat_target_does_not_choose_one_same_named_nested_leaf() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("src")).unwrap();
    std::fs::write(
        temp.path().join("fe.toml"),
        "[ingot]\nname = \"ambiguous_controls\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("src/lib.fe"),
        r#"
use core::actor::InitialState
use std::web::view::{Param, Surface, Extent}
use std::webgpu::{GpuProgram, WebGpuBackend, FragmentSurface}
struct Group { value: f32 }
struct State { left: Group, right: Group }
struct Params { value: Param }
actor AmbiguousSurface uses (GpuProgram<WebGpuBackend>) {
    left: Group,
    right: Group,
    fn initial() -> State uses (InitialState) {
        State { left: Group { value: 1.0 }, right: Group { value: 2.0 } }
    }
    const fn view() -> Surface<Params> {
        Surface { extent: Extent::square(32), params: Params {
            value: Param::range(min: 0.0, max: 8.0, init: 1.0),
        } }
    }
    fn shade(self, x: i32, y: i32) -> i32 uses (FragmentSurface) { -1 }
}
"#,
    )
    .unwrap();
    let url = url::Url::from_directory_path(temp.path()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let top = db
        .workspace()
        .containing_ingot(&db, url)
        .unwrap()
        .root_mod(&db);
    let diagnostics = db.run_on_top_mod(top).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let error = WebBundle::compile(&db, top, WebBuildOptions::render("shade", None))
        .err()
        .expect("ambiguous target must not compile a bundle");
    assert!(error.to_string().contains("found 2"), "{error}");
}

#[test]
fn typed_update_does_not_grant_access_to_private_nested_fields() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nested_param_binding_actor");
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("src")).unwrap();
    std::fs::copy(root.join("fe.toml"), temp.path().join("fe.toml")).unwrap();
    std::fs::copy(
        root.join("src/scalar.fe"),
        temp.path().join("src/scalar.fe"),
    )
    .unwrap();
    let source = std::fs::read_to_string(root.join("src/lib.fe"))
        .unwrap()
        .replace(
            "type CoordinateControls<T> = Pair<T>",
            "struct RawControl { head: Param }\ntype CoordinateControls<T> = Pair<T>",
        )
        .replace(
            "center: CoordinateControls<Param>",
            "center: CoordinateControls<RawControl>",
        )
        .replace("re: Param::range", "re: RawControl { head: Param::range")
        .replace("im: Param::range", "im: RawControl { head: Param::range")
        .replace(".projected(\"head\"),", " },");
    std::fs::write(temp.path().join("src/lib.fe"), source).unwrap();
    let url = url::Url::from_directory_path(temp.path()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let top = db
        .workspace()
        .containing_ingot(&db, url)
        .unwrap()
        .root_mod(&db);
    let diagnostics = db.run_on_top_mod(top).format_diags(&db);
    assert!(diagnostics.contains("private"), "{diagnostics}");
}
