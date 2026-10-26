//! WebAssembly output for programs whose exports are functions of numbers and
//! text.
//!
//! The module is lowered by the same lowerer as host-native and SP1 output,
//! for the `wasm32-unknown-native` target: one linear address space and
//! 32-bit pointers. The exports are the `pub fn` items of the file; their
//! bodies may use aggregates, constants and dynamic memory, which the backend
//! keeps in the linear memory. An export may take `std::wasm::HostText` parameters
//! and return a `std::wasm::Reply`; a wrapper converts them to and from the
//! numbers of the boundary, and a module with a text parameter exports the
//! allocator through which the host places the text.

use driver::DriverDataBase;
use hir::hir_def::TopLevelMod;
use mir::{WasmExport, WasmExportPackage, build_wasm_export_package};
use sonatina_codegen::{
    Compile,
    isa::wasm::{WasmBackend, WasmError},
};
use sonatina_ir::{ir_writer::ModuleWriter, isa::wasm32::Wasm32};
use sonatina_triple::{Architecture, OperatingSystem, TargetTriple, Vendor};

use super::{LowerError, ensure_module_sonatina_ir_valid, lower_runtime, to_sonatina_opt_level};
use crate::OptLevel;

#[derive(Debug, Clone, Copy, Default)]
pub struct WasmOutputSelection {
    pub ir: bool,
    pub wasm: bool,
}

#[derive(Debug)]
pub struct WasmArtifacts {
    pub ir: Option<String>,
    pub wasm: Option<Vec<u8>>,
}

fn create_wasm32_isa() -> Wasm32 {
    Wasm32::new(TargetTriple::new(
        Architecture::Wasm32,
        Vendor::Unknown,
        OperatingSystem::Native,
    ))
}

/// Joins the errors of the backend. Input that it does not translate is the
/// user's to change; any other failure is a compiler fault.
fn lower_wasm_errors(errors: Vec<WasmError>) -> LowerError {
    let unsupported = errors
        .iter()
        .all(|error| matches!(error, WasmError::Unsupported(_)));
    if unsupported {
        let messages = errors.into_iter().map(|error| match error {
            WasmError::Unsupported(message) => message,
            other => other.to_string(),
        });
        LowerError::Unsupported(messages.collect::<Vec<_>>().join("; "))
    } else {
        let messages = errors.iter().map(ToString::to_string);
        LowerError::Internal(messages.collect::<Vec<_>>().join("; "))
    }
}

pub fn emit_module_wasm_artifacts(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    opt_level: OptLevel,
    outputs: WasmOutputSelection,
) -> Result<WasmArtifacts, LowerError> {
    let WasmExportPackage { package, exports } = build_wasm_export_package(db, top_mod)?;
    let isa = create_wasm32_isa();
    let adapters = exports
        .iter()
        .filter(|export| export.needs_adapter())
        .map(|export| lower_runtime::WasmAdapter {
            instance: export.instance,
            name: export.name.clone(),
            params: export.params.clone(),
            result: export.result,
        })
        .collect::<Vec<_>>();
    let module = lower_runtime::compile_wasm_package_sonatina(db, &package, &isa, &adapters)?;
    let backend = if exports.iter().any(WasmExport::takes_text) {
        WasmBackend::new().with_host_allocator()
    } else {
        WasmBackend::new()
    };

    let mut compile =
        Compile::new(module, backend).with_opt_level(to_sonatina_opt_level(opt_level));
    compile.optimize();
    ensure_module_sonatina_ir_valid(compile.module())?;
    let ir = outputs
        .ir
        .then(|| ModuleWriter::new(compile.module()).dump_string());
    let wasm = if outputs.wasm {
        let artifact = compile.compile().map_err(lower_wasm_errors)?;
        Some(artifact.into_bytes())
    } else {
        None
    };
    Ok(WasmArtifacts { ir, wasm })
}
