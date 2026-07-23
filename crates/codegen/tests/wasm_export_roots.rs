//! R3.4c enabler-proof (wasm-worker/WebGPU interop doc section 9): the wasm
//! backend admits value-param-carrying `pub` top-level functions as export
//! ROOTS.
//!
//! A module whose ONLY functions are param-carrying `pub fn`s (no zero-param
//! `main`) yields an EMPTY runtime package under the EVM-style root rule
//! (`runtime_root_candidate` rejects any param-carrying function as `NotRoot`,
//! so with no zero-param seed the package is empty and the emitted wasm has no
//! functions). `build_wasm_runtime_package` admits such functions as roots and
//! synthesizes NO wrapper: the lowered entries ARE the exports. These tests
//! prove the enabler with R1-level scalars only, independent of the R3.4b WIP.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

fn exports(bytes: &[u8]) -> Vec<(String, wasmparser::ExternalKind)> {
    let mut exports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let wasmparser::Payload::ExportSection(reader) = payload.unwrap() {
            for export in reader {
                let export = export.unwrap();
                exports.push((export.name.to_owned(), export.kind));
            }
        }
    }
    exports.sort_by(|left, right| left.0.cmp(&right.0));
    exports
}

/// Compile Fe source to wasm bytes through the wasm backend.
fn compile_to_wasm(name: &str, source: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .unwrap_or_else(|err| panic!("wasm compilation of `{name}` failed: {err}"))
        .into_bytecode()
        .expect("wasm output should be bytecode")
}

/// Compile Fe source through the wasm backend, expecting a fail-closed error.
fn compile_to_wasm_err(name: &str, source: &str) -> String {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect_err("compilation should fail closed")
        .to_string()
}

/// THE R3.4c ENABLER: a module whose only entries are param-carrying `pub` fns
/// produces a NON-empty wasm module, and each function is exported and callable
/// with its value parameters. This exact shape yields an EMPTY package under
/// the EVM-style zero-param root rule.
#[test]
fn wasm_param_carrying_pub_fns_are_export_roots() {
    // No zero-param `main`: the only entries are value-param `pub` fns.
    let source = "fn inc(x: u64) -> u64 { x + 1 }\n\
                  pub fn double(x: u64) -> u64 { x + x }\n\
                  pub fn add(a: u64, b: u64) -> u64 { inc(a + b - 1) }\n";

    let wasm = compile_to_wasm("wasm_export_roots.fe", source);
    assert!(!wasm.is_empty(), "the wasm module must be non-empty");
    assert_eq!(&wasm[..4], b"\0asm", "output must be a wasm module");
    wasmparser::validate(&wasm).expect("produced invalid wasm");
    assert_eq!(
        exports(&wasm),
        vec![
            ("add".to_owned(), wasmparser::ExternalKind::Func),
            ("double".to_owned(), wasmparser::ExternalKind::Func),
            ("memory".to_owned(), wasmparser::ExternalKind::Memory),
        ],
        "reachable private helpers remain callable definitions, not host ABI exports",
    );

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");

    // `double` is a root purely because it is a `pub` param-carrying entry: the
    // host calls it directly with its value parameter.
    let double = instance
        .get_typed_func::<i64, i64>(&mut store, "double")
        .expect("`double` should be exported as an export root");
    assert_eq!(double.call(&mut store, 21).unwrap(), 42, "double(21) == 42");
    assert_eq!(double.call(&mut store, 0).unwrap(), 0);

    // The second param-carrying `pub` fn is admitted as a root too (all `pub`
    // top-level fns are roots, name-sorted).
    let add = instance
        .get_typed_func::<(i64, i64), i64>(&mut store, "add")
        .expect("`add` should be exported as an export root");
    assert_eq!(
        add.call(&mut store, (20, 22)).unwrap(),
        42,
        "add(20, 22) == 42"
    );
}

/// The wasm backend fails closed on contracts (interop doc 9.2): the wasm root
/// path has no contract lowering, so a module with a contract is rejected
/// rather than given silent EVM-shaped behavior.
#[test]
fn wasm_contract_module_fails_closed() {
    let source = "contract C {}\n";
    let message = compile_to_wasm_err("wasm_contract.fe", source);
    assert!(
        message.contains("does not support contracts"),
        "unexpected error: {message}"
    );
}

/// R3.4c fail-closed (interop doc 9.3): a `pub` export root with a SURVIVING
/// (non-erased) effect binding is rejected with the named diagnostic. The
/// `uses (p: mut MemPtr<Foo>)` handle effect resolves against the ambient host
/// root as a NON-zero-sized, memory-materialized provider, so its synthesized
/// effect arg survives erasure. A wasm export root's host-visible signature is
/// exactly its value params; a surviving effect param would be an extra
/// argument the host cannot supply, so it must fail closed rather than export a
/// mis-shaped entry.
#[test]
fn wasm_surviving_effect_root_fails_closed() {
    let source = "use core::MemPtr\n\
                  struct Foo { a: u256, b: u256 }\n\
                  pub fn exported() uses (p: mut MemPtr<Foo>) {\n\
                  \x20   let _r = p.raw()\n\
                  }\n";
    let message = compile_to_wasm_err("wasm_surviving_effect.fe", source);
    assert!(
        message.contains("surviving (non-erased) effect parameter")
            && message.contains("fully-erased effect parameters"),
        "expected the named surviving-effect diagnostic, got: {message}"
    );
}
