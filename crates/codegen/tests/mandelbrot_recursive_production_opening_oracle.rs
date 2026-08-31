//! Process-isolated gate for production opening relations over a retained receipt.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WasmCompileOptions, compile_prepared_runtime_package_wasm,
    prepare_runtime_package_wasm_with_options,
};
use hir::hir_def::HirIngot;
use p3_baby_bear::{BabyBear, default_babybear_poseidon2_16};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_symmetric::Permutation;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::Instant;
use url::Url;
use wasmtime::Val;

const CHILD_MODE: &str = "FE_MANDELBROT_RECURSIVE_OPENING_CHILD";
const RECEIPT_PATH: &str = "MB2_PRODUCTION_SECURITY_RECEIPT";
const TRACE_PATH: &str = "MB2_PRODUCTION_SECURITY_TRACE";
const WASM_PATH: &str = "FE_MANDELBROT_RECURSIVE_OPENING_WASM";
const GATE: &str = "retained_production_receipt_authenticates_both_opening_roles";
const ENTRY: &str = "audit_retained_production_opening_relations";
const RECURSIVE_ENTRY: &str = "audit_retained_production_recursive_transcript_relation";
const RECEIPT_BYTES: usize = 948_808;
const RECEIPT_SHA256: &str = "c789a067f63b4ab73d8a4c0b36932e4252b6270b0be3e17cc5d5c27980be3ceb";
const TRACE_BYTES: usize = 972;
const TRACE_SHA256: &str = "537df3ee19012a933816b92688f0c648fe2519cee1b1dadcbd442d502865d865";

#[derive(Debug)]
struct OpeningShape {
    indices: Vec<u32>,
    sibling_count: u32,
}

fn reference_permutation(input: [u32; 16]) -> [u32; 16] {
    let mut state = input.map(BabyBear::from_u32);
    default_babybear_poseidon2_16().permute_mut(&mut state);
    state.map(|value| value.as_canonical_u32())
}

fn reference_field_commitment(tag: &[u8; 4], fields: &[u32]) -> [u32; 8] {
    let mut message = vec![u32::from_be_bytes(*tag), fields.len() as u32];
    message.extend_from_slice(fields);
    let mut state = [0u32; 16];
    for block in message.chunks(8) {
        state[..block.len()].copy_from_slice(block);
        state = reference_permutation(state);
    }
    state[..8].try_into().unwrap()
}

fn reference_zero_interval_digests() -> [[u32; 8]; 3] {
    let mut claim = vec![0u32; 10];
    claim.push(8);
    let start = vec![0u32; 12];
    let mut end = vec![0u32; 12];
    end[0] = 1;
    [
        reference_field_commitment(b"RS01", &claim),
        reference_field_commitment(b"RB01", &start),
        reference_field_commitment(b"RB01", &end),
    ]
}

fn compile_gate(entry: &str, path: &Path) {
    let started = Instant::now();
    let prepared = {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mandelbrot_recursive_production_opening_oracle_ingot");
        let url = Url::from_directory_path(fixture.canonicalize().unwrap()).unwrap();
        let mut db = DriverDataBase::default();
        assert!(
            !driver::init_ingot(&mut db, &url),
            "recursive production opening fixture diagnostics",
        );
        let ingot = db
            .workspace()
            .containing_ingot(&db, url)
            .expect("recursive production opening fixture ingot");
        let top_mod = ingot.root_mod(&db);
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
        assert!(
            diagnostics.is_empty(),
            "unexpected recursive production opening diagnostics:\n{diagnostics}",
        );
        let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, entry)
            .unwrap_or_else(|error| panic!("recursive production opening entry: {error}"));
        prepare_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .unwrap_or_else(|error| panic!("recursive production opening preparation: {error}"))
    };
    let bytes = compile_prepared_runtime_package_wasm(prepared)
        .expect("recursive production opening Wasm emission")
        .bytes;
    wasmparser::validate(&bytes).expect("recursive production opening Wasm should validate");
    let digest = hex::encode(Sha256::digest(&bytes));
    std::fs::write(path, &bytes).expect("recursive production opening Wasm should persist");
    eprintln!(
        "recursive production opening gate: compiled {} bytes with SHA-256 {} in {:.2?}",
        bytes.len(),
        digest,
        started.elapsed(),
    );
}

