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
    dead_ret::{DeadRetElimConfig, run_dead_ret_elim},
    exact_func_merge::run_exact_private_func_merge,
    forwarded_ret::{ForwardedRetElimConfig, run_forwarded_ret_elim},
    inliner::{FullInlineCloneRecord, Inliner, InlinerConfig},
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
    compile_runtime_package_spirv_authored_raster_with_resources(
        db,
        package,
        vertex_entry,
        fragment_entry,
        &[],
    )
}

/// Lower an authored vertex/fragment pair whose shared actor-state suffix
/// contains compiler-described storage resources. Resource argument indices
/// refer to the vertex entry; Sonatina projects them onto the corresponding
/// fragment-state slots and emits one shared bind-group interface.
pub fn compile_runtime_package_spirv_authored_raster_with_resources(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    vertex_entry: &str,
    fragment_entry: &str,
    resources: &[SpirvExternalResource],
) -> Result<SpirvArtifact, LowerError> {
    let (mut module, _import_modules) = compile_runtime_package_shader_ir(db, package)?;
    inline_spirv_named_calls(&mut module, &[vertex_entry, fragment_entry]);
    ensure_spirv_entries_call_free(&module, &[vertex_entry, fragment_entry])?;

    let mut backend = SpirvBackend::new().with_authored_raster(vertex_entry, fragment_entry);
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
    preserve_helpers: bool,
) -> std::collections::HashSet<sonatina_ir::module::FuncRef> {
    let trace = std::env::var_os("FE_SPIRV_INLINE_TRACE").is_some();
    let functions_before_merge = module.funcs().len();
    let instructions_before_merge = spirv_module_instruction_count(module);
    let merge_stats = run_exact_private_func_merge(module, roots);
    if trace {
        eprintln!(
            "fe spirv exact function merge: candidates={}, merged={}, rewritten_refs={}, rounds={}, functions={}->{}, instructions={}->{}",
            merge_stats.candidate_functions,
            merge_stats.merged_functions,
            merge_stats.rewritten_references,
            merge_stats.refinement_rounds,
            functions_before_merge,
            module.funcs().len(),
            instructions_before_merge,
            spirv_module_instruction_count(module),
        );
    }
    let snapshot = std::env::var_os("FE_SPIRV_INLINE_SNAPSHOT_DIR").map(|directory| {
        (
            directory,
            SPIRV_INLINE_SNAPSHOT_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        )
    });
    // The SPIR-V translator admits ordinary scalar helpers plus helpers that
    // borrow entry-rooted resource arrays. Other aggregates and object or arena
    // lifetime changes remain closed.
    // Flatten only those helpers that cross the closed boundary. Expanding every
    // helper bottom-up retains enormous intermediate copies for generated proof
    // graphs. Function count is not a useful proxy for that expansion: a proof
    // kernel with only a few dozen helpers can contain a dense generated graph.
    // Rooted inlining therefore treats helper bodies as immutable sources and
    // materializes only the portions that the backend cannot represent directly.
    // Retain every declared function because the stock dead-function pass's
    // object-root model is not populated by the wasm-path module lowerer.
    let mut inliner_config = spirv_inliner_config();
    inliner_config.record_full_clone_ids = trace;
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
    let preserved_helpers = if preserve_helpers {
        normalize_spirv_helper_graph(module);
        spirv_helper_candidates(module, roots)
    } else {
        std::collections::HashSet::new()
    };
    if !preserved_helpers.is_empty() {
        let helpers = preserved_helpers.iter().copied().collect::<Vec<_>>();
        run_function_passes_on(
            module,
            &helpers,
            &[Pass::RangeBranchSimplify, Pass::CfgCleanup],
        );
    }
    if trace {
        trace_spirv_helper_classification(module, roots, &preserved_helpers);
    }
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
            "fe spirv inliner: strategy=rooted, functions={}, roots={}, preserved_helpers={}, initial_insts={initial_root_insts}",
            module.funcs().len(),
            roots.len(),
            preserved_helpers.len()
        );
    }
    let mut clone_records = Vec::new();
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
        let changed = stats.changed;
        clone_records.extend(stats.full_clone_records);
        if trace {
            eprintln!(
                "fe spirv rooted inliner: frontier={frontier}, changed={}, after_inline_insts={}",
                changed,
                spirv_root_instruction_count(module, roots)
            );
            trace_spirv_full_inline_clone_survival(
                module,
                &clone_records,
                frontier,
                "after_inline",
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
                trace_spirv_full_inline_clone_survival(
                    module,
                    &clone_records,
                    frontier,
                    pass.as_str(),
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
        if !changed {
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
            trace_spirv_full_inline_clone_survival(
                module,
                &clone_records,
                MAX_FRONTIERS,
                pass.as_str(),
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
    let forwarded_ret_stats = run_forwarded_ret_elim(module, ForwardedRetElimConfig::default());
    let dead_ret_stats = run_dead_ret_elim(module, DeadRetElimConfig::default());
    if forwarded_ret_stats.removed_rets != 0 || dead_ret_stats.removed_rets != 0 {
        let mut affected = roots.to_vec();
        affected.extend(preserved_helpers.iter().copied());
        affected.sort_unstable_by_key(|function| function.as_u32());
        affected.dedup();
        run_function_passes_on(module, &affected, &spirv_post_inline_cleanup_passes());
    }
    if trace {
        eprintln!(
            "fe spirv forwarded return lanes: functions={}, removed_rets={}, calls={}, replaced_call_results={}, rounds={}, blocked_higher_order={}",
            forwarded_ret_stats.rewritten_funcs,
            forwarded_ret_stats.removed_rets,
            forwarded_ret_stats.rewritten_calls,
            forwarded_ret_stats.replaced_call_results,
            forwarded_ret_stats.rounds,
            forwarded_ret_stats.blocked_higher_order_funcs,
        );
        eprintln!(
            "fe spirv dead return lanes: functions={}, removed_rets={}, calls={}, removed_call_results={}, rounds={}, blocked_higher_order={}",
            dead_ret_stats.rewritten_funcs,
            dead_ret_stats.removed_rets,
            dead_ret_stats.rewritten_calls,
            dead_ret_stats.removed_call_results,
            dead_ret_stats.rounds,
            dead_ret_stats.blocked_higher_order_funcs,
        );
    }
    preserved_helpers
}

fn trace_spirv_full_inline_clone_survival(
    module: &sonatina_ir::Module,
    records: &[FullInlineCloneRecord],
    frontier: usize,
    stage: &str,
) {
    #[derive(Default)]
    struct Census {
        callsites: usize,
        cloned_insts: usize,
        surviving_insts: usize,
    }

    let mut by_callee = std::collections::BTreeMap::<String, Census>::new();
    for record in records {
        let name = module
            .ctx
            .func_sig(record.callee, |signature| signature.name().to_owned());
        let surviving = module.func_store.view(record.caller, |function| {
            record
                .instructions
                .iter()
                .filter(|&&inst| function.layout.is_inst_inserted(inst))
                .count()
        });
        let census = by_callee.entry(name).or_default();
        census.callsites += 1;
        census.cloned_insts += record.instructions.len();
        census.surviving_insts += surviving;
    }

    for (callee, census) in by_callee {
        eprintln!(
            "fe spirv rooted helper clones: frontier={frontier}, stage={stage}, callee={callee}, callsites={}, cloned_insts={}, surviving_insts={}",
            census.callsites, census.cloned_insts, census.surviving_insts,
        );
    }
}

fn spirv_helper_memory_effect_is_lowerable(
    inst_set: &dyn sonatina_ir::InstSetBase,
    instruction: &dyn sonatina_ir::Inst,
) -> bool {
    use sonatina_ir::{
        InstDowncast,
        inst::data::{Alloca, Gep, Mload, Mstore, ObjIndex, ObjLoad, ObjProj, ObjStore},
    };

    <&Alloca as InstDowncast>::downcast(inst_set, instruction).is_some()
        || <&Gep as InstDowncast>::downcast(inst_set, instruction).is_some()
        || <&Mload as InstDowncast>::downcast(inst_set, instruction).is_some()
        || <&Mstore as InstDowncast>::downcast(inst_set, instruction).is_some()
        || <&ObjIndex as InstDowncast>::downcast(inst_set, instruction).is_some()
        || <&ObjLoad as InstDowncast>::downcast(inst_set, instruction).is_some()
        || <&ObjProj as InstDowncast>::downcast(inst_set, instruction).is_some()
        || <&ObjStore as InstDowncast>::downcast(inst_set, instruction).is_some()
}

fn spirv_helper_instruction_accesses_resource(
    inst_set: &dyn sonatina_ir::InstSetBase,
    instruction: &dyn sonatina_ir::Inst,
) -> bool {
    use sonatina_ir::{
        InstDowncast,
        inst::data::{ObjLoad, ObjStore},
    };

    <&ObjLoad as InstDowncast>::downcast(inst_set, instruction).is_some()
        || <&ObjStore as InstDowncast>::downcast(inst_set, instruction).is_some()
}

fn spirv_helper_scalar_type(ty: sonatina_ir::Type) -> bool {
    matches!(
        ty,
        sonatina_ir::Type::I1 | sonatina_ir::Type::I32 | sonatina_ir::Type::F32
    )
}

fn spirv_helper_resource_root_type(module: &sonatina_ir::Module, ty: sonatina_ir::Type) -> bool {
    let Some(sonatina_ir::types::CompoundType::ObjRef(referent)) = ty.resolve_compound(&module.ctx)
    else {
        return false;
    };
    matches!(
        referent.resolve_compound(&module.ctx),
        Some(sonatina_ir::types::CompoundType::Array { .. })
    )
}

fn spirv_helper_result_abi_type(module: &sonatina_ir::Module, ty: sonatina_ir::Type) -> bool {
    spirv_helper_scalar_type(ty) || spirv_helper_resource_root_type(module, ty)
}

fn spirv_helper_argument_abi_type(module: &sonatina_ir::Module, ty: sonatina_ir::Type) -> bool {
    spirv_helper_result_abi_type(module, ty)
        || matches!(
            ty.resolve_compound(&module.ctx),
            Some(sonatina_ir::types::CompoundType::Ptr(_))
        )
}

fn spirv_helper_body_type(module: &sonatina_ir::Module, ty: sonatina_ir::Type) -> bool {
    spirv_helper_scalar_type(ty)
        || matches!(
            ty.resolve_compound(&module.ctx),
            Some(
                sonatina_ir::types::CompoundType::ObjRef(_)
                    | sonatina_ir::types::CompoundType::Ptr(_)
            )
        )
}

fn normalize_spirv_helper_graph(module: &mut sonatina_ir::Module) {
    // Helper eligibility should not depend on lowering artifacts such as an
    // empty entry jump or a return-only cursor wrapper. Normalize every body,
    // then erase only calls whose results are exact argument/immediate aliases.
    // Full inlining and instruction splicing stay disabled here: this pass
    // exposes the callable graph without duplicating any substantive work.
    let functions = module.funcs();
    run_function_passes_on(module, &functions, &[Pass::CfgCleanup]);
    Inliner::new(InlinerConfig {
        enable_single_block_splice: false,
        enable_full_inliner: false,
        ..InlinerConfig::default()
    })
    .run(module);
    let functions = module.funcs();
    run_function_passes_on(module, &functions, &[Pass::CfgCleanup]);
}

fn spirv_root_expansion_counts(
    module: &sonatina_ir::Module,
    roots: &[sonatina_ir::module::FuncRef],
) -> std::collections::HashMap<sonatina_ir::module::FuncRef, usize> {
    // Count the occurrences produced by rooted expansion, not only the number
    // of distinct source call instructions. A generated wrapper chain may
    // contain one call at each layer yet materialize a leaf dozens of times.
    // Cap propagation just above the largest useful profitability threshold,
    // which also makes an unexpected recursive call graph terminate.
    const MAX_EXPANDED_CALL_COUNT: usize = 129;
    let mut counts = std::collections::HashMap::new();
    let mut pending = std::collections::VecDeque::new();
    for &root in roots {
        let count = counts.entry(root).or_insert(0usize);
        if *count == 0 {
            *count = 1;
            pending.push_back((root, 1usize));
        }
    }
    while let Some((function_ref, added_occurrences)) = pending.pop_front() {
        if let Some(callees) = module.func_store.try_view(function_ref, |function| {
            let mut callees = std::collections::HashMap::new();
            for instruction in function
                .layout
                .iter_block()
                .flat_map(|block| function.layout.iter_inst(block))
            {
                if let Some(call) = function.dfg.call_info(instruction) {
                    *callees.entry(call.callee()).or_insert(0usize) += 1;
                }
            }
            callees
        }) {
            for (callee, calls_per_body) in callees {
                let added = added_occurrences
                    .saturating_mul(calls_per_body)
                    .min(MAX_EXPANDED_CALL_COUNT);
                let count = counts.entry(callee).or_insert(0usize);
                let next = count.saturating_add(added).min(MAX_EXPANDED_CALL_COUNT);
                let propagated = next - *count;
                if propagated != 0 {
                    *count = next;
                    pending.push_back((callee, propagated));
                }
            }
        }
    }
    counts
}

fn spirv_resource_passthrough_outline_worthy(
    expanded_call_count: usize,
    instruction_count: usize,
) -> bool {
    // A tiny resource-cursor wrapper is normally more useful inlined because
    // scalar cleanup can see through it. Once repeated source work crosses
    // this conservative budget, retaining one proven passthrough helper is the
    // smaller representation. This is a target cost policy, never a legality
    // exception: resource identity is still independently proved downstream.
    const MIN_AVOIDED_SOURCE_INSTRUCTIONS: usize = 128;
    instruction_count.saturating_mul(expanded_call_count.saturating_sub(1))
        >= MIN_AVOIDED_SOURCE_INSTRUCTIONS
}

fn spirv_profitable_helper_dependency_closure(
    module: &sonatina_ir::Module,
    roots: &[sonatina_ir::module::FuncRef],
    expansion_counts: &std::collections::HashMap<sonatina_ir::module::FuncRef, usize>,
) -> std::collections::HashSet<sonatina_ir::module::FuncRef> {
    let roots = roots
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let mut required = std::collections::HashSet::new();
    let mut pending = Vec::new();
    for function_ref in module.funcs() {
        if roots.contains(&function_ref) {
            continue;
        }
        let Some(instruction_count) = module.func_store.try_view(function_ref, |function| {
            function
                .layout
                .iter_block()
                .map(|block| function.layout.iter_inst(block).count())
                .sum::<usize>()
        }) else {
            continue;
        };
        if spirv_resource_passthrough_outline_worthy(
            expansion_counts
                .get(&function_ref)
                .copied()
                .unwrap_or_default(),
            instruction_count,
        ) {
            pending.push(function_ref);
        }
    }

    while let Some(function_ref) = pending.pop() {
        if roots.contains(&function_ref) || !required.insert(function_ref) {
            continue;
        }
        if let Some(callees) = module.func_store.try_view(function_ref, |function| {
            function
                .layout
                .iter_block()
                .flat_map(|block| function.layout.iter_inst(block))
                .filter_map(|instruction| function.dfg.call_info(instruction))
                .map(|call| call.callee())
                .collect::<Vec<_>>()
        }) {
            pending.extend(callees);
        }
    }
    required
}

fn spirv_helper_candidates(
    module: &sonatina_ir::Module,
    roots: &[sonatina_ir::module::FuncRef],
) -> std::collections::HashSet<sonatina_ir::module::FuncRef> {
    use sonatina_ir::inst::control_flow;

    fn classify(
        module: &sonatina_ir::Module,
        roots: &std::collections::HashSet<sonatina_ir::module::FuncRef>,
        function_ref: sonatina_ir::module::FuncRef,
        call_counts: &std::collections::HashMap<sonatina_ir::module::FuncRef, usize>,
        profitable_dependencies: &std::collections::HashSet<sonatina_ir::module::FuncRef>,
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
        let signature_carries_resource = signature
            .args()
            .iter()
            .chain(signature.ret_tys())
            .copied()
            .any(|ty| spirv_helper_resource_root_type(module, ty));
        let signature_ok = signature
            .args()
            .iter()
            .copied()
            .all(|ty| spirv_helper_argument_abi_type(module, ty))
            && signature
                .ret_tys()
                .iter()
                .copied()
                .all(|ty| spirv_helper_result_abi_type(module, ty));
        let Some((body_ok, callees, accesses_resource, instruction_count)) =
            module.func_store.try_view(function_ref, |function| {
                let inst_set = function.inst_set();
                // Helper outlining is only profitable if the backend can
                // represent the helper as an independent structured function.
                // Preflight the same structurizer used by Naga lowering so a
                // newly legal ABI does not turn an existing inlinable CFG into
                // a late backend failure.
                let mut body_ok = signature_ok
                    && sonatina_codegen::structurize::structurize_function(function).is_ok();
                let mut callees = Vec::new();
                let mut accesses_resource = false;
                let mut instruction_count = 0usize;
                for block in function.layout.iter_block() {
                    for instruction in function.layout.iter_inst(block) {
                        instruction_count += 1;
                        let data = function.dfg.inst(instruction);
                        accesses_resource |=
                            spirv_helper_instruction_accesses_resource(inst_set, data);
                        let direct_call = function.dfg.call_info(instruction);
                        if let Some(call) = direct_call {
                            callees.push(call.callee());
                        } else if data.declared_effect_hint().has_memory_effect()
                            && !spirv_helper_memory_effect_is_lowerable(inst_set, data)
                        {
                            body_ok = false;
                        }
                        if <&control_flow::CallIndirect as sonatina_ir::InstDowncast>::downcast(
                            inst_set, data,
                        )
                        .is_some()
                        {
                            body_ok = false;
                        }
                        if function
                            .dfg
                            .inst_results(instruction)
                            .iter()
                            .copied()
                            .any(|value| {
                                !spirv_helper_body_type(module, function.dfg.value_ty(value))
                            })
                            || data.collect_values().into_iter().any(|value| {
                                !spirv_helper_body_type(module, function.dfg.value_ty(value))
                            })
                        {
                            body_ok = false;
                        }
                    }
                }
                (body_ok, callees, accesses_resource, instruction_count)
            })
        else {
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
            if !classify(
                module,
                roots,
                callee,
                call_counts,
                profitable_dependencies,
                states,
                accepted,
            ) {
                callees_ok = false;
            }
        }
        // A resource-only cursor or error-state wrapper blocks useful scalar
        // simplification across the call while doing no resource work itself.
        // Inline that wrapper. Preserve the richer capability boundary only
        // where the helper actually loads from or stores to the resource.
        let passthrough_outline_worthy = spirv_resource_passthrough_outline_worthy(
            call_counts.get(&function_ref).copied().unwrap_or_default(),
            instruction_count,
        );
        if body_ok
            && callees_ok
            && (!signature_carries_resource
                || accesses_resource
                || passthrough_outline_worthy
                || profitable_dependencies.contains(&function_ref))
        {
            accepted.insert(function_ref);
        }
        states.insert(function_ref, 2);
        accepted.contains(&function_ref)
    }

    let roots = roots
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let call_counts =
        spirv_root_expansion_counts(module, &roots.iter().copied().collect::<Vec<_>>());
    let profitable_dependencies = spirv_profitable_helper_dependency_closure(
        module,
        &roots.iter().copied().collect::<Vec<_>>(),
        &call_counts,
    );
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
        classify(
            module,
            &roots,
            callee,
            &call_counts,
            &profitable_dependencies,
            &mut states,
            &mut accepted,
        );
    }
    accepted
}

fn trace_spirv_helper_classification(
    module: &sonatina_ir::Module,
    roots: &[sonatina_ir::module::FuncRef],
    accepted: &std::collections::HashSet<sonatina_ir::module::FuncRef>,
) {
    use sonatina_ir::inst::control_flow;

    let call_counts = spirv_root_expansion_counts(module, roots);
    let profitable_dependencies =
        spirv_profitable_helper_dependency_closure(module, roots, &call_counts);
    let root_set = roots
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let mut reachable = std::collections::HashSet::new();
    let mut worklist = roots.to_vec();
    while let Some(function_ref) = worklist.pop() {
        if !reachable.insert(function_ref) {
            continue;
        }
        if let Some(callees) = module.func_store.try_view(function_ref, |function| {
            function
                .layout
                .iter_block()
                .flat_map(|block| function.layout.iter_inst(block))
                .filter_map(|instruction| function.dfg.call_info(instruction))
                .map(|call| call.callee())
                .collect::<Vec<_>>()
        }) {
            worklist.extend(callees);
        }
    }

    let mut accepted_instructions = 0usize;
    let mut rejected = Vec::new();
    for function_ref in reachable
        .iter()
        .copied()
        .filter(|function| !root_set.contains(function))
    {
        let instruction_count = module
            .func_store
            .try_view(function_ref, |function| {
                function
                    .layout
                    .iter_block()
                    .map(|block| function.layout.iter_inst(block).count())
                    .sum::<usize>()
            })
            .unwrap_or_default();
        if accepted.contains(&function_ref) {
            accepted_instructions += instruction_count;
            continue;
        }

        let mut reasons = std::collections::BTreeSet::new();
        let signature_carries_resource =
            module.ctx.get_sig(function_ref).is_some_and(|signature| {
                signature
                    .args()
                    .iter()
                    .chain(signature.ret_tys())
                    .copied()
                    .any(|ty| spirv_helper_resource_root_type(module, ty))
            });
        module
            .ctx
            .get_sig(function_ref)
            .map(|signature| {
                if !signature
                    .args()
                    .iter()
                    .copied()
                    .all(|ty| spirv_helper_argument_abi_type(module, ty))
                {
                    reasons.insert("signature_args");
                }
                if !signature
                    .ret_tys()
                    .iter()
                    .copied()
                    .all(|ty| spirv_helper_result_abi_type(module, ty))
                {
                    reasons.insert("signature_results");
                }
            })
            .unwrap_or_else(|| {
                reasons.insert("missing_signature");
            });
        module.func_store.try_view(function_ref, |function| {
            let inst_set = function.inst_set();
            let mut accesses_resource = false;
            if let Err(error) = sonatina_codegen::structurize::structurize_function(function) {
                reasons.insert("unstructured_control");
                eprintln!(
                    "fe spirv helper structurize rejection: function={}, error={error}",
                    module
                        .ctx
                        .func_sig(function_ref, |signature| signature.name().to_owned()),
                );
                eprintln!(
                    "fe spirv rejected helper IR:\n{}",
                    sonatina_ir::ir_writer::FuncWriter::new(function_ref, function).dump_string(),
                );
            }
            for block in function.layout.iter_block() {
                for instruction in function.layout.iter_inst(block) {
                    let data = function.dfg.inst(instruction);
                    accesses_resource |= spirv_helper_instruction_accesses_resource(inst_set, data);
                    if let Some(call) = function.dfg.call_info(instruction) {
                        if !accepted.contains(&call.callee()) {
                            reasons.insert("rejected_callee");
                        }
                    } else if data.declared_effect_hint().has_memory_effect()
                        && !spirv_helper_memory_effect_is_lowerable(inst_set, data)
                    {
                        reasons.insert("memory_effect");
                    }
                    if <&control_flow::CallIndirect as sonatina_ir::InstDowncast>::downcast(
                        inst_set, data,
                    )
                    .is_some()
                    {
                        reasons.insert("indirect_call");
                    }
                    if function
                        .dfg
                        .inst_results(instruction)
                        .iter()
                        .copied()
                        .any(|value| !spirv_helper_body_type(module, function.dfg.value_ty(value)))
                        || data.collect_values().into_iter().any(|value| {
                            !spirv_helper_body_type(module, function.dfg.value_ty(value))
                        })
                    {
                        reasons.insert("non_scalar_value");
                    }
                }
            }
            if signature_carries_resource
                && !accesses_resource
                && !spirv_resource_passthrough_outline_worthy(
                    call_counts.get(&function_ref).copied().unwrap_or_default(),
                    instruction_count,
                )
                && !profitable_dependencies.contains(&function_ref)
            {
                reasons.insert("resource_passthrough_only");
            }
        });
        if reasons.is_empty() {
            reasons.insert("unclassified");
        }
        rejected.push((function_ref, instruction_count, reasons));
    }

    let mut by_reason = std::collections::BTreeMap::<&str, (usize, usize)>::new();
    for (_, instructions, reasons) in &rejected {
        for &reason in reasons {
            let totals = by_reason.entry(reason).or_default();
            totals.0 += 1;
            totals.1 += instructions;
        }
    }
    eprintln!(
        "fe spirv helper classification: reachable={}, accepted={}, accepted_insts={}, rejected={}, rejected_insts={}",
        reachable.len(),
        accepted.len(),
        accepted_instructions,
        rejected.len(),
        rejected
            .iter()
            .map(|(_, instructions, _)| instructions)
            .sum::<usize>(),
    );
    for (reason, (functions, instructions)) in by_reason {
        eprintln!(
            "fe spirv helper rejection: reason={reason}, functions={functions}, instructions={instructions}"
        );
    }
    rejected.sort_unstable_by_key(|(_, instructions, _)| std::cmp::Reverse(*instructions));
    for (function_ref, instructions, reasons) in rejected {
        let name = module
            .ctx
            .get_sig(function_ref)
            .map(|signature| signature.name().to_string())
            .unwrap_or_else(|| format!("{function_ref:?}"));
        eprintln!(
            "fe spirv rejected helper: function={name}, instructions={instructions}, reasons={}",
            reasons.into_iter().collect::<Vec<_>>().join("+")
        );
    }
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
        // marks backend-lowerable helpers `Never`; every other reachable
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

fn spirv_module_instruction_count(module: &sonatina_ir::Module) -> usize {
    module
        .funcs()
        .into_iter()
        .map(|function_ref| {
            module.func_store.view(function_ref, |function| {
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
            "SPIR-V entry `{entry_name}` retains a call outside the automatically lowerable helper graph after bounded inlining: {callee}"
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
