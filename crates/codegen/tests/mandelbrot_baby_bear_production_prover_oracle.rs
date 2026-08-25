//! Executable gate for the production sparse BabyBear prover.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WasmCompileOptions, compile_runtime_package_wasm_with_options};
use hir::hir_def::HirIngot;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use url::Url;

const CHILD_MODE: &str = "FE_MANDELBROT_PRODUCTION_GATE_CHILD";
const RECEIPT_PATH: &str = "FE_MANDELBROT_PRODUCTION_GATE_RECEIPT";
const SINGLE_RECEIPT_GATE: &str = "production_prover_executes_and_its_canonical_receipt_verifies";
const ADJACENT_RECEIPT_GATE: &str =
    "production_adjacent_receipts_merge_through_recursive_authority";

fn compile_gate(entry: &str) -> Vec<u8> {
    eprintln!("production prover gate: initialize Fe ingot for {entry}");
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_baby_bear_production_prover_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "production prover fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("production prover fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected production prover diagnostics:\n{diagnostics}",
    );
    eprintln!("production prover gate: lower exact Fe entry {entry} to Wasm");
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, entry)
        .unwrap_or_else(|error| panic!("production proof entry {entry}: {error}"));
    let bytes =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .unwrap_or_else(|error| panic!("production proof entry {entry} Wasm: {error}"))
            .bytes;
    wasmparser::validate(&bytes).expect("production prover Wasm should validate");
    eprintln!(
        "production prover gate: {entry} Wasm ready ({} bytes)",
        bytes.len(),
    );
    bytes
}

#[test]
fn production_base_lde_executes_in_its_arena_owned_workspace() {
    let wasm = compile_gate("production_base_lde_checkpoint");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).expect("base LDE Wasm module should load");
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("base LDE Wasm should instantiate");
    let checkpoint = instance
        .get_typed_func::<(), i32>(&mut store, "production_base_lde_checkpoint")
        .expect("production base LDE checkpoint export");
    assert_ne!(
        checkpoint
            .call(&mut store, ())
            .expect("production base LDE checkpoint should execute"),
        0,
    );
}

#[test]
fn production_composition_opening_executes_with_compact_local_storage() {
    let wasm = compile_gate("production_composition_opening_checkpoint");
    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, wasm).expect("composition opening Wasm module should load");
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("composition opening Wasm should instantiate");
    let checkpoint = instance
        .get_typed_func::<(), i32>(&mut store, "production_composition_opening_checkpoint")
        .expect("production composition opening checkpoint export");
    assert_ne!(
        checkpoint
            .call(&mut store, ())
            .expect("production composition opening should execute"),
        0,
    );
}

fn second_receipt_path(first: &Path) -> PathBuf {
    first.with_file_name("receipt-right.bin")
}

fn read_canonical_receipt(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    pointer: i32,
    length: i32,
) -> Vec<u8> {
    assert!(pointer > 0, "production prover must return owned bytes");
    assert!(
        length > 0 && length % 4 == 0,
        "receipt must be a word stream",
    );
    let memory = instance
        .get_memory(&mut *store, "memory")
        .expect("production prover should export memory");
    let mut receipt = vec![0_u8; length as usize];
    memory
        .read(&*store, pointer as usize, &mut receipt)
        .expect("canonical receipt must be readable");
    assert_eq!(u32::from_le_bytes(receipt[0..4].try_into().unwrap()), 1);
    receipt
}

fn produce_canonical_receipt(path: &Path) {
    let engine = wasmtime::Engine::default();
    let prover_wasm = compile_gate("production_zero_interval_receipt");
    eprintln!("production prover gate: instantiate zero-import Wasm");
    let prover_module =
        wasmtime::Module::new(&engine, prover_wasm).expect("prover Wasm module should load");
    assert!(prover_module.imports().next().is_none());
    let mut prover_store = wasmtime::Store::new(&engine, ());
    let prover = wasmtime::Instance::new(&mut prover_store, &prover_module, &[])
        .expect("production prover Wasm should instantiate");

    prover
        .get_typed_func::<(), ()>(&mut prover_store, "fe_cabi_reset")
        .expect("canonical arena reset export")
        .call(&mut prover_store, ())
        .expect("canonical arena reset should run");
    let prove = prover
        .get_typed_func::<(), (i32, i32)>(&mut prover_store, "production_zero_interval_receipt")
        .expect("production receipt export");
    eprintln!("production prover gate: execute Fe prover");
    let (pointer, length) = prove
        .call(&mut prover_store, ())
        .expect("production Fe prover should execute");
    eprintln!("production prover gate: receipt ready at {pointer} ({length} bytes)",);
    let receipt = read_canonical_receipt(&mut prover_store, &prover, pointer, length);
    std::fs::write(path, receipt).expect("canonical receipt should persist between gate processes");
}

