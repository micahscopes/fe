use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

fn compile_to_wasm(source: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///browser_typed_alloc.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("typed browser object allocation should lower")
        .into_bytecode()
        .expect("Wasm bytecode")
}

fn compile_to_wasm_err(source: &str) -> String {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///browser_typed_alloc_rejected.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect_err("forged provider call site must fail closed")
        .to_string()
}

#[test]
fn typed_browser_objects_use_the_authoritative_wasm_layout() {
    let wasm = compile_to_wasm(
        r#"
use core::{BrowserPtr, alloc_browser_object}

struct Payload {
    values: [u32; 8],
    marker: u32,
}

struct Workspace {
    marker: u32,
    payload: BrowserPtr<Payload>,
}

fn initialize(_ seed: u32)
    uses (payload: mut Payload)
{
    let mut index: usize = 0
    while index < 8 {
        payload.values[index] = seed + index.downcast_truncate()
        index = index + 1
    }
    payload.marker = seed + 100
}

fn checksum() -> u32
    uses (payload: Payload)
{
    let mut total: u32 = payload.marker
    let mut index: usize = 0
    while index < 8 {
        total = total + payload.values[index]
        index = index + 1
    }
    total
}

fn value_at(_ index: usize) -> u32
    uses (payload: Payload)
{
    payload.values[index]
}

fn initialize_workspace(_ payload: BrowserPtr<Payload>)
    uses (workspace: mut Workspace)
{
    workspace.marker = 77
    workspace.payload = payload
}

fn workspace_payload_address() -> u32
    uses (workspace: Workspace)
{
    if workspace.marker != 77 { return 0 }
    workspace.payload.address()
}

pub fn two_objects(_ left_seed: u32, _ right_seed: u32) -> u32 {
    let left: BrowserPtr<Payload> = alloc_browser_object<Payload>()
    let right: BrowserPtr<Payload> = alloc_browser_object<Payload>()
    with (left) { initialize(left_seed) }
    with (right) { initialize(right_seed) }
    let left_sum = with (left) { checksum() }
    let right_sum = with (right) { checksum() }
    left_sum * 1000 + right_sum
}

pub fn read_index(_ seed: u32, _ raw_index: u32) -> u32 {
    let payload: BrowserPtr<Payload> = alloc_browser_object<Payload>()
    with (payload) { initialize(seed) }
    let index: usize = raw_index as usize
    with (payload) { value_at(index) }
}

pub fn nested_typed_pointer_field() -> u32 {
    let payload: BrowserPtr<Payload> = alloc_browser_object<Payload>()
    let workspace: BrowserPtr<Workspace> = alloc_browser_object<Workspace>()
    with (workspace) { initialize_workspace(payload) }
    let stored = with (workspace) { workspace_payload_address() }
    if stored == payload.address() { 1 } else { 0 }
}
"#,
    );
    wasmparser::validate(&wasm).expect("typed allocation emitted invalid Wasm");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert_eq!(
        module.imports().count(),
        0,
        "allocator must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let run = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "two_objects")
        .unwrap();
    let read_index = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "read_index")
        .unwrap();
    let nested_pointer = instance
        .get_typed_func::<(), i32>(&mut store, "nested_typed_pointer_field")
        .unwrap();

    // checksum(seed) = (seed + 100) + sum(seed .. seed + 7)
    assert_eq!(run.call(&mut store, (10, 30)).unwrap(), 218_398);
    assert_eq!(read_index.call(&mut store, (10, 7)).unwrap(), 17);
    assert_eq!(nested_pointer.call(&mut store, ()).unwrap(), 1);
    assert!(
        read_index.call(&mut store, (10, 8)).is_err(),
        "typed dynamic indexes must trap at the derived array bound"
    );
}

