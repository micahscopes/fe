//! End-to-end acceptance: the first genuinely-Fe-compiled wasm.
//!
//! Each test takes Fe source, compiles it Fe -> MIR -> Sonatina IR (Wasm32 ISA)
//! -> WAFFLE -> wasm bytes through `BackendKind::Wasm`, executes the bytes under
//! wasmtime, and asserts the result. The same source is also compiled through
//! the EVM backend (`BackendKind::Sonatina`) as the cross-backend twin: it
//! proves one Fe source lowers on both targets, and the wasm result is asserted
//! equal to the known EVM-semantics value (Fe integer arithmetic is identical
//! across backends; the EVM backend's value-correctness is covered by the full
//! EVM suite + byte-identity gate).
//!
//! R1 scope: scalar u64 arithmetic, a loop/phi (`sum_to`), and a call pair.
//! Non-overflowing values only (the WAFFLE translator fakes overflow flags as
//! 0; real checked semantics are R2).

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

/// Compile Fe source to wasm bytes through the wasm backend.
fn compile_to_wasm(name: &str, source: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .unwrap_or_else(|err| panic!("wasm compilation of `{name}` failed: {err}"));
    let bytes = output
        .into_bytecode()
        .expect("wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("produced invalid wasm");
    bytes
}

/// Compile the same Fe source through the EVM backend (the cross-backend twin).
/// Returns the EVM runtime bytecode, proving one source lowers on both targets.
fn compile_to_evm(name: &str, source: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    BackendKind::Sonatina
        .create()
        .compile(
            &db,
            top_mod,
            layout_for(BackendKind::Sonatina),
            OptLevel::O0,
        )
        .unwrap_or_else(|err| panic!("evm twin compilation of `{name}` failed: {err}"))
        .into_bytecode()
        .expect("evm output should be bytecode")
}

fn instantiate(bytes: &[u8]) -> (wasmtime::Store<()>, wasmtime::Instance) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    (store, instance)
}

/// Collect the `(module, name)` of every function import in the emitted wasm,
/// scanned from the bytes with `wasmparser` (asserted, not assumed).
fn func_imports(bytes: &[u8]) -> Vec<(String, String)> {
    use wasmparser::{Payload, TypeRef};
    let mut imports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let Payload::ImportSection(reader) = payload.expect("valid wasm payload") {
            for import in reader.into_imports() {
                let import = import.expect("valid import entry");
                if let TypeRef::Func(_) = import.ty {
                    imports.push((import.module.to_string(), import.name.to_string()));
                }
            }
        }
    }
    imports
}

/// THE MILESTONE: `#[target(wasm)] pub fn add(a, b) -> a + b`, compiled Fe ->
/// wasm, executed under wasmtime, add(2, 3) == 5, and equal to the EVM twin.
#[test]
fn fe_add_runs_on_wasm_and_matches_evm_twin() {
    let source = "pub fn add(a: u64, b: u64) -> u64 { a + b }\n\
                  pub fn main() -> u64 { add(2, 3) }\n";

    // Cross-backend twin: the identical source also compiles to EVM.
    let evm = compile_to_evm("wasm_add.fe", source);
    assert!(!evm.is_empty(), "evm twin bytecode must be non-empty");
    // Fe `add(2, 3)` has the same integer semantics on both backends.
    let evm_twin_result: i64 = 5;

    let wasm = compile_to_wasm("wasm_add.fe", source);
    let (mut store, instance) = instantiate(&wasm);

    let add = instance
        .get_typed_func::<(i64, i64), i64>(&mut store, "add")
        .expect("`add` export should exist");
    let wasm_result = add.call(&mut store, (2, 3)).expect("add(2, 3) should run");
    assert_eq!(wasm_result, 5, "Fe->wasm add(2, 3) should be 5");
    assert_eq!(
        wasm_result, evm_twin_result,
        "Fe->wasm add(2, 3) must equal the EVM twin"
    );

    // A few more non-overflowing points.
    assert_eq!(add.call(&mut store, (40, 2)).unwrap(), 42);
    assert_eq!(add.call(&mut store, (0, 0)).unwrap(), 0);

    // `main()` calls `add(2, 3)` internally and returns 5.
    let main = instance
        .get_typed_func::<(), i64>(&mut store, "main")
        .expect("`main` export should exist");
    assert_eq!(main.call(&mut store, ()).unwrap(), 5, "main() should be 5");
}

