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

#[test]
fn canonical_bounded_lists_roundtrip_as_owned_typed_payloads_and_trap_invalid_results() {
    let source = r#"
use core::BrowserList

struct U32Request {
    values: BrowserList<u32, 4>,
    mode: u32,
}

struct F32Request {
    values: BrowserList<f32, 4>,
}

struct EmptyRequest {
    values: BrowserList<u32, 0>,
}

pub fn echo_u32(request: own U32Request) -> BrowserList<u32, 4> {
    let len = if request.mode == 0 { request.values.len } else { request.mode }
    BrowserList { ptr: request.values.ptr, len }
}

pub fn echo_f32(request: own F32Request) -> BrowserList<f32, 4> {
    BrowserList { ptr: request.values.ptr, len: request.values.len }
}

pub fn echo_empty(request: own EmptyRequest) -> BrowserList<u32, 0> {
    BrowserList { ptr: request.values.ptr, len: request.values.len }
}
"#;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///wasm_canonical_bounded_lists.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let declarations = ["echo_u32", "echo_f32", "echo_empty"]
        .map(|entry| canonical_lane_decl_from_entry(&db, top_mod, entry, entry).unwrap());
    let manifest = CanonicalInterfaceManifest::build(declarations.to_vec()).unwrap();
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "echo_u32").unwrap();
    let artifact = compile_runtime_package_wasm_with_options(
        &db,
        &package,
        WasmCompileOptions::default().with_canonical_lane(manifest.lanes[0].clone()),
    )
    .unwrap();

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &artifact.bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    let echo_u32 = instance
        .get_typed_func::<i32, i32>(&mut store, "fe_cabi_echo_u32")
        .unwrap();

    let read_descriptor = |store: &wasmtime::Store<()>, pointer: i32| {
        let mut descriptor = [0u8; 8];
        memory
            .read(store, pointer as usize, &mut descriptor)
            .unwrap();
        (
            u32::from_le_bytes(descriptor[0..4].try_into().unwrap()),
            u32::from_le_bytes(descriptor[4..8].try_into().unwrap()),
        )
    };
    let request_ptr = 64usize;
    let payload_ptr = 128usize;
    let values = [1u32, 0x01020304, u32::MAX];
    let payload = values
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    memory.write(&mut store, payload_ptr, &payload).unwrap();
    let mut request = Vec::new();
    request.extend_from_slice(&(payload_ptr as u32).to_le_bytes());
    request.extend_from_slice(&(values.len() as u32).to_le_bytes());
    request.extend_from_slice(&0u32.to_le_bytes());
    memory.write(&mut store, request_ptr, &request).unwrap();
    let response = echo_u32.call(&mut store, request_ptr as i32).unwrap();
    let (copied_ptr, copied_len) = read_descriptor(&store, response);
    assert_eq!(copied_len, values.len() as u32, "length is an element count");
    assert_eq!(copied_ptr % 4, 0);
    assert_ne!(copied_ptr, payload_ptr as u32);
    let mut copied = vec![0; payload.len()];
    memory
        .read(&store, copied_ptr as usize, &mut copied)
        .unwrap();
    assert_eq!(copied, payload);

    // A forged Fe result cannot publish more than MAX elements.
    memory
        .write(&mut store, request_ptr + 8, &5u32.to_le_bytes())
        .unwrap();
    assert!(echo_u32.call(&mut store, request_ptr as i32).is_err());
    // Nor can a non-empty typed result publish a misaligned source.
    memory
        .write(&mut store, request_ptr, &129u32.to_le_bytes())
        .unwrap();
    memory
        .write(&mut store, request_ptr + 8, &0u32.to_le_bytes())
        .unwrap();
    assert!(echo_u32.call(&mut store, request_ptr as i32).is_err());

    // Empty descriptors do not dereference or impose alignment on an ignored pointer.
    memory
        .write(&mut store, request_ptr, &129u32.to_le_bytes())
        .unwrap();
    memory
        .write(&mut store, request_ptr + 4, &0u32.to_le_bytes())
        .unwrap();
    memory
        .write(&mut store, request_ptr + 8, &0u32.to_le_bytes())
        .unwrap();
    let response = echo_u32.call(&mut store, request_ptr as i32).unwrap();
    let (_, copied_len) = read_descriptor(&store, response);
    assert_eq!(copied_len, 0);

    // Compile each lane from its own MIR root, matching production's one-entry package.
    let f32_package =
        mir::build_wasm_runtime_package_for_entry(&db, top_mod, "echo_f32").unwrap();
    let f32_artifact = compile_runtime_package_wasm_with_options(
        &db,
        &f32_package,
        WasmCompileOptions::default().with_canonical_lane(manifest.lanes[1].clone()),
    )
    .unwrap();
    let f32_module = wasmtime::Module::new(&engine, &f32_artifact.bytes).unwrap();
    let mut f32_store = wasmtime::Store::new(&engine, ());
    let f32_instance = wasmtime::Instance::new(&mut f32_store, &f32_module, &[]).unwrap();
    let f32_memory = f32_instance.get_memory(&mut f32_store, "memory").unwrap();
    let echo_f32 = f32_instance
        .get_typed_func::<i32, i32>(&mut f32_store, "fe_cabi_echo_f32")
        .unwrap();
    let f32_bits = [1.5f32.to_bits(), (-0.0f32).to_bits(), 0x7fc0_1234];
    let f32_payload = f32_bits
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    f32_memory
        .write(&mut f32_store, payload_ptr, &f32_payload)
        .unwrap();
    let mut f32_request = Vec::new();
    f32_request.extend_from_slice(&(payload_ptr as u32).to_le_bytes());
    f32_request.extend_from_slice(&(f32_bits.len() as u32).to_le_bytes());
    f32_memory
        .write(&mut f32_store, request_ptr, &f32_request)
        .unwrap();
    let f32_response = echo_f32
        .call(&mut f32_store, request_ptr as i32)
        .unwrap();
    let mut f32_descriptor = [0u8; 8];
    f32_memory
        .read(&f32_store, f32_response as usize, &mut f32_descriptor)
        .unwrap();
    let f32_copied_ptr =
        u32::from_le_bytes(f32_descriptor[0..4].try_into().unwrap()) as usize;
    let f32_copied_len = u32::from_le_bytes(f32_descriptor[4..8].try_into().unwrap());
    assert_eq!(f32_copied_len, f32_bits.len() as u32);
    assert_eq!(f32_copied_ptr % 4, 0);
    assert_ne!(f32_copied_ptr, payload_ptr);
    let mut f32_copied = vec![0; f32_payload.len()];
    f32_memory
        .read(&f32_store, f32_copied_ptr, &mut f32_copied)
        .unwrap();
    assert_eq!(
        f32_copied, f32_payload,
        "the Wasm transport preserves f32 payload bits"
    );

    let empty_package =
        mir::build_wasm_runtime_package_for_entry(&db, top_mod, "echo_empty").unwrap();
    let empty_artifact = compile_runtime_package_wasm_with_options(
        &db,
        &empty_package,
        WasmCompileOptions::default().with_canonical_lane(manifest.lanes[2].clone()),
    )
    .unwrap();
    let empty_module = wasmtime::Module::new(&engine, &empty_artifact.bytes).unwrap();
    let mut empty_store = wasmtime::Store::new(&engine, ());
    let empty_instance = wasmtime::Instance::new(&mut empty_store, &empty_module, &[]).unwrap();
    let empty_memory = empty_instance.get_memory(&mut empty_store, "memory").unwrap();
    let echo_empty = empty_instance
        .get_typed_func::<i32, i32>(&mut empty_store, "fe_cabi_echo_empty")
        .unwrap();
    empty_memory
        .write(&mut empty_store, request_ptr, &129u32.to_le_bytes())
        .unwrap();
    let empty_response = echo_empty
        .call(&mut empty_store, request_ptr as i32)
        .unwrap();
    let mut empty_descriptor = [0u8; 8];
    empty_memory
        .read(
            &empty_store,
            empty_response as usize,
            &mut empty_descriptor,
        )
        .unwrap();
    assert_eq!(
        u32::from_le_bytes(empty_descriptor[4..8].try_into().unwrap()),
        0
    );
}