fn produce_adjacent_canonical_receipts(first_path: &Path) {
    let engine = wasmtime::Engine::default();
    let prover_wasm = compile_gate("production_zero_adjacent_interval_receipt");
    eprintln!("production prover gate: instantiate adjacent zero-import Wasm");
    let prover_module =
        wasmtime::Module::new(&engine, prover_wasm).expect("adjacent prover Wasm should load");
    assert!(prover_module.imports().next().is_none());
    let mut prover_store = wasmtime::Store::new(&engine, ());
    let prover = wasmtime::Instance::new(&mut prover_store, &prover_module, &[])
        .expect("adjacent production prover Wasm should instantiate");
    let reset = prover
        .get_typed_func::<(), ()>(&mut prover_store, "fe_cabi_reset")
        .expect("adjacent prover canonical arena reset export");
    let prove = prover
        .get_typed_func::<i32, (i32, i32)>(
            &mut prover_store,
            "production_zero_adjacent_interval_receipt",
        )
        .expect("adjacent production receipt export");

    let right_path = second_receipt_path(first_path);
    for (leaf, path) in [(0_i32, first_path), (1_i32, right_path.as_path())] {
        reset
            .call(&mut prover_store, ())
            .expect("adjacent prover canonical arena reset should run");
        eprintln!("production prover gate: execute adjacent Fe leaf {leaf}");
        let (pointer, length) = prove
            .call(&mut prover_store, leaf)
            .expect("adjacent production Fe prover should execute");
        eprintln!(
            "production prover gate: adjacent leaf {leaf} receipt ready at {pointer} ({length} bytes)",
        );
        let receipt = read_canonical_receipt(&mut prover_store, &prover, pointer, length);
        std::fs::write(path, receipt)
            .expect("adjacent canonical receipt should persist between gate processes");
    }
}

fn verify_canonical_receipt_entry(path: &Path, entry: &str, zero_means_valid: bool) {
    let receipt = std::fs::read(path).expect("prover child must persist its canonical receipt");
    let length = i32::try_from(receipt.len()).expect("canonical receipt length should fit i32");
    let engine = wasmtime::Engine::default();
    let verifier_wasm = compile_gate(entry);
    let verifier_module =
        wasmtime::Module::new(&engine, verifier_wasm).expect("verifier Wasm module should load");
    assert!(verifier_module.imports().next().is_none());
    let mut verifier_store = wasmtime::Store::new(&engine, ());
    let verifier = wasmtime::Instance::new(&mut verifier_store, &verifier_module, &[])
        .expect("production verifier Wasm should instantiate");
    verifier
        .get_typed_func::<(), ()>(&mut verifier_store, "fe_cabi_reset")
        .expect("verifier canonical arena reset export")
        .call(&mut verifier_store, ())
        .expect("verifier canonical arena reset should run");
    let verifier_pointer = verifier
        .get_typed_func::<(i32, i32), i32>(&mut verifier_store, "fe_cabi_alloc")
        .expect("verifier canonical allocator export")
        .call(&mut verifier_store, (length, 4))
        .expect("verifier receipt allocation should succeed");
    let verifier_memory = verifier
        .get_memory(&mut verifier_store, "memory")
        .expect("production verifier should export memory");
    verifier_memory
        .write(&mut verifier_store, verifier_pointer as usize, &receipt)
        .expect("canonical receipt must fit verifier memory");
    let verify = verifier
        .get_typed_func::<(i32, i32), i32>(&mut verifier_store, entry)
        .expect("production receipt verifier export");
    eprintln!("production prover gate: verify and mutate canonical receipt");
    let clean = verify
        .call(&mut verifier_store, (verifier_pointer, length))
        .expect("production receipt verifier should execute");
    if zero_means_valid {
        assert_eq!(
            clean, 0,
            "the emitted receipt must verify and mint the exact recursive leaf authority",
        );
    } else {
        assert_ne!(
            clean, 0,
            "the emitted receipt must pass the direct production verifier",
        );
    }

    verifier_memory
        .write(
            &mut verifier_store,
            verifier_pointer as usize,
            &0_u32.to_le_bytes(),
        )
        .expect("receipt mutation should fit memory");
    let mutated = verify
        .call(&mut verifier_store, (verifier_pointer, length))
        .expect("mutated production receipt verifier should execute");
    if zero_means_valid {
        assert_ne!(
            mutated, 0,
            "changing the receipt validity word must fail closed",
        );
    } else {
        assert_eq!(
            mutated, 0,
            "changing the receipt validity word must fail closed",
        );
    }
}