/// `sum_to(n) = 0 + 1 + ... + (n-1)`: a loop with a loop-carried accumulator and
/// counter (phis inserted by Sonatina's SSA-variable machinery), compiled
/// Fe -> wasm and executed under wasmtime.
#[test]
fn fe_sum_to_loop_runs_on_wasm() {
    let source = "pub fn sum_to(n: u64) -> u64 {\n\
                  \x20   let mut acc: u64 = 0\n\
                  \x20   let mut i: u64 = 0\n\
                  \x20   while i < n {\n\
                  \x20       acc = acc + i\n\
                  \x20       i = i + 1\n\
                  \x20   }\n\
                  \x20   acc\n\
                  }\n\
                  pub fn main() -> u64 { sum_to(10) }\n";

    // Cross-backend twin.
    let evm = compile_to_evm("wasm_sum_to.fe", source);
    assert!(!evm.is_empty(), "evm twin bytecode must be non-empty");

    let wasm = compile_to_wasm("wasm_sum_to.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let sum_to = instance
        .get_typed_func::<i64, i64>(&mut store, "sum_to")
        .expect("`sum_to` export should exist");

    // sum_to(n) = n*(n-1)/2, all well within u64.
    for n in [0i64, 1, 5, 10, 100] {
        let expected = n * (n - 1) / 2;
        assert_eq!(
            sum_to.call(&mut store, n).unwrap(),
            expected,
            "sum_to({n}) should be {expected}"
        );
    }
}

/// R3.2 THE MILESTONE: a Fe `extern` host function becomes a real wasm import.
///
/// `extern { pub unsafe fn host_add(a, b) -> u64 }` is a non-builtin extern (no
/// Fe body, not a recognized runtime builtin), so it lowers to a DECLARED-
/// EXTERNAL runtime function with `Linkage::External` and no body, which the
/// WAFFLE backend (R3.1 pass-0) emits as a `("fe", "host_add")` wasm import.
/// `use_host` calls it; wasmtime satisfies the import through a `Linker` stub.
/// Because `host_add` has no body, the only way `use_host`/`main` can run at all
/// is via the emitted import, so a passing run proves the import path end to end.
#[test]
fn fe_extern_host_import_runs_on_wasm() {
    let source = "extern {\n\
                  \x20   pub unsafe fn host_add(a: u64, b: u64) -> u64\n\
                  }\n\
                  pub fn use_host(a: u64, b: u64) -> u64 { host_add(a, b) }\n\
                  pub fn main() -> u64 { use_host(2, 3) }\n";

    let wasm = compile_to_wasm("wasm_host_import.fe", source);

    // Scan the emitted bytes: the ("fe", "host_add") func import must be present.
    let imports = func_imports(&wasm);
    assert!(
        imports.contains(&("fe".to_string(), "host_add".to_string())),
        "expected a (\"fe\", \"host_add\") func import in the emitted wasm, found {imports:?}"
    );

    // Instantiate through a Linker that satisfies the import with a stub
    // (host_add(a, b) = a + b). The plain empty-imports `Instance::new` path used
    // by the other R1 tests cannot instantiate this module: it has an import.
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    linker
        .func_wrap("fe", "host_add", |a: u64, b: u64| a + b)
        .expect("binding the ('fe','host_add') host stub should succeed");
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("wasmtime should instantiate with the host import satisfied");

    // use_host(a, b) calls the host import; the stub returns a + b.
    let use_host = instance
        .get_typed_func::<(u64, u64), u64>(&mut store, "use_host")
        .expect("`use_host` export should exist");
    assert_eq!(
        use_host.call(&mut store, (2, 3)).unwrap(),
        5,
        "use_host(2, 3) should call the host import and return 5"
    );
    assert_eq!(use_host.call(&mut store, (40, 2)).unwrap(), 42);

    // main() calls use_host(2, 3) internally, which calls the host import.
    let main = instance
        .get_typed_func::<(), u64>(&mut store, "main")
        .expect("`main` export should exist");
    assert_eq!(main.call(&mut store, ()).unwrap(), 5, "main() should be 5");
}

/// R3.3 THE MILESTONE: `#[wasm_import(module = "fe:host")]` on an extern block
/// names the wasm import MODULE. `host_log` becomes a `("fe:host", "host_log")`
/// import instead of the flat `("fe", "host_log")` v0 default. The module string
/// threads HIR (block attribute propagated onto the extern `Func`) -> runtime
/// package -> the WasmBackend side table -> WAFFLE import emission. wasmtime
/// satisfies the import through a `Linker` bound at `("fe:host", "host_log")`.
#[test]
fn fe_wasm_import_module_attribute_names_module() {
    let source = "#[wasm_import(module = \"fe:host\")]\n\
                  extern {\n\
                  \x20   pub unsafe fn host_log(a: u64, b: u64) -> u64\n\
                  }\n\
                  pub fn use_host(a: u64, b: u64) -> u64 { host_log(a, b) }\n\
                  pub fn main() -> u64 { use_host(2, 3) }\n";

    let wasm = compile_to_wasm("wasm_import_module.fe", source);

    // The attribute's module is on the emitted import: ("fe:host", "host_log").
    let imports = func_imports(&wasm);
    assert!(
        imports.contains(&("fe:host".to_string(), "host_log".to_string())),
        "expected a (\"fe:host\", \"host_log\") func import in the emitted wasm, found {imports:?}"
    );
    // The flat "fe" module must NOT appear for this symbol (the attribute won).
    assert!(
        !imports.contains(&("fe".to_string(), "host_log".to_string())),
        "the attribute module should replace the flat \"fe\" default, found {imports:?}"
    );

    // Instantiate through a Linker bound at the attribute's module namespace.
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    linker
        .func_wrap("fe:host", "host_log", |a: u64, b: u64| a + b)
        .expect("binding the ('fe:host','host_log') host stub should succeed");
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("wasmtime should instantiate with the fe:host import satisfied");

    let main = instance
        .get_typed_func::<(), u64>(&mut store, "main")
        .expect("`main` export should exist");
    assert_eq!(
        main.call(&mut store, ()).unwrap(),
        5,
        "main() should call the fe:host import and return 5"
    );
}

// ===========================================================================
// R3.4b THE KEYSTONE: a Fe-compiled-to-wasm host program drives the WebGPU
// `Dispatch` + `Wait` capability import table against a wasmtime FAKE DEVICE and
// the pinned NTT-8 vectors land in wasm linear memory.
//
// The Fe host program is a PURE ORCHESTRATOR (the Path A-prime memory model,
// interop section 8): region handles are host-minted (the `main`/`main_begin`
// entries receive their `MemPtr<u32>`s as exported-fn parameters), so there is no
// `from_raw` and no Fe-side address arithmetic - every step is a raw import call.
// The fake device owns a host-side buffer table; `gpu_dispatch` is a pinned-vector
// table lookup (NO field arithmetic in the stub, so the oracle cannot lie).
// WebGPU EXECUTION is browser-only and never sandbox-verifiable; this run proves
// the CONTRACT (the import op-set and the memory model), not a GPU.
// ===========================================================================

/// The pinned size-8 forward-NTT probe input over `F_12289`
/// (`crates/fe/tests/fixtures/fe_test/ntt_exec.fe`).
const NTT8_INPUT: [u32; 8] = [5, 15, 39, 77, 129, 195, 275, 369];
/// The pinned size-8 forward-NTT output (the probe's pinned sample).
const NTT8_OUTPUT: [u32; 8] = [1104, 6528, 7157, 1035, 12081, 7898, 4772, 8621];

/// The Fe host program (host-minted handles, Path A-prime). Two entries over the
/// same straight-line sequence:
///   - `main`: create -> upload -> dispatch -> readback_begin -> wait, threading
///     BOTH `Dispatch` and `Wait`; when it returns, `output` is resident.
///   - `main_begin` + `on_ready`: the degraded/continuation twin. `main_begin`
///     composes WITHOUT `Wait` in scope (create..readback_begin only) and returns
///     the `PendingId`; the host later drives `on_ready(token)`, which completes
///     the resident copy. This proves degraded mode composes by construction and
///     that readback is honestly asynchronous (no copy until completion).
const WEBGPU_NTT8_SRC: &str = r#"
use core::MemPtr
use std::webgpu::{Dispatch, Wait, WebGpuBackend, KernelId, WorkerGpu, PendingId}

// The pinned NTT-8 kernel is the page's pipeline-table index 0 (layout.json).
fn ntt8_on_gpu(_ input: MemPtr<u32>, _ output: MemPtr<u32>)
    uses (gpu: mut Dispatch<WebGpuBackend>, w: mut Wait<WebGpuBackend>)
{
    let buf = gpu.create(8)
    gpu.upload(input, 8, buf)
    gpu.dispatch(KernelId::new(0), 1)
    let p = gpu.readback_begin(buf, 8, output)
    w.wait(p)
}

pub fn main(_ input: MemPtr<u32>, _ output: MemPtr<u32>) {
    with (Dispatch<WebGpuBackend> = WorkerGpu {}, Wait<WebGpuBackend> = WorkerGpu {}) {
        ntt8_on_gpu(input, output)
    }
}

fn ntt8_begin(_ input: MemPtr<u32>, _ output: MemPtr<u32>) -> PendingId
    uses (gpu: mut Dispatch<WebGpuBackend>)
{
    let buf = gpu.create(8)
    gpu.upload(input, 8, buf)
    gpu.dispatch(KernelId::new(0), 1)
    gpu.readback_begin(buf, 8, output)
}

pub fn main_begin(_ input: MemPtr<u32>, _ output: MemPtr<u32>) -> PendingId {
    with (Dispatch<WebGpuBackend> = WorkerGpu {}) {
        ntt8_begin(input, output)
    }
}

fn wait_ready(_ pending: PendingId) uses (w: mut Wait<WebGpuBackend>) {
    w.wait(pending)
}

pub fn on_ready(_ pending: PendingId) {
    with (Wait<WebGpuBackend> = WorkerGpu {}) {
        wait_ready(pending)
    }
}
"#;

/// The wasmtime FAKE DEVICE: a host-side buffer table plus a pending-readback
/// table and an op-sequence log. It mirrors the `fe:webgpu` / `fe:async` import
/// op-set op-for-op. `gpu_dispatch` is a PINNED-VECTOR TABLE LOOKUP with no field
/// arithmetic: an unrecognized input traps, so the NTT-8 oracle is a lookup, not a
/// re-implementation of the transform.
#[derive(Default)]
struct FakeDevice {
    /// Buffer table: `buffers[handle]` is a device buffer of `u32` words.
    buffers: Vec<Vec<u32>>,
    /// Pending readbacks: `pending[token]` records the copy `wait`/`on_ready`
    /// will perform (source buffer, word count, destination byte offset).
    pending: Vec<PendingReadback>,
    /// The op-sequence log, one entry per serviced import call.
    log: Vec<&'static str>,
}

struct PendingReadback {
    src_buffer: usize,
    len_words: usize,
    dst_addr: usize,
}

/// Read `len` little-endian `u32` words from `mem` starting at byte offset `addr`.
fn read_words(mem: &[u8], addr: usize, len: usize) -> Vec<u32> {
    (0..len)
        .map(|i| {
            let off = addr + i * 4;
            u32::from_le_bytes(mem[off..off + 4].try_into().expect("4 bytes in range"))
        })
        .collect()
}

/// Write `words` as little-endian `u32`s into `mem` starting at byte offset `addr`.
fn write_words(mem: &mut [u8], addr: usize, words: &[u32]) {
    for (i, word) in words.iter().enumerate() {
        let off = addr + i * 4;
        mem[off..off + 4].copy_from_slice(&word.to_le_bytes());
    }
}

/// Build a `Linker` that services the `fe:webgpu` + `fe:async` import op-set with a
/// `FakeDevice`, and instantiate `wasm` against it. The `FakeDevice` lives in the
/// store data; host bodies reach both it and the instance's exported `memory`
/// through `Caller`.
fn instantiate_fake_device(wasm: &[u8]) -> (wasmtime::Store<FakeDevice>, wasmtime::Instance) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, FakeDevice::default());
    let mut linker = wasmtime::Linker::new(&engine);

    // gpu_buffer_create(len) -> handle: mint a fresh buffer handle.
    linker
        .func_wrap(
            "fe:webgpu",
            "gpu_buffer_create",
            |mut caller: wasmtime::Caller<'_, FakeDevice>, len: i32| -> i32 {
                let dev = caller.data_mut();
                dev.buffers.push(vec![0u32; len as usize]);
                dev.log.push("create");
                (dev.buffers.len() - 1) as i32
            },
        )
        .expect("bind gpu_buffer_create");

    // gpu_upload(src, len, dst): copy `len` words OUT of exported memory at byte
    // offset `src` into buffer `dst`.
    linker
        .func_wrap(
            "fe:webgpu",
            "gpu_upload",
            |mut caller: wasmtime::Caller<'_, FakeDevice>,
             src: i32,
             len: i32,
             dst: i32|
             -> Result<(), wasmtime::Error> {
                let memory = caller
                    .get_export("memory")
                    .and_then(wasmtime::Extern::into_memory)
                    .ok_or_else(|| wasmtime::Error::msg("instance has no exported `memory`"))?;
                let (mem, dev) = memory.data_and_store_mut(&mut caller);
                let words = read_words(mem, src as usize, len as usize);
                let dst = dst as usize;
                if dst >= dev.buffers.len() {
                    return Err(wasmtime::Error::msg("gpu_upload: unknown buffer handle"));
                }
                dev.buffers[dst] = words;
                dev.log.push("upload");
                Ok(())
            },
        )
        .expect("bind gpu_upload");

    // gpu_dispatch(kernel, groups): the pinned-vector table lookup. The single
    // uploaded buffer must equal the pinned NTT-8 input; replace it with the pinned
    // output. Any unrecognized input TRAPS (no field arithmetic here).
    linker
        .func_wrap(
            "fe:webgpu",
            "gpu_dispatch",
            |mut caller: wasmtime::Caller<'_, FakeDevice>,
             _kernel: i32,
             _groups: i32|
             -> Result<(), wasmtime::Error> {
                let dev = caller.data_mut();
                let hit = dev
                    .buffers
                    .iter_mut()
                    .find(|buf| buf.as_slice() == NTT8_INPUT.as_slice());
                match hit {
                    Some(buf) => {
                        *buf = NTT8_OUTPUT.to_vec();
                        dev.log.push("dispatch");
                        Ok(())
                    }
                    None => Err(wasmtime::Error::msg(
                        "gpu_dispatch: no buffer holds the pinned NTT-8 input (unrecognized \
                         kernel input; the stub does no field arithmetic)",
                    )),
                }
            },
        )
        .expect("bind gpu_dispatch");

    // gpu_readback_begin(src, len, dst) -> token: mint a token and RECORD the
    // pending copy WITHOUT touching memory (readback is honestly asynchronous).
    linker
        .func_wrap(
            "fe:webgpu",
            "gpu_readback_begin",
            |mut caller: wasmtime::Caller<'_, FakeDevice>, src: i32, len: i32, dst: i32| -> i32 {
                let dev = caller.data_mut();
                dev.pending.push(PendingReadback {
                    src_buffer: src as usize,
                    len_words: len as usize,
                    dst_addr: dst as usize,
                });
                dev.log.push("readback_begin");
                (dev.pending.len() - 1) as i32
            },
        )
        .expect("bind gpu_readback_begin");

    // wait(token): perform the recorded copy (buffer -> exported memory at dst).
    linker
        .func_wrap(
            "fe:async",
            "wait",
            |mut caller: wasmtime::Caller<'_, FakeDevice>, token: i32| -> Result<(), wasmtime::Error> {
                let memory = caller
                    .get_export("memory")
                    .and_then(wasmtime::Extern::into_memory)
                    .ok_or_else(|| wasmtime::Error::msg("instance has no exported `memory`"))?;
                let (mem, dev) = memory.data_and_store_mut(&mut caller);
                let token = token as usize;
                let pending = dev
                    .pending
                    .get(token)
                    .ok_or_else(|| wasmtime::Error::msg("wait: unknown pending token"))?;
                let words = dev.buffers[pending.src_buffer][..pending.len_words].to_vec();
                write_words(mem, pending.dst_addr, &words);
                dev.log.push("wait");
                Ok(())
            },
        )
        .expect("bind wait");

    let instance = linker
        .instantiate(&mut store, &module)
        .expect("wasmtime should instantiate with the fe:webgpu / fe:async imports satisfied");
    (store, instance)
}

