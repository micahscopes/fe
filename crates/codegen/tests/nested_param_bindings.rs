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
    let module = wasmtime::Module::new(&engine, &bundle.wasm).unwrap();
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let initialize = instance
        .get_func(&mut store, "fe_surface_initialize_v1")
        .unwrap();
    let replace = instance
        .get_func(&mut store, "fe_surface_state_replace_v1")
        .unwrap();
    let transition = instance
        .get_func(&mut store, "fe_surface_transition_scheduled_v1")
        .unwrap();
    let alloc = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    let initial = [
        1.0f32, 0.125, -0.0625, 0.03125, -1.0, 0.125, -0.0625, 0.03125, 2.0, 32.0, 11.0, 13.0,
        19.0, 23.0, 17.0,
    ];
    let mut values = vec![wasmtime::Val::F32(0); initial.len()];
    initialize.call(&mut store, &[], &mut values).unwrap();
    let bits = |values: &[wasmtime::Val]| {
        values
            .iter()
            .map(|v| match v {
                wasmtime::Val::F32(bits) => *bits,
                other => panic!("unexpected scalar {other:?}"),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(bits(&values), initial.map(f32::to_bits));
    for index in [0u32, 1, 2, 3, 4, 255] {
        replace.call(&mut store, &values, &mut []).unwrap();
        let ptr = alloc.call(&mut store, (52, 4)).unwrap();
        let mut event = [0u8; 52];
        event[32..36].copy_from_slice(&32.0f32.to_le_bytes());
        event[36..40].copy_from_slice(&32.0f32.to_le_bytes());
        event[40..44].copy_from_slice(&1u32.to_le_bytes());
        event[44..48].copy_from_slice(&index.to_le_bytes());
        event[48..52].copy_from_slice(&3.0f32.to_le_bytes());
        memory.write(&mut store, ptr as usize, &event).unwrap();
        let mut actual = vec![wasmtime::Val::F32(0); initial.len()];
        transition
            .call(
                &mut store,
                &[wasmtime::Val::I32(ptr), wasmtime::Val::I32(1)],
                &mut actual,
            )
            .unwrap();
        let mut expected = initial;
        match index {
            1 => expected[8] = 3.0,
            2 => expected[0] = 3.0,
            3 => expected[4] = 3.0,
            _ => {}
        }
        assert_eq!(
            bits(&actual),
            expected.map(f32::to_bits),
            "resident event {index}"
        );
    }
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
fn ambiguous_and_duplicate_flat_targets_are_diagnostics() {
    let source = r#"
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
"#;
    let duplicate = source
        .replace("struct Group { value: f32 }", "struct Group { value: f32 }\nstruct Hidden { spare: f32 }\nstruct GroupParams { value: Param }")
        .replace("right: Group", "right: Hidden")
        .replace("right: Group { value: 2.0 }", "right: Hidden { spare: 2.0 }")
        .replace("right: Hidden { value: 2.0 }", "right: Hidden { spare: 2.0 }")
        .replace("struct Params { value: Param }", "struct Params { value: Param, left: GroupParams }")
        .replace("value: Param::range(min: 0.0, max: 8.0, init: 1.0),", "value: Param::range(min: 0.0, max: 8.0, init: 1.0), left: GroupParams { value: Param::range(min: 0.0, max: 8.0, init: 1.0) },");
    for (source, expected) in [
        (source, "found 2"),
        (duplicate.as_str(), "duplicates actor scalar target"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("src")).unwrap();
        std::fs::write(
            temp.path().join("fe.toml"),
            "[ingot]\nname = \"invalid_controls\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
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
            .expect("invalid target must not compile a bundle");
        assert!(error.to_string().contains(expected), "{error}");
    }
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

#[test]
fn record_updates_remain_inside_their_short_circuit_operand() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/record_update_scope")
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
    for index in [0, 1, 2, 255] {
        assert_eq!(
            edit.call(&mut store, index).unwrap(),
            1,
            "denominator {index}"
        );
    }
}