fn call_recursive_audit(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    receipt: &[u8],
    trace: &[u8],
    mutation: u32,
    declared_receipt_length: usize,
    declared_trace_length: usize,
) -> [u32; 19] {
    instance
        .get_typed_func::<(), ()>(&mut *store, "fe_cabi_reset")
        .expect("canonical arena reset export")
        .call(&mut *store, ())
        .expect("canonical arena reset should run");
    let allocate = instance
        .get_typed_func::<(i32, i32), i32>(&mut *store, "fe_cabi_alloc")
        .expect("canonical allocator export");
    let receipt_pointer = allocate
        .call(&mut *store, (i32::try_from(receipt.len() + 4).unwrap(), 4))
        .expect("receipt allocation should succeed");
    let trace_pointer = allocate
        .call(&mut *store, (i32::try_from(trace.len() + 4).unwrap(), 4))
        .expect("trace allocation should succeed");
    let memory = instance
        .get_memory(&mut *store, "memory")
        .expect("recursive transcript memory");
    memory
        .write(&mut *store, receipt_pointer as usize, receipt)
        .expect("retained receipt should fit Wasm memory");
    memory
        .write(&mut *store, trace_pointer as usize, trace)
        .expect("retained trace should fit Wasm memory");
    let function = instance
        .get_func(&mut *store, RECURSIVE_ENTRY)
        .expect("recursive transcript relation export");
    let mut results = vec![Val::I32(0); 19];
    let digests = reference_zero_interval_digests();
    let mut arguments = vec![
        Val::I32(mutation as i32),
        Val::I32(receipt_pointer),
        Val::I32(i32::try_from(declared_receipt_length).unwrap()),
        Val::I32(trace_pointer),
        Val::I32(i32::try_from(declared_trace_length).unwrap()),
    ];
    arguments.extend(
        digests
            .into_iter()
            .flatten()
            .map(|value| Val::I32(value as i32)),
    );
    function
        .call(&mut *store, &arguments, &mut results)
        .expect("recursive transcript relation should execute");
    std::array::from_fn(|index| match results[index] {
        Val::I32(value) => value as u32,
        ref other => panic!("unexpected recursive transcript result {index}: {other:?}"),
    })
}

fn receipt_words(bytes: &[u8]) -> Vec<u32> {
    assert_eq!(bytes.len() % 4, 0);
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect()
}

fn parse_opening(words: &[u32], cursor: &mut usize, value_width: usize) -> OpeningShape {
    assert_eq!(words[*cursor], 1, "opening validity");
    *cursor += 1;
    assert_eq!(words[*cursor], 1, "multipath validity");
    *cursor += 1;
    let leaf_count = words[*cursor] as usize;
    *cursor += 1;
    assert!(leaf_count > 0 && leaf_count <= 456);
    let indices = words[*cursor..*cursor + leaf_count].to_vec();
    *cursor += leaf_count;
    let sibling_count = words[*cursor];
    *cursor += 1;
    assert!(sibling_count as usize <= 5_928);
    *cursor += sibling_count as usize * 8;
    *cursor += leaf_count * value_width;
    assert!(*cursor <= words.len());
    OpeningShape {
        indices,
        sibling_count,
    }
}

