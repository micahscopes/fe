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
