//! Fe shader driver for the Sonatina Naga backend.
//!
//! Shared portable lowering constructs a module under the Shader ISA. Browser
//! compute and raster requests explicitly select WebGPU capabilities and
//! WGSL/SPIR-V encodings. Legacy scalar/grid capability adapters remain pending
//! migration. Validation establishes artifact validity, not execution
//! or numerical correctness on a device.

use crate::sonatina::{
    LowerError,
    bloat_capture::{CaptureConfig, CaptureObserver, Intervention},
    wasm_lower::compile_runtime_package_shader_ir,
};
use compiler_db::DriverDataBase;
use hir::hir_def::TopLevelMod;
use mir::{RuntimePackage, build_wasm_runtime_package_for_entry};
use sonatina_codegen::isa::naga::{
    NagaBackend, ShaderCompileRequest, ShaderEncoding, ShaderEnvironment, ShaderPipeline,
    ShaderTargetContract,
};
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

/// Lower a MIR runtime package to naga-validated SPIR-V using the Shader ISA.
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
    // Preserve runtime section identity while the shared lowerer constructs the
    // shader module. Root optimization at those declarations, not module order.
    let (mut module, entry_functions) = compile_runtime_package_shader_ir(db, package)?;
    let backend = SpirvBackend::new().with_workgroup_size(
        workgroup_size[0],
        workgroup_size[1],
        workgroup_size[2],
    );
    let preserved_helpers = inline_spirv_calls(&mut module, &entry_functions, &backend)?;
    ensure_spirv_entry_calls_lowerable(&module, &entry_functions, &preserved_helpers)?;

    backend
        .compile_entry(&module, entry_functions[0])
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
    let (mut module, entry_functions) = compile_runtime_package_shader_ir(db, package)?;

    let entry = entry_functions.first().copied().ok_or_else(|| {
        LowerError::Spirv("compute package has no runtime section entry".to_owned())
    })?;
    compile_webgpu_request(
        &mut module,
        ShaderPipeline::Compute {
            entry,
            workgroup_size,
            dispatch_grid,
        },
        resources,
        builtin_arguments,
    )
}