fn independent_multipath_shape(indices: &[u32]) -> (u32, u32) {
    assert!(!indices.is_empty());
    assert!(indices.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(indices.iter().all(|index| *index < 8_192));
    let mut active = indices.to_vec();
    let mut hashes = 0_u32;
    let mut used_siblings = 0_u32;
    for _ in 0..13 {
        let mut next = Vec::with_capacity(active.len());
        let mut cursor = 0;
        while cursor < active.len() {
            let index = active[cursor];
            let paired =
                index & 1 == 0 && cursor + 1 < active.len() && active[cursor + 1] == index + 1;
            cursor += if paired { 2 } else { 1 };
            if !paired {
                used_siblings += 1;
            }
            next.push(index / 2);
            hashes += 1;
        }
        active = next;
    }
    assert_eq!(active, [0]);
    (hashes, used_siblings)
}

fn independent_receipt_shapes(bytes: &[u8]) -> (OpeningShape, OpeningShape) {
    let words = receipt_words(bytes);
    assert_eq!(words[0], 1, "receipt validity");
    assert_eq!(words[1], 1, "base root validity");
    assert_eq!(words[10], 1, "interaction root validity");
    assert_eq!(words[11], 1, "interaction base-root validity");
    assert_eq!(&words[2..10], &words[12..20], "bound base root");
    let mut cursor = 28;
    let base = parse_opening(&words, &mut cursor, 260);
    let interaction = parse_opening(&words, &mut cursor, 152);
    (base, interaction)
}

fn call_audit(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    receipt: &[u8],
    mutation: u32,
    declared_length: usize,
) -> [u32; 13] {
    instance
        .get_typed_func::<(), ()>(&mut *store, "fe_cabi_reset")
        .expect("canonical arena reset export")
        .call(&mut *store, ())
        .expect("canonical arena reset should run");
    let allocation_length = receipt.len() + 4;
    let pointer = instance
        .get_typed_func::<(i32, i32), i32>(&mut *store, "fe_cabi_alloc")
        .expect("canonical allocator export")
        .call(&mut *store, (i32::try_from(allocation_length).unwrap(), 4))
        .expect("receipt allocation should succeed");
    let memory = instance
        .get_memory(&mut *store, "memory")
        .expect("recursive production opening memory");
    memory
        .write(&mut *store, pointer as usize, receipt)
        .expect("retained receipt should fit Wasm memory");
    memory
        .write(
            &mut *store,
            pointer as usize + receipt.len(),
            &0_u32.to_le_bytes(),
        )
        .expect("trailing canonical probe should fit Wasm memory");
    let function = instance
        .get_func(&mut *store, ENTRY)
        .expect("recursive production opening export");
    let mut results = vec![Val::I32(0); 13];
    function
        .call(
            &mut *store,
            &[
                Val::I32(mutation as i32),
                Val::I32(pointer),
                Val::I32(i32::try_from(declared_length).unwrap()),
            ],
            &mut results,
        )
        .expect("recursive production opening audit should execute");
    std::array::from_fn(|index| match results[index] {
        Val::I32(value) => value as u32,
        ref other => panic!("unexpected recursive production result {index}: {other:?}"),
    })
}

fn execute_gate(receipt_path: &Path, wasm_path: &Path) {
    let started = Instant::now();
    let receipt = std::fs::read(receipt_path).expect("retained receipt should be readable");
    assert_eq!(receipt.len(), RECEIPT_BYTES);
    assert_eq!(hex::encode(Sha256::digest(&receipt)), RECEIPT_SHA256);
    let (base, interaction) = independent_receipt_shapes(&receipt);
    let (base_hashes, base_siblings) = independent_multipath_shape(&base.indices);
    let (interaction_hashes, interaction_siblings) =
        independent_multipath_shape(&interaction.indices);
    assert_eq!(base_siblings, base.sibling_count);
    assert_eq!(interaction_siblings, interaction.sibling_count);

    let wasm = std::fs::read(wasm_path).expect("opening gate Wasm should be readable");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).expect("opening gate module should load");
    assert!(
        module.imports().next().is_none(),
        "gate must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("opening gate module should instantiate");

    let clean = call_audit(&mut store, &instance, &receipt, 0, receipt.len());
    assert_eq!(clean[0], 1, "canonical receipt decode");
    assert_eq!(&clean[1..3], &[1, 1], "base relation");
    assert_eq!(&clean[5..7], &[1, 1], "interaction relation");
    assert_eq!(&clean[3..5], &[base_hashes, base_siblings]);
    assert_eq!(&clean[7..9], &[interaction_hashes, interaction_siblings],);
    assert_eq!(clean[9], base.indices.len() as u32);
    assert_eq!(clean[10], interaction.indices.len() as u32);
    assert_eq!(clean[11], base.sibling_count);
    assert_eq!(clean[12], interaction.sibling_count);

    for mutation in 1..=12 {
        let result = call_audit(&mut store, &instance, &receipt, mutation, receipt.len());
        assert_eq!(result[0], 1, "typed mutation {mutation} decodes first");
        if matches!(mutation, 1 | 2 | 3 | 10 | 12) {
            assert_eq!(result[1], 0, "base mutation {mutation} must reject");
        } else {
            assert_eq!(result[5], 0, "interaction mutation {mutation} must reject",);
        }
    }

    assert_eq!(
        call_audit(&mut store, &instance, &receipt, 0, receipt.len() - 4,)[0],
        0,
        "truncated receipt must reject during canonical decoding",
    );
    assert_eq!(
        call_audit(&mut store, &instance, &receipt, 0, receipt.len() + 4,)[0],
        0,
        "trailing receipt bytes must reject during canonical decoding",
    );
    eprintln!(
        "recursive production opening gate: {} base leaves, {} interaction leaves, {base_hashes}/{interaction_hashes} hashes, finished in {:.2?}",
        base.indices.len(),
        interaction.indices.len(),
        started.elapsed(),
    );
}

fn execute_recursive_gate(receipt_path: &Path, trace_path: &Path, wasm_path: &Path) {
    let started = Instant::now();
    let receipt = std::fs::read(receipt_path).expect("retained receipt should be readable");
    let trace = std::fs::read(trace_path).expect("retained verifier trace should be readable");
    assert_eq!(receipt.len(), RECEIPT_BYTES);
    assert_eq!(hex::encode(Sha256::digest(&receipt)), RECEIPT_SHA256);
    assert_eq!(trace.len(), TRACE_BYTES);
    assert_eq!(hex::encode(Sha256::digest(&trace)), TRACE_SHA256);

    let wasm = std::fs::read(wasm_path).expect("recursive transcript Wasm should be readable");
    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, wasm).expect("recursive transcript module should load");
    assert!(
        module.imports().next().is_none(),
        "gate must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("recursive transcript module should instantiate");

    let clean = call_recursive_audit(
        &mut store,
        &instance,
        &receipt,
        &trace,
        0,
        receipt.len(),
        trace.len(),
    );
    assert_eq!(&clean[..11], &[1; 11], "complete joined relation");
    assert!(
        clean[11..].iter().any(|value| *value != 0),
        "transcript digest"
    );

    for mutation in 1..=12 {
        let result = call_recursive_audit(
            &mut store,
            &instance,
            &receipt,
            &trace,
            mutation,
            receipt.len(),
            trace.len(),
        );
        assert_eq!(result[0], 1, "typed mutation {mutation} decodes first");
        assert_eq!(result[5], 0, "typed mutation {mutation} must reject");
    }
    for mutation in 13..=16 {
        let result = call_recursive_audit(
            &mut store,
            &instance,
            &receipt,
            &trace,
            mutation,
            receipt.len(),
            trace.len(),
        );
        assert_eq!(result[5], 0, "task rewire {mutation} must reject");
        assert_eq!(result[7 + mutation as usize - 13], 0);
    }
    for mutation in [17, 18] {
        let result = call_recursive_audit(
            &mut store,
            &instance,
            &receipt,
            &trace,
            mutation,
            receipt.len(),
            trace.len(),
        );
        assert_eq!(result[5], 0, "joined mutation {mutation} must reject");
    }
    assert_eq!(
        call_recursive_audit(
            &mut store,
            &instance,
            &receipt,
            &trace,
            0,
            receipt.len() - 4,
            trace.len(),
        )[0],
        0,
    );
    assert_eq!(
        call_recursive_audit(
            &mut store,
            &instance,
            &receipt,
            &trace,
            0,
            receipt.len(),
            trace.len() - 4,
        )[1],
        0,
    );
    eprintln!(
        "recursive production transcript gate: joined header, openings, and transcript in {:.2?}",
        started.elapsed(),
    );
}

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("retained opening gate requires `{name}`"))
}

