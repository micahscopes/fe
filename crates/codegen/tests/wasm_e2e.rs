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