/// Browser compilation chooses its capability profile and encodings explicitly.
/// Resource and builtin evidence belongs to this request, not mutable backend
/// mode flags. The backend validates the final physical shader interface.
fn compile_webgpu_request(
    module: &mut sonatina_ir::Module,
    pipeline: ShaderPipeline,
    resources: &[SpirvExternalResource],
    builtin_arguments: &[SpirvBuiltinArgument],
) -> Result<SpirvArtifact, LowerError> {
    let target = ShaderTargetContract::new(
        ShaderEnvironment::WebGpu,
        [ShaderEncoding::Wgsl, ShaderEncoding::Spirv],
    )
    .map_err(|error| LowerError::Spirv(error.to_string()))?;
    let mut request = ShaderCompileRequest::new(&target, pipeline);
    request.resources = resources;
    request.builtin_arguments = builtin_arguments;
    // Establish the complete browser request before helper selection. The
    // contextual backend query needs resource identity and stage restrictions,
    // not only the types visible in an isolated helper signature.
    let roots = match request.pipeline {
        ShaderPipeline::Raster { vertex, fragment } => vec![vertex, fragment],
        ShaderPipeline::Compute { entry, .. }
        | ShaderPipeline::Fullscreen { entry }
        | ShaderPipeline::LegacyScalar { entry, .. }
        | ShaderPipeline::LegacyGrid { entry, .. } => vec![entry],
    };
    let pipeline_name = match request.pipeline {
        ShaderPipeline::Compute { .. } => "compute",
        ShaderPipeline::Raster { .. } => "raster",
        ShaderPipeline::Fullscreen { .. } => "fullscreen",
        ShaderPipeline::LegacyScalar { .. } => "legacy_scalar",
        ShaderPipeline::LegacyGrid { .. } => "legacy_grid",
    };
    let strict_observation = std::env::var_os("FE_OBSERVE_STRICT").is_some();
    let mut capture = match std::env::var_os("FE_BLOAT_CAPTURE_DIR") {
        None => None,
        Some(directory) => {
            let environment = ["FE_BLOAT_CAPTURE_DIR", "FE_BLOAT_FORCE_INLINE_HELPERS"]
                .into_iter()
                .filter_map(|key| std::env::var(key).ok().map(|value| (key.into(), value)))
                .collect();
            let intervention = match std::env::var("FE_BLOAT_FORCE_INLINE_HELPERS") {
                Ok(requested) => Intervention::ForceInlineNamedRetainedHelpers { requested },
                Err(_) => Intervention::None,
            };
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| LowerError::Spirv(error.to_string()))?
                .as_nanos();
            let observer = CaptureObserver::new(
                CaptureConfig {
                    directory: directory.into(),
                    request_id: format!("request-{}-{nonce}-{pipeline_name}", std::process::id()),
                    environment,
                    intervention,
                    strict: strict_observation,
                    max_events: std::env::var("FE_OBSERVE_MAX_EVENTS")
                        .ok()
                        .map(|value| value.parse::<usize>())
                        .transpose()
                        .map_err(|error| {
                            LowerError::Spirv(format!("invalid observation event budget: {error}"))
                        })?
                        .unwrap_or(100_000),
                },
                module,
                &roots,
                pipeline_name,
            );
            match observer {
                Ok(observer) => Some(observer),
                Err(error) if strict_observation => return Err(capture_error(error)),
                Err(error) => {
                    eprintln!("fe observation unavailable: {error}");
                    None
                }
            }
        }
    };
    trace_contextual_helper_analysis(module, &request, "before_inline");
    let preserved_helpers = match inline_spirv_calls_from_roots(
        module,
        &roots,
        |module| NagaBackend::analyze_request_helpers(module, &request),
        capture.as_mut(),
    ) {
        Ok(helpers) => helpers,
        Err(error) => {
            if let Some(observer) = capture.take() {
                observer
                    .fail(&error.to_string(), "unknown")
                    .map_err(|capture_error| {
                        LowerError::Spirv(format!(
                            "{error}; bloat capture failure record: {capture_error}"
                        ))
                    })?;
            }
            return Err(error);
        }
    };
    trace_contextual_helper_analysis(module, &request, "after_inline");
    for &entry in &roots {
        if let Err(error) = ensure_spirv_entry_calls_lowerable(module, &[entry], &preserved_helpers)
        {
            if let Some(observer) = capture.take() {
                observer
                    .fail(&error.to_string(), "final")
                    .map_err(|capture_error| {
                        LowerError::Spirv(format!(
                            "{error}; bloat capture failure record: {capture_error}"
                        ))
                    })?;
            }
            return Err(error);
        }
    }
    match NagaBackend::compile_request(module, &request) {
        Ok(artifact) => {
            if let Some(observer) = capture.take() {
                if let Err(error) = observer.complete(&artifact, "final") {
                    if strict_observation {
                        return Err(capture_error(error));
                    }
                    eprintln!("fe observation incomplete: {error}");
                }
            }
            Ok(artifact)
        }
        Err(errors) => {
            let error = LowerError::Spirv(
                errors
                    .iter()
                    .map(|error| error.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            );
            if let Some(observer) = capture.take() {
                observer
                    .fail(&error.to_string(), "final")
                    .map_err(|capture_error| {
                        LowerError::Spirv(format!(
                            "{error}; bloat capture failure record: {capture_error}"
                        ))
                    })?;
            }
            Err(error)
        }
    }
}

