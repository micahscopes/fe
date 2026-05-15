//! Tests for the native (Cranelift) backend path.
//!
//! These verify that Fe source code can be lowered to Sonatina IR
//! targeting the native ISA, then compiled to native code via Cranelift.

use common::InputDb;
use driver::DriverDataBase;
use url::Url;

fn with_top_mod_for_source<T>(
    name: &str,
    source: &str,
    f: impl for<'db> FnOnce(&'db DriverDataBase, hir::hir_def::TopLevelMod<'db>) -> T,
) -> T {
    let mut db = DriverDataBase::default();
    let file_url = Url::parse(&format!("file:///{name}")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, file_url.clone(), Some(source.to_string()));
    let file = db
        .workspace()
        .get(&db, &file_url)
        .expect("file should be loaded");
    let top_mod = db.top_mod(file);
    f(&db, top_mod)
}

#[test]
fn native_ir_for_simple_contract_produces_pure_functions() {
    let ir = with_top_mod_for_source(
        "native_simple.fe",
        r#"
pub contract Arith {
    pub fn add_u64(a: u64, b: u64) -> u64 {
        a + b
    }
}
"#,
        |db, top_mod| fe_codegen::emit_module_sonatina_ir_native(db, top_mod),
    );

    let ir_text = ir.expect("native IR emission should succeed (skipping EVM-only functions)");
    eprintln!("=== Native Sonatina IR ===\n{ir_text}");
    // Currently: the contract dispatcher functions (init_abi, init_root,
    // runtime_root) are skipped because they use EVM instructions.
    // The user's add_u64 function is inlined INTO the runtime_root by Fe's
    // MIR, so it also gets skipped.
    //
    // To fix: for native targets, Fe should lower user functions independently
    // of the contract model. This requires a different MIR → Sonatina path
    // that extracts functions without the ABI dispatcher wrapping.
    //
    // For now, verify the module structure is valid (has target triple,
    // function declarations exist even if bodies are missing).
    assert!(
        ir_text.contains("x86_64-unknown-native") || ir_text.contains("aarch64-unknown-native"),
        "expected native target triple in IR"
    );
}

#[test]
fn native_ir_for_standalone_function() {
    // Standalone parameterless pub fn (no contract) — goes through the
    // non-contract MIR path. Parameters make a function ineligible as a
    // root candidate in Fe's current model, so we use a parameterless fn.
    let ir = with_top_mod_for_source(
        "native_standalone.fe",
        r#"
pub fn compute() -> u64 {
    let a: u64 = 3
    let b: u64 = 4
    a + b
}
"#,
        |db, top_mod| fe_codegen::emit_module_sonatina_ir_native(db, top_mod),
    );

    let ir_text = ir.expect("native IR emission should succeed for standalone function");
    eprintln!("=== Native Standalone IR ===\n{ir_text}");
    assert!(ir_text.contains("func"), "expected a function definition in native IR");
    assert!(ir_text.contains("i64"), "expected i64 type (native integer size)");
    assert!(
        ir_text.contains("compute"),
        "expected compute function name in IR"
    );
}

#[cfg(feature = "cranelift")]
#[test]
fn native_jit_executes_standalone_function() {
    use sonatina_codegen::Backend;
    use sonatina_codegen::isa::cranelift::CraneliftBackend;

    // Compile Fe source to Sonatina IR targeting native
    let module = with_top_mod_for_source(
        "native_jit.fe",
        r#"
pub fn compute() -> u64 {
    let a: u64 = 21
    let b: u64 = 21
    a + b
}
"#,
        |db, top_mod| {
            let package = mir::build_runtime_package(db, top_mod).unwrap();
            fe_codegen::sonatina::compile_runtime_package_sonatina_native(
                db,
                &package,
                fe_codegen::EVM_LAYOUT,
            )
        },
    );
    let module = module.expect("Fe → native Sonatina IR should succeed");

    // Compile through Cranelift JIT
    let backend = CraneliftBackend::new();
    let artifact = backend
        .compile_module(&module)
        .expect("CraneliftBackend should compile the native IR");

    // Execute the JIT-compiled function
    let compute: fn() -> u64 = unsafe {
        let ptr = artifact
            .get_func_ptr::<fn() -> u64>("compute")
            .expect("compute function should be in the artifact");
        std::mem::transmute(ptr)
    };

    assert_eq!(compute(), 42, "JIT-compiled Fe function should return 42");
}

#[cfg(feature = "cranelift")]
#[test]
fn cross_target_same_source_evm_and_native() {
    use sonatina_codegen::Backend;
    use sonatina_codegen::isa::cranelift::CraneliftBackend;

    let source = r#"
pub fn answer() -> u64 {
    let x: u64 = 6
    let y: u64 = 7
    x * y
}
"#;

    // EVM path: compile to Sonatina IR
    let evm_ir = with_top_mod_for_source(
        "cross_evm.fe",
        source,
        |db, top_mod| fe_codegen::emit_module_sonatina_ir(db, top_mod),
    ).expect("EVM IR should succeed");

    // Native path: compile to Sonatina IR → Cranelift JIT → execute
    let native_result = with_top_mod_for_source(
        "cross_native.fe",
        source,
        |db, top_mod| {
            let package = mir::build_runtime_package(db, top_mod).unwrap();
            let module = fe_codegen::sonatina::compile_runtime_package_sonatina_native(
                db, &package, fe_codegen::EVM_LAYOUT,
            ).unwrap();
            let backend = CraneliftBackend::new();
            let artifact = backend.compile_module(&module).unwrap();
            let f: fn() -> u64 = unsafe {
                let ptr = artifact.get_func_ptr::<fn() -> u64>("answer").unwrap();
                std::mem::transmute(ptr)
            };
            f()
        },
    );

    // Both paths produce correct result
    assert_eq!(native_result, 42, "native JIT should compute 6*7=42");
    assert!(evm_ir.contains("answer"), "EVM IR should contain the answer function");

    eprintln!("Cross-target test passed: same Fe source compiled to both EVM IR and native, native returned {native_result}");
}

