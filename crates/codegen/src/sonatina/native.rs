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
    let expected = vec![Type::I32, Type::I32];
    resolve_checked_entry(&module, entry, &expected, &[Type::I32])?;
    let expected_definitions = expected_definition_names(&module);
    let artifact = compile_and_verify_definitions(module, &expected_definitions)?;

    Ok(NativeI32EntryArtifact {
        artifact,
        entry: entry.to_owned(),
    })
}

/// The Rollcall Poseidon-Merkle root builder's exact native ABI: one output-limb
/// index plus four leaves' worth of 13-bit Montgomery limbs (`k` + 4 * 20 = 81
/// `u32`/`i32` scalars), returning one output limb. This is a SECOND narrow,
/// explicitly checked wrapper alongside [`NativeI32EntryArtifact`], exactly what
/// this module's own architecture note calls for ("another explicitly checked
/// artifact wrapper rather than an untyped compiler seam"): Fe has no
/// cross-backend array/pointer ABI, so a real field-arithmetic kernel with N
/// scalar limbs needs its own fixed-arity entry rather than widening the 2-arg
/// one into a variadic hole. Extend with a further sibling if another kernel
/// needs a different fixed arity; do not generalize this into an unchecked
/// arbitrary-arity seam.
pub const MERKLE_ROOT_NATIVE_ENTRY_ARITY: usize = 81;

/// An owning JIT artifact with one verified public
/// `(i32 x 81) -> i32` entry (see [`MERKLE_ROOT_NATIVE_ENTRY_ARITY`]).
pub struct NativeMerkleRootEntryArtifact {
    artifact: CraneliftArtifact,
    entry: String,
}

impl NativeMerkleRootEntryArtifact {
    pub fn entry_name(&self) -> &str {
        &self.entry
    }

    /// Call the entry with exactly [`MERKLE_ROOT_NATIVE_ENTRY_ARITY`] `i32`
    /// arguments (`args[0]` = the output-limb index `k`, `args[1..]` = the
    /// leaves' limbs in the same layout the wasm export takes).
    pub fn call(&self, args: &[i32; MERKLE_ROOT_NATIVE_ENTRY_ARITY]) -> i32 {
        #[rustfmt::skip]
        type Entry = extern "C" fn(
            i32, i32, i32, i32, i32, i32, i32, i32, i32, i32,
            i32, i32, i32, i32, i32, i32, i32, i32, i32, i32,
            i32, i32, i32, i32, i32, i32, i32, i32, i32, i32,
            i32, i32, i32, i32, i32, i32, i32, i32, i32, i32,
            i32, i32, i32, i32, i32, i32, i32, i32, i32, i32,
            i32, i32, i32, i32, i32, i32, i32, i32, i32, i32,
            i32, i32, i32, i32, i32, i32, i32, i32, i32, i32,
            i32, i32, i32, i32, i32, i32, i32, i32, i32, i32,
            i32,
        ) -> i32;
        const _: () = assert!(MERKLE_ROOT_NATIVE_ENTRY_ARITY == 81);

        // SAFETY: construction verifies the Sonatina signature (81 x I32 -> I32)
        // and linkage before compilation. The pointer remains valid because
        // `self` owns the JIT module for the entire call.
        let function: Entry = unsafe {
            let pointer = self
                .artifact
                .get_func_ptr::<Entry>(&self.entry)
                .expect("verified native entry disappeared from its artifact");
            std::mem::transmute(pointer)
        };
        #[rustfmt::skip]
        let result = function(
            args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7], args[8], args[9],
            args[10], args[11], args[12], args[13], args[14], args[15], args[16], args[17], args[18], args[19],
            args[20], args[21], args[22], args[23], args[24], args[25], args[26], args[27], args[28], args[29],
            args[30], args[31], args[32], args[33], args[34], args[35], args[36], args[37], args[38], args[39],
            args[40], args[41], args[42], args[43], args[44], args[45], args[46], args[47], args[48], args[49],
            args[50], args[51], args[52], args[53], args[54], args[55], args[56], args[57], args[58], args[59],
            args[60], args[61], args[62], args[63], args[64], args[65], args[66], args[67], args[68], args[69],
            args[70], args[71], args[72], args[73], args[74], args[75], args[76], args[77], args[78], args[79],
            args[80],
        );
        result
    }
}

/// Compile one runtime package for the current host and bind the exact public
/// `(i32 x 81) -> i32` entry (see [`MERKLE_ROOT_NATIVE_ENTRY_ARITY`] /
/// [`NativeMerkleRootEntryArtifact`]).
pub fn compile_runtime_package_native_merkle_root_entry(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    entry: &str,
) -> Result<NativeMerkleRootEntryArtifact, LowerError> {
    let module = wasm_lower::compile_runtime_package_native(db, package)?;
    let expected = vec![Type::I32; MERKLE_ROOT_NATIVE_ENTRY_ARITY];
    resolve_checked_entry(&module, entry, &expected, &[Type::I32])?;
    let expected_definitions = expected_definition_names(&module);
    let artifact = compile_and_verify_definitions(module, &expected_definitions)?;

    Ok(NativeMerkleRootEntryArtifact {
        artifact,
        entry: entry.to_owned(),
    })
}

/// Resolve `entry` to exactly one public function with the given argument /
/// return types. Shared by every narrow native entry wrapper in this module so
/// each one only states its own fixed ABI, not the resolution machinery.
fn resolve_checked_entry(
    module: &sonatina_ir::Module,
    entry: &str,
    expected_args: &[Type],
    expected_rets: &[Type],
) -> Result<sonatina_ir::module::FuncRef, LowerError> {
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
        if signature.args() != expected_args || signature.ret_tys() != expected_rets {
            return Err(LowerError::Unsupported(format!(
                "native entry `{entry}` must have ABI ({}) -> {}; got ({}) -> {}",
                describe_types(expected_args),
                describe_types(expected_rets),
                describe_types(signature.args()),
                describe_types(signature.ret_tys()),
            )));
        }
        Ok(())
    })?;
    Ok(*func_ref)
}

/// Lowercase, comma-joined rendering of a Sonatina `Type` list (`I32, I32` ->
/// `i32, i32`), matching this module's pre-existing hand-written error text
/// (`"must have ABI (i32, i32) -> i32"`) for the 2-arg entry, and generalizing
/// the same style to any other fixed-arity wrapper this module gains.
fn describe_types(types: &[Type]) -> String {
    types
        .iter()
        .map(|ty| format!("{ty:?}").to_lowercase())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Sonatina's current Cranelift translator reports a per-function error by
/// skipping that definition and continuing. Record every definition in the
/// entry-rooted package so [`compile_and_verify_definitions`] can turn that
/// upstream fail-open behavior into a fail-closed artifact postcondition.
fn expected_definition_names(module: &sonatina_ir::Module) -> Vec<String> {
    module
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
        .collect::<Vec<_>>()
}

fn compile_and_verify_definitions(
    module: sonatina_ir::Module,
    expected_definitions: &[String],
) -> Result<CraneliftArtifact, LowerError> {
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

    Ok(artifact)
}
