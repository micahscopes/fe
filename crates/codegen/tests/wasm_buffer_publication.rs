//! Runtime publications remain typed Fe values, not authored byte manifests.
use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WasmCompileOptions, compile_runtime_package_wasm_with_options};
use url::Url;

#[test]
fn raster_publications_resolve_nominal_targets_and_execute_fe_policy() {
    let base = include_str!("fixtures/actor_raster_typed/src/lib.fe");
    let source = format!(
        r#"
use core::BrowserBytes
use std::webgpu::{{StorageBuffer, BufferPublication, PublicationVersion, PublishBuffer}}
type Target = StorageBuffer<u32, 4>
struct State {{ tint: f32 }}
struct Upload {{}}
impl Upload {{
    pub fn publication(_ state: State) -> BufferPublication<Target> {{
        BufferPublication {{ version: PublicationVersion {{ epoch: 1, revision: if state.tint < 1.5 {{ 1 }} else {{ 2 }} }},
            source: BrowserBytes {{ ptr: 1024, len: 4 }}, target_byte_offset: 0 }}
    }}
}}
{}
"#,
        base.replace("    tint: f32,", "    payload: Target,\n    tint: f32,")
            .replace(
                "uses (FragmentStage<MeshVarying>)",
                "uses (FragmentStage<MeshVarying>, PublishBuffer<Upload>)"
            )
            .replace(
                "shade_color(varying.heat, self.tint)",
                "shade_color(varying.heat, self.tint) ^ self.payload.load(index: 0)"
            )
    );
    for (duplicate, absent) in [(false, false), (true, false), (false, true)] {
        let source = if duplicate {
            source.replace(
                "    payload: Target,",
                "    payload: Target,\n    other: Target,",
            )
        } else if absent {
            source.replace(
                "    payload: Target,",
                "    payload: StorageBuffer<u32, 8>,",
            )
        } else {
            source.clone()
        };
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///raster_publication.fe").unwrap();
        db.workspace().touch(&mut db, url.clone(), Some(source));
        let file = db.workspace().get(&db, &url).unwrap();
        let top = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top).format_diags(&db);
        assert!(diagnostics.is_empty(), "{diagnostics}");
        let result = fe_codegen::WebBundle::compile(
            &db,
            top,
            fe_codegen::WebBuildOptions::render("shade", None),
        );
        if duplicate || absent {
            let error = result
                .err()
                .expect("ambiguous or missing target must fail")
                .to_string();
            assert!(error.contains("exactly one actor resource"), "{error}");
            continue;
        }
        let bundle = result.unwrap();
        if let Some(path) = std::env::var_os("FE_PUBLICATION_TEST_SITE") {
            bundle.write_atomic(std::path::Path::new(&path)).unwrap();
        }
        assert!(
            bundle.manifest.resources[0]
                .buffer_usage
                .contains(&fe_codegen::WebBufferUsage::CopyDst)
        );
        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &bundle.wasm).unwrap();
        let mut store = wasmtime::Store::new(&engine, ());
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        let binding = instance
            .get_typed_func::<(), u32>(&mut store, "fe_buffer_publication_binding_v1_0")
            .unwrap();
        assert_eq!(binding.call(&mut store, ()).unwrap(), 0);
        let policy = instance
            .get_typed_func::<f32, (u32, u32, u32, u32, u32)>(
                &mut store,
                "fe_buffer_publication_v1_0",
            )
            .unwrap();
        assert_eq!(policy.call(&mut store, 0.5).unwrap(), (1, 1, 1024, 4, 0));
        assert_eq!(policy.call(&mut store, 2.0).unwrap(), (1, 2, 1024, 4, 0));
    }
}

#[test]
fn packed_word_publications_preserve_identity_and_check_byte_arithmetic() {
    let source = r#"
use core::{BrowserList, BrowserPtr}
use core::option::Option
use std::webgpu::{BufferPublication, PublicationVersion}
struct Target {}
pub fn describe(_ epoch: u32, _ revision: u32, _ ptr: u32, _ len: u32, _ offset: u32)
    -> (u32, u32, u32, u32, u32, u32) {
    let words: BrowserList<u32, 1073741824> = BrowserList { ptr: BrowserPtr::from_u32(ptr), len: len }
    let result = BufferPublication<Target>::from_words(
        PublicationVersion { epoch: epoch, revision: revision }, words, offset)
    match result {
        Option::Some(value) => (0, value.version.epoch, value.version.revision,
            value.source.ptr, value.source.len, value.target_byte_offset),
        Option::None => (1, 0, 0, 0, 0, 0),
    }
}
pub fn bounded(_ len: u32) -> bool {
    let words: BrowserList<u32, 8> = BrowserList { ptr: BrowserPtr::from_u32(0), len: len }
    match BufferPublication<Target>::from_words(PublicationVersion { epoch: 0, revision: 0 }, words, 0) {
        Option::Some(_) => true,
        Option::None => false,
    }
}
"#;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///buffer_publication.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let package = mir::build_wasm_runtime_package_for_entries(
        &db,
        top,
        &["describe".to_owned(), "bounded".to_owned()],
    )
    .unwrap();
    let artifact = compile_runtime_package_wasm_with_options(
        &db,
        &package,
        WasmCompileOptions::default().with_optimization(),
    )
    .unwrap();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &artifact.bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let describe = instance
        .get_typed_func::<(u32, u32, u32, u32, u32), (u32, u32, u32, u32, u32, u32)>(
            &mut store, "describe",
        )
        .unwrap();
    assert_eq!(
        describe.call(&mut store, (7, 12, 1024, 5, 3)).unwrap(),
        (0, 7, 12, 1024, 20, 12)
    );
    assert_eq!(
        describe.call(&mut store, (7, 12, 1024, 0, 0)).unwrap(),
        (0, 7, 12, 1024, 0, 0)
    );
    for args in [(0, 0, 0, 1073741824, 0), (0, 0, 0, 1, 1073741824)] {
        assert_eq!(describe.call(&mut store, args).unwrap(), (1, 0, 0, 0, 0, 0));
    }
    let bounded = instance
        .get_typed_func::<u32, u32>(&mut store, "bounded")
        .unwrap();
    assert_eq!(bounded.call(&mut store, 8).unwrap(), 1);
    assert_eq!(bounded.call(&mut store, 9).unwrap(), 0);
}
