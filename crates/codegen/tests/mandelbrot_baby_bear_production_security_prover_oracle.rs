//! Isolated-process gate for the policy-sized sparse BabyBear receipt.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WasmCompileOptions, compile_prepared_runtime_package_wasm,
    prepare_runtime_package_wasm_with_options,
};
use hir::hir_def::HirIngot;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::Instant;
use url::Url;

const CHILD_MODE: &str = "FE_MANDELBROT_SECURITY_GATE_CHILD";
const RECEIPT_PATH: &str = "FE_MANDELBROT_SECURITY_GATE_RECEIPT";
const GATE: &str = "production_security_prover_executes_and_its_canonical_receipt_verifies";
const LEFT_PROVER_ENTRY: &str = "production_security_zero_interval_receipt";
const RIGHT_PROVER_ENTRY: &str = "production_security_one_interval_receipt";
const VERIFIER_ENTRY: &str = "audit_production_security_zero_interval_receipt_matrix";
const TRACE_WRITER_ENTRY: &str = "production_security_adjacent_interval_task_trace";
const TRACE_REPLAY_ENTRY: &str = "audit_production_security_adjacent_interval_task_trace";
const SECURITY_TASK_COUNT: usize = 120;
const TRACE_HEADER_WORDS: usize = 3;
const TRACE_ROW_WORDS: usize = 2;
const TRACE_BYTES: usize = (TRACE_HEADER_WORDS + TRACE_ROW_WORDS * SECURITY_TASK_COUNT) * 4;

fn compile_gate_with_evidence(entry: &str, evidence_path: Option<&Path>) -> Vec<u8> {
    let started = Instant::now();
    eprintln!("security prover gate: initialize Fe ingot for {entry}");
    let prepared = {
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
        eprintln!("security prover gate: prepare exact Fe entry {entry}");
        let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, entry)
            .unwrap_or_else(|error| panic!("security proof entry {entry}: {error}"));
        prepare_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .unwrap_or_else(|error| panic!("security proof entry {entry} preparation: {error}"))
    };
    eprintln!("security prover gate: emit prepared Fe entry {entry} to Wasm");
    let bytes = compile_prepared_runtime_package_wasm(prepared)
        .unwrap_or_else(|error| panic!("security proof entry {entry} Wasm: {error}"))
        .bytes;
    if let Err(error) = wasmparser::validate(&bytes) {
        if let Some(path) = evidence_path {
            std::fs::write(path, &bytes)
                .expect("invalid security gate Wasm should persist for diagnosis");
        }
        panic!(
            "security prover Wasm should validate: {error:?}; invalid module{}",
            evidence_path
                .map(|path| format!(" persisted at {}", path.display()))
                .unwrap_or_default(),
        );
    }
    // Persist the validated artifact while the compiler database is still
    // alive. Dropping a policy-sized specialization graph can take long enough
    // that an isolated compiler child may be stopped at its memory guard after
    // successful emission but before returning this byte vector to its caller.
    // A `Wasm ready` message therefore means the exact validated module is
    // already durable, rather than merely resident in that compiler process.
    if let Some(path) = evidence_path {
        std::fs::write(path, &bytes)
            .expect("valid security gate Wasm should persist before compiler teardown");
    }
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

fn second_prover_wasm_path(receipt: &Path) -> PathBuf {
    receipt.with_file_name("prover-right.wasm")
}

fn verifier_wasm_path(receipt: &Path) -> PathBuf {
    receipt.with_file_name("verifier.wasm")
}

fn trace_writer_wasm_path(receipt: &Path) -> PathBuf {
    receipt.with_file_name("trace-writer.wasm")
}

fn trace_replay_wasm_path(receipt: &Path) -> PathBuf {
    receipt.with_file_name("trace-replay.wasm")
}

fn task_trace_path(receipt: &Path) -> PathBuf {
    receipt.with_file_name("task-trace.bin")
}

fn second_receipt_path(receipt: &Path) -> PathBuf {
    receipt.with_file_name("receipt-right.bin")
}

fn second_task_trace_path(receipt: &Path) -> PathBuf {
    receipt.with_file_name("task-trace-right.bin")
}

