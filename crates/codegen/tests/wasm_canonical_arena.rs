use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    CanonicalInterfaceManifest, CanonicalShape, WasmCompileOptions,
    canonical_lane_decl_from_entry, compile_runtime_package_wasm_with_options,
};
use url::Url;

fn source_module(db: &mut DriverDataBase) -> Url {
    let url = Url::parse("file:///wasm_canonical_arena.fe").unwrap();
    db.workspace().touch(
        db,
        url.clone(),
        Some("pub fn update(value: u32) -> u32 { value + 1 }\n".to_owned()),
    );
    url
}

#[test]
fn canonical_arena_emission_is_explicit_and_typed() {
    let mut db = DriverDataBase::default();
    let url = source_module(&mut db);
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "update").unwrap();
    let ordinary =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .unwrap();
    let canonical = compile_runtime_package_wasm_with_options(
        &db,
        &package,
        WasmCompileOptions::default().with_canonical_arena(),
    )
    .unwrap();

    let engine = wasmtime::Engine::default();
    let ordinary_module = wasmtime::Module::new(&engine, &ordinary.bytes).unwrap();
    let canonical_module = wasmtime::Module::new(&engine, &canonical.bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let ordinary_instance = wasmtime::Instance::new(&mut store, &ordinary_module, &[]).unwrap();
    let ordinary_memory = ordinary_instance.get_memory(&mut store, "memory").unwrap();
    assert!(!ordinary_memory.ty(&store).is_64());
    assert!(
        ordinary_instance
            .get_func(&mut store, "fe_cabi_alloc")
            .is_none()
    );
    assert!(
        ordinary_instance
            .get_func(&mut store, "fe_cabi_reset")
            .is_none()
    );

    let canonical_instance = wasmtime::Instance::new(&mut store, &canonical_module, &[]).unwrap();
    let canonical_memory = canonical_instance.get_memory(&mut store, "memory").unwrap();
    assert!(!canonical_memory.ty(&store).is_64());
    canonical_instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .unwrap();
    canonical_instance
        .get_typed_func::<(), ()>(&mut store, "fe_cabi_reset")
        .unwrap();
}

#[test]
fn fe_malloc_uses_shared_byte_aligned_canonical_arena_and_grows_memory() {
    let source = r#"
use core::effect_ref::alloc_bytes

pub fn allocate_pair(first_size: u32, second_size: u32) {
    let first = alloc_bytes(first_size)
    let second = alloc_bytes(second_size)
}

pub fn allocate_large(size: u32) {
    let allocation = alloc_bytes(size)
}
"#;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///wasm_fe_malloc.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let package = mir::build_wasm_runtime_package(&db, top_mod).unwrap();
    let artifact = compile_runtime_package_wasm_with_options(
        &db,
        &package,
        WasmCompileOptions::default().with_canonical_arena(),
    )
    .unwrap();

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &artifact.bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    let alloc = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .unwrap();
    let reset = instance
        .get_typed_func::<(), ()>(&mut store, "fe_cabi_reset")
        .unwrap();
    let pair = instance
        .get_typed_func::<(i32, i32), ()>(&mut store, "allocate_pair")
        .unwrap();
    let large = instance
        .get_typed_func::<i32, ()>(&mut store, "allocate_large")
        .unwrap();

    reset.call(&mut store, ()).unwrap();
    pair.call(&mut store, (13, 29)).unwrap();
    // MemAllocDynamic promises byte alignment only. With align=1, the cursor
    // after 13 and 29 bytes is exact, proving both Fe allocations used the same
    // arena cursor and occupied consecutive non-overlapping ranges.
    assert_eq!(alloc.call(&mut store, (1, 1)).unwrap(), 1024 + 13 + 29);

    reset.call(&mut store, ()).unwrap();
    let pages_before = memory.size(&store);
    large.call(&mut store, 200_000).unwrap();
    assert!(memory.size(&store) > pages_before);
    assert_eq!(alloc.call(&mut store, (1, 1)).unwrap(), 1024 + 200_000);
}

