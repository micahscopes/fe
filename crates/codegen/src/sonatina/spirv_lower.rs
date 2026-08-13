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

use compiler_db::DriverDataBase;
use hir::hir_def::TopLevelMod;
use mir::{RuntimePackage, build_wasm_runtime_package_for_entry};
use sonatina_codegen::Backend as _;
use sonatina_codegen::isa::spirv::{SpirvArtifact, SpirvBackend, SpirvExternalResource};
use sonatina_codegen::optim::{Pass, Pipeline, Step, inliner::InlinerConfig};

use crate::sonatina::{LowerError, wasm_lower::compile_runtime_package_shader_ir};

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
    let (mut module, _import_modules) = compile_runtime_package_shader_ir(db, package)?;
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

/// Lower an explicit unit-returning compute stage with compiler-described
/// storage resources. Resource arguments stay typed object roots in Sonatina
/// IR and become storage globals in the emitted shader.
pub fn compile_runtime_package_spirv_compute_with_resources(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    workgroup_size: [u32; 3],
    resources: &[SpirvExternalResource],
) -> Result<SpirvArtifact, LowerError> {
    let (mut module, _import_modules) = compile_runtime_package_shader_ir(db, package)?;
    inline_spirv_calls(&mut module);
    ensure_spirv_entry_call_free(&module)?;

    let mut backend = SpirvBackend::new().with_compute().with_workgroup_size(
        workgroup_size[0],
        workgroup_size[1],
        workgroup_size[2],
    );
    for resource in resources {
        backend = backend.with_external_resource(resource.clone());
    }
    backend.compile_module(&module).map_err(|errors| {
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
    let (mut module, _import_modules) = compile_runtime_package_shader_ir(db, package)?;
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
    let (mut module, _import_modules) = compile_runtime_package_shader_ir(db, package)?;
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

/// Lower a fragment stage whose compiler-described storage resources are
/// rooted directly in the function arguments.
pub fn compile_runtime_package_spirv_render_with_resources(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    resources: &[SpirvExternalResource],
) -> Result<SpirvArtifact, LowerError> {
    let (mut module, _import_modules) = compile_runtime_package_shader_ir(db, package)?;
    inline_spirv_calls(&mut module);
    ensure_spirv_entry_call_free(&module)?;

    let mut backend = SpirvBackend::new().with_render();
    for resource in resources {
        backend = backend.with_external_resource(resource.clone());
    }
    backend.compile_module(&module).map_err(|errors| {
        LowerError::Spirv(
            errors
                .iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        )
    })
}

/// Lower two Fe behaviors as one authored raster module. The package must
/// contain both public entries; Sonatina checks their flattened signatures as
/// one position/varying/state interface and emits the paired stage entrypoints.
pub fn compile_runtime_package_spirv_authored_raster(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    vertex_entry: &str,
    fragment_entry: &str,
) -> Result<SpirvArtifact, LowerError> {
    let (mut module, _import_modules) = compile_runtime_package_shader_ir(db, package)?;
    inline_spirv_calls(&mut module);
    ensure_spirv_entries_call_free(&module, &[vertex_entry, fragment_entry])?;

    SpirvBackend::new()
        .with_authored_raster(vertex_entry, fragment_entry)
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

/// Build the render-shaped MIR runtime package rooted at `entry` and lower it
/// straight to naga-validated WGSL, in one call. Composes
/// [`mir::build_wasm_runtime_package_for_entry`] (the package boundary) with
/// [`compile_runtime_package_spirv_render`] (the render lowering), so a caller
/// that only needs WGSL out of a Fe source (the in-browser Fe -> WGSL facade
/// path) does not need `mir` on its own dependency graph. The two crates'
/// `LowerError` types differ; the existing `From<mir::LowerError> for
/// LowerError` conversion (see the top of this module) makes `?` compose them
/// without an explicit map here, same as every other call site in this file.
pub fn compile_render_wgsl<'db>(
    db: &'db DriverDataBase,
    top_mod: TopLevelMod<'db>,
    entry: &str,
) -> Result<SpirvArtifact, LowerError> {
    let package = build_wasm_runtime_package_for_entry(db, top_mod, entry)?;
    compile_runtime_package_spirv_render(db, &package)
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
        // In the SPIR-V lane the inliner is a LEGALITY pass, not an optimization:
        // `ensure_spirv_entry_call_free` below makes any residual call in the entry
        // a hard error, because the SPIR-V translator consumes only the entry
        // function. So inline UNCONDITIONALLY — the per-inlinee size caps and depth
        // are 0 ("no cap": `exceeds_cap`/depth gate on `> 0`) and the thresholds are
        // maxed so the cost model never declines a call. The only remaining
        // decliners are the genuinely-irreducible cases (`#[inline(never)]`, a
        // missing body, and the recursive-SCC guard), for which
        // `ensure_spirv_entry_call_free` is the fail-closed backstop with a
        // callee-named error. Ordinary loop-carrying `pub fn`s (dec's operators and
        // their helpers) flatten into the fragment entry with no annotation.
        max_inlinee_blocks: 0,
        max_inlinee_insts: 0,
        max_growth_per_caller: 0,
        max_inline_depth: 0,
        inline_threshold: i32::MAX,
        inline_threshold_cold: i32::MAX,
        // The one finite bound: a coarse OOM fuse that converts a pathological
        // inlining blowup into the same `not call-free` diagnostic instead of an
        // OOM-killed process. NOTE it does NOT bound `#[inline(always)]` (ALWAYS
        // short-circuits before the growth check in sonatina's `decide_inline`), so
        // it protects the ordinary / `#[inline]` paths only. Deliberately
        // untestable: any fixture that trips it re-creates the size-cap bind this
        // fix removed.
        max_total_growth: 10_000_000,
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

fn ensure_spirv_entries_call_free(
    module: &sonatina_ir::Module,
    entry_names: &[&str],
) -> Result<(), LowerError> {
    for entry_name in entry_names {
        let entry = module
            .funcs()
            .iter()
            .copied()
            .find(|entry| {
                module
                    .ctx
                    .get_sig(*entry)
                    .is_some_and(|signature| signature.name() == *entry_name)
            })
            .ok_or_else(|| {
                LowerError::Spirv(format!(
                    "SPIR-V entry `{entry_name}` is absent after lowering"
                ))
            })?;
        let residual = module.func_store.view(entry, |function| {
            function.layout.iter_block().find_map(|block| {
                function.layout.iter_inst(block).find_map(|inst| {
                    let call = function.dfg.call_info(inst)?;
                    let callee = call.callee();
                    Some(
                        module
                            .ctx
                            .get_sig(callee)
                            .map(|signature| signature.name().to_string())
                            .unwrap_or_else(|| format!("{callee:?}")),
                    )
                })
            })
        });
        if let Some(callee) = residual {
            return Err(LowerError::Spirv(format!(
                "SPIR-V entry `{entry_name}` is not call-free after bounded inlining; residual call to `{callee}`"
            )));
        }
    }
    Ok(())
}