fn compile_gate_to_path(entry: &str, path: &Path) {
    drop(compile_gate_with_evidence(entry, Some(path)));
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

fn produce_canonical_receipt(path: &Path, prover_path: &Path, entry: &str, leaf: u32) {
    let started = Instant::now();
    let engine = wasmtime::Engine::default();
    let wasm = read_compiled_gate(prover_path, "security prover");
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
        .get_typed_func::<(), (i32, i32)>(&mut store, entry)
        .unwrap_or_else(|error| panic!("security receipt export {entry}: {error}"));
    eprintln!("security prover gate: execute Fe leaf {leaf}");
    let (pointer, length) = prove
        .call(&mut store, ())
        .expect("security Fe prover should execute");
    let receipt = read_receipt(&mut store, &instance, pointer, length);
    eprintln!(
        "security prover gate: leaf {leaf} receipt ready ({} bytes, {:.2?})",
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

fn produce_canonical_receipt_trace(path: &Path) {
    let started = Instant::now();
    let engine = wasmtime::Engine::default();
    let wasm = read_compiled_gate(&trace_writer_wasm_path(path), "security trace writer");
    let module =
        wasmtime::Module::new(&engine, wasm).expect("security trace writer module should load");
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("security trace writer Wasm should instantiate");
    let reset = instance
        .get_typed_func::<(), ()>(&mut store, "fe_cabi_reset")
        .expect("security trace writer arena reset export");
    let allocate = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .expect("security trace writer allocator export");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("security trace writer should export memory");
    let write_trace = instance
        .get_typed_func::<(i32, i32, i32), (i32, i32)>(&mut store, TRACE_WRITER_ENTRY)
        .expect("security receipt trace writer export");
    let right_receipt = second_receipt_path(path);
    let right_trace = second_task_trace_path(path);
    for (leaf, receipt_path, trace_path) in [
        (0_i32, path, task_trace_path(path)),
        (1_i32, right_receipt.as_path(), right_trace),
    ] {
        let receipt =
            std::fs::read(receipt_path).expect("prover child must persist its adjacent receipt");
        let length = i32::try_from(receipt.len()).expect("receipt length should fit i32");
        reset
            .call(&mut store, ())
            .expect("security trace writer arena reset should run");
        let pointer = allocate
            .call(&mut store, (length, 4))
            .expect("security trace writer receipt allocation should succeed");
        memory
            .write(&mut store, pointer as usize, &receipt)
            .expect("security receipt must fit trace writer memory");
        let (trace_pointer, trace_length) = write_trace
            .call(&mut store, (leaf, pointer, length))
            .expect("security receipt trace writer should execute");
        assert_eq!(trace_length as usize, TRACE_BYTES);
        let mut trace = vec![0_u8; TRACE_BYTES];
        memory
            .read(&store, trace_pointer as usize, &mut trace)
            .expect("canonical security task trace should be readable");
        let word =
            |index: usize| u32::from_le_bytes(trace[index * 4..index * 4 + 4].try_into().unwrap());
        assert_eq!(word(0), 1, "task trace version");
        assert_eq!(word(1), 1, "task trace validity");
        assert_eq!(word(2), SECURITY_TASK_COUNT as u32, "task trace count");
        for position in 0..SECURITY_TASK_COUNT {
            let base = TRACE_HEADER_WORDS + position * TRACE_ROW_WORDS;
            assert_eq!(word(base), position as u32, "task trace row {position}");
            assert_eq!(word(base + 1), 1, "task trace result {position}");
        }
        std::fs::write(trace_path, trace)
            .expect("canonical task trace should persist between processes");
        eprintln!(
            "security prover gate: leaf {leaf} canonical task trace ready ({TRACE_BYTES} bytes, {:.2?})",
            started.elapsed(),
        );
    }
}

fn verify_canonical_receipt_trace(path: &Path) {
    let started = Instant::now();
    let receipt = std::fs::read(path).expect("prover child must persist its receipt");
    let trace = std::fs::read(task_trace_path(path))
        .expect("trace writer child must persist its canonical trace");
    assert_eq!(trace.len(), TRACE_BYTES);
    let receipt_length = i32::try_from(receipt.len()).expect("receipt length should fit i32");
    let trace_length = i32::try_from(trace.len()).expect("trace length should fit i32");
    let engine = wasmtime::Engine::default();
    let wasm = read_compiled_gate(&trace_replay_wasm_path(path), "security trace replay");
    let module =
        wasmtime::Module::new(&engine, wasm).expect("security trace replay module should load");
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("security trace replay Wasm should instantiate");
    let reset = instance
        .get_typed_func::<(), ()>(&mut store, "fe_cabi_reset")
        .expect("security trace replay arena reset export");
    reset
        .call(&mut store, ())
        .expect("security trace replay arena reset should run");
    let allocate = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .expect("security trace replay allocator export");
    let receipt_pointer = allocate
        .call(&mut store, (receipt_length + 4, 4))
        .expect("security trace replay receipt allocation should succeed");
    let trace_pointer = allocate
        .call(&mut store, (trace_length + 4, 4))
        .expect("security trace replay trace allocation should succeed");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("security trace replay should export memory");
    memory
        .write(&mut store, receipt_pointer as usize, &receipt)
        .expect("security receipt must fit trace replay memory");
    memory
        .write(
            &mut store,
            receipt_pointer as usize + receipt.len(),
            &0_u32.to_le_bytes(),
        )
        .expect("receipt trailing canonicality probe must fit replay memory");
    memory
        .write(&mut store, trace_pointer as usize, &trace)
        .expect("security task trace must fit replay memory");
    memory
        .write(
            &mut store,
            trace_pointer as usize + trace.len(),
            &0_u32.to_le_bytes(),
        )
        .expect("trace trailing canonicality probe must fit replay memory");
    let audit = instance
        .get_typed_func::<(i32, i32, i32, i32, i32), (i32, i32, i32, i32)>(
            &mut store,
            TRACE_REPLAY_ENTRY,
        )
        .expect("security receipt trace replay export");
    let run = |store: &mut wasmtime::Store<()>, leaf: i32, receipt_len: i32, trace_len: i32| {
        audit
            .call(
                store,
                (leaf, receipt_pointer, receipt_len, trace_pointer, trace_len),
            )
            .expect("security receipt trace replay should execute")
    };

    assert_eq!(
        run(&mut store, 0, receipt_length, trace_length),
        (1, 0, 0, 0)
    );
    memory
        .write(&mut store, trace_pointer as usize, &2_u32.to_le_bytes())
        .expect("trace version mutation should fit replay memory");
    assert_eq!(
        run(&mut store, 0, receipt_length, trace_length),
        (0, 0, 0, 0)
    );
    memory
        .write(&mut store, trace_pointer as usize, &1_u32.to_le_bytes())
        .expect("trace version restoration should fit replay memory");
    memory
        .write(&mut store, trace_pointer as usize + 4, &2_u32.to_le_bytes())
        .expect("invalid header boolean should fit replay memory");
    assert_eq!(
        run(&mut store, 0, receipt_length, trace_length),
        (0, 0, 0, 0)
    );
    memory
        .write(&mut store, trace_pointer as usize + 4, &1_u32.to_le_bytes())
        .expect("header boolean restoration should fit replay memory");
    memory
        .write(
            &mut store,
            trace_pointer as usize + 8,
            &((SECURITY_TASK_COUNT - 1) as u32).to_le_bytes(),
        )
        .expect("trace count mutation should fit replay memory");
    assert_eq!(
        run(&mut store, 0, receipt_length, trace_length),
        (0, 0, 0, 0)
    );
    memory
        .write(
            &mut store,
            trace_pointer as usize + 8,
            &(SECURITY_TASK_COUNT as u32).to_le_bytes(),
        )
        .expect("trace count restoration should fit replay memory");
    let first_task = trace_pointer as usize + TRACE_HEADER_WORDS * 4;
    memory
        .write(&mut store, first_task, &6_u32.to_le_bytes())
        .expect("coherent task mutation should fit replay memory");
    assert_eq!(
        run(&mut store, 0, receipt_length, trace_length),
        (1, 1, 0, 0)
    );
    memory
        .write(&mut store, first_task, &0_u32.to_le_bytes())
        .expect("task restoration should fit replay memory");
    memory
        .write(&mut store, first_task + 4, &0_u32.to_le_bytes())
        .expect("stored result mutation should fit replay memory");
    assert_eq!(
        run(&mut store, 0, receipt_length, trace_length),
        (1, 0, 1, 0)
    );
    memory
        .write(&mut store, first_task + 4, &2_u32.to_le_bytes())
        .expect("invalid result boolean should fit replay memory");
    assert_eq!(
        run(&mut store, 0, receipt_length, trace_length),
        (0, 0, 0, 0)
    );
    memory
        .write(&mut store, first_task + 4, &1_u32.to_le_bytes())
        .expect("stored result restoration should fit replay memory");
    memory
        .write(
            &mut store,
            first_task,
            &((SECURITY_TASK_COUNT + 1) as u32).to_le_bytes(),
        )
        .expect("invalid task position should fit replay memory");
    assert_eq!(
        run(&mut store, 0, receipt_length, trace_length),
        (0, 0, 0, 0)
    );
    memory
        .write(&mut store, first_task, &0_u32.to_le_bytes())
        .expect("task position restoration should fit replay memory");
    assert_eq!(
        run(&mut store, 0, receipt_length, trace_length - 4),
        (0, 0, 0, 0),
    );
    assert_eq!(
        run(&mut store, 0, receipt_length, trace_length + 4),
        (0, 0, 0, 0),
    );
    assert_eq!(
        run(&mut store, 0, receipt_length - 4, trace_length),
        (0, 0, 0, 0),
    );
    assert_eq!(
        run(&mut store, 0, receipt_length + 4, trace_length),
        (0, 0, 0, 0),
    );

    let right_receipt = std::fs::read(second_receipt_path(path))
        .expect("prover child must persist its right receipt");
    let right_trace = std::fs::read(second_task_trace_path(path))
        .expect("trace writer child must persist its right canonical trace");
    assert_eq!(right_trace.len(), TRACE_BYTES);
    let right_receipt_length =
        i32::try_from(right_receipt.len()).expect("right receipt length should fit i32");
    let right_trace_length =
        i32::try_from(right_trace.len()).expect("right trace length should fit i32");
    reset
        .call(&mut store, ())
        .expect("security trace replay arena reset should run for the right leaf");
    let right_receipt_pointer = allocate
        .call(&mut store, (right_receipt_length, 4))
        .expect("right receipt allocation should succeed");
    let right_trace_pointer = allocate
        .call(&mut store, (right_trace_length, 4))
        .expect("right trace allocation should succeed");
    memory
        .write(&mut store, right_receipt_pointer as usize, &right_receipt)
        .expect("right receipt must fit trace replay memory");
    memory
        .write(&mut store, right_trace_pointer as usize, &right_trace)
        .expect("right trace must fit replay memory");
    assert_eq!(
        audit
            .call(
                &mut store,
                (
                    1,
                    right_receipt_pointer,
                    right_receipt_length,
                    right_trace_pointer,
                    right_trace_length,
                ),
            )
            .expect("right security receipt trace replay should execute"),
        (1, 0, 0, 0),
    );
    let cross_leaf = audit
        .call(
            &mut store,
            (
                0,
                right_receipt_pointer,
                right_receipt_length,
                right_trace_pointer,
                right_trace_length,
            ),
        )
        .expect("cross-leaf security trace replay should execute");
    assert_eq!(cross_leaf.0, 1, "cross-leaf trace remains canonical");
    assert_eq!(cross_leaf.1, 0, "the verifier task topology is shared");
    assert!(
        cross_leaf.2 > 0 && cross_leaf.3 > 0,
        "a right receipt and trace cannot verify as the left public interval",
    );
    assert_eq!(
        audit
            .call(
                &mut store,
                (
                    2,
                    right_receipt_pointer,
                    right_receipt_length,
                    right_trace_pointer,
                    right_trace_length,
                ),
            )
            .expect("out-of-range leaf replay should execute"),
        (0, 0, 0, 0),
    );
    eprintln!(
        "security prover gate: adjacent canonical task trace replay finished ({:.2?})",
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
        "compile-left-prover" => compile_gate_to_path(LEFT_PROVER_ENTRY, &prover_wasm_path(&path)),
        "prove-left" => {
            produce_canonical_receipt(&path, &prover_wasm_path(&path), LEFT_PROVER_ENTRY, 0)
        }
        "compile-right-prover" => {
            compile_gate_to_path(RIGHT_PROVER_ENTRY, &second_prover_wasm_path(&path))
        }
        "prove-right" => produce_canonical_receipt(
            &second_receipt_path(&path),
            &second_prover_wasm_path(&path),
            RIGHT_PROVER_ENTRY,
            1,
        ),
        "compile-verifier" => compile_gate_to_path(VERIFIER_ENTRY, &verifier_wasm_path(&path)),
        "verify" => verify_canonical_receipt(&path),
        "compile-trace-writer" => {
            compile_gate_to_path(TRACE_WRITER_ENTRY, &trace_writer_wasm_path(&path))
        }
        "write-trace" => produce_canonical_receipt_trace(&path),
        "compile-trace-replay" => {
            compile_gate_to_path(TRACE_REPLAY_ENTRY, &trace_replay_wasm_path(&path))
        }
        "verify-trace" => verify_canonical_receipt_trace(&path),
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

    for mode in [
        "compile-left-prover",
        "prove-left",
        "compile-right-prover",
        "prove-right",
        "compile-verifier",
        "verify",
        "compile-trace-writer",
        "write-trace",
        "compile-trace-replay",
        "verify-trace",
    ] {
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