#[test]
fn canonical_browser_descriptors_roundtrip_owned_bytes_and_utf8() {
    let source = r#"
use core::{BrowserBytes, BrowserString}

struct Request {
    bytes: BrowserBytes,
    text: BrowserString,
}

struct Response {
    bytes: BrowserBytes,
    text: BrowserString,
}

pub fn echo(request: own Request) -> Response {
    Response {
        bytes: BrowserBytes {
            ptr: request.bytes.ptr,
            len: request.bytes.len,
        },
        text: BrowserString {
            ptr: request.text.ptr,
            len: request.text.len,
        },
    }
}
"#;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///wasm_canonical_descriptors.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let declaration = canonical_lane_decl_from_entry(&db, top_mod, "echo", "echo").unwrap();
    let manifest = CanonicalInterfaceManifest::build(vec![declaration]).unwrap();
    let lane = manifest.lanes[0].clone();
    let CanonicalShape::Record { fields } = &lane.request.shape else {
        panic!("request record")
    };
    assert!(matches!(fields[0].layout.shape, CanonicalShape::Bytes { .. }));
    assert!(matches!(
        fields[1].layout.shape,
        CanonicalShape::String { ref encoding, .. } if encoding == "utf-8"
    ));

    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "echo").unwrap();
    let artifact = compile_runtime_package_wasm_with_options(
        &db,
        &package,
        WasmCompileOptions::default().with_canonical_lane(lane),
    )
    .unwrap();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &artifact.bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    let echo = instance
        .get_typed_func::<i32, i32>(&mut store, "fe_cabi_echo")
        .unwrap();
    let alloc = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .unwrap();

    let request_ptr = 64usize;
    let bytes_ptr = 128usize;
    let text_ptr = 192usize;
    let bytes = b"x\0y";
    let text = "a\0héllo 🌍".as_bytes();
    memory.write(&mut store, bytes_ptr, bytes).unwrap();
    memory.write(&mut store, text_ptr - 1, &[0xcc]).unwrap();
    memory.write(&mut store, text_ptr, text).unwrap();
    memory
        .write(&mut store, text_ptr + text.len(), &[0xdd])
        .unwrap();
    let mut request = Vec::new();
    request.extend_from_slice(&(bytes_ptr as u32).to_le_bytes());
    request.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    request.extend_from_slice(&(text_ptr as u32).to_le_bytes());
    request.extend_from_slice(&(text.len() as u32).to_le_bytes());
    memory.write(&mut store, request_ptr, &request).unwrap();

    let response_ptr = echo.call(&mut store, request_ptr as i32).unwrap() as usize;
    let mut response = [0u8; 16];
    memory.read(&store, response_ptr, &mut response).unwrap();
    let word = |offset| {
        u32::from_le_bytes(response[offset..offset + 4].try_into().unwrap()) as usize
    };
    let copied_bytes_ptr = word(0);
    let copied_bytes_len = word(4);
    let copied_text_ptr = word(8);
    let copied_text_len = word(12);
    assert_eq!(copied_bytes_len, bytes.len());
    assert_eq!(copied_text_len, text.len());
    assert_ne!(copied_bytes_ptr, bytes_ptr);
    assert_ne!(copied_text_ptr, text_ptr);
    assert!(copied_bytes_ptr + copied_bytes_len <= memory.data_size(&store));
    assert!(copied_text_ptr + copied_text_len <= memory.data_size(&store));
    let mut copied_bytes = vec![0; copied_bytes_len];
    let mut copied_text = vec![0; copied_text_len];
    memory
        .read(&store, copied_bytes_ptr, &mut copied_bytes)
        .unwrap();
    memory
        .read(&store, copied_text_ptr, &mut copied_text)
        .unwrap();
    assert_eq!(copied_bytes, bytes);
    assert_eq!(copied_text, text);
    assert_eq!(std::str::from_utf8(&copied_text).unwrap(), "a\0héllo 🌍");
    assert_eq!(
        alloc.call(&mut store, (1, 1)).unwrap() as usize,
        copied_text_ptr + copied_text_len
    );
}
