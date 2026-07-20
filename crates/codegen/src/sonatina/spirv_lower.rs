//! Fe -> SPIR-V wire (slice S1): the shortest path from a Fe source to
//! naga-validated SPIR-V words.
//!
//! There is no SPIR-V lowering port. `compile_runtime_package_wasm` already
//! builds a Sonatina `Module` under the Wasm32 ISA whose `inst_set()` is
//! `NativeInstSet` (`wasm_lower.rs`), and Sonatina's `SpirvBackend` consumes any
//! `Module` by downcasting generically against `function.inst_set()`
//! (`isa/spirv/mod.rs`). So the wasm-path Module is SPIR-V-consumable
//! *unchanged*: this driver just hands it over. For an Add/Mul/Return
//! single-function kernel every op is inside SPIR-V's envelope.
//!
//! The naga validator runs *inside* `SpirvBackend::compile_module`, so a
//! returned `SpirvArtifact` is already a structurally-valid compute module (the
//! honest rung this slice earns is R-val: validated, NOT executed). Execution on
//! a real GPU runtime (lavapipe) is a later slice.

use driver::DriverDataBase;
use mir::RuntimePackage;
use sonatina_codegen::Backend as _;
use sonatina_codegen::isa::spirv::{SpirvArtifact, SpirvBackend};

use crate::sonatina::{LowerError, compile_runtime_package_wasm};

/// Lower a MIR runtime package to naga-validated SPIR-V by reusing the wasm-path
/// Sonatina `Module`.
///
/// Returns the `SpirvArtifact` (`words: Vec<u32>` little-endian SPIR-V, plus an
/// optional WGSL side artifact for later GPU execution). Any translation or
/// naga-validation failure surfaces as [`LowerError::Spirv`]; the caller decides
/// how to present it. This is fail-closed by construction: an unsupported op or
/// an invalid module yields an error, never wrong SPIR-V.
pub fn compile_runtime_package_spirv(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
) -> Result<SpirvArtifact, LowerError> {
    // REUSE the wasm-path Module. The import side-table is irrelevant to SPIR-V
    // (compute shaders have no wasm-style imports), so it is discarded here.
    let (module, _import_modules) = compile_runtime_package_wasm(db, package)?;

    SpirvBackend::new()
        .compile_module(&module)
        .map_err(|errors| {
            LowerError::Spirv(
                errors
                    .iter()
                    .map(|error| error.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })
}
