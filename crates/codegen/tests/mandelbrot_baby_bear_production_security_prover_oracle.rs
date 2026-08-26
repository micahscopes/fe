//! Isolated-process gate for the policy-sized sparse BabyBear receipt.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WasmCompileOptions, compile_runtime_package_wasm_with_options};
use hir::hir_def::HirIngot;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::Instant;
use url::Url;

const CHILD_MODE: &str = "FE_MANDELBROT_SECURITY_GATE_CHILD";
const RECEIPT_PATH: &str = "FE_MANDELBROT_SECURITY_GATE_RECEIPT";
const GATE: &str = "production_security_prover_executes_and_its_canonical_receipt_verifies";
const PROVER_ENTRY: &str = "production_security_zero_interval_receipt";
const VERIFIER_ENTRY: &str = "audit_production_security_zero_interval_receipt_matrix";

fn compile_gate(entry: &str) -> Vec<u8> {
    let started = Instant::now();
    eprintln!("security prover gate: initialize Fe ingot for {entry}");
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_baby_bear_production_security_prover_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "security prover fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("security prover fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected security prover diagnostics:\n{diagnostics}",
    );
    eprintln!("security prover gate: lower exact Fe entry {entry} to Wasm");
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, entry)
        .unwrap_or_else(|error| panic!("security proof entry {entry}: {error}"));
    let bytes =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .unwrap_or_else(|error| panic!("security proof entry {entry} Wasm: {error}"))
            .bytes;
    wasmparser::validate(&bytes).expect("security prover Wasm should validate");
    eprintln!(
        "security prover gate: {entry} Wasm ready ({} bytes, {:.2?})",
        bytes.len(),
        started.elapsed(),
    );
    bytes
}

fn prover_wasm_path(receipt: &Path) -> PathBuf {
    receipt.with_file_name("prover.wasm")
}

fn verifier_wasm_path(receipt: &Path) -> PathBuf {
    receipt.with_file_name("verifier.wasm")
}

fn compile_gate_to_path(entry: &str, path: &Path) {
    std::fs::write(path, compile_gate(entry))
        .expect("compiled security gate Wasm should persist between processes");
}

fn read_compiled_gate(path: &Path, role: &str) -> Vec<u8> {
    let bytes = std::fs::read(path)
        .unwrap_or_else(|error| panic!("{role} compiler child must persist Wasm: {error}"));
    wasmparser::validate(&bytes)
        .unwrap_or_else(|error| panic!("persisted {role} Wasm should validate: {error}"));
    bytes
}

fn read_receipt(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    pointer: i32,
    length: i32,
) -> Vec<u8> {
    assert!(pointer > 0, "security prover must return owned bytes");
    assert!(
        length > 0 && length % 4 == 0,
        "receipt must be a word stream"
    );
    let memory = instance
        .get_memory(&mut *store, "memory")
        .expect("security prover should export memory");
    let mut receipt = vec![0_u8; length as usize];
    memory
        .read(&*store, pointer as usize, &mut receipt)
        .expect("canonical security receipt must be readable");
    assert_eq!(u32::from_le_bytes(receipt[0..4].try_into().unwrap()), 1);
    receipt
}

fn produce_canonical_receipt(path: &Path) {
    let started = Instant::now();
    let engine = wasmtime::Engine::default();
    let wasm = read_compiled_gate(&prover_wasm_path(path), "security prover");
    let module = wasmtime::Module::new(&engine, wasm).expect("security prover module should load");
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("security prover Wasm should instantiate");
    instance
        .get_typed_func::<(), ()>(&mut store, "fe_cabi_reset")
        .expect("canonical arena reset export")
        .call(&mut store, ())
        .expect("canonical arena reset should run");
    let prove = instance
        .get_typed_func::<(), (i32, i32)>(&mut store, PROVER_ENTRY)
        .expect("security receipt export");
    eprintln!("security prover gate: execute Fe prover");
    let (pointer, length) = prove
        .call(&mut store, ())
        .expect("security Fe prover should execute");
    let receipt = read_receipt(&mut store, &instance, pointer, length);
    eprintln!(
        "security prover gate: receipt ready ({} bytes, {:.2?})",
        receipt.len(),
        started.elapsed(),
    );
    std::fs::write(path, receipt).expect("security receipt should persist between processes");
}