#[test]
fn fe_allocated_bytes_can_be_returned_through_one_canonical_call() {
    let source = r#"
use core::AllocatedBrowserBytes
use core::effect_ref::alloc_bytes

struct Request {
    seed: u8,
}

pub fn frame(request: own Request) -> AllocatedBrowserBytes {
    let first = alloc_bytes(1)
    first.write(0x46)
    let second = alloc_bytes(1)
    second.write(0x65)
    let third = alloc_bytes(1)
    third.write(0x21)
    let len = if request.seed == 0 { 3 } else { 4294967295 }
    AllocatedBrowserBytes { ptr: first, len }
}
"#;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///wasm_allocated_browser_bytes.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let declaration = canonical_lane_decl_from_entry(&db, top_mod, "frame", "frame").unwrap();
    let manifest = CanonicalInterfaceManifest::build(vec![declaration]).unwrap();
    let lane = manifest.lanes[0].clone();
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "frame").unwrap();
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
    let frame = instance
        .get_typed_func::<i32, i32>(&mut store, "fe_cabi_frame")
        .unwrap();
    let response_ptr = frame.call(&mut store, 0).unwrap() as usize;
    let mut descriptor = [0u8; 8];
    memory.read(&store, response_ptr, &mut descriptor).unwrap();
    let pointer = u32::from_le_bytes(descriptor[0..4].try_into().unwrap()) as usize;
    let length = u32::from_le_bytes(descriptor[4..8].try_into().unwrap()) as usize;
    let mut bytes = vec![0; length];
    memory.read(&store, pointer, &mut bytes).unwrap();
    assert_eq!(bytes, b"Fe!");
    memory.write(&mut store, 0, &[1]).unwrap();
    assert!(
        frame.call(&mut store, 0).is_err(),
        "an oversized Fe descriptor must trap instead of exposing memory"
    );
}