/// Observe the backend query at the two integration boundaries. This trace
/// never selects helpers or substitutes a fallback after a query rejection.
fn trace_contextual_helper_analysis(
    module: &sonatina_ir::Module,
    request: &ShaderCompileRequest<'_>,
    phase: &str,
) {
    if std::env::var_os("FE_SPIRV_INLINE_TRACE").is_none() {
        return;
    }
    match NagaBackend::analyze_request_helpers(module, request) {
        Ok(analysis) => {
            eprintln!(
                "fe naga helper query: phase={phase}, callable={}, rejected={}",
                analysis.callable.len(),
                analysis.rejected.len()
            );
            for (function, reason) in analysis.rejected {
                eprintln!(
                    "fe naga helper rejection: phase={phase}, function={}, reason={reason}",
                    module
                        .ctx
                        .func_sig(function, |signature| signature.name().to_owned())
                );
            }
        }
        Err(errors) => {
            for error in errors {
                eprintln!("fe naga helper query unavailable: phase={phase}, reason={error}");
            }
        }
    }
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
    // Root optimization at the runtime section declarations.
    let (mut module, entry_functions) = compile_runtime_package_shader_ir(db, package)?;
    let backend = SpirvBackend::new()
        .with_workgroup_size(workgroup_size[0], workgroup_size[1], workgroup_size[2])
        .with_grid();
    let preserved_helpers = inline_spirv_calls(&mut module, &entry_functions, &backend)?;
    ensure_spirv_entry_calls_lowerable(&module, &entry_functions, &preserved_helpers)?;

    backend
        .compile_entry(&module, entry_functions[0])
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
/// Uses a fullscreen raster request under the checked WebGPU profile.
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
    // Root optimization at the runtime section declarations.
    let (mut module, entry_functions) = compile_runtime_package_shader_ir(db, package)?;

    let entry = entry_functions.first().copied().ok_or_else(|| {
        LowerError::Spirv("render package has no runtime section entry".to_owned())
    })?;
    compile_webgpu_request(&mut module, ShaderPipeline::Fullscreen { entry }, &[], &[])
}

/// Lower a fragment stage whose compiler-described storage resources are
/// rooted directly in the function arguments.
pub fn compile_runtime_package_spirv_render_with_resources(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    resources: &[SpirvExternalResource],
) -> Result<SpirvArtifact, LowerError> {
    let (mut module, entry_functions) = compile_runtime_package_shader_ir(db, package)?;

    let entry = entry_functions.first().copied().ok_or_else(|| {
        LowerError::Spirv("render package has no runtime section entry".to_owned())
    })?;
    compile_webgpu_request(
        &mut module,
        ShaderPipeline::Fullscreen { entry },
        resources,
        &[],
    )
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
    compile_runtime_package_spirv_authored_raster_with_interface(
        db,
        package,
        vertex_entry,
        fragment_entry,
        resources,
        &[],
    )
}

/// Lower an authored raster pair with compiler-described resources and a
/// physical vertex invocation context. The ordinary path supplies no explicit
/// builtins and retains the established implicit vertex index; an instanced
/// draw supplies vertex and instance indices as two source-language arguments.
pub fn compile_runtime_package_spirv_authored_raster_with_interface(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    vertex_entry: &str,
    fragment_entry: &str,
    resources: &[SpirvExternalResource],
    builtin_arguments: &[SpirvBuiltinArgument],
) -> Result<SpirvArtifact, LowerError> {
    let (mut module, entry_functions) = compile_runtime_package_shader_ir(db, package)?;
    // Resolve the public API's names only among declared runtime entries, once.
    // Optimization, legality checking and emission all retain these identities.
    let resolve = |name: &str| {
        entry_functions
            .iter()
            .copied()
            .find(|&entry| {
                module
                    .ctx
                    .func_sig(entry, |signature| signature.name() == name)
            })
            .ok_or_else(|| {
                LowerError::Spirv(format!(
                    "raster runtime entry `{name}` is absent after lowering"
                ))
            })
    };
    let vertex = resolve(vertex_entry)?;
    let fragment = resolve(fragment_entry)?;
    compile_webgpu_request(
        &mut module,
        ShaderPipeline::Raster { vertex, fragment },
        resources,
        builtin_arguments,
    )
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
    roots: &[sonatina_ir::module::FuncRef],
    backend: &SpirvBackend,
) -> Result<std::collections::HashSet<sonatina_ir::module::FuncRef>, LowerError> {
    // The legacy single-entry API selects the first runtime section. Preserve
    // that policy without relying on Sonatina declaration order.
    let entry = roots.first().copied().ok_or_else(|| {
        LowerError::Spirv("shader package has no runtime section entry".to_owned())
    })?;
    inline_spirv_calls_from_roots(
        module,
        &[entry],
        |module| backend.analyze_entry_helpers(module, entry),
        None,
    )
}