fn verify_canonical_receipt(path: &Path) {
    verify_canonical_receipt_entry(
        path,
        "audit_production_zero_interval_receipt_matrix",
        true,
    );
}

fn verify_direct_canonical_receipt(path: &Path) {
    verify_canonical_receipt_entry(path, "verify_production_zero_interval_receipt", false);
}

fn write_receipt_to_verifier(
    store: &mut wasmtime::Store<()>,
    verifier: &wasmtime::Instance,
    receipt: &[u8],
) -> (i32, i32) {
    let length = i32::try_from(receipt.len()).expect("canonical receipt length should fit i32");
    let pointer = verifier
        .get_typed_func::<(i32, i32), i32>(&mut *store, "fe_cabi_alloc")
        .expect("verifier canonical allocator export")
        .call(&mut *store, (length, 4))
        .expect("verifier receipt allocation should succeed");
    verifier
        .get_memory(&mut *store, "memory")
        .expect("production verifier should export memory")
        .write(&mut *store, pointer as usize, receipt)
        .expect("canonical receipt must fit verifier memory");
    (pointer, length)
}

fn verify_adjacent_canonical_receipts(first_path: &Path) {
    let left_receipt =
        std::fs::read(first_path).expect("adjacent prover child must persist its left receipt");
    let right_receipt = std::fs::read(second_receipt_path(first_path))
        .expect("adjacent prover child must persist its right receipt");
    let engine = wasmtime::Engine::default();
    let entry = "audit_production_zero_adjacent_recursive_merge";
    let verifier_wasm = compile_gate(entry);
    let verifier_module = wasmtime::Module::new(&engine, verifier_wasm)
        .expect("adjacent recursive verifier Wasm module should load");
    assert!(verifier_module.imports().next().is_none());
    let mut verifier_store = wasmtime::Store::new(&engine, ());
    let verifier = wasmtime::Instance::new(&mut verifier_store, &verifier_module, &[])
        .expect("adjacent recursive verifier Wasm should instantiate");
    verifier
        .get_typed_func::<(), ()>(&mut verifier_store, "fe_cabi_reset")
        .expect("adjacent verifier canonical arena reset export")
        .call(&mut verifier_store, ())
        .expect("adjacent verifier canonical arena reset should run");
    let (left_pointer, left_length) =
        write_receipt_to_verifier(&mut verifier_store, &verifier, &left_receipt);
    let (right_pointer, right_length) =
        write_receipt_to_verifier(&mut verifier_store, &verifier, &right_receipt);
    let audit = verifier
        .get_typed_func::<(i32, i32, i32, i32), i32>(&mut verifier_store, entry)
        .expect("adjacent recursive authority audit export");

    eprintln!("production prover gate: verify and merge adjacent recursive authorities");
    assert_eq!(
        audit
            .call(
                &mut verifier_store,
                (left_pointer, left_length, right_pointer, right_length),
            )
            .expect("adjacent recursive authority audit should execute"),
        0,
        "two real adjacent receipts must mint and merge into the exact two-leaf authority",
    );
    assert_ne!(
        audit
            .call(
                &mut verifier_store,
                (left_pointer, left_length, left_pointer, left_length),
            )
            .expect("duplicate recursive authority audit should execute"),
        0,
        "one verified receipt must not be duplicated into two adjacent leaves",
    );
    assert_ne!(
        audit
            .call(
                &mut verifier_store,
                (right_pointer, right_length, left_pointer, left_length),
            )
            .expect("swapped recursive authority audit should execute"),
        0,
        "verified receipt order must remain part of recursive authority",
    );

    verifier
        .get_memory(&mut verifier_store, "memory")
        .expect("adjacent production verifier should export memory")
        .write(
            &mut verifier_store,
            right_pointer as usize,
            &0_u32.to_le_bytes(),
        )
        .expect("adjacent receipt mutation should fit verifier memory");
    assert_ne!(
        audit
            .call(
                &mut verifier_store,
                (left_pointer, left_length, right_pointer, right_length),
            )
            .expect("mutated adjacent recursive authority audit should execute"),
        0,
        "a mutated adjacent receipt must fail before recursive merge",
    );
}

