//! Runtime publications remain typed Fe values, not authored byte manifests.
use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WasmCompileOptions, compile_runtime_package_wasm_with_options};
use url::Url;

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