fn run_requested_child() -> bool {
    let Ok(mode) = std::env::var(CHILD_MODE) else {
        return false;
    };
    let receipt = required_path(RECEIPT_PATH);
    let wasm = required_path(WASM_PATH);
    match mode.as_str() {
        "compile" => compile_gate(ENTRY, &wasm),
        "execute" => execute_gate(&receipt, &wasm),
        "compile-recursive" => compile_gate(RECURSIVE_ENTRY, &wasm),
        "execute-recursive" => execute_recursive_gate(&receipt, &required_path(TRACE_PATH), &wasm),
        _ => panic!("unknown recursive production opening child mode {mode}"),
    }
    true
}

fn run_child(mode: &str, receipt: &Path, trace: Option<&Path>, wasm: &Path) -> ExitStatus {
    let mut command = Command::new(std::env::current_exe().expect("current test executable"));
    command
        .arg(GATE)
        .arg("--exact")
        .arg("--ignored")
        .arg("--nocapture")
        .env(CHILD_MODE, mode)
        .env(RECEIPT_PATH, receipt)
        .env(WASM_PATH, wasm);
    if let Some(trace) = trace {
        command.env(TRACE_PATH, trace);
    }
    command
        .status()
        .unwrap_or_else(|error| panic!("recursive production opening {mode} child: {error}"))
}