fn child_receipt_path() -> PathBuf {
    std::env::var_os(RECEIPT_PATH)
        .map(PathBuf::from)
        .expect("production gate child requires its receipt path")
}

fn run_requested_child() -> bool {
    let Ok(mode) = std::env::var(CHILD_MODE) else {
        return false;
    };
    let path = child_receipt_path();
    match mode.as_str() {
        "prove" => produce_canonical_receipt(&path),
        "verify" => verify_canonical_receipt(&path),
        "verify-direct" => verify_direct_canonical_receipt(&path),
        "prove-adjacent" => produce_adjacent_canonical_receipts(&path),
        "verify-adjacent" => verify_adjacent_canonical_receipts(&path),
        _ => panic!("unknown production gate child mode {mode}"),
    }
    true
}

fn run_gate_child(test_name: &str, mode: &str, receipt_path: &Path) -> ExitStatus {
    Command::new(std::env::current_exe().expect("current test executable"))
        .arg(test_name)
        .arg("--exact")
        .arg("--nocapture")
        .arg("--quiet")
        .env(CHILD_MODE, mode)
        .env(RECEIPT_PATH, receipt_path)
        .status()
        .unwrap_or_else(|error| panic!("production gate {mode} child should start: {error}"))
}

#[test]
fn production_prover_executes_and_its_canonical_receipt_verifies() {
    if run_requested_child() {
        return;
    }

    // Compiler databases retain large allocation arenas for their process
    // lifetime. Run proving and verification as separate test processes so the
    // operating system reclaims the prover arena before verifier lowering.
    let scratch_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fe-test-scratch");
    std::fs::create_dir_all(&scratch_root).expect("workspace test scratch directory");
    let scratch = tempfile::Builder::new()
        .prefix("production-proof-gate-")
        .tempdir_in(scratch_root)
        .expect("workspace-backed production gate scratch directory");
    let receipt_path = scratch.path().join("receipt.bin");

    let prove_status = run_gate_child(SINGLE_RECEIPT_GATE, "prove", &receipt_path);
    if !prove_status.success() {
        let retained = scratch.keep();
        panic!(
            "production gate prove child failed; retained evidence at {}",
            retained.display(),
        );
    }
    let verify_status = run_gate_child(SINGLE_RECEIPT_GATE, "verify", &receipt_path);
    if !verify_status.success() {
        let retained = scratch.keep();
        panic!(
            "production gate verify child failed; retained evidence at {}",
            retained.display(),
        );
    }
}

#[test]
fn production_adjacent_receipts_merge_through_recursive_authority() {
    if run_requested_child() {
        return;
    }

    let scratch_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fe-test-scratch");
    std::fs::create_dir_all(&scratch_root).expect("workspace test scratch directory");
    let scratch = tempfile::Builder::new()
        .prefix("production-adjacent-proof-gate-")
        .tempdir_in(scratch_root)
        .expect("workspace-backed adjacent proof gate scratch directory");
    let receipt_path = scratch.path().join("receipt-left.bin");

    let prove_status = run_gate_child(ADJACENT_RECEIPT_GATE, "prove-adjacent", &receipt_path);
    if !prove_status.success() {
        let retained = scratch.keep();
        panic!(
            "adjacent production gate prove child failed; retained evidence at {}",
            retained.display(),
        );
    }
    let verify_status = run_gate_child(ADJACENT_RECEIPT_GATE, "verify-adjacent", &receipt_path);
    if !verify_status.success() {
        let retained = scratch.keep();
        panic!(
            "adjacent production gate verify child failed; retained evidence at {}",
            retained.display(),
        );
    }
}
