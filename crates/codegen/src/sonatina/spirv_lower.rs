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
use sonatina_codegen::optim::{Pass, Pipeline, Step, inliner::InlinerConfig};

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
    // The default workgroup size is `SpirvBackend`'s own (`[64, 1, 1]`). Callers
    // that ship a scalar-mode kernel to a browser want `[1, 1, 1]` (a dispatch of
    // (1,1,1) against a 64-wide workgroup is a benign same-value write race on
    // lavapipe but must not reach Chrome); they use
    // [`compile_runtime_package_spirv_with_workgroup`].
    compile_runtime_package_spirv_with_workgroup(db, package, SpirvBackend::new().workgroup_size)
}

/// Lower a MIR runtime package to naga-validated SPIR-V at a caller-chosen
/// workgroup size.
///
/// This is the fe-side driver seam ratified for the browser rung: a scalar-mode
/// kernel is compiled at `[1, 1, 1]` so a single invocation writes the single
/// output slot (no 64-way same-value write race), and the resulting
/// `SpirvLayout.workgroup_size` records exactly what was set. Everything else is
/// identical to [`compile_runtime_package_spirv`]; the word scalar and bindings
/// are still content-derived by the SPIR-V translator, never threaded here.
pub fn compile_runtime_package_spirv_with_workgroup(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    workgroup_size: [u32; 3],
) -> Result<SpirvArtifact, LowerError> {
    // CONSULT (DispatchKind axis): the SPIR-V target realizes the `Kernel` kind.
    // The entry point is invoked directly as a grid dispatch (`OpEntryPoint` /
    // `@compute`) against a bound resource interface; its envelope is stated by
    // the unit's `SpirvLayout` (the Kernel kind's interface statement), not an
    // in-band selector, and there is no synthesized dispatch root. Naming what
    // this lowering already does; a mismatch fires in debug, zero release effect.
    debug_assert!(
        {
            let kind = crate::dispatch::DispatchKind::for_backend(crate::BackendKind::Spirv);
            matches!(kind, crate::dispatch::DispatchKind::Kernel) && kind.entries_invoked_directly()
        },
        "SPIR-V lowering must realize the Kernel DispatchKind (entries invoked directly)"
    );
    // REUSE the wasm-path Module. The import side-table is irrelevant to SPIR-V
    // (compute shaders have no wasm-style imports), so it is discarded here.
    let (mut module, _import_modules) = compile_runtime_package_wasm(db, package)?;
    inline_spirv_calls(&mut module);
    ensure_spirv_entry_call_free(&module)?;

    SpirvBackend::new()
        .with_workgroup_size(workgroup_size[0], workgroup_size[1], workgroup_size[2])
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

/// Lower a MIR runtime package to naga-validated SPIR-V in GRID mode: one
/// invocation per pixel, kernel args 0,1 bound to `global_invocation_id.xy`,
/// args 2.. loaded from the broadcast input struct, the return value stored at
/// `output[gid.y * (num_workgroups.x * wgx) + gid.x]`. Grid is a driver-declared
/// envelope fact (there is no content signal), the same ruling as workgroup
/// size: the resulting `SpirvLayout` states it and the kernel-blind runner
/// consumes it. Fail-closed in the translator: u32 word only, >= 2 args, no
/// ObjAlloc, workgroup z == 1.
///
/// Body is identical to [`compile_runtime_package_spirv_with_workgroup`] plus
/// `.with_grid()` on the `SpirvBackend` builder.
pub fn compile_runtime_package_spirv_grid(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    workgroup_size: [u32; 3],
) -> Result<SpirvArtifact, LowerError> {
    // CONSULT (DispatchKind axis): the SPIR-V target realizes the `Kernel` kind.
    // The entry point is invoked directly as a grid dispatch (`OpEntryPoint` /
    // `@compute`) against a bound resource interface; its envelope is stated by
    // the unit's `SpirvLayout` (the Kernel kind's interface statement), not an
    // in-band selector, and there is no synthesized dispatch root. Naming what
    // this lowering already does; a mismatch fires in debug, zero release effect.
    debug_assert!(
        {
            let kind = crate::dispatch::DispatchKind::for_backend(crate::BackendKind::Spirv);
            matches!(kind, crate::dispatch::DispatchKind::Kernel) && kind.entries_invoked_directly()
        },
        "SPIR-V lowering must realize the Kernel DispatchKind (entries invoked directly)"
    );
    // REUSE the wasm-path Module (see `compile_runtime_package_spirv_with_workgroup`).
    let (mut module, _import_modules) = compile_runtime_package_wasm(db, package)?;
    inline_spirv_calls(&mut module);
    ensure_spirv_entry_call_free(&module)?;

    SpirvBackend::new()
        .with_workgroup_size(workgroup_size[0], workgroup_size[1], workgroup_size[2])
        .with_grid()
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

/// Lower a MIR runtime package to naga-validated SPIR-V in RENDER mode: ONE
/// SPIR-V module with TWO entry points. A fixed fullscreen-triangle `@vertex`
/// stage synthesizes 3 vertices from `@builtin(vertex_index)` (no vertex buffer,
/// no varyings), and a `@fragment` stage binds args 0,1 to `u32(position.xy)`
/// (the render analog of Grid's `global_invocation_id.xy`), runs the SAME
/// mode-blind body translation, and returns `unpack4x8unorm(result)` as an
/// `@location(0) vec4<f32>` color written straight to the render target. There is
/// NO output storage buffer (binding 0 absent); args 2.. still load from the
/// broadcast input struct at `@group(0) @binding(1)`.
///
/// Render is a driver-declared envelope fact (there is no content signal), the
/// same ruling as workgroup size and grid: the resulting `SpirvLayout` states it
/// (`mode: Render`, `vertex_entry`, `fragment_entry`, `color_target_format`) and
/// the kernel-blind runner consumes it. Render mode has no workgroup size (the
/// layout records `[0, 0, 0]`). Fail-closed in the translator: u32 word only,
/// >= 2 args, no ObjAlloc, mutually exclusive with grid/batch.
///
/// Body is identical to [`compile_runtime_package_spirv_grid`] plus `.with_render()`
/// (and NO `.with_grid()`/workgroup size) on the `SpirvBackend` builder.
pub fn compile_runtime_package_spirv_render(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
) -> Result<SpirvArtifact, LowerError> {
    // CONSULT (DispatchKind axis): the SPIR-V target realizes the `Kernel` kind.
    // In render mode the two entry points (`@vertex` + `@fragment`) are still
    // invoked directly by the render pipeline against a bound resource interface;
    // its envelope is stated by the unit's `SpirvLayout`, not an in-band selector,
    // and there is no synthesized dispatch root. Naming what this lowering already
    // does; a mismatch fires in debug, zero release effect.
    debug_assert!(
        {
            let kind = crate::dispatch::DispatchKind::for_backend(crate::BackendKind::Spirv);
            matches!(kind, crate::dispatch::DispatchKind::Kernel) && kind.entries_invoked_directly()
        },
        "SPIR-V lowering must realize the Kernel DispatchKind (entries invoked directly)"
    );
    // REUSE the wasm-path Module (see `compile_runtime_package_spirv_with_workgroup`).
    let (mut module, _import_modules) = compile_runtime_package_wasm(db, package)?;
    inline_spirv_calls(&mut module);
    ensure_spirv_entry_call_free(&module)?;

    SpirvBackend::new()
        .with_render()
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

fn inline_spirv_calls(module: &mut sonatina_ir::Module) {
    // The SPIR-V translator currently consumes only the first (entry) function
    // and deliberately rejects calls. Reuse Sonatina's CFG-aware full inliner
    // and constant/CFG cleanup here, while retaining every declared function:
    // the stock optimization pipelines include dead-function elimination whose
    // object-root model is not populated by the wasm-path module lowerer.
    let mut pipeline = Pipeline::new();
    pipeline.inliner_config = InlinerConfig {
        enable_full_inliner: true,
        // Auto calls are subject to target-local block/instruction and growth
        // caps. `inline` may bypass the local-size cap but still obeys growth
        // and depth limits. `inline(always)` intentionally overrides cost caps
        // in Sonatina, while its recursive-SCC generation guard still prevents
        // unbounded recursive expansion.
        max_inlinee_blocks: 64,
        max_inlinee_insts: 4_096,
        max_growth_per_caller: 65_536,
        max_total_growth: 262_144,
        max_inline_depth: 64,
        ..InlinerConfig::default()
    };
    pipeline.add_step(Step::Inline);
    pipeline.add_step(Step::FuncPasses(vec![
        Pass::CfgCleanup,
        Pass::BranchCanonicalize,
        Pass::Sccp,
        Pass::ScalarCanonicalize,
        Pass::Gvn,
        Pass::CfgCleanup,
    ]));
    pipeline.add_step(Step::Inline);
    pipeline.add_step(Step::FuncPasses(vec![
        Pass::CfgCleanup,
        Pass::BranchCanonicalize,
        Pass::Sccp,
        Pass::ScalarCanonicalize,
        Pass::Gvn,
        Pass::CfgCleanup,
    ]));
    pipeline.run(module);
}

fn ensure_spirv_entry_call_free(module: &sonatina_ir::Module) -> Result<(), LowerError> {
    let Some(&entry) = module.funcs().first() else {
        return Err(LowerError::Spirv(
            "SPIR-V module has no entry function".to_string(),
        ));
    };
    let entry_name = module
        .ctx
        .get_sig(entry)
        .map(|signature| signature.name().to_string())
        .unwrap_or_else(|| format!("{entry:?}"));
    let residual = module.func_store.view(entry, |function| {
        function.layout.iter_block().find_map(|block| {
            function.layout.iter_inst(block).find_map(|inst| {
                let call = function.dfg.call_info(inst)?;
                let callee = call.callee();
                let callee_name = module
                    .ctx
                    .get_sig(callee)
                    .map(|signature| signature.name().to_string())
                    .unwrap_or_else(|| format!("{callee:?}"));
                Some(callee_name)
            })
        })
    });
    match residual {
        Some(callee) => Err(LowerError::Spirv(format!(
            "SPIR-V entry `{entry_name}` is not call-free after bounded inlining; residual call to `{callee}`"
        ))),
        None => Ok(()),
    }
}