fn inline_spirv_calls_from_roots(
    module: &mut sonatina_ir::Module,
    roots: &[sonatina_ir::module::FuncRef],
    analyze: impl FnOnce(
        &sonatina_ir::Module,
    ) -> Result<
        sonatina_codegen::isa::naga::ShaderHelperAnalysis,
        Vec<sonatina_codegen::isa::naga::SpirvError>,
    >,
    mut capture: Option<&mut CaptureObserver>,
) -> Result<std::collections::HashSet<sonatina_ir::module::FuncRef>, LowerError> {
    let trace = std::env::var_os("FE_SPIRV_INLINE_TRACE").is_some();
    let trace_clones = std::env::var_os("FE_SPIRV_INLINE_TRACE_CLONES").is_some();
    if let Some(observer) = capture.as_deref_mut() {
        observer
            .stage(module, roots, "pre-merge", "pre_merge", &[], true, None)
            .map_err(capture_error)?;
    }
    let functions_before_merge = module.funcs().len();
    let instructions_before_merge = spirv_module_instruction_count(module);
    let merge_stats = run_exact_private_func_merge(module, roots);
    if let Some(observer) = capture.as_deref_mut() {
        observer
            .stage(
                module,
                roots,
                "post-merge",
                "post_merge",
                &["pre-merge"],
                true,
                None,
            )
            .or_else(|error| observer.record_error(error))
            .map_err(capture_error)?;
        observer
            .exact_merge(
                merge_stats.candidate_functions,
                merge_stats.merged_functions,
                merge_stats.rewritten_references,
                merge_stats.refinement_rounds,
            )
            .or_else(|error| observer.record_error(error))
            .map_err(capture_error)?;
    }
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
    inliner_config.record_full_clone_ids = trace_clones || capture.is_some();
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
    normalize_spirv_helper_graph(module);
    if let Some(observer) = capture.as_deref_mut() {
        observer
            .stage(
                module,
                roots,
                "normalized",
                "normalized_helper_graph",
                &["post-merge"],
                true,
                None,
            )
            .or_else(|error| observer.record_error(error))
            .map_err(capture_error)?;
    }
    let analysis = analyze(module).map_err(|errors| {
        LowerError::Spirv(
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    if let Some(observer) = capture.as_deref_mut() {
        observer
            .helper_analysis(module, &analysis)
            .or_else(|error| observer.record_error(error))
            .map_err(capture_error)?;
    }
    let selection = select_profitable_naga_helpers(module, roots, analysis, trace)?;
    if let Some(observer) = capture.as_deref_mut() {
        observer
            .helper_selection(
                module,
                &selection.baseline,
                &selection.selected,
                &selection.forced_inline,
                &selection.consequential_inline,
            )
            .or_else(|error| observer.record_error(error))
            .map_err(capture_error)?;
    }
    let preserved_helpers = selection
        .selected
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    if !preserved_helpers.is_empty() {
        let helpers = preserved_helpers.iter().copied().collect::<Vec<_>>();
        run_function_passes_on(
            module,
            &helpers,
            &[Pass::RangeBranchSimplify, Pass::CfgCleanup],
        );
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
    let mut previous_stage = "normalized".to_owned();
    for frontier in 0..MAX_FRONTIERS {
        inliner_config.record_full_clone_ids = trace_clones
            || capture
                .as_ref()
                .is_some_and(|observer| observer.is_recording());
        if !inliner_config.record_full_clone_ids {
            clone_records.clear();
        }
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
        let after_inline_stage = format!("frontier-{frontier:02}-after-inline");
        if let Some(observer) = capture.as_deref_mut() {
            observer
                .stage(
                    module,
                    roots,
                    &after_inline_stage,
                    "rooted_frontier_after_inline",
                    &[&previous_stage],
                    false,
                    Some(&stats),
                )
                .or_else(|error| observer.record_error(error))
                .map_err(capture_error)?;
        }
        let new_records = stats.full_clone_records;
        if let Some(observer) = capture.as_deref_mut() {
            observer
                .inline_events(module, &new_records, frontier, &after_inline_stage)
                .or_else(|error| observer.record_error(error))
                .map_err(capture_error)?;
        }
        clone_records.extend(new_records);
        if trace {
            eprintln!(
                "fe spirv rooted inliner: frontier={frontier}, changed={}, after_inline_insts={}",
                changed,
                spirv_root_instruction_count(module, roots)
            );
        }
        if trace_clones {
            trace_spirv_full_inline_clone_survival(
                module,
                &clone_records,
                frontier,
                "after_inline",
            );
        }
        if let Some(observer) = capture.as_deref_mut() {
            observer
                .clone_census(
                    module,
                    &clone_records,
                    frontier,
                    "after_inline",
                    &after_inline_stage,
                )
                .or_else(|error| observer.record_error(error))
                .map_err(capture_error)?;
        }
        let mut cleanup_predecessor = after_inline_stage;
        cleanup_predecessor = observe_cleanup(
            module,
            roots,
            &cleanup,
            capture.as_deref_mut(),
            &clone_records,
            frontier,
            &format!("frontier-{frontier:02}-cleanup"),
            "rooted_frontier_cleanup",
            cleanup_predecessor,
            trace,
            trace_clones,
        )?;
        let after_cleanup_stage = format!("frontier-{frontier:02}-after-cleanup");
        if let Some(observer) = capture.as_deref_mut() {
            observer
                .stage(
                    module,
                    roots,
                    &after_cleanup_stage,
                    "rooted_frontier_after_cleanup",
                    &[&cleanup_predecessor],
                    false,
                    None,
                )
                .or_else(|error| observer.record_error(error))
                .map_err(capture_error)?;
        }
        previous_stage = after_cleanup_stage;
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
    previous_stage = observe_cleanup(
        module,
        roots,
        &final_cleanup,
        capture.as_deref_mut(),
        &clone_records,
        MAX_FRONTIERS,
        "post-inline-cleanup",
        "post_inline_cleanup",
        previous_stage,
        trace,
        trace_clones,
    )?;
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
    if let Some(observer) = capture.as_deref_mut() {
        observer
            .stage(
                module,
                roots,
                "final",
                "final_shader_ir",
                &[&previous_stage],
                true,
                None,
            )
            .or_else(|error| observer.record_error(error))
            .map_err(capture_error)?;
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
    Ok(preserved_helpers)
}

fn capture_error(error: String) -> LowerError {
    LowerError::Spirv(format!("bloat capture: {error}"))
}

#[allow(clippy::too_many_arguments)]
fn observe_cleanup(
    module: &sonatina_ir::Module,
    roots: &[sonatina_ir::module::FuncRef],
    passes: &[Pass],
    capture: Option<&mut CaptureObserver>,
    records: &[FullInlineCloneRecord],
    frontier: usize,
    prefix: &str,
    kind: &str,
    previous: String,
    trace: bool,
    trace_clones: bool,
) -> Result<String, LowerError> {
    use sonatina_codegen::optim::pipeline::{
        PassObservation, PassObserver, run_function_passes_on_observed,
    };
    if capture.is_none() && !trace && !trace_clones {
        run_function_passes_on(module, roots, passes);
        return Ok(previous);
    }
    struct Observer<'a> {
        capture: Option<&'a mut CaptureObserver>,
        roots: &'a [sonatina_ir::module::FuncRef],
        records: &'a [FullInlineCloneRecord],
        frontier: usize,
        prefix: &'a str,
        kind: &'a str,
        previous: String,
        trace: bool,
        trace_clones: bool,
        error: Option<String>,
    }
    impl PassObserver for Observer<'_> {
        fn before_pass(&mut self, event: PassObservation, module: &sonatina_ir::Module) {
            if self.trace {
                eprintln!(
                    "fe spirv observed cleanup: pass={}, before_insts={}",
                    event.pass.as_str(),
                    spirv_root_instruction_count(module, self.roots)
                );
            }
        }
        fn after_pass(&mut self, event: PassObservation, module: &sonatina_ir::Module) {
            let stage = format!("{}-{:02}-{}", self.prefix, event.index, event.pass.as_str());
            if self.error.is_none() {
                if let Some(capture) = self.capture.as_deref_mut() {
                    let result = capture
                        .stage(
                            module,
                            self.roots,
                            &stage,
                            self.kind,
                            &[&self.previous],
                            false,
                            None,
                        )
                        .and_then(|()| {
                            capture.clone_census(
                                module,
                                self.records,
                                self.frontier,
                                event.pass.as_str(),
                                &stage,
                            )
                        });
                    self.error = result.or_else(|error| capture.record_error(error)).err();
                }
            }
            if self.trace {
                eprintln!(
                    "fe spirv observed cleanup: pass={}, after_insts={}",
                    event.pass.as_str(),
                    spirv_root_instruction_count(module, self.roots)
                );
            }
            if self.trace_clones {
                trace_spirv_full_inline_clone_survival(
                    module,
                    self.records,
                    self.frontier,
                    event.pass.as_str(),
                );
            }
            self.previous = stage;
        }
    }
    let mut observer = Observer {
        capture,
        roots,
        records,
        frontier,
        prefix,
        kind,
        previous,
        trace,
        trace_clones,
        error: None,
    };
    run_function_passes_on_observed(module, roots, passes, &mut observer);
    if let Some(error) = observer.error {
        return Err(capture_error(error));
    }
    Ok(observer.previous)
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

/// Profitability is a frontend policy over backend-authorized helpers, not a
/// second ABI classifier. Resource-only wrappers stay inline unless their
/// measured expansion or a profitable caller justifies retaining them.
fn select_profitable_naga_helpers(
    module: &sonatina_ir::Module,
    roots: &[sonatina_ir::module::FuncRef],
    analysis: sonatina_codegen::isa::naga::ShaderHelperAnalysis,
    trace: bool,
) -> Result<HelperSelection, LowerError> {
    let counts = spirv_root_expansion_counts(module, roots);
    let dependencies = spirv_profitable_helper_dependency_closure(module, roots, &counts);
    let mut selected = std::collections::HashSet::new();
    for helper in &analysis.callable {
        let carries_resource = module.ctx.func_sig(helper.function, |signature| {
            signature
                .args()
                .iter()
                .chain(signature.ret_tys())
                .copied()
                .any(|ty| spirv_helper_resource_root_type(module, ty))
        });
        if !carries_resource
            || helper.accesses_resource
            || spirv_resource_passthrough_outline_worthy(
                counts.get(&helper.function).copied().unwrap_or_default(),
                helper.instruction_count,
            )
            || dependencies.contains(&helper.function)
        {
            selected.insert(helper.function);
        }
    }
    if trace {
        for (function, reason) in &analysis.rejected {
            eprintln!(
                "fe naga rejected helper: function={}, reason={reason}",
                module
                    .ctx
                    .func_sig(*function, |signature| signature.name().to_owned())
            );
        }
    }
    close_retained_helper_set(module, &mut selected);
    let baseline_set = selected.clone();
    let requested = force_inline_helper_names()?;
    if requested.is_empty() {
        let mut baseline = baseline_set.into_iter().collect::<Vec<_>>();
        baseline.sort_unstable_by_key(|function| function.as_u32());
        return Ok(HelperSelection {
            selected: baseline.clone(),
            baseline,
            forced_inline: Vec::new(),
            consequential_inline: Vec::new(),
        });
    }
    let mut forced_inline = Vec::new();
    for requested_name in requested {
        let callable = analysis
            .callable
            .iter()
            .filter(|helper| {
                module.ctx.func_sig(helper.function, |signature| {
                    signature.name() == requested_name
                })
            })
            .map(|helper| helper.function)
            .collect::<Vec<_>>();
        let rejections = analysis
            .rejected
            .iter()
            .filter(|(function, _)| {
                module
                    .ctx
                    .func_sig(*function, |signature| signature.name() == requested_name)
            })
            .map(|(_, reason)| reason.as_str())
            .collect::<Vec<_>>();
        validate_force_inline_resolution(
            &requested_name,
            callable.len(),
            &rejections,
            callable
                .first()
                .is_some_and(|function| baseline_set.contains(function)),
        )?;
        match callable.as_slice() {
            [function] => forced_inline.push(*function),
            _ => unreachable!("validated one callable helper"),
        }
    }
    for function in &forced_inline {
        selected.remove(function);
    }
    close_retained_helper_set(module, &mut selected);
    let mut consequential_inline = baseline_set
        .difference(&selected)
        .copied()
        .filter(|function| !forced_inline.contains(function))
        .collect::<Vec<_>>();
    let mut baseline = baseline_set.into_iter().collect::<Vec<_>>();
    let mut selected = selected.into_iter().collect::<Vec<_>>();
    baseline.sort_unstable_by_key(|function| function.as_u32());
    selected.sort_unstable_by_key(|function| function.as_u32());
    forced_inline.sort_unstable_by_key(|function| function.as_u32());
    consequential_inline.sort_unstable_by_key(|function| function.as_u32());
    Ok(HelperSelection {
        baseline,
        selected,
        forced_inline,
        consequential_inline,
    })
}

fn validate_force_inline_resolution(
    requested_name: &str,
    callable_count: usize,
    rejection_reasons: &[&str],
    unique_is_baseline_retained: bool,
) -> Result<(), LowerError> {
    match callable_count {
        0 if rejection_reasons.is_empty() => Err(LowerError::Spirv(format!(
            "FE_BLOAT_FORCE_INLINE_HELPERS names unknown helper `{requested_name}`"
        ))),
        0 => Err(LowerError::Spirv(format!(
            "FE_BLOAT_FORCE_INLINE_HELPERS helper `{requested_name}` was rejected by the backend: {}",
            rejection_reasons.join("; ")
        ))),
        1 if !unique_is_baseline_retained => Err(LowerError::Spirv(format!(
            "FE_BLOAT_FORCE_INLINE_HELPERS helper `{requested_name}` is backend-callable but was not retained by the baseline frontend policy"
        ))),
        1 => Ok(()),
        count => Err(LowerError::Spirv(format!(
            "FE_BLOAT_FORCE_INLINE_HELPERS helper name `{requested_name}` is ambiguous across {count} backend-callable functions"
        ))),
    }
}

struct HelperSelection {
    baseline: Vec<sonatina_ir::module::FuncRef>,
    selected: Vec<sonatina_ir::module::FuncRef>,
    forced_inline: Vec<sonatina_ir::module::FuncRef>,
    consequential_inline: Vec<sonatina_ir::module::FuncRef>,
}

fn force_inline_helper_names() -> Result<Vec<String>, LowerError> {
    parse_force_inline_helper_names(std::env::var_os("FE_BLOAT_FORCE_INLINE_HELPERS"))
}

fn parse_force_inline_helper_names(
    raw: Option<std::ffi::OsString>,
) -> Result<Vec<String>, LowerError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let raw = raw.into_string().map_err(|_| {
        LowerError::Spirv("FE_BLOAT_FORCE_INLINE_HELPERS is not valid UTF-8".into())
    })?;
    let mut names = Vec::new();
    for name in raw.split(',').map(str::trim) {
        if name.is_empty() {
            return Err(LowerError::Spirv(
                "FE_BLOAT_FORCE_INLINE_HELPERS contains an empty helper name".into(),
            ));
        }
        if names.iter().any(|existing| existing == name) {
            return Err(LowerError::Spirv(format!(
                "FE_BLOAT_FORCE_INLINE_HELPERS repeats helper `{name}`"
            )));
        }
        names.push(name.to_owned());
    }
    Ok(names)
}

/// Keep the retained set closed: a helper is independently lowerable only
/// when every direct callee is retained under the same scalar ABI.
fn close_retained_helper_set(
    module: &sonatina_ir::Module,
    selected: &mut std::collections::HashSet<sonatina_ir::module::FuncRef>,
) {
    loop {
        let rejected = selected
            .iter()
            .copied()
            .filter(|&function_ref| {
                module
                    .func_store
                    .try_view(function_ref, |function| {
                        function
                            .layout
                            .iter_block()
                            .flat_map(|block| function.layout.iter_inst(block))
                            .filter_map(|instruction| function.dfg.call_info(instruction))
                            .any(|call| !selected.contains(&call.callee()))
                    })
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();
        if rejected.is_empty() {
            break;
        }
        for function in rejected {
            selected.remove(&function);
        }
    }
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
        let expanded_calls = expansion_counts
            .get(&function_ref)
            .copied()
            .unwrap_or_default();
        let Some((instruction_count, repeated_loop)) =
            module.func_store.try_view(function_ref, |function| {
                let instruction_count = function
                    .layout
                    .iter_block()
                    .map(|block| function.layout.iter_inst(block).count())
                    .sum::<usize>();
                // A loop is substantive work even when its buffer operations are
                // delegated to another helper. Preserve repeated loop bodies rather
                // than classifying them as tiny resource-passing wrappers. This is
                // only a cost preference: normal ABI, effect and structurizer checks
                // still decide whether the function can actually remain callable.
                let repeated_loop = expanded_calls > 1 && {
                    let mut cfg = sonatina_ir::ControlFlowGraph::default();
                    cfg.compute(function);
                    let mut domtree = sonatina_codegen::domtree::DomTree::new();
                    domtree.compute(&cfg);
                    let mut loops = sonatina_codegen::loop_analysis::LoopTree::new();
                    loops.compute(&cfg, &domtree);
                    loops.loops().next().is_some()
                };
                (instruction_count, repeated_loop)
            })
        else {
            continue;
        };
        if spirv_resource_passthrough_outline_worthy(expanded_calls, instruction_count)
            || repeated_loop
        {
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
    entries: &[sonatina_ir::module::FuncRef],
    preserved_helpers: &std::collections::HashSet<sonatina_ir::module::FuncRef>,
) -> Result<(), LowerError> {
    let Some(&entry) = entries.first() else {
        return Err(LowerError::Spirv(
            "SPIR-V module has no runtime section entry".to_owned(),
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

#[cfg(test)]
mod bloat_policy_tests {
    use super::{parse_force_inline_helper_names, validate_force_inline_resolution};

    #[test]
    fn named_force_inline_policy_is_explicit_and_deduplicated() {
        assert!(parse_force_inline_helper_names(None).unwrap().is_empty());
        assert_eq!(
            parse_force_inline_helper_names(Some("mix_words, helper_b".into())).unwrap(),
            ["mix_words", "helper_b"]
        );
        assert!(
            parse_force_inline_helper_names(Some("mix_words,mix_words".into()))
                .unwrap_err()
                .to_string()
                .contains("repeats")
        );
        assert!(
            parse_force_inline_helper_names(Some("mix_words,".into()))
                .unwrap_err()
                .to_string()
                .contains("empty")
        );
    }

    #[test]
    fn named_policy_rejects_unknown_rejected_unretained_and_ambiguous_helpers() {
        assert!(
            validate_force_inline_resolution("missing", 0, &[], false)
                .unwrap_err()
                .to_string()
                .contains("unknown")
        );
        assert!(
            validate_force_inline_resolution("bad_abi", 0, &["unsupported pointer"], false)
                .unwrap_err()
                .to_string()
                .contains("rejected by the backend")
        );
        assert!(
            validate_force_inline_resolution("inline_already", 1, &[], false)
                .unwrap_err()
                .to_string()
                .contains("not retained")
        );
        assert!(
            validate_force_inline_resolution("duplicate", 2, &[], true)
                .unwrap_err()
                .to_string()
                .contains("ambiguous")
        );
        validate_force_inline_resolution("mix_words", 1, &[], true).unwrap();
    }
}