#[test]
fn typed_memory_provider_scopes_rewind_non_escaping_temporaries() {
    let wasm = compile_to_wasm(
        r#"
use core::{BrowserBytes, BrowserPtr, alloc_browser_object}

struct State { total: u32 }

fn initialize() uses (state: mut State) { state.total = 0 }

fn accumulate(_ seed: u32) uses (state: mut State) {
    let mut scratch = [0; 256]
    let mut index: usize = 0
    while index < 256 {
        scratch[index] = seed + index.downcast_truncate()
        index = index + 1
    }
    state.total = state.total + scratch[255]
}

pub fn run(_ count: u32) -> BrowserBytes {
    let state: BrowserPtr<State> = alloc_browser_object<State>()
    with (state) { initialize() }
    let mut iteration: u32 = 0
    while iteration < count {
        with (state) { accumulate(iteration) }
        iteration = iteration + 1
    }
    BrowserBytes { ptr: state.address(), len: 4 }
}
"#,
    );
    wasmparser::validate(&wasm).expect("provider-scoped allocation emitted invalid Wasm");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert_eq!(module.imports().count(), 0);
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let reset = instance
        .get_typed_func::<(), ()>(&mut store, "fe_cabi_reset")
        .unwrap();
    let run = instance
        .get_typed_func::<i32, (i32, i32)>(&mut store, "run")
        .unwrap();
    let alloc = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();

    reset.call(&mut store, ()).unwrap();
    let (pointer, length) = run.call(&mut store, 128).unwrap();
    assert_eq!(length, 4);
    let mut total = [0_u8; 4];
    memory.read(&store, pointer as usize, &mut total).unwrap();
    assert_eq!(u32::from_le_bytes(total), 40_768);

    let next = alloc.call(&mut store, (1, 1)).unwrap();
    assert!(
        next < 2048,
        "callee-local scratch escaped its memory-provider scope: arena top {next}",
    );
}

#[test]
fn typed_memory_provider_provenance_requires_every_call_site() {
    let error = compile_to_wasm_err(
        r#"
use core::{BrowserBytes, BrowserPtr, alloc_browser_object}

struct State { total: u32 }

fn accumulate(_ seed: u32) uses (state: mut State) {
    let mut scratch = [0; 256]
    scratch[255] = seed
    state.total = state.total + scratch[255]
}

pub fn arena_owned(_ seed: u32) -> BrowserBytes {
    let state: BrowserPtr<State> = alloc_browser_object<State>()
    with (state) { accumulate(seed) }
    BrowserBytes { ptr: state.address(), len: 4 }
}

pub fn forged(_ address: u32, _ seed: u32) {
    let state: BrowserPtr<State> = BrowserPtr::from_u32(address)
    with (state) { accumulate(seed) }
}
"#,
    );
    assert!(
        error.contains(
            "a function that allocates a local array cannot also use a direct host memory region"
        ),
        "unexpected fail-closed diagnostic: {error}",
    );
}

#[test]
fn typed_pointer_fields_require_arena_owned_storage() {
    let error = compile_to_wasm_err(
        r#"
use core::BrowserPtr

struct Payload { marker: u32 }
struct Workspace { payload: BrowserPtr<Payload> }

fn store_payload(_ payload: BrowserPtr<Payload>)
    uses (workspace: mut Workspace)
{
    workspace.payload = payload
}

pub fn forged(_ workspace_address: u32, _ payload_address: u32) {
    let workspace: BrowserPtr<Workspace> = BrowserPtr::from_u32(workspace_address)
    let payload: BrowserPtr<Payload> = BrowserPtr::from_u32(payload_address)
    with (workspace) { store_payload(payload) }
}
"#,
    );
    assert!(
        error.contains("statement `Store")
            && error.contains("while lowering Wasm function `store_payload`"),
        "unexpected fail-closed diagnostic: {error}",
    );
}

