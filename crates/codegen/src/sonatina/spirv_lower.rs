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

use crate::sonatina::{LowerError, wasm_lower::compile_runtime_package_shader_ir};
use compiler_db::DriverDataBase;
use hir::hir_def::TopLevelMod;
use mir::{RuntimePackage, build_wasm_runtime_package_for_entry};
use sonatina_codegen::Backend as _;
use sonatina_codegen::isa::spirv::{
    SpirvArtifact, SpirvBackend, SpirvBuiltinArgument, SpirvExternalResource,
};
use sonatina_codegen::optim::{
    Pass,
    inliner::{Inliner, InlinerConfig},
    run_function_passes_on,
};
use sonatina_ir::ir_writer::ModuleWriter;
use std::{
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

static SPIRV_INLINE_SNAPSHOT_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

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
    let preserved_helpers = inline_spirv_calls(&mut module);
    ensure_spirv_entry_calls_lowerable(&module, &preserved_helpers)?;

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
    compile_runtime_package_spirv_compute_with_interface(
        db,
        package,
        workgroup_size,
        [1, 1, 1],
        resources,
        &[],
    )
}

/// Lower an explicit compute stage whose complete interface was derived from
/// Fe types. Builtin arguments are source parameters supplied directly by the
/// physical shader invocation context rather than a host-populated buffer.
pub fn compile_runtime_package_spirv_compute_with_interface(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    workgroup_size: [u32; 3],
    dispatch_grid: [u32; 3],
    resources: &[SpirvExternalResource],
    builtin_arguments: &[SpirvBuiltinArgument],
) -> Result<SpirvArtifact, LowerError> {
    let (mut module, _import_modules) = compile_runtime_package_shader_ir(db, package)?;
    let preserved_helpers = inline_spirv_calls(&mut module);
    ensure_spirv_entry_calls_lowerable(&module, &preserved_helpers)?;

    let mut backend = SpirvBackend::new()
        .with_compute()
        .with_workgroup_size(workgroup_size[0], workgroup_size[1], workgroup_size[2])
        .with_dispatch_grid(dispatch_grid[0], dispatch_grid[1], dispatch_grid[2]);
    for resource in resources {
        backend = backend.with_external_resource(resource.clone());
    }
    for argument in builtin_arguments {
        backend = backend.with_builtin_argument(*argument);
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
    let preserved_helpers = inline_spirv_calls(&mut module);
    ensure_spirv_entry_calls_lowerable(&module, &preserved_helpers)?;

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
    let preserved_helpers = inline_spirv_calls(&mut module);
    ensure_spirv_entry_calls_lowerable(&module, &preserved_helpers)?;

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
    let preserved_helpers = inline_spirv_calls(&mut module);
    ensure_spirv_entry_calls_lowerable(&module, &preserved_helpers)?;

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
    inline_spirv_named_calls(&mut module, &[vertex_entry, fragment_entry]);
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

fn inline_spirv_calls(
    module: &mut sonatina_ir::Module,
) -> std::collections::HashSet<sonatina_ir::module::FuncRef> {
    let roots = module.funcs().into_iter().take(1).collect::<Vec<_>>();
    inline_spirv_calls_from_roots(module, &roots, true)
}

fn inline_spirv_named_calls(module: &mut sonatina_ir::Module, entry_names: &[&str]) {
    let roots = module
        .funcs()
        .into_iter()
        .filter(|&function| {
            module.ctx.func_sig(function, |signature| {
                entry_names.contains(&signature.name())
            })
        })
        .collect::<Vec<_>>();
    let preserved = inline_spirv_calls_from_roots(module, &roots, false);
    debug_assert!(preserved.is_empty());
}

fn inline_spirv_calls_from_roots(
    module: &mut sonatina_ir::Module,
    roots: &[sonatina_ir::module::FuncRef],
    preserve_scalar_helpers: bool,
) -> std::collections::HashSet<sonatina_ir::module::FuncRef> {
    let trace = std::env::var_os("FE_SPIRV_INLINE_TRACE").is_some();
    let snapshot = std::env::var_os("FE_SPIRV_INLINE_SNAPSHOT_DIR").map(|directory| {
        (
            directory,
            SPIRV_INLINE_SNAPSHOT_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        )
    });
    // The SPIR-V translator admits ordinary scalar helper call graphs but keeps
    // aggregate, object, arena, and resource-crossing helper ABIs closed.
    // Flatten only those helpers that cross the closed boundary. Expanding every
    // helper bottom-up retains enormous intermediate copies for generated proof
    // graphs. Function count is not a useful proxy for that expansion: a proof
    // kernel with only a few dozen helpers can contain a dense generated graph.
    // Rooted inlining therefore treats helper bodies as immutable sources and
    // materializes only the portions that the backend cannot represent directly.
    // Retain every declared function because the stock dead-function pass's
    // object-root model is not populated by the wasm-path module lowerer.
    let mut inliner_config = spirv_inliner_config();
    // GVN's sparse predicated solver can require many GiB on one generated
    // proof entry after only a few frontiers. It is not a legality pass, and
    // the local CFG/SCCP sequence already removes the dead structure exposed
    // by each inline.
    let cleanup = spirv_rooted_inline_cleanup_passes();

    // One frontier corresponds to one semantic call-graph layer, not one
    // source-level recursion. FCO-derived factor trees retain typed wrappers
    // around each plan node, so a valid RBin<Pair, 12> stage is deeper than 16
    // layers after MIR lowering. Instruction growth remains the physical OOM
    // fuse below; this bound only rejects unexpectedly deep finite graphs.
    const MAX_FRONTIERS: usize = 64;
    const MAX_ROOT_GROWTH: usize = 10_000_000;
    let initial_root_insts = spirv_root_instruction_count(module, roots);
    if let Some((directory, sequence)) = &snapshot {
        write_spirv_inline_snapshot(module, roots, Path::new(directory), *sequence, "pre");
    }
    let preserved_helpers = if preserve_scalar_helpers {
        spirv_scalar_helper_candidates(module, roots)
    } else {
        std::collections::HashSet::new()
    };
    let original_hints = preserved_helpers
        .iter()
        .map(|&function| (function, module.ctx.inline_hint(function)))
        .collect::<Vec<_>>();
    for &function in &preserved_helpers {
        module
            .ctx
            .set_inline_hint(function, sonatina_ir::InlineHint::Never);
    }
    if trace {
        eprintln!(
            "fe spirv inliner: strategy=rooted, functions={}, roots={}, preserved_scalar_helpers={}, initial_insts={initial_root_insts}",
            module.funcs().len(),
            roots.len(),
            preserved_helpers.len()
        );
    }
    for frontier in 0..MAX_FRONTIERS {
        let consumed_growth =
            spirv_root_instruction_count(module, roots).saturating_sub(initial_root_insts);
        let Some(remaining_growth) = MAX_ROOT_GROWTH.checked_sub(consumed_growth) else {
            break;
        };
        if remaining_growth == 0 {
            break;
        }
        inliner_config.max_total_growth = remaining_growth;
        let stats = Inliner::new(inliner_config).run_one_frontier_from_roots(module, roots);
        if trace {
            eprintln!(
                "fe spirv rooted inliner: frontier={frontier}, changed={}, after_inline_insts={}",
                stats.changed,
                spirv_root_instruction_count(module, roots)
            );
        }
        if trace {
            for pass in cleanup {
                eprintln!(
                    "fe spirv rooted cleanup: frontier={frontier}, pass={}, before_insts={}",
                    pass.as_str(),
                    spirv_root_instruction_count(module, roots)
                );
                run_function_passes_on(module, roots, &[pass]);
                eprintln!(
                    "fe spirv rooted cleanup: frontier={frontier}, pass={}, after_insts={}",
                    pass.as_str(),
                    spirv_root_instruction_count(module, roots)
                );
            }
        } else {
            run_function_passes_on(module, roots, &cleanup);
        }
        if trace {
            eprintln!(
                "fe spirv rooted inliner: frontier={frontier}, after_cleanup_insts={}",
                spirv_root_instruction_count(module, roots)
            );
        }
        if !stats.changed {
            break;
        }
    }
    let final_cleanup = spirv_post_inline_cleanup_passes();
    if trace {
        for pass in final_cleanup {
            eprintln!(
                "fe spirv post-inline cleanup: pass={}, before_insts={}",
                pass.as_str(),
                spirv_root_instruction_count(module, roots)
            );
            run_function_passes_on(module, roots, &[pass]);
            eprintln!(
                "fe spirv post-inline cleanup: pass={}, after_insts={}",
                pass.as_str(),
                spirv_root_instruction_count(module, roots)
            );
        }
    } else {
        run_function_passes_on(module, roots, &final_cleanup);
    }
    if let Some((directory, sequence)) = &snapshot {
        write_spirv_inline_snapshot(module, roots, Path::new(directory), *sequence, "post");
    }
    for (function, hint) in original_hints {
        module.ctx.set_inline_hint(function, hint);
    }
    preserved_helpers
}

fn spirv_scalar_helper_candidates(
    module: &sonatina_ir::Module,
    roots: &[sonatina_ir::module::FuncRef],
) -> std::collections::HashSet<sonatina_ir::module::FuncRef> {
    use sonatina_ir::{Type, inst::control_flow};

    fn scalar_type(ty: Type) -> bool {
        matches!(ty, Type::I1 | Type::I32 | Type::F32)
    }

    fn classify(
        module: &sonatina_ir::Module,
        roots: &std::collections::HashSet<sonatina_ir::module::FuncRef>,
        function_ref: sonatina_ir::module::FuncRef,
        states: &mut std::collections::HashMap<sonatina_ir::module::FuncRef, u8>,
        accepted: &mut std::collections::HashSet<sonatina_ir::module::FuncRef>,
    ) -> bool {
        if roots.contains(&function_ref) {
            return false;
        }
        match states.get(&function_ref).copied() {
            Some(1) => return false,
            Some(2) => return accepted.contains(&function_ref),
            _ => {}
        }
        states.insert(function_ref, 1);

        let Some(signature) = module.ctx.get_sig(function_ref) else {
            states.insert(function_ref, 2);
            return false;
        };
        let result_arity = signature.ret_tys().len();
        let signature_ok = signature.args().iter().copied().all(scalar_type)
            && signature.ret_tys().iter().copied().all(scalar_type);
        let Some((body_ok, callees)) = module.func_store.try_view(function_ref, |function| {
            let inst_set = function.inst_set();
            let mut body_ok = signature_ok;
            let mut callees = Vec::new();
            let mut return_sites = 0usize;
            for block in function.layout.iter_block() {
                for instruction in function.layout.iter_inst(block) {
                    let data = function.dfg.inst(instruction);
                    let direct_call = function.dfg.call_info(instruction);
                    if let Some(call) = direct_call {
                        callees.push(call.callee());
                    } else if data.declared_effect_hint().has_memory_effect() {
                        body_ok = false;
                    }
                    if <&control_flow::CallIndirect as sonatina_ir::InstDowncast>::downcast(
                        inst_set, data,
                    )
                    .is_some()
                        || <&control_flow::Unreachable as sonatina_ir::InstDowncast>::downcast(
                            inst_set, data,
                        )
                        .is_some()
                    {
                        body_ok = false;
                    }
                    if <&control_flow::Return as sonatina_ir::InstDowncast>::downcast(
                        inst_set, data,
                    )
                    .is_some()
                    {
                        return_sites += 1;
                    }
                    if function
                        .dfg
                        .inst_results(instruction)
                        .iter()
                        .copied()
                        .any(|value| !scalar_type(function.dfg.value_ty(value)))
                        || data
                            .collect_values()
                            .into_iter()
                            .any(|value| !scalar_type(function.dfg.value_ty(value)))
                    {
                        body_ok = false;
                    }
                }
            }
            // WGSL represents several scalar results with one logical result
            // struct. The backend currently admits one canonical tuple-return
            // site and fails closed on structured multi-site transport.
            if result_arity > 1 && return_sites != 1 {
                body_ok = false;
            }
            (body_ok, callees)
        }) else {
            states.insert(function_ref, 2);
            return false;
        };
        // Visit every reachable child even after one child proves this parent
        // ineligible. Aggregate resource wrappers often appear above otherwise
        // self-contained scalar arithmetic islands. Short-circuiting here would
        // hide those islands from the helper ABI merely because their parent
        // must still be inlined.
        let mut callees_ok = true;
        for callee in callees {
            if !classify(module, roots, callee, states, accepted) {
                callees_ok = false;
            }
        }
        if body_ok && callees_ok {
            accepted.insert(function_ref);
        }
        states.insert(function_ref, 2);
        accepted.contains(&function_ref)
    }

    let roots = roots
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let root_callees = roots
        .iter()
        .flat_map(|&root| {
            module
                .func_store
                .try_view(root, |function| {
                    function
                        .layout
                        .iter_block()
                        .flat_map(|block| function.layout.iter_inst(block))
                        .filter_map(|instruction| function.dfg.call_info(instruction))
                        .map(|call| call.callee())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let mut states = std::collections::HashMap::new();
    let mut accepted = std::collections::HashSet::new();
    for callee in root_callees {
        classify(module, &roots, callee, &mut states, &mut accepted);
    }
    accepted
}

fn write_spirv_inline_snapshot(
    module: &sonatina_ir::Module,
    roots: &[sonatina_ir::module::FuncRef],
    directory: &Path,
    sequence: usize,
    phase: &str,
) {
    let root_names = roots
        .iter()
        .map(|root| {
            module
                .ctx
                .get_sig(*root)
                .map(|signature| sanitize_snapshot_name(signature.name()))
                .unwrap_or_else(|| sanitize_snapshot_name(&format!("function_{root:?}")))
        })
        .collect::<Vec<_>>()
        .join("+");
    let root_names = if root_names.is_empty() {
        "no_roots".to_owned()
    } else {
        root_names
    };
    let path = directory.join(format!("{sequence:04}-{root_names}-{phase}.sona"));
    if let Err(error) = std::fs::create_dir_all(directory).and_then(|()| {
        let mut writer = ModuleWriter::new(module);
        std::fs::write(&path, writer.dump_string())
    }) {
        eprintln!(
            "fe spirv inliner: could not write {} snapshot `{}`: {error}",
            phase,
            path.display()
        );
    } else {
        eprintln!(
            "fe spirv inliner: wrote {} snapshot `{}`",
            phase,
            path.display()
        );
    }
}

fn sanitize_snapshot_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn spirv_inliner_config() -> InlinerConfig {
    InlinerConfig {
        enable_full_inliner: true,
        // In the SPIR-V lane this inliner remains a legality pass for helpers
        // outside the scalar call ABI. The automatic classifier temporarily
        // marks backend-lowerable scalar helpers `Never`; every other reachable
        // helper is inlined unconditionally. The per-inlinee size caps and depth
        // are 0 ("no cap": `exceeds_cap`/depth gate on `> 0`) and the thresholds
        // are maxed so the cost model never declines an eligible call. Missing
        // bodies, recursion, authored `#[inline(never)]` on a non-lowerable
        // helper, and residual unsupported calls still fail closed with the
        // callee named.
        max_inlinee_blocks: 0,
        max_inlinee_insts: 0,
        max_growth_per_caller: 0,
        max_inline_depth: 0,
        inline_threshold: i32::MAX,
        inline_threshold_cold: i32::MAX,
        // The one finite bound: a coarse OOM fuse that converts a pathological
        // inlining blowup into a named residual-call diagnostic instead of an
        // OOM-killed process. NOTE it does NOT bound `#[inline(always)]` (ALWAYS
        // short-circuits before the growth check in sonatina's `decide_inline`), so
        // it protects the ordinary / `#[inline]` paths only. Deliberately
        // untestable: any fixture that trips it re-creates the size-cap bind this
        // fix removed.
        max_total_growth: 10_000_000,
        ..InlinerConfig::default()
    }
}

fn spirv_rooted_inline_cleanup_passes() -> [Pass; 5] {
    [
        Pass::CfgCleanup,
        Pass::BranchCanonicalize,
        Pass::Sccp,
        Pass::ScalarCanonicalize,
        Pass::CfgCleanup,
    ]
}

fn spirv_post_inline_cleanup_passes() -> [Pass; 6] {
    [
        Pass::KnownBitsSimplify,
        Pass::CheckedArithElim,
        Pass::RangeBranchSimplify,
        Pass::Sccp,
        Pass::ScalarCanonicalize,
        Pass::CfgCleanup,
    ]
}

fn spirv_root_instruction_count(
    module: &sonatina_ir::Module,
    roots: &[sonatina_ir::module::FuncRef],
) -> usize {
    roots
        .iter()
        .map(|&root| {
            module.func_store.view(root, |function| {
                function
                    .layout
                    .iter_block()
                    .map(|block| function.layout.iter_inst(block).count())
                    .sum::<usize>()
            })
        })
        .sum()
}

fn ensure_spirv_entry_calls_lowerable(
    module: &sonatina_ir::Module,
    preserved_helpers: &std::collections::HashSet<sonatina_ir::module::FuncRef>,
) -> Result<(), LowerError> {
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
                if preserved_helpers.contains(&callee) {
                    return None;
                }
                let callee_name = module
                    .ctx
                    .get_sig(callee)
                    .map(|signature| signature.name().to_string())
                    .unwrap_or_else(|| format!("{callee:?}"));
                let linkage = module.ctx.func_linkage(callee);
                let hints = module.ctx.func_hints(callee);
                Some(format!(
                    "`{callee_name}` (linkage={linkage:?}, hints={hints:?})"
                ))
            })
        })
    });
    match residual {
        Some(callee) => Err(LowerError::Spirv(format!(
            "SPIR-V entry `{entry_name}` retains a call outside the automatically lowerable scalar helper graph after bounded inlining: {callee}"
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
                    Some({
                        let name = module
                            .ctx
                            .get_sig(callee)
                            .map(|signature| signature.name().to_string())
                            .unwrap_or_else(|| format!("{callee:?}"));
                        let linkage = module.ctx.func_linkage(callee);
                        let hints = module.ctx.func_hints(callee);
                        format!("`{name}` (linkage={linkage:?}, hints={hints:?})")
                    })
                })
            })
        });
        if let Some(callee) = residual {
            return Err(LowerError::Spirv(format!(
                "SPIR-V entry `{entry_name}` is not call-free after bounded inlining; residual call to {callee}"
            )));
        }
    }
    Ok(())
}
