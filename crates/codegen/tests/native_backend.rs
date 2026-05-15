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
fn native_ir_for_simple_contract_hits_evm_instruction_barrier() {
    // This test documents the current state: Fe's lowerer emits EVM-specific
    // instructions (EvmReturn, calldata loading, etc.) even for pure arithmetic
    // in a contract context. The NativeISA correctly rejects these.
    //
    // Once TargetLowering is implemented, this test should be updated to expect
    // success for pure computation functions.
    let result = std::panic::catch_unwind(|| {
        with_top_mod_for_source(
            "native_simple.fe",
            r#"
pub contract Arith {
    pub fn add_u64(a: u64, b: u64) -> u64 {
        a + b
    }
}
"#,
            |db, top_mod| fe_codegen::emit_module_sonatina_ir_native(db, top_mod),
        )
    });

    assert!(
        result.is_err(),
        "expected EVM instruction assertion to fire on native ISA — \
         this will pass once TargetLowering is implemented"
    );
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