#[test]
fn typed_provider_reborrows_preserve_arena_provenance() {
    let wasm = compile_to_wasm(
        r#"
use core::{BrowserPtr, alloc_browser_object}

struct Input {
    valid: bool,
    values: [u32; 8],
}

struct Output {
    valid: bool,
    values: [u32; 8],
}

fn transform(_ input: ref Input, _ output: mut Output) -> bool {
    output.valid = false
    if !input.valid { return false }
    let mut scratch = [0; 8]
    let mut index: usize = 0
    while index < 8 {
        scratch[index] = input.values[index] + 1
        output.values[index] = scratch[index]
        index = index + 1
    }
    output.valid = true
    true
}

fn prepare(_ seed: u32) uses (input: mut Input) {
    input.valid = true
    let mut index: usize = 0
    while index < 8 {
        input.values[index] = seed + index.downcast_truncate()
        index = index + 1
    }
}

fn forward() -> bool uses (input: Input, output: mut Output) {
    transform(ref input, mut output)
}

fn read_last() -> u32 uses (output: Output) {
    if !output.valid { return 0 }
    output.values[7]
}

pub fn run(_ seed: u32) -> u32 {
    let input: BrowserPtr<Input> = alloc_browser_object<Input>()
    let output: BrowserPtr<Output> = alloc_browser_object<Output>()
    with (input) { prepare(seed) }
    if !with (input, output) { forward() } { return 0 }
    with (output) { read_last() }
}
"#,
    );
    wasmparser::validate(&wasm).expect("provider reborrow fixture emitted invalid Wasm");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let run = instance
        .get_typed_func::<i32, i32>(&mut store, "run")
        .unwrap();
    assert_eq!(run.call(&mut store, 10).unwrap(), 18);
}

#[test]
fn typed_pointer_fields_preserve_arena_provenance_across_actor_stages() {
    let wasm = compile_to_wasm(
        r#"
use core::{BrowserPtr, alloc_browser_object}

struct Payload {
    valid: bool,
    value: u32,
}

struct Workspace {
    valid: bool,
    input: BrowserPtr<Payload>,
    output: BrowserPtr<Payload>,
}

fn initialize_workspace(
    _ input: BrowserPtr<Payload>,
    _ output: BrowserPtr<Payload>,
)
    uses (workspace: mut Workspace)
{
    workspace.valid = false
    workspace.input = input
    workspace.output = output
    workspace.valid = true
}

fn initialize_payload(_ value: u32) uses (payload: mut Payload) {
    payload.valid = true
    payload.value = value
}

fn transform() -> bool uses (input: Payload, output: mut Payload) {
    output.valid = false
    if !input.valid { return false }
    let mut scratch = [0; 8]
    scratch[7] = input.value + 1
    output.value = scratch[7]
    output.valid = true
    true
}

fn run_stage() -> bool uses (workspace: Workspace) {
    if !workspace.valid { return false }
    let input = workspace.input
    let output = workspace.output
    with (input, output) { transform() }
}

fn read() -> u32 uses (payload: Payload) {
    if !payload.valid { return 0 }
    payload.value
}

pub fn run(_ value: u32) -> u32 {
    let input: BrowserPtr<Payload> = alloc_browser_object<Payload>()
    let output: BrowserPtr<Payload> = alloc_browser_object<Payload>()
    let workspace: BrowserPtr<Workspace> = alloc_browser_object<Workspace>()
    with (input) { initialize_payload(value) }
    with (workspace) { initialize_workspace(input, output) }
    if !with (workspace) { run_stage() } { return 0 }
    with (output) { read() }
}
"#,
    );
    wasmparser::validate(&wasm).expect("actor workspace pointer fixture emitted invalid Wasm");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let run = instance
        .get_typed_func::<i32, i32>(&mut store, "run")
        .unwrap();
    assert_eq!(run.call(&mut store, 41).unwrap(), 42);
}

#[test]
fn oversized_private_value_parameters_use_the_derived_indirect_abi() {
    let wasm = compile_to_wasm(
        r#"
fn changed(mut values: own [u32; 1001]) -> u32 {
    values[0] = 99
    values[1000] + values[0]
}

pub fn run() -> u32 {
    let mut values = [0; 1001]
    values[0] = 1
    values[1000] = 42
    changed(values: values) * 1000 + values[0]
}
"#,
    );
    wasmparser::validate(&wasm).expect("indirect aggregate ABI emitted invalid Wasm");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let run = instance
        .get_typed_func::<(), i32>(&mut store, "run")
        .unwrap();
    assert_eq!(run.call(&mut store, ()).unwrap(), 141_001);
}

#[test]
fn oversized_public_value_parameters_fail_before_wasm_emission() {
    let error = compile_to_wasm_err(
        r#"
pub fn run(_ values: [u32; 1001]) -> u32 {
    values[1000]
}
"#,
    );
    assert!(
        error.contains("exceeding the validated limit of 1000"),
        "unexpected oversized public ABI diagnostic: {error}",
    );
}
