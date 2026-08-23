//! Executable gate for the production sparse BabyBear prover.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WasmCompileOptions, compile_runtime_package_wasm_with_options};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;

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

#[test]
fn production_prover_executes_and_its_canonical_receipt_verifies() {
    let prover_wasm = compile_gate("production_zero_interval_receipt");
    eprintln!("production prover gate: instantiate zero-import Wasm");
    let engine = wasmtime::Engine::default();
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
    assert!(
        pointer > 0,
        "production prover must return owned canonical bytes"
    );
    assert!(
        length > 0 && length % 4 == 0,
        "receipt must be a word stream"
    );

    let prover_memory = prover
        .get_memory(&mut prover_store, "memory")
        .expect("production prover should export memory");
    let mut receipt = vec![0_u8; length as usize];
    prover_memory
        .read(&prover_store, pointer as usize, &mut receipt)
        .expect("canonical receipt must be readable");
    assert_eq!(u32::from_le_bytes(receipt[0..4].try_into().unwrap()), 1);

    let verifier_wasm = compile_gate("verify_production_zero_interval_receipt");
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
        .get_typed_func::<(i32, i32), i32>(
            &mut verifier_store,
            "verify_production_zero_interval_receipt",
        )
        .expect("production receipt verifier export");
    eprintln!("production prover gate: verify and mutate canonical receipt");
    assert_ne!(
        verify
            .call(&mut verifier_store, (verifier_pointer, length))
            .expect("production receipt verifier should execute"),
        0,
        "the emitted receipt must verify through the canonical decoder",
    );

    verifier_memory
        .write(
            &mut verifier_store,
            verifier_pointer as usize,
            &0_u32.to_le_bytes(),
        )
        .expect("receipt mutation should fit memory");
    assert_eq!(
        verify
            .call(&mut verifier_store, (verifier_pointer, length))
            .expect("mutated production receipt verifier should execute"),
        0,
        "changing the receipt validity word must fail closed",
    );
}