/// R3.4b THE KEYSTONE (LANDED): `main` drives create -> upload -> dispatch ->
/// readback_begin -> wait against the fake device, and the pinned NTT-8 outputs land
/// in wasm linear memory. The op-sequence log proves the exact import op-set was walked.
///
/// Path to green (over R3.4c's exported-param-entry enabler `build_wasm_runtime_package`):
/// (1) the fixture's `with (gpu: WorkerGpu {})` COLON syntax is not with-block grammar
/// (only `Key = value` / bare shorthand parse), so `with` silently fell back to a call
/// `with(...)`; corrected to the supported `with (Dispatch<WebGpuBackend> = WorkerGpu {},
/// ...)` keyed-by-trait form. (2) `MemPtr<u32>` classifies at the extern boundary as
/// `RawAddr { space: Memory }`, not the `Ref { Provider { Memory } }` the WIP admitted;
/// `wasm_lower::ty_for_class` + `runtime::is_wasm_import_boundary_class` corrected. (3)
/// Amendment 4's transport-newtype extension (architect ruling): the raw externs take
/// the single-`u32`-field capability newtypes (`WebGpuRef<u32, Global>`, `KernelId`,
/// `PendingId`) WHOLE, each transported as its one word, so the `WorkerGpu` bodies are
/// pure pass-through with ZERO field reads - no `RExpr::Load`/place needed, staying
/// inside the SSA-value-only wasm model. (4) host-import field NAME is the extern's base
/// op identifier (`mir::wasm_import_name`), decoupled from the mangled Sonatina symbol,
/// and per-effect-scope duplicate import instances dedup to one import per (module, op).
#[test]
fn fe_webgpu_ntt8_runs_on_wasm_fake_device() {
    let wasm = compile_to_wasm("wasm_webgpu_ntt8.fe", WEBGPU_NTT8_SRC);

    // The capability import op-set is on the emitted wasm, module-named per R3.3.
    let imports = func_imports(&wasm);
    for expected in [
        ("fe:webgpu", "gpu_buffer_create"),
        ("fe:webgpu", "gpu_upload"),
        ("fe:webgpu", "gpu_dispatch"),
        ("fe:webgpu", "gpu_readback_begin"),
        ("fe:async", "wait"),
    ] {
        assert!(
            imports.contains(&(expected.0.to_string(), expected.1.to_string())),
            "expected import {expected:?} in the emitted wasm, found {imports:?}"
        );
    }

    let (mut store, instance) = instantiate_fake_device(&wasm);
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("the emitted wasm should export `memory`");

    // Host-chosen regions, clear of the 1024-word bump base: input at byte 4096,
    // output at byte 8192.
    const INPUT_ADDR: i32 = 4096;
    const OUTPUT_ADDR: i32 = 8192;

    // The host writes the 8 input words into wasm memory.
    let mut input_bytes = [0u8; 32];
    for (i, word) in NTT8_INPUT.iter().enumerate() {
        input_bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    memory
        .write(&mut store, INPUT_ADDR as usize, &input_bytes)
        .expect("writing the input words should succeed");

    // Fe drives the whole sequence; `output` is resident when `main` returns.
    let main = instance
        .get_typed_func::<(i32, i32), ()>(&mut store, "main")
        .expect("`main` export should exist");
    main.call(&mut store, (INPUT_ADDR, OUTPUT_ADDR))
        .expect("main(input, output) should run the full dispatch sequence");

    // The 8 output words in wasm memory equal the pinned NTT-8 outputs.
    let mut output_bytes = [0u8; 32];
    memory
        .read(&store, OUTPUT_ADDR as usize, &mut output_bytes)
        .expect("reading the output words should succeed");
    let output: Vec<u32> = (0..8)
        .map(|i| u32::from_le_bytes(output_bytes[i * 4..i * 4 + 4].try_into().unwrap()))
        .collect();
    assert_eq!(
        output,
        NTT8_OUTPUT.to_vec(),
        "the Fe-compiled wasm should land the pinned NTT-8 outputs in linear memory \
         through the capability import table"
    );

    // The op-sequence log is exactly the ratified walk.
    assert_eq!(
        store.data().log,
        vec!["create", "upload", "dispatch", "readback_begin", "wait"],
        "the fake device should service exactly the ratified op-sequence"
    );
}

/// R3.4b twin: the `on_ready` continuation lane. `main_begin` composes WITHOUT
/// `Wait` (create..readback_begin) and returns the `PendingId`; the output region
/// is UNCHANGED until the host drives `on_ready(token)`, which completes the copy
/// with the same token (the async-honesty assert + the continuation re-entry).
///
/// LANDED alongside `fe_webgpu_ntt8_runs_on_wasm_fake_device`: `main_begin` composes
/// WITHOUT `Wait` and returns the token, the output region stays UNCHANGED until the
/// host drives the exported `on_ready(token)` continuation, which completes the copy.
#[test]
fn fe_webgpu_ntt8_on_ready_continuation() {
    let wasm = compile_to_wasm("wasm_webgpu_ntt8.fe", WEBGPU_NTT8_SRC);
    let (mut store, instance) = instantiate_fake_device(&wasm);
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("the emitted wasm should export `memory`");

    const INPUT_ADDR: i32 = 4096;
    const OUTPUT_ADDR: i32 = 8192;

    let mut input_bytes = [0u8; 32];
    for (i, word) in NTT8_INPUT.iter().enumerate() {
        input_bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    memory
        .write(&mut store, INPUT_ADDR as usize, &input_bytes)
        .expect("writing the input words should succeed");

    // main_begin: create -> upload -> dispatch -> readback_begin, returns the token.
    // NO `wait`, so the copy has not happened yet.
    let main_begin = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "main_begin")
        .expect("`main_begin` export should exist");
    let token = main_begin
        .call(&mut store, (INPUT_ADDR, OUTPUT_ADDR))
        .expect("main_begin should run create..readback_begin");

    // Async-honesty: the output region is UNCHANGED before completion.
    let mut before = [0u8; 32];
    memory
        .read(&store, OUTPUT_ADDR as usize, &mut before)
        .expect("reading the output region should succeed");
    assert_eq!(
        before,
        [0u8; 32],
        "readback_begin must NOT touch memory: the output region stays unchanged \
         until the continuation completes"
    );
    assert_eq!(
        store.data().log,
        vec!["create", "upload", "dispatch", "readback_begin"],
        "main_begin should stop at readback_begin (no wait yet)"
    );

    // The host drives the exported continuation with the returned token; on_ready
    // completes the resident copy.
    let on_ready = instance
        .get_typed_func::<i32, ()>(&mut store, "on_ready")
        .expect("`on_ready` export should exist");
    on_ready
        .call(&mut store, token)
        .expect("on_ready(token) should complete the readback");

    // After the continuation, the pinned outputs are resident.
    let mut after = [0u8; 32];
    memory
        .read(&store, OUTPUT_ADDR as usize, &mut after)
        .expect("reading the output region should succeed");
    let output: Vec<u32> = (0..8)
        .map(|i| u32::from_le_bytes(after[i * 4..i * 4 + 4].try_into().unwrap()))
        .collect();
    assert_eq!(
        output,
        NTT8_OUTPUT.to_vec(),
        "on_ready(token) should land the pinned NTT-8 outputs in linear memory"
    );
    assert_eq!(
        store.data().log,
        vec!["create", "upload", "dispatch", "readback_begin", "wait"],
        "on_ready should complete the deferred wait with the right token"
    );
}

/// A two-function call pair compiled Fe -> wasm: `apply` calls `add`.
#[test]
fn fe_call_pair_runs_on_wasm() {
    let source = "pub fn add(a: u64, b: u64) -> u64 { a + b }\n\
                  pub fn apply(a: u64, b: u64) -> u64 { add(a, b) }\n\
                  pub fn main() -> u64 { apply(20, 22) }\n";

    let evm = compile_to_evm("wasm_call_pair.fe", source);
    assert!(!evm.is_empty(), "evm twin bytecode must be non-empty");

    let wasm = compile_to_wasm("wasm_call_pair.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let apply = instance
        .get_typed_func::<(i64, i64), i64>(&mut store, "apply")
        .expect("`apply` export should exist");
    assert_eq!(apply.call(&mut store, (20, 22)).unwrap(), 42);
    assert_eq!(apply.call(&mut store, (2, 3)).unwrap(), 5);
}