#[test]
#[ignore = "requires MB2_PRODUCTION_SECURITY_RECEIPT pointing to the retained exact receipt"]
fn retained_production_receipt_authenticates_both_opening_roles() {
    if run_requested_child() {
        return;
    }
    let receipt = required_path(RECEIPT_PATH);
    let scratch_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fe-test-scratch");
    std::fs::create_dir_all(&scratch_root).expect("workspace test scratch directory");
    let scratch = tempfile::Builder::new()
        .prefix("recursive-production-opening-")
        .tempdir_in(scratch_root)
        .expect("workspace-backed recursive opening scratch");
    let wasm = scratch.path().join("opening-relations.wasm");
    for mode in ["compile", "execute"] {
        let status = run_child(mode, &receipt, None, &wasm);
        if !status.success() {
            let retained = scratch.keep();
            panic!(
                "recursive production opening {mode} failed; retained evidence at {}",
                retained.display(),
            );
        }
    }
}

#[test]
#[ignore = "requires retained production receipt and canonical verifier trace"]
fn retained_production_receipt_and_trace_form_one_recursive_transcript_relation() {
    if run_requested_child() {
        return;
    }
    let receipt = required_path(RECEIPT_PATH);
    let trace = required_path(TRACE_PATH);
    let scratch_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fe-test-scratch");
    std::fs::create_dir_all(&scratch_root).expect("workspace test scratch directory");
    let scratch = tempfile::Builder::new()
        .prefix("recursive-production-transcript-")
        .tempdir_in(scratch_root)
        .expect("workspace-backed recursive transcript scratch");
    let wasm = scratch.path().join("recursive-transcript.wasm");
    for mode in ["compile-recursive", "execute-recursive"] {
        let status = run_child(mode, &receipt, Some(&trace), &wasm);
        if !status.success() {
            let retained = scratch.keep();
            panic!(
                "recursive production transcript {mode} failed; retained evidence at {}",
                retained.display(),
            );
        }
    }
}