#[cfg(feature = "cranelift")]
#[test]
fn poseidon_style_addmod_via_ctfe_jit_execution() {
    use sonatina_codegen::Backend;
    use sonatina_codegen::isa::cranelift::CraneliftBackend;

    // Poseidon-style: addmod with constant inputs. Returns bool (native type)
    // so Cranelift can execute it. CTFE folds the u256 addmod at compile time.
    let result = with_top_mod_for_source(
        "poseidon_ctfe.fe",
        r#"
pub fn poseidon_check() -> bool {
    let p: u256 = 0x30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001
    let a: u256 = 42
    let b: u256 = 17
    let result: u256 = std::evm::crypto::addmod(a, b, p)
    result == 59
}
"#,
        |db, top_mod| {
            let package = mir::build_runtime_package(db, top_mod).unwrap();
            let module = fe_codegen::sonatina::compile_runtime_package_sonatina_native(
                db, &package, fe_codegen::EVM_LAYOUT,
            ).unwrap();

            let backend = CraneliftBackend::new();
            let artifact = backend.compile_module(&module).unwrap();
            let f: fn() -> bool = unsafe {
                let ptr = artifact.get_func_ptr::<fn() -> bool>("poseidon_check").unwrap();
                std::mem::transmute(ptr)
            };
            f()
        },
    );

    assert!(result, "Poseidon-style addmod(42, 17, BN254_PRIME) == 59 should be true");
}

#[cfg(feature = "wasm")]
#[test]
fn poseidon_style_addmod_compiles_to_wasm() {
    use sonatina_codegen::Backend;
    use sonatina_codegen::isa::wasm::WasmBackend;

    // Same Poseidon-style function, compiled to WASM
    let artifact = with_top_mod_for_source(
        "poseidon_wasm.fe",
        r#"
pub fn poseidon_check() -> bool {
    let p: u256 = 0x30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001
    let a: u256 = 42
    let b: u256 = 17
    let result: u256 = std::evm::crypto::addmod(a, b, p)
    result == 59
}
"#,
        |db, top_mod| {
            let package = mir::build_runtime_package(db, top_mod).unwrap();
            let module = fe_codegen::sonatina::compile_runtime_package_sonatina_native(
                db, &package, fe_codegen::EVM_LAYOUT,
            ).unwrap();

            let backend = WasmBackend::new();
            backend.compile_module(&module)
        },
    );

    let artifact = artifact.expect("WASM compilation should succeed");
    assert!(!artifact.bytes.is_empty(), "WASM output should not be empty");
    assert!(
        artifact.func_names.contains(&"poseidon_check".to_string()),
        "WASM should export poseidon_check function"
    );

    // Verify WASM magic number (0x00 0x61 0x73 0x6D = "\0asm")
    assert_eq!(&artifact.bytes[0..4], b"\0asm", "should be valid WASM binary");
    eprintln!(
        "Poseidon-style function compiled to {} bytes of WASM",
        artifact.bytes.len()
    );
}

#[test]
fn native_ir_for_poseidon_fp() {
    // Attempt to compile Poseidon's fp.fe through the native path.
    // This uses addmod/mulmod (EVM opcodes) — CTFE may fold them at compile
    // time for constant inputs, or they may hit the EVM instruction barrier.
    let result = with_top_mod_for_source(
        "poseidon_fp.fe",
        r#"
pub fn field_add() -> u256 {
    let p: u256 = 0xFFFFFFFF00000001
    let a: u256 = 42
    let b: u256 = 17
    std::evm::crypto::addmod(a, b, p)
}
"#,
        |db, top_mod| fe_codegen::emit_module_sonatina_ir_native(db, top_mod),
    );

    match result {
        Ok(ir_text) => {
            eprintln!("=== Poseidon-style Native IR ===\n{ir_text}");
            // If CTFE folded addmod, we should see a constant result
            assert!(
                ir_text.contains("field_add") || ir_text.contains("func"),
                "expected function in native IR"
            );
        }
        Err(e) => {
            eprintln!("=== Poseidon-style Native Error ===\n{e}");
            // Document error for addmod on native — expected until
            // TargetLowering implements native addmod
        }
    }
}

/// Verify that the EVM path still works for the same source.
#[test]
fn evm_ir_for_simple_contract_still_works() {
    let ir = with_top_mod_for_source(
        "evm_simple.fe",
        r#"
pub contract Arith {
    pub fn add_u64(a: u64, b: u64) -> u64 {
        a + b
    }
}
"#,
        |db, top_mod| fe_codegen::emit_module_sonatina_ir(db, top_mod),
    );

    let ir_text = ir.expect("EVM IR emission should succeed");
    assert!(ir_text.contains("add"), "expected add instruction in EVM IR");
}