fn verify_canonical_receipt(path: &Path) {
    let started = Instant::now();
    let receipt = std::fs::read(path).expect("prover child must persist its receipt");
    let length = i32::try_from(receipt.len()).expect("receipt length should fit i32");
    let engine = wasmtime::Engine::default();
    let wasm = read_compiled_gate(&verifier_wasm_path(path), "security verifier");
    let module =
        wasmtime::Module::new(&engine, wasm).expect("security verifier module should load");
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("security verifier Wasm should instantiate");
    instance
        .get_typed_func::<(), ()>(&mut store, "fe_cabi_reset")
        .expect("security verifier arena reset export")
        .call(&mut store, ())
        .expect("security verifier arena reset should run");
    let pointer = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .expect("security verifier allocator export")
        .call(&mut store, (length + 4, 4))
        .expect("security verifier receipt allocation should succeed");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("security verifier should export memory");
    memory
        .write(&mut store, pointer as usize, &receipt)
        .expect("security receipt must fit verifier memory");
    memory
        .write(
            &mut store,
            pointer as usize + receipt.len(),
            &0_u32.to_le_bytes(),
        )
        .expect("trailing canonicality probe must fit verifier memory");
    let verify = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, VERIFIER_ENTRY)
        .expect("security receipt verifier export");

    assert_eq!(
        verify
            .call(&mut store, (pointer, length))
            .expect("security receipt verifier should execute"),
        0,
        "the policy-sized receipt and complete typed mutation matrix must pass",
    );
    memory
        .write(&mut store, pointer as usize, &0_u32.to_le_bytes())
        .expect("validity mutation should fit verifier memory");
    assert_ne!(
        verify
            .call(&mut store, (pointer, length))
            .expect("mutated security receipt verifier should execute"),
        0,
        "changing the receipt validity word must fail closed",
    );
    memory
        .write(&mut store, pointer as usize, &receipt[0..4])
        .expect("validity restoration should fit verifier memory");
    assert_ne!(
        verify
            .call(&mut store, (pointer, length - 4))
            .expect("truncated security receipt verifier should execute"),
        0,
        "truncated canonical bytes must fail closed",
    );
    assert_ne!(
        verify
            .call(&mut store, (pointer, length + 4))
            .expect("trailing security receipt verifier should execute"),
        0,
        "trailing canonical bytes must fail closed",
    );
    eprintln!(
        "security prover gate: clean and mutation verification finished ({:.2?})",
        started.elapsed(),
    );
}

fn child_receipt_path() -> PathBuf {
    std::env::var_os(RECEIPT_PATH)
        .map(PathBuf::from)
        .expect("security gate child requires its receipt path")
}

fn run_requested_child() -> bool {
    let Ok(mode) = std::env::var(CHILD_MODE) else {
        return false;
    };
    let path = child_receipt_path();
    match mode.as_str() {
        "compile-prover" => compile_gate_to_path(PROVER_ENTRY, &prover_wasm_path(&path)),
        "prove" => produce_canonical_receipt(&path),
        "compile-verifier" => compile_gate_to_path(VERIFIER_ENTRY, &verifier_wasm_path(&path)),
        "verify" => verify_canonical_receipt(&path),
        _ => panic!("unknown security gate child mode {mode}"),
    }
    true
}

fn run_gate_child(mode: &str, receipt_path: &Path) -> ExitStatus {
    Command::new(std::env::current_exe().expect("current test executable"))
        .arg(GATE)
        .arg("--exact")
        .arg("--nocapture")
        .arg("--quiet")
        .env(CHILD_MODE, mode)
        .env(RECEIPT_PATH, receipt_path)
        .status()
        .unwrap_or_else(|error| panic!("security gate {mode} child should start: {error}"))
}

#[test]
fn production_security_prover_executes_and_its_canonical_receipt_verifies() {
    if run_requested_child() {
        return;
    }

    // Compiler databases and Wasmtime retain large arenas. Compile, execute,
    // and verify in separate processes so their peak residency cannot overlap.
    let scratch_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fe-test-scratch");
    std::fs::create_dir_all(&scratch_root).expect("workspace test scratch directory");
    let scratch = tempfile::Builder::new()
        .prefix("production-security-proof-gate-")
        .tempdir_in(scratch_root)
        .expect("workspace-backed security gate scratch directory");
    let receipt_path = scratch.path().join("receipt.bin");

    for mode in ["compile-prover", "prove", "compile-verifier", "verify"] {
        let status = run_gate_child(mode, &receipt_path);
        if !status.success() {
            let retained = scratch.keep();
            panic!(
                "security gate {mode} child failed; retained evidence at {}",
                retained.display(),
            );
        }
    }
}
