//! Host-native execution through Sonatina IR and Cranelift.
//!
//! This API is deliberately narrow: callers select a public
//! `(i32, i32) -> i32` entry and can invoke it only while the owning JIT
//! artifact is alive. Expanding the ABI requires another explicitly checked
//! artifact wrapper rather than an untyped compiler seam.

use compiler_db::DriverDataBase;
use mir::RuntimePackage;
use sonatina_codegen::{
    Backend as _,
    isa::cranelift::{CraneliftArtifact, CraneliftBackend},
};
use sonatina_ir::{Linkage, Type};

use super::{LowerError, wasm_lower};

/// An owning JIT artifact with one verified public `(i32, i32) -> i32` entry.
///
/// The executable allocation is owned by `artifact`; no callable pointer can
/// escape this value through the safe API.
pub struct NativeI32EntryArtifact {
    artifact: CraneliftArtifact,
    entry: String,
}

impl NativeI32EntryArtifact {
    pub fn entry_name(&self) -> &str {
        &self.entry
    }

    pub fn call(&self, lhs: i32, rhs: i32) -> i32 {
        type Entry = extern "C" fn(i32, i32) -> i32;

        // SAFETY: construction verifies the Sonatina signature and linkage
        // before compilation. The pointer remains valid because `self` owns the
        // JIT module for the entire call.
        let function: Entry = unsafe {
            let pointer = self
                .artifact
                .get_func_ptr::<Entry>(&self.entry)
                .expect("verified native entry disappeared from its artifact");
            std::mem::transmute(pointer)
        };
        function(lhs, rhs)
    }
}

/// Compile one runtime package for the current host and bind an exact public
/// `(i32, i32) -> i32` entry.
pub fn compile_runtime_package_native_i32_entry(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    entry: &str,
) -> Result<NativeI32EntryArtifact, LowerError> {
    let module = wasm_lower::compile_runtime_package_native(db, package)?;

    let matching: Vec<_> = module
        .funcs()
        .into_iter()
        .filter(|&func_ref| {
            module
                .ctx
                .func_sig(func_ref, |signature| signature.name() == entry)
        })
        .collect();
    let [func_ref] = matching.as_slice() else {
        return Err(LowerError::Unsupported(format!(
            "native entry `{entry}` must resolve to exactly one function, found {}",
            matching.len()
        )));
    };
    module.ctx.func_sig(*func_ref, |signature| {
        if signature.linkage() != Linkage::Public {
            return Err(LowerError::Unsupported(format!(
                "native entry `{entry}` must have public linkage"
            )));
        }
        if signature.args() != [Type::I32, Type::I32] || signature.ret_tys() != [Type::I32] {
            return Err(LowerError::Unsupported(format!(
                "native entry `{entry}` must have ABI (i32, i32) -> i32; got ({:?}) -> {:?}",
                signature.args(),
                signature.ret_tys()
            )));
        }
        Ok(())
    })?;

    // Sonatina's current Cranelift translator reports a per-function error by
    // skipping that definition and continuing. Record every definition in this
    // entry-rooted package so we can turn that upstream fail-open behavior into
    // a fail-closed artifact postcondition.
    let expected_definitions = module
        .funcs()
        .into_iter()
        .filter(|&func_ref| {
            module
                .func_store
                .view(func_ref, |function| function.layout.entry_block().is_some())
        })
        .map(|func_ref| {
            module
                .ctx
                .func_sig(func_ref, |signature| signature.name().to_owned())
        })
        .collect::<Vec<_>>();

    let artifact = CraneliftBackend::new()
        .compile_module(&module)
        .map_err(|errors| {
            LowerError::Internal(format!(
                "native backend: {}",
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            ))
        })?;

    let missing_definitions = expected_definitions
        .iter()
        .filter(|name| {
            // `get_finalized_function` panics for a declared-but-skipped
            // definition. Contain that private upstream assertion and convert
            // it to this API's ordinary fail-closed error.
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                artifact.get_func_ptr::<()>(name)
            }))
            .ok()
            .flatten()
            .is_none()
        })
        .cloned()
        .collect::<Vec<_>>();
    if !missing_definitions.is_empty() {
        return Err(LowerError::Internal(format!(
            "native backend skipped definitions: {}",
            missing_definitions.join(", ")
        )));
    }

    Ok(NativeI32EntryArtifact {
        artifact,
        entry: entry.to_owned(),
    })
}
