//! Executed exactness gate for the Fe-authored Mandelbrot proof pass graph.
//!
//! The test compiles the real actor, executes every compute pass in manifest
//! order, honors each Fe-derived repeat count, and reads back only test
//! evidence. The expected LDE uses a direct DFT and the expected roots use the
//! independent Plonky3 Poseidon2 implementation.

use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WebBinding, WebBindingAccess, WebBindingRole, WebBuildOptions, WebBundle, WebBundleMode,
    WebScalarKind, resolve_web_entry,
};
use hir::hir_def::HirIngot;
use p3_baby_bear::{
    BABYBEAR_POSEIDON2_RC_16_EXTERNAL_FINAL, BABYBEAR_POSEIDON2_RC_16_EXTERNAL_INITIAL,
    BABYBEAR_POSEIDON2_RC_16_INTERNAL, BabyBear, default_babybear_poseidon2_16,
};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_symmetric::Permutation;
use serde::Deserialize;
use sonatina_codegen::isa::spirv::{
    Access, SpirvBuiltinArgument, SpirvBuiltinSource, SpirvExternalResource, SpirvResourceElement,
    SpirvScalarKind,
};
use url::Url;

const MODULUS: u32 = 2_013_265_921;
const TWO_ADICITY: u32 = 27;
const TRACE_ROWS: usize = 4;
const LDE_ROWS: usize = 16;
const COLUMN_COUNT: usize = 4;
const MAIN_COLUMN_COUNT: usize = 17;
const AUXILIARY_COLUMN_COUNT: usize = 411;
const AIR_COLUMN_COUNT: usize = MAIN_COLUMN_COUNT + AUXILIARY_COLUMN_COUNT;
const AIR_TRACE_WORDS: usize = AIR_COLUMN_COUNT * TRACE_ROWS;
const AIR_LDE_WORDS: usize = AIR_COLUMN_COUNT * LDE_ROWS;
const AIR_INPUT_GRID_LANES: usize = AIR_COLUMN_COUNT * TRACE_ROWS / 2;
const AIR_OUTPUT_GRID_LANES: usize = AIR_COLUMN_COUNT * LDE_ROWS / 2;
const LDE_START: usize = 16;
const CLEAN_ROOT: usize = 80;
const OBSERVED_ROOT: usize = 88;
const TRACE_VALID: usize = 96;
const LDE_VALID_START: usize = 97;
const ROOTS_EQUAL: usize = 101;
const MODE_CORRECT: usize = 102;
const CLEAN_COMMIT_STATE: usize = 107;
const OBSERVED_COMMIT_STATE: usize = 187;
const COMMIT_CURSOR: usize = 32;
const COMMIT_BLOCK: usize = 48;
const COMMIT_VALID: usize = 64;
const POSEIDON_WIDTH: usize = 16;
const DONE_BLOCK: u32 = 9;
const ROUND_CONSTANT_COUNT: usize = 8 * POSEIDON_WIDTH + 13;
const PARAMETER_START: usize = 267;
const PARAMETER_END: usize = PARAMETER_START + ROUND_CONSTANT_COUNT;
const FRI_ROUNDS: usize = 4;
const FRI_VARIANTS: usize = 2;
const EXTENSION_WORDS: usize = 4;
const FRI_EVALUATIONS: usize = 15;
const FRI_EVALUATION_WORDS: usize = FRI_EVALUATIONS * EXTENSION_WORDS;
const FRI_CLEAN: usize = PARAMETER_END;
const FRI_OBSERVED: usize = FRI_CLEAN + FRI_EVALUATION_WORDS;
const FRI_CHALLENGES: usize = FRI_OBSERVED + FRI_EVALUATION_WORDS;
const FRI_CHALLENGE_WORDS: usize = FRI_VARIANTS * FRI_ROUNDS * EXTENSION_WORDS;
const FRI_ROOTS: usize = FRI_CHALLENGES + FRI_CHALLENGE_WORDS;
const FRI_ROOT_WORDS: usize = FRI_VARIANTS * FRI_ROUNDS * 8;
const FRI_TRANSCRIPTS: usize = FRI_ROOTS + FRI_ROOT_WORDS;
const FRI_TRANSCRIPT_WORDS: usize = FRI_VARIANTS * (FRI_ROUNDS + 1) * 8;
const FRI_STATUS: usize = FRI_TRANSCRIPTS + FRI_TRANSCRIPT_WORDS;
const FRI_CHALLENGE_VALID: usize = FRI_STATUS;
const FRI_ROOT_VALID: usize = FRI_CHALLENGE_VALID + FRI_VARIANTS * FRI_ROUNDS;
const FRI_TRANSCRIPT_VALID: usize = FRI_ROOT_VALID + FRI_VARIANTS * FRI_ROUNDS;
const FRI_ROUND_VALID: usize = FRI_TRANSCRIPT_VALID + FRI_VARIANTS * (FRI_ROUNDS + 1);
const FRI_EQUAL: usize = FRI_ROUND_VALID + FRI_VARIANTS * FRI_ROUNDS;
const FRI_CORRECT: usize = FRI_EQUAL + 1;
const FRI_COLOR: usize = FRI_CORRECT + 1;
const FRI_QUERY_INDICES: usize = FRI_COLOR + 1;
const FRI_QUERY_EVALUATIONS: usize = FRI_QUERY_INDICES + FRI_VARIANTS;
const FRI_QUERY_EVALUATIONS_PER_VARIANT: usize = 7;
const FRI_QUERY_EVALUATION_WORDS: usize =
    FRI_VARIANTS * FRI_QUERY_EVALUATIONS_PER_VARIANT * EXTENSION_WORDS;
const FRI_QUERY_SIBLINGS: usize = FRI_QUERY_EVALUATIONS + FRI_QUERY_EVALUATION_WORDS;
const FRI_QUERY_SIBLINGS_PER_VARIANT: usize = 6;
const FRI_QUERY_SIBLING_WORDS: usize = FRI_VARIANTS * FRI_QUERY_SIBLINGS_PER_VARIANT * 8;
const FRI_QUERY_STATUS: usize = FRI_QUERY_SIBLINGS + FRI_QUERY_SIBLING_WORDS;
const FRI_QUERY_INDEX_VALID: usize = FRI_QUERY_STATUS;
const FRI_QUERY_OPENING_VALID: usize = FRI_QUERY_INDEX_VALID + FRI_VARIANTS;
const FRI_QUERY_EQUAL: usize = FRI_QUERY_OPENING_VALID + FRI_VARIANTS;
const FRI_QUERY_CORRECT: usize = FRI_QUERY_EQUAL + 1;
const FRI_QUERY_COLOR: usize = FRI_QUERY_CORRECT + 1;
const AIR_TRACE_START: usize = FRI_QUERY_COLOR + 1;
const AIR_LDE_VALID_START: usize = AIR_TRACE_START + AIR_TRACE_WORDS;
const MAIN_LDE_ROOT: usize = AIR_LDE_VALID_START + AIR_COLUMN_COUNT;
const AUXILIARY_LDE_ROOT: usize = MAIN_LDE_ROOT + 8;
const AIR_LDE_ROOT_VALID: usize = AUXILIARY_LDE_ROOT + 8;
const PACKED_MAIN_TRACE: usize = AIR_LDE_ROOT_VALID + 2;
const PACKED_MAIN_TRACE_WORDS: usize = TRACE_ROWS * 7;
const PACKED_AUXILIARY_TRACE: usize = PACKED_MAIN_TRACE + PACKED_MAIN_TRACE_WORDS;
const PACKED_AUXILIARY_TRACE_WORDS: usize = TRACE_ROWS * 14;
const PACKED_PUBLIC: usize = PACKED_AUXILIARY_TRACE + PACKED_AUXILIARY_TRACE_WORDS;
const PACKED_PUBLIC_WORDS: usize = 4;
const MAIN_TRACE_ROOT: usize = PACKED_PUBLIC + PACKED_PUBLIC_WORDS;
const AUXILIARY_TRACE_ROOT: usize = MAIN_TRACE_ROOT + 8;
const PUBLIC_DIGEST: usize = AUXILIARY_TRACE_ROOT + 8;
const PRODUCTION_TRACE_DIGEST_VALID: usize = PUBLIC_DIGEST + 8;
const AIR_TRANSCRIPT: usize = PRODUCTION_TRACE_DIGEST_VALID + 3;
const AIR_TRANSCRIPT_VALID: usize = AIR_TRANSCRIPT + 8;
const CANONICAL_PUBLIC: usize = AIR_TRANSCRIPT_VALID + 1;
const CANONICAL_PUBLIC_WORDS: usize = 8;
const COMPOSITION_CHALLENGE: usize = CANONICAL_PUBLIC + CANONICAL_PUBLIC_WORDS;
const COMPOSITION_VALUES: usize = COMPOSITION_CHALLENGE + EXTENSION_WORDS;
const COMPOSITION_VALUE_WORDS: usize = LDE_ROWS * EXTENSION_WORDS;
const COMPOSITION_ROOT: usize = COMPOSITION_VALUES + COMPOSITION_VALUE_WORDS;
const COMPOSITION_TRANSCRIPT: usize = COMPOSITION_ROOT + 8;
const COMPOSITION_VALUE_VALID: usize = COMPOSITION_TRANSCRIPT + 8;
const COMPOSITION_VALID: usize = COMPOSITION_VALUE_VALID + LDE_ROWS;
const PROOF_WORDS: usize = COMPOSITION_VALID + 3;
const TAMPER_LDE_FIELD: usize = 17;
const DOMAIN: [u8; 4] = *b"MGDL";
const COMPUTE_PASSES: usize = 48;
const MAIN_LDE_COMMITMENT_PASS: usize = 7;
const AUXILIARY_LDE_COMMITMENT_PASS: usize = 9;
const AIR_LDE_TREE_PASS: usize = 11;
const TRACE_COMMITMENT_PASS: usize = 13;
const TRACE_TREE_PASS: usize = 15;
const AIR_TRANSCRIPT_PASS: usize = 17;
const COMPOSITION_CHALLENGE_PASS: usize = 19;
const COMPOSITION_EVALUATION_FIRST_PASS: usize = 20;
const COMPOSITION_EVALUATION_PASSES: usize = 11;
const COMPOSITION_COMMITMENT_PASS: usize = 32;
const COMPOSITION_TREE_PASS: usize = 34;
const COMPOSITION_TRANSCRIPT_PASS: usize = 36;
const COMMITMENT_PASS: usize = 38;
const FRI_FIRST_PASS: usize = 41;
const FRI_QUERY_SAMPLE_PASS: usize = 45;
const FRI_QUERY_EXTRACT_PASS: usize = 46;
const FRI_ROUND_REPEATS: [u32; FRI_ROUNDS] = [403, 358, 313, 268];
const EXT_NONRESIDUE: u32 = 11;
const BROWSER_RECEIPTS: &str = "MB2_BROWSER_PROOF_RECEIPTS";
const FRI_HASH_STATE_WORDS: usize = 16 * 2 * POSEIDON_WIDTH;
const FRI_HASH_VALID_WORDS: usize = 16;
const FRI_HASH_TAIL_WORDS: usize = FRI_VARIANTS * 10;
const FRI_TREE_WORDS: usize = FRI_VARIANTS * 31 * 8;
const FRI_TREE_VALID_WORDS: usize = FRI_VARIANTS * 31;
const FRI_PROGRESS_WORDS: usize = FRI_ROUNDS * 256;
const FRI_QUERY_PROGRESS_WORDS: usize = 256;
const FRI_QUERY_COPY_WORDS: usize = FRI_QUERY_EVALUATION_WORDS + FRI_QUERY_SIBLING_WORDS;
const COMPOSITION_FOLD_SCRATCH_START: usize = FRI_HASH_STATE_WORDS
    + FRI_HASH_VALID_WORDS
    + FRI_HASH_TAIL_WORDS
    + FRI_TREE_WORDS
    + FRI_TREE_VALID_WORDS
    + FRI_PROGRESS_WORDS
    + FRI_QUERY_PROGRESS_WORDS
    + FRI_QUERY_COPY_WORDS;
const COMPOSITION_FOLD_SCRATCH_WORDS: usize = LDE_ROWS * 2 * EXTENSION_WORDS;
const COMPOSITION_ALL_ROWS_SCRATCH_START: usize =
    COMPOSITION_FOLD_SCRATCH_START + COMPOSITION_FOLD_SCRATCH_WORDS;
const COMPOSITION_PAIR_ROWS_SCRATCH_START: usize =
    COMPOSITION_ALL_ROWS_SCRATCH_START + LDE_ROWS * EXTENSION_WORDS;
const COMPOSITION_FIRST_ROW_SCRATCH_START: usize =
    COMPOSITION_PAIR_ROWS_SCRATCH_START + LDE_ROWS * EXTENSION_WORDS;
const COMPOSITION_FOLD_VALID_SCRATCH_START: usize =
    COMPOSITION_FIRST_ROW_SCRATCH_START + LDE_ROWS * EXTENSION_WORDS;
const FRI_SCRATCH_WORDS: usize = COMPOSITION_FOLD_VALID_SCRATCH_START + LDE_ROWS;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repo-root ancestor")
        .to_path_buf()
}

fn compile_proof_graph() -> WebBundle {
    let trace = std::env::var_os("FE_WEB_STAGE_TRACE").is_some();
    let started = std::time::Instant::now();
    let checkpoint = |phase: &str| {
        if trace {
            eprintln!(
                "[mandelbrot proof compile] phase={phase}, elapsed_ms={}",
                started.elapsed().as_millis()
            );
        }
    };
    let dir = repo_root().join("demos/sketches/mandelbrot_proof_gpu");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "Mandelbrot proof GPU ingot initialization diagnostics"
    );
    checkpoint("ingot_initialized");
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("Mandelbrot proof GPU ingot should resolve");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "Mandelbrot proof GPU source diagnostics:\n{diagnostics}"
    );
    checkpoint("source_diagnostics_clean");
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    checkpoint("entry_resolved");
    let bundle = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("demos/sketches/mandelbrot_proof_gpu".into())),
    )
    .expect("Mandelbrot proof actor should compile into a WebBundle");
    checkpoint("bundle_compiled");
    bundle
}

fn compute_invocation_builtin_arguments() -> Vec<SpirvBuiltinArgument> {
    use SpirvBuiltinSource as Source;
    [
        Source::GlobalInvocationIdX,
        Source::GlobalInvocationIdY,
        Source::GlobalInvocationIdZ,
        Source::LocalInvocationIdX,
        Source::LocalInvocationIdY,
        Source::LocalInvocationIdZ,
        Source::WorkgroupIdX,
        Source::WorkgroupIdY,
        Source::WorkgroupIdZ,
        Source::NumWorkgroupsX,
        Source::NumWorkgroupsY,
        Source::NumWorkgroupsZ,
        Source::LocalInvocationIndex,
    ]
    .into_iter()
    .enumerate()
    .map(|(arg_index, source)| SpirvBuiltinArgument {
        arg_index: arg_index as u32,
        source,
    })
    .collect()
}

fn proof_gpu_resources(arg_offset: u32) -> Vec<SpirvExternalResource> {
    [
        ("proof", PROOF_WORDS as u32),
        ("lde_inverse_values", AIR_TRACE_WORDS as u32),
        ("lde_inverse_progress", AIR_INPUT_GRID_LANES as u32),
        ("lde_values", AIR_LDE_WORDS as u32),
        ("lde_progress", AIR_OUTPUT_GRID_LANES as u32),
        ("fri_scratch", 2_874),
    ]
    .into_iter()
    .enumerate()
    .map(|(binding, (name, length))| SpirvExternalResource {
        arg_index: arg_offset + binding as u32,
        group: 0,
        binding: binding as u32,
        name: name.to_owned(),
        access: Access::ReadWrite,
        element: SpirvResourceElement::Scalar(SpirvScalarKind::U32),
        stride: 4,
        length,
    })
    .collect()
}

fn compile_proof_compute_stage(
    entry: &str,
    uses_invocation_context: bool,
) -> sonatina_codegen::isa::spirv::SpirvArtifact {
    let dir = repo_root().join("demos/sketches/mandelbrot_proof_gpu");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "Mandelbrot proof GPU ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("Mandelbrot proof GPU ingot should resolve");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "Mandelbrot proof GPU source diagnostics:\n{diagnostics}"
    );
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, entry)
        .unwrap_or_else(|error| panic!("{entry} runtime package: {error}"));
    let builtins = uses_invocation_context
        .then(compute_invocation_builtin_arguments)
        .unwrap_or_default();
    let resource_arg_offset = u32::try_from(builtins.len()).expect("builtin count fits u32");
    fe_codegen::compile_runtime_package_spirv_compute_with_interface(
        &db,
        &package,
        [1, 1, 1],
        [1, 1, 1],
        &proof_gpu_resources(resource_arg_offset),
        &builtins,
    )
    .unwrap_or_else(|error| panic!("{entry} browser WebGPU lowering: {error}"))
}

fn request_browser_profile_device() -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let allow_skip = std::env::var_os("MB2_ALLOW_GPU_SKIP").is_some();
    let instance = wgpu::Instance::default();
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        ..Default::default()
    })) {
        Ok(adapter) => adapter,
        Err(error) if allow_skip => {
            eprintln!("  Mandelbrot proof graph SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
            return None;
        }
        Err(error) => panic!(
            "Mandelbrot proof graph has no WebGPU adapter ({error:?}). Set up Vulkan/lavapipe, \
             or set MB2_ALLOW_GPU_SKIP to record an explicit non-execution."
        ),
    };
    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            ..Default::default()
        })) {
            Ok(pair) => pair,
            Err(error) if allow_skip => {
                eprintln!("  Mandelbrot proof graph SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
                return None;
            }
            Err(error) => panic!("Mandelbrot proof device request failed: {error:?}"),
        };
    Some((adapter, device, queue))
}

fn buffer_type(binding: &WebBinding) -> wgpu::BufferBindingType {
    wgpu::BufferBindingType::Storage {
        read_only: binding.access == WebBindingAccess::Read,
    }
}

struct ComputeKernel {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

struct BoundPass {
    group: wgpu::BindGroup,
    buffers: Vec<(u32, WebBindingRole, wgpu::Buffer)>,
}

struct DeviceResource {
    name: String,
    buffer: wgpu::Buffer,
}

fn compile_kernels(device: &wgpu::Device, bundle: &WebBundle) -> Vec<ComputeKernel> {
    bundle.manifest.passes[..COMPUTE_PASSES]
        .iter()
        .zip(&bundle.pass_wgsl[..COMPUTE_PASSES])
        .map(|(pass, shader)| {
            let entries = pass
                .layout
                .bindings
                .iter()
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding: binding.binding,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: buffer_type(binding),
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                })
                .collect::<Vec<_>>();
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(pass.source_entry.as_str()),
                entries: &entries,
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(pass.source_entry.as_str()),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(pass.source_entry.as_str()),
                source: wgpu::ShaderSource::Wgsl(shader.source.as_str().into()),
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(pass.source_entry.as_str()),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
            ComputeKernel { layout, pipeline }
        })
        .collect()
}

fn allocate_resources(device: &wgpu::Device, bundle: &WebBundle) -> Vec<DeviceResource> {
    bundle
        .manifest
        .resources
        .iter()
        .map(|resource| DeviceResource {
            name: resource.name.clone(),
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(resource.name.as_str()),
                size: u64::from(resource.length) * 4,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        })
        .collect()
}

fn resource<'a>(resources: &'a [DeviceResource], name: &str) -> &'a wgpu::Buffer {
    &resources
        .iter()
        .find(|resource| resource.name == name)
        .unwrap_or_else(|| panic!("missing actor resource `{name}`"))
        .buffer
}

fn largest_wgsl_functions(source: &str, count: usize) -> Vec<(&str, usize)> {
    let mut starts = Vec::new();
    if source.starts_with("fn ") {
        starts.push(0);
    }
    starts.extend(source.match_indices("\nfn ").map(|(index, _)| index + 1));
    let mut functions = starts
        .iter()
        .enumerate()
        .map(|(index, start)| {
            let end = starts.get(index + 1).copied().unwrap_or(source.len());
            let name_start = start + 3;
            let name_end = source[name_start..]
                .find('(')
                .map(|offset| name_start + offset)
                .unwrap_or(end);
            (&source[name_start..name_end], end - start)
        })
        .collect::<Vec<_>>();
    functions.sort_unstable_by_key(|(_, bytes)| std::cmp::Reverse(*bytes));
    functions.truncate(count);
    functions
}

fn scalar_input(binding: &WebBinding, tamper: f32) -> Vec<u8> {
    let mut bytes = vec![0u8; binding.span as usize];
    for member in &binding.members {
        assert_eq!(member.scalar, WebScalarKind::F32);
        assert_eq!(member.width, 4);
        let value = match member.name.as_str() {
            "tamper" => tamper,
            "res" => 512.0,
            other => panic!("unexpected proof actor scalar input `{other}`"),
        };
        let start = member.offset as usize;
        bytes[start..start + 4].copy_from_slice(&value.to_bits().to_le_bytes());
    }
    bytes
}

fn bind_case(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bundle: &WebBundle,
    kernels: &[ComputeKernel],
    resources: &[DeviceResource],
    tamper: f32,
) -> Vec<BoundPass> {
    bundle.manifest.passes[..COMPUTE_PASSES]
        .iter()
        .zip(kernels)
        .map(|(pass, kernel)| {
            let buffers = pass
                .layout
                .bindings
                .iter()
                .filter(|binding| binding.role != WebBindingRole::Resource)
                .map(|binding| {
                    let usage = match binding.role {
                        WebBindingRole::Input => {
                            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST
                        }
                        WebBindingRole::Output => {
                            wgpu::BufferUsages::STORAGE
                                | wgpu::BufferUsages::COPY_SRC
                                | wgpu::BufferUsages::COPY_DST
                        }
                        WebBindingRole::Resource => unreachable!(),
                    };
                    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(binding.name.as_str()),
                        size: u64::from(binding.span),
                        usage,
                        mapped_at_creation: false,
                    });
                    if binding.role == WebBindingRole::Input {
                        queue.write_buffer(&buffer, 0, &scalar_input(binding, tamper));
                    }
                    (binding.binding, binding.role, buffer)
                })
                .collect::<Vec<_>>();
            let entries = pass
                .layout
                .bindings
                .iter()
                .map(|binding| {
                    let resource = if binding.role == WebBindingRole::Resource {
                        resource(resources, binding.name.as_str()).as_entire_binding()
                    } else {
                        buffers
                            .iter()
                            .find(|(slot, _, _)| *slot == binding.binding)
                            .expect("owned pass binding")
                            .2
                            .as_entire_binding()
                    };
                    wgpu::BindGroupEntry {
                        binding: binding.binding,
                        resource,
                    }
                })
                .collect::<Vec<_>>();
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(pass.source_entry.as_str()),
                layout: &kernel.layout,
                entries: &entries,
            });
            BoundPass { group, buffers }
        })
        .collect()
}

fn map_bytes(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(1_800)),
        })
        .expect("Mandelbrot proof WebGPU submission should complete");
    rx.recv()
        .expect("map callback should fire")
        .expect("test-only staging buffer should map");
    let bytes = slice.get_mapped_range().to_vec();
    buffer.unmap();
    bytes
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one u32")))
        .collect()
}

struct ExecutionReceipt {
    proof: Vec<u32>,
    air_trace: Vec<u32>,
    air_lde_valid: Vec<u32>,
    air_lde: Vec<u32>,
    traps: Vec<u32>,
}

fn execute_case(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bundle: &WebBundle,
    kernels: &[ComputeKernel],
    tamper: f32,
) -> ExecutionReceipt {
    let proof_bytes = (PROOF_WORDS * 4) as u64;
    let air_lde_bytes = (AIR_LDE_WORDS * 4) as u64;
    let resources = allocate_resources(device, bundle);
    let proof = resource(&resources, "proof");
    let air_lde = resource(&resources, "lde_values");
    let bound = bind_case(device, queue, bundle, kernels, &resources, tamper);
    let trap_bytes = bound
        .iter()
        .flat_map(|pass| &pass.buffers)
        .filter(|(_, role, _)| *role == WebBindingRole::Output)
        .map(|(_, _, buffer)| buffer.size())
        .sum::<u64>();
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Mandelbrot proof test-only readback"),
        size: proof_bytes + air_lde_bytes + trap_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Fe Mandelbrot proof graph"),
    });
    for ((manifest_pass, kernel), resources) in bundle.manifest.passes[..COMPUTE_PASSES]
        .iter()
        .zip(kernels)
        .zip(&bound)
    {
        let dispatch = manifest_pass.dispatch.expect("compute dispatch");
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(manifest_pass.source_entry.as_str()),
            timestamp_writes: None,
        });
        pass.set_pipeline(&kernel.pipeline);
        pass.set_bind_group(0, &resources.group, &[]);
        for _ in 0..manifest_pass.repeat {
            pass.dispatch_workgroups(dispatch[0], dispatch[1], dispatch[2]);
        }
        drop(pass);
    }
    encoder.copy_buffer_to_buffer(&proof, 0, &staging, 0, proof_bytes);
    let air_lde_offset = proof_bytes;
    encoder.copy_buffer_to_buffer(&air_lde, 0, &staging, air_lde_offset, air_lde_bytes);
    let mut trap_offset = air_lde_offset + air_lde_bytes;
    for (_, _, buffer) in bound
        .iter()
        .flat_map(|pass| &pass.buffers)
        .filter(|(_, role, _)| *role == WebBindingRole::Output)
    {
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, trap_offset, buffer.size());
        trap_offset += buffer.size();
    }
    assert_eq!(trap_offset, proof_bytes + air_lde_bytes + trap_bytes,);
    queue.submit(Some(encoder.finish()));

    let bytes = map_bytes(device, &staging);
    let air_lde_end = (air_lde_offset + air_lde_bytes) as usize;
    let proof_words = words(&bytes[..proof_bytes as usize]);
    ExecutionReceipt {
        air_trace: proof_words[AIR_TRACE_START..AIR_LDE_VALID_START].to_vec(),
        air_lde_valid: proof_words[AIR_LDE_VALID_START..MAIN_LDE_ROOT].to_vec(),
        proof: proof_words,
        air_lde: words(&bytes[air_lde_offset as usize..air_lde_end]),
        traps: words(&bytes[air_lde_end..]),
    }
}

fn pow_mod(mut base: u64, mut exponent: u32) -> u32 {
    let modulus = u64::from(MODULUS);
    base %= modulus;
    let mut result = 1u64;
    while exponent != 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus;
        }
        base = base * base % modulus;
        exponent >>= 1;
    }
    result as u32
}

fn add_mod(left: u32, right: u32) -> u32 {
    ((u64::from(left) + u64::from(right)) % u64::from(MODULUS)) as u32
}

fn sub_mod(left: u32, right: u32) -> u32 {
    ((u64::from(left) + u64::from(MODULUS) - u64::from(right)) % u64::from(MODULUS)) as u32
}

fn mul_mod(left: u32, right: u32) -> u32 {
    (u64::from(left) * u64::from(right) % u64::from(MODULUS)) as u32
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Ext4([u32; 4]);

impl Ext4 {
    fn zero() -> Self {
        Self([0; 4])
    }

    fn one() -> Self {
        Self::from_base(1)
    }

    fn from_base(value: u32) -> Self {
        Self([value % MODULUS, 0, 0, 0])
    }

    fn add(self, other: Self) -> Self {
        Self(std::array::from_fn(|index| {
            add_mod(self.0[index], other.0[index])
        }))
    }

    fn sub(self, other: Self) -> Self {
        Self(std::array::from_fn(|index| {
            sub_mod(self.0[index], other.0[index])
        }))
    }

    fn scale(self, scalar: u32) -> Self {
        Self(self.0.map(|coefficient| mul_mod(coefficient, scalar)))
    }

    fn mul(self, other: Self) -> Self {
        let mut coefficients = [0u32; 7];
        for left in 0..4 {
            for right in 0..4 {
                coefficients[left + right] = add_mod(
                    coefficients[left + right],
                    mul_mod(self.0[left], other.0[right]),
                );
            }
        }
        for degree in (4..=6).rev() {
            coefficients[degree - 4] = add_mod(
                coefficients[degree - 4],
                mul_mod(coefficients[degree], EXT_NONRESIDUE),
            );
        }
        Self(
            coefficients[..4]
                .try_into()
                .expect("four extension coefficients"),
        )
    }

    fn pow(self, mut exponent: u32) -> Self {
        let mut base = self;
        let mut result = Self::one();
        while exponent != 0 {
            if exponent & 1 == 1 {
                result = result.mul(base);
            }
            base = base.mul(base);
            exponent >>= 1;
        }
        result
    }
}

fn direct_ntt(values: &[u32], inverse: bool) -> Vec<u32> {
    let log_n = values.len().ilog2();
    let maximal_root = pow_mod(31, 15);
    let mut root = pow_mod(u64::from(maximal_root), 1 << (TWO_ADICITY - log_n));
    if inverse {
        root = pow_mod(u64::from(root), MODULUS - 2);
    }
    let modulus = u64::from(MODULUS);
    let mut output = vec![0u32; values.len()];
    for (index, slot) in output.iter_mut().enumerate() {
        let point = pow_mod(u64::from(root), index as u32);
        let mut power = 1u64;
        let mut sum = 0u64;
        for value in values {
            sum = (sum + u64::from(*value) * power) % modulus;
            power = power * u64::from(point) % modulus;
        }
        *slot = sum as u32;
    }
    if inverse {
        let scale = pow_mod(values.len() as u64, MODULUS - 2);
        for value in &mut output {
            *value = (u64::from(*value) * u64::from(scale) % modulus) as u32;
        }
    }
    output
}

fn direct_coset_lde(values: &[u32], output_len: usize, shift: u32) -> Vec<u32> {
    let coefficients = direct_ntt(values, true);
    let maximal_root = pow_mod(31, 15);
    let root = pow_mod(
        u64::from(maximal_root),
        1 << (TWO_ADICITY - output_len.ilog2()),
    );
    let modulus = u64::from(MODULUS);
    (0..output_len)
        .map(|index| {
            let point =
                u64::from(shift) * u64::from(pow_mod(u64::from(root), index as u32)) % modulus;
            coefficients
                .iter()
                .fold((0u64, 1u64), |(sum, power), value| {
                    (
                        (sum + u64::from(*value) * power) % modulus,
                        power * point % modulus,
                    )
                })
                .0 as u32
        })
        .collect()
}

fn trace_columns() -> [[u32; TRACE_ROWS]; COLUMN_COUNT] {
    [
        [0, 1, 2, 3],
        [0, 9_437_184, 28_901_376, 102_576_384],
        [1, 1, 1, 1],
        [0, 0, 0, 1],
    ]
}

fn signed_magnitude(value: i32) -> (u32, u32) {
    if value < 0 {
        (1, value.unsigned_abs())
    } else {
        (0, value as u32)
    }
}

/// Independent Rust expansion of the canonical Q12 witness. This deliberately
/// does not call the Fe scalar prover or decode a generated artifact.
fn reference_air_rows() -> [[u32; MAIN_COLUMN_COUNT]; TRACE_ROWS] {
    const Q12_SCALE: i32 = 4_096;
    const ESCAPE_RADIUS_SQUARED_Q24: i32 = 67_108_864;
    let mut rows = [[0u32; MAIN_COLUMN_COUNT]; TRACE_ROWS];
    let mut z_re = 0i32;
    let mut z_im = 0i32;
    for (step, row) in rows.iter_mut().enumerate() {
        let rr = z_re * z_re;
        let ii = z_im * z_im;
        let magnitude = rr + ii;
        let real_numerator = rr - ii;
        let q_re = real_numerator >> 12;
        let r_re = real_numerator - q_re * Q12_SCALE;
        let imaginary_numerator = (z_re * 2) * z_im;
        let q_im = imaginary_numerator >> 12;
        let r_im = imaginary_numerator - q_im * Q12_SCALE;
        let (zr_sign, zr) = signed_magnitude(z_re);
        let (zi_sign, zi) = signed_magnitude(z_im);
        let (q_re_sign, q_re_magnitude) = signed_magnitude(q_re);
        let (q_im_sign, q_im_magnitude) = signed_magnitude(q_im);
        let terminal = u32::from(magnitude >= ESCAPE_RADIUS_SQUARED_Q24);
        *row = [
            step as u32,
            zr_sign,
            zr,
            zi_sign,
            zi,
            rr as u32,
            ii as u32,
            magnitude as u32,
            q_re_sign,
            q_re_magnitude,
            r_re as u32,
            q_im_sign,
            q_im_magnitude,
            r_im as u32,
            terminal,
            1,
            terminal,
        ];
        if step + 1 < TRACE_ROWS {
            z_re = q_re + 3_072;
            z_im = q_im;
        }
    }
    assert_eq!(rows[TRACE_ROWS - 1][16], 1);
    rows
}

fn append_range_witness(output: &mut Vec<u32>, value: u32, width: usize) {
    for bit in 0..width {
        output.push((value >> bit) & 1);
    }
    let mut seen = 0;
    for bit in 0..width {
        seen |= (value >> bit) & 1;
        output.push(seen);
    }
}

fn reference_auxiliary_row(row: &[u32; MAIN_COLUMN_COUNT]) -> Vec<u32> {
    let mut output = Vec::with_capacity(AUXILIARY_COLUMN_COUNT);
    for (column, width) in [
        (0, 21),
        (2, 15),
        (4, 15),
        (5, 30),
        (6, 30),
        (7, 31),
        (9, 18),
        (10, 12),
        (12, 19),
        (13, 12),
    ] {
        append_range_witness(&mut output, row[column], width);
    }
    let mut terminal_high_any = 0;
    for bit in 26..31 {
        terminal_high_any |= (row[7] >> bit) & 1;
        output.push(terminal_high_any);
    }
    assert_eq!(output.len(), AUXILIARY_COLUMN_COUNT);
    output
}

fn reference_air_trace() -> Vec<u32> {
    let rows = reference_air_rows();
    let auxiliary = rows.iter().map(reference_auxiliary_row).collect::<Vec<_>>();
    let mut trace = Vec::with_capacity(AIR_TRACE_WORDS);
    for column in 0..MAIN_COLUMN_COUNT {
        trace.extend(rows.iter().map(|row| row[column]));
    }
    for column in 0..AUXILIARY_COLUMN_COUNT {
        trace.extend(auxiliary.iter().map(|row| row[column]));
    }
    assert_eq!(trace.len(), AIR_TRACE_WORDS);
    trace
}

fn reference_air_lde(trace: &[u32]) -> Vec<u32> {
    trace
        .chunks_exact(TRACE_ROWS)
        .flat_map(|column| direct_coset_lde(column, LDE_ROWS, 7))
        .collect()
}

fn reference_permutation(input: [u32; POSEIDON_WIDTH]) -> [u32; POSEIDON_WIDTH] {
    let mut state = input.map(BabyBear::from_u32);
    default_babybear_poseidon2_16().permute_mut(&mut state);
    state.map(|value| value.as_canonical_u32())
}

fn reference_montgomery_parameters() -> Vec<u32> {
    let mut parameters = Vec::with_capacity(ROUND_CONSTANT_COUNT);
    for round in BABYBEAR_POSEIDON2_RC_16_EXTERNAL_INITIAL {
        parameters.extend(round.map(|value| value.as_canonical_u32()));
    }
    parameters.extend(BABYBEAR_POSEIDON2_RC_16_INTERNAL.map(|value| value.as_canonical_u32()));
    for round in BABYBEAR_POSEIDON2_RC_16_EXTERNAL_FINAL {
        parameters.extend(round.map(|value| value.as_canonical_u32()));
    }
    let radix = (1u64 << 32) % u64::from(MODULUS);
    parameters
        .into_iter()
        .map(|value| (u64::from(value) * radix % u64::from(MODULUS)) as u32)
        .collect()
}

fn reference_prefixed_commitment(tag: &[u8; 4], prefix: u32, fields: &[u32]) -> [u32; 8] {
    let mut message = vec![u32::from_be_bytes(*tag), prefix];
    message.extend_from_slice(fields);
    let mut state = [0u32; POSEIDON_WIDTH];
    for block in message.chunks(8) {
        state[..block.len()].copy_from_slice(block);
        state = reference_permutation(state);
    }
    state[..8].try_into().expect("eight digest fields")
}

fn reference_field_commitment(tag: &[u8; 4], fields: &[u32]) -> [u32; 8] {
    reference_prefixed_commitment(tag, fields.len() as u32, fields)
}

fn reference_commitment(fields: &[u32]) -> [u32; 8] {
    reference_field_commitment(&DOMAIN, fields)
}

fn pack_bounded_words(words: &[u32], widths: &[u32], field_count: usize) -> Vec<u32> {
    assert_eq!(words.len(), widths.len());
    let mut fields = vec![0u32; field_count];
    let mut bit_length = 0usize;
    for (&word, &width) in words.iter().zip(widths) {
        assert!(width <= 32);
        if width < 32 {
            assert_eq!(word >> width, 0, "source word must fit its audited width");
        }
        assert!(bit_length + width as usize <= field_count * 30);
        let mut remaining = width as usize;
        let mut source_offset = 0usize;
        while remaining != 0 {
            let field = bit_length / 30;
            let used = bit_length % 30;
            let take = remaining.min(30 - used);
            let mask = (1u32 << take) - 1;
            let payload = (word >> source_offset) & mask;
            fields[field] |= payload << used;
            bit_length += take;
            source_offset += take;
            remaining -= take;
        }
    }
    fields
}

struct ReferenceProductionTranscript {
    packed_main: Vec<u32>,
    packed_auxiliary: Vec<u32>,
    packed_public: Vec<u32>,
    main_trace_root: [u32; 8],
    auxiliary_trace_root: [u32; 8],
    public_digest: [u32; 8],
    air_transcript: [u32; 8],
}

fn reference_production_transcript(
    main_lde_root: [u32; 8],
    auxiliary_lde_root: [u32; 8],
) -> ReferenceProductionTranscript {
    const ROW_WIDTHS: [u32; MAIN_COLUMN_COUNT] =
        [21, 1, 15, 1, 15, 30, 30, 31, 1, 18, 12, 1, 19, 12, 1, 1, 1];
    const PUBLIC_WIDTHS: [u32; 8] = [1, 14, 1, 13, 21, 21, 21, 22];
    const PUBLIC_WORDS: [u32; 8] = [0, 3_072, 0, 0, 4, 3, 4, 4];

    let rows = reference_air_rows();
    let auxiliary_rows = rows.iter().map(reference_auxiliary_row).collect::<Vec<_>>();
    let mut packed_main = Vec::with_capacity(PACKED_MAIN_TRACE_WORDS);
    let mut packed_auxiliary = Vec::with_capacity(PACKED_AUXILIARY_TRACE_WORDS);
    let mut main_leaves = Vec::with_capacity(TRACE_ROWS);
    let mut auxiliary_leaves = Vec::with_capacity(TRACE_ROWS);
    for (row, auxiliary) in rows.iter().zip(&auxiliary_rows) {
        let main_fields = pack_bounded_words(row, &ROW_WIDTHS, 7);
        let auxiliary_fields = pack_bounded_words(auxiliary, &vec![1; AUXILIARY_COLUMN_COUNT], 14);
        main_leaves.push(reference_prefixed_commitment(b"BR01", 210, &main_fields));
        auxiliary_leaves.push(reference_prefixed_commitment(
            b"BA01",
            AUXILIARY_COLUMN_COUNT as u32,
            &auxiliary_fields,
        ));
        packed_main.extend(main_fields);
        packed_auxiliary.extend(auxiliary_fields);
    }
    let packed_public = pack_bounded_words(&PUBLIC_WORDS, &PUBLIC_WIDTHS, PACKED_PUBLIC_WORDS);
    let main_trace_root = reference_digest_root(main_leaves);
    let auxiliary_trace_root = reference_digest_root(auxiliary_leaves);
    let public_digest = reference_prefixed_commitment(b"BP01", 114, &packed_public);

    let bind = |tag: &[u8; 4], left: &[u32; 8], right: &[u32; 8]| {
        let fields = left.iter().chain(right).copied().collect::<Vec<_>>();
        reference_field_commitment(tag, &fields)
    };
    let statement = bind(b"BS01", &public_digest, &main_trace_root);
    let trace = bind(b"BT01", &statement, &auxiliary_trace_root);
    let main_lde = bind(b"BL02", &trace, &main_lde_root);
    let air_transcript = bind(b"BY02", &main_lde, &auxiliary_lde_root);

    ReferenceProductionTranscript {
        packed_main,
        packed_auxiliary,
        packed_public,
        main_trace_root,
        auxiliary_trace_root,
        public_digest,
        air_transcript,
    }
}

fn protocol_round_tag(prefix: [u8; 2], round: usize) -> [u8; 4] {
    assert!((1..100).contains(&round));
    [
        prefix[0],
        prefix[1],
        b'0' + (round / 10) as u8,
        b'0' + (round % 10) as u8,
    ]
}

fn reference_compress(left: &[u32; 8], right: &[u32; 8]) -> [u32; 8] {
    let mut state = [0u32; POSEIDON_WIDTH];
    state[..8].copy_from_slice(left);
    state[8..].copy_from_slice(right);
    reference_permutation(state)[..8]
        .try_into()
        .expect("eight digest fields")
}

fn reference_digest_root(mut layer: Vec<[u32; 8]>) -> [u32; 8] {
    assert!(!layer.is_empty());
    assert!(layer.len().is_power_of_two());
    while layer.len() > 1 {
        layer = layer
            .chunks_exact(2)
            .map(|children| reference_compress(&children[0], &children[1]))
            .collect();
    }
    layer[0]
}

fn reference_air_lde_roots(air_lde: &[u32]) -> ([u32; 8], [u32; 8]) {
    assert_eq!(air_lde.len(), AIR_LDE_WORDS);
    let mut main_leaves = Vec::with_capacity(LDE_ROWS);
    let mut auxiliary_leaves = Vec::with_capacity(LDE_ROWS);
    for row in 0..LDE_ROWS {
        let main = (0..MAIN_COLUMN_COUNT)
            .map(|column| air_lde[column * LDE_ROWS + row])
            .collect::<Vec<_>>();
        let auxiliary = (0..AUXILIARY_COLUMN_COUNT)
            .map(|column| air_lde[(MAIN_COLUMN_COUNT + column) * LDE_ROWS + row])
            .collect::<Vec<_>>();
        main_leaves.push(reference_field_commitment(b"BL01", &main));
        auxiliary_leaves.push(reference_field_commitment(b"BY01", &auxiliary));
    }
    (
        reference_digest_root(main_leaves),
        reference_digest_root(auxiliary_leaves),
    )
}

#[derive(Clone, Copy)]
struct ReferenceConstraintFold {
    challenge: Ext4,
    power: Ext4,
    value: Ext4,
    absorbed: usize,
}

impl ReferenceConstraintFold {
    fn new(challenge: Ext4) -> Self {
        Self {
            challenge,
            power: Ext4::one(),
            value: Ext4::zero(),
            absorbed: 0,
        }
    }

    fn absorb(&mut self, residual: Ext4) {
        self.value = self.value.add(self.power.mul(residual));
        self.power = self.power.mul(self.challenge);
        self.absorbed += 1;
    }

    fn next_family(&mut self) {
        self.value = Ext4::zero();
    }
}

fn reference_ext_bit_residual(value: Ext4) -> Ext4 {
    value.mul(value.sub(Ext4::one()))
}

fn reference_ext_signed_value(sign: Ext4, magnitude: Ext4) -> Ext4 {
    magnitude.sub(sign.mul(magnitude).mul(Ext4::from_base(2)))
}

fn reference_absorb_residuals<const N: usize>(
    fold: &mut ReferenceConstraintFold,
    residuals: [Ext4; N],
) {
    for residual in residuals {
        fold.absorb(residual);
    }
}

fn reference_local_residuals(row: &[Ext4; MAIN_COLUMN_COUNT]) -> [Ext4; 9] {
    let zr = reference_ext_signed_value(row[1], row[2]);
    let zi = reference_ext_signed_value(row[3], row[4]);
    let q_re = reference_ext_signed_value(row[8], row[9]);
    let q_im = reference_ext_signed_value(row[11], row[12]);
    [
        reference_ext_bit_residual(row[1]),
        reference_ext_bit_residual(row[3]),
        reference_ext_bit_residual(row[8]),
        reference_ext_bit_residual(row[11]),
        row[5].sub(zr.mul(zr)),
        row[6].sub(zi.mul(zi)),
        row[7].sub(row[5]).sub(row[6]),
        row[5]
            .sub(row[6])
            .sub(Ext4::from_base(4_096).mul(q_re))
            .sub(row[10]),
        Ext4::from_base(2)
            .mul(zr)
            .mul(zi)
            .sub(Ext4::from_base(4_096).mul(q_im))
            .sub(row[13]),
    ]
}

fn reference_row_state_residuals(row: &[Ext4; MAIN_COLUMN_COUNT]) -> [Ext4; 5] {
    [
        reference_ext_bit_residual(row[15]),
        reference_ext_bit_residual(row[16]),
        reference_ext_bit_residual(row[14]),
        row[16].sub(row[15].mul(row[14])),
        Ext4::one().sub(row[15]).mul(row[14].sub(Ext4::one())),
    ]
}

fn reference_transition_residuals(
    c_re: (Ext4, Ext4, Ext4),
    c_im: (Ext4, Ext4, Ext4),
    row: &[Ext4; MAIN_COLUMN_COUNT],
    next: &[Ext4; MAIN_COLUMN_COUNT],
) -> [Ext4; 9] {
    [
        reference_ext_bit_residual(c_re.0),
        reference_ext_bit_residual(c_im.0),
        reference_ext_bit_residual(row[8]),
        reference_ext_bit_residual(row[11]),
        reference_ext_bit_residual(next[1]),
        reference_ext_bit_residual(next[3]),
        next[0].sub(row[0]).sub(Ext4::one()),
        reference_ext_signed_value(next[1], next[2])
            .sub(reference_ext_signed_value(row[8], row[9]))
            .sub(c_re.2),
        reference_ext_signed_value(next[3], next[4])
            .sub(reference_ext_signed_value(row[11], row[12]))
            .sub(c_im.2),
    ]
}

fn reference_absorb_range(
    fold: &mut ReferenceConstraintFold,
    value: Ext4,
    auxiliary: &[Ext4; AUXILIARY_COLUMN_COUNT],
    start: usize,
    width: usize,
) {
    let mut reconstructed = Ext4::zero();
    let mut weight = Ext4::one();
    let mut previous_any = Ext4::zero();
    for index in 0..width {
        let bit = auxiliary[start + index];
        let any = auxiliary[start + width + index];
        fold.absorb(reference_ext_bit_residual(bit));
        fold.absorb(reference_ext_bit_residual(any));
        fold.absorb(any.sub(previous_any).sub(bit).add(previous_any.mul(bit)));
        reconstructed = reconstructed.add(bit.mul(weight));
        weight = weight.add(weight);
        previous_any = any;
    }
    fold.absorb(value.sub(reconstructed));
}

fn reference_absorb_signed_range(
    fold: &mut ReferenceConstraintFold,
    sign: Ext4,
    magnitude: Ext4,
    auxiliary: &[Ext4; AUXILIARY_COLUMN_COUNT],
    start: usize,
    width: usize,
) {
    fold.absorb(reference_ext_bit_residual(sign));
    reference_absorb_range(fold, magnitude, auxiliary, start, width);
    let nonzero = auxiliary[start + 2 * width - 1];
    fold.absorb(sign.mul(Ext4::one().sub(nonzero)));
}

fn reference_absorb_terminal_range(
    fold: &mut ReferenceConstraintFold,
    row: &[Ext4; MAIN_COLUMN_COUNT],
    auxiliary: &[Ext4; AUXILIARY_COLUMN_COUNT],
    magnitude_start: usize,
    terminal_start: usize,
) {
    reference_absorb_range(fold, row[7], auxiliary, magnitude_start, 31);
    fold.absorb(reference_ext_bit_residual(row[14]));
    let mut previous = Ext4::zero();
    for index in 0..5 {
        let bit = auxiliary[magnitude_start + 26 + index];
        let any = auxiliary[terminal_start + index];
        fold.absorb(reference_ext_bit_residual(any));
        fold.absorb(any.sub(previous).sub(bit).add(previous.mul(bit)));
        previous = any;
    }
    fold.absorb(row[14].sub(previous));
}

fn reference_signed_i32(value: i32) -> (Ext4, Ext4, Ext4) {
    let sign = Ext4::from_base(u32::from(value < 0));
    let magnitude = Ext4::from_base(value.unsigned_abs());
    (sign, magnitude, reference_ext_signed_value(sign, magnitude))
}

fn reference_constraint_numerators(
    challenge: Ext4,
    c_re_q12: i32,
    c_im_q12: i32,
    row: &[Ext4; MAIN_COLUMN_COUNT],
    next: &[Ext4; MAIN_COLUMN_COUNT],
    auxiliary: &[Ext4; AUXILIARY_COLUMN_COUNT],
) -> [Ext4; 4] {
    let c_re = reference_signed_i32(c_re_q12);
    let c_im = reference_signed_i32(c_im_q12);
    let continue_selector = row[15].mul(Ext4::one().sub(row[16]));
    let freeze_selector = row[16].add(Ext4::one().sub(row[15]));
    let mut fold = ReferenceConstraintFold::new(challenge);
    reference_absorb_residuals(&mut fold, reference_local_residuals(row));
    reference_absorb_residuals(&mut fold, reference_row_state_residuals(row));

    let mut cursor = 0usize;
    reference_absorb_range(&mut fold, row[0], auxiliary, cursor, 21);
    cursor += 2 * 21;
    reference_absorb_signed_range(&mut fold, row[1], row[2], auxiliary, cursor, 15);
    cursor += 2 * 15;
    reference_absorb_signed_range(&mut fold, row[3], row[4], auxiliary, cursor, 15);
    cursor += 2 * 15;
    reference_absorb_range(&mut fold, row[5], auxiliary, cursor, 30);
    cursor += 2 * 30;
    reference_absorb_range(&mut fold, row[6], auxiliary, cursor, 30);
    cursor += 2 * 30;
    let magnitude_start = cursor;
    cursor += 2 * 31;
    reference_absorb_signed_range(&mut fold, row[8], row[9], auxiliary, cursor, 18);
    cursor += 2 * 18;
    reference_absorb_range(&mut fold, row[10], auxiliary, cursor, 12);
    cursor += 2 * 12;
    reference_absorb_signed_range(&mut fold, row[11], row[12], auxiliary, cursor, 19);
    cursor += 2 * 19;
    reference_absorb_range(&mut fold, row[13], auxiliary, cursor, 12);
    cursor += 2 * 12;
    assert_eq!(cursor + 5, AUXILIARY_COLUMN_COUNT);
    reference_absorb_terminal_range(&mut fold, row, auxiliary, magnitude_start, cursor);
    assert_eq!(fold.absorbed, 653);
    let all_rows = fold.value;

    fold.next_family();
    reference_absorb_residuals(&mut fold, reference_row_state_residuals(row));
    reference_absorb_residuals(&mut fold, reference_row_state_residuals(next));
    fold.absorb(continue_selector.mul(next[15].sub(Ext4::one())));
    fold.absorb(freeze_selector.mul(next[15]));
    fold.absorb(freeze_selector.mul(next[16]));
    for column in 0..15 {
        fold.absorb(freeze_selector.mul(next[column].sub(row[column])));
    }
    for residual in reference_transition_residuals(c_re, c_im, row, next) {
        fold.absorb(continue_selector.mul(residual));
    }
    assert_eq!(fold.absorbed, 690);
    let pair_rows = fold.value;

    fold.next_family();
    reference_absorb_residuals(&mut fold, reference_row_state_residuals(row));
    fold.absorb(row[15].sub(Ext4::one()));
    fold.absorb(row[16]);
    fold.absorb(row[0]);
    fold.absorb(row[1]);
    fold.absorb(row[2]);
    fold.absorb(row[3]);
    fold.absorb(row[4]);
    assert_eq!(fold.absorbed, 702);
    let first_row = fold.value;

    fold.next_family();
    reference_absorb_residuals(&mut fold, reference_row_state_residuals(row));
    fold.absorb(row[15].mul(Ext4::one().sub(row[16])));
    assert_eq!(fold.absorbed, 708);
    [all_rows, pair_rows, first_row, fold.value]
}

fn reference_lde_main(air_lde: &[u32], evaluation: usize) -> [Ext4; MAIN_COLUMN_COUNT] {
    std::array::from_fn(|column| Ext4::from_base(air_lde[column * LDE_ROWS + evaluation]))
}

fn reference_lde_auxiliary(air_lde: &[u32], evaluation: usize) -> [Ext4; AUXILIARY_COLUMN_COUNT] {
    std::array::from_fn(|column| {
        Ext4::from_base(air_lde[(MAIN_COLUMN_COUNT + column) * LDE_ROWS + evaluation])
    })
}

fn reference_air_composition_value(air_lde: &[u32], evaluation: usize, challenge: Ext4) -> Ext4 {
    let maximal_root = pow_mod(31, 15);
    let lde_root = pow_mod(
        u64::from(maximal_root),
        1 << (TWO_ADICITY - LDE_ROWS.ilog2()),
    );
    let trace_root = pow_mod(
        u64::from(maximal_root),
        1 << (TWO_ADICITY - TRACE_ROWS.ilog2()),
    );
    let point = mul_mod(7, pow_mod(u64::from(lde_root), evaluation as u32));
    let next_evaluation = (evaluation + LDE_ROWS / TRACE_ROWS) % LDE_ROWS;
    let row = reference_lde_main(air_lde, evaluation);
    let next = reference_lde_main(air_lde, next_evaluation);
    let auxiliary = reference_lde_auxiliary(air_lde, evaluation);
    let numerators = reference_constraint_numerators(challenge, 3_072, 0, &row, &next, &auxiliary);

    let last_trace_point = pow_mod(u64::from(trace_root), MODULUS - 2);
    let trace_zerofier = sub_mod(pow_mod(u64::from(point), TRACE_ROWS as u32), 1);
    let first_zerofier = sub_mod(point, 1);
    let last_zerofier = sub_mod(point, last_trace_point);
    assert_ne!(trace_zerofier, 0);
    assert_ne!(first_zerofier, 0);
    assert_ne!(last_zerofier, 0);
    let trace_inverse = pow_mod(u64::from(trace_zerofier), MODULUS - 2);
    let first_inverse = pow_mod(u64::from(first_zerofier), MODULUS - 2);
    let last_inverse = pow_mod(u64::from(last_zerofier), MODULUS - 2);
    numerators[0]
        .scale(trace_inverse)
        .add(numerators[1].scale(mul_mod(last_zerofier, trace_inverse)))
        .add(numerators[2].scale(first_inverse))
        .add(numerators[3].scale(last_inverse))
}

struct ReferenceCompositionReceipt {
    challenge: Ext4,
    values: Vec<Ext4>,
    root: [u32; 8],
    transcript: [u32; 8],
}

fn reference_composition_receipt(
    air_lde: &[u32],
    air_transcript: [u32; 8],
) -> ReferenceCompositionReceipt {
    assert_eq!(air_lde.len(), AIR_LDE_WORDS);
    let challenge_digest = reference_field_commitment(b"BC01", &air_transcript);
    let challenge = Ext4(
        challenge_digest[..EXTENSION_WORDS]
            .try_into()
            .expect("one composition challenge"),
    );
    let values = (0..LDE_ROWS)
        .map(|evaluation| reference_air_composition_value(air_lde, evaluation, challenge))
        .collect::<Vec<_>>();
    let leaves = values
        .iter()
        .map(|value| reference_field_commitment(b"BC02", &value.0))
        .collect::<Vec<_>>();
    let root = reference_digest_root(leaves);
    let binding_fields = air_transcript.into_iter().chain(root).collect::<Vec<_>>();
    let transcript = reference_field_commitment(b"BC03", &binding_fields);
    ReferenceCompositionReceipt {
        challenge,
        values,
        root,
        transcript,
    }
}

fn scratch_ext4(words: &[u32], start: usize) -> Ext4 {
    Ext4(
        words[start..start + EXTENSION_WORDS]
            .try_into()
            .expect("four scratch extension coefficients"),
    )
}

fn assert_composition_scratch(words: &[u32], air_lde: &[u32], challenge: Ext4) {
    assert_eq!(words.len(), FRI_SCRATCH_WORDS);
    let expected_power = challenge.pow(708);
    for evaluation in 0..LDE_ROWS {
        let next_evaluation = (evaluation + LDE_ROWS / TRACE_ROWS) % LDE_ROWS;
        let row = reference_lde_main(air_lde, evaluation);
        let next = reference_lde_main(air_lde, next_evaluation);
        let auxiliary = reference_lde_auxiliary(air_lde, evaluation);
        let numerators =
            reference_constraint_numerators(challenge, 3_072, 0, &row, &next, &auxiliary);
        let fold_start = COMPOSITION_FOLD_SCRATCH_START + evaluation * 2 * EXTENSION_WORDS;
        let actual_power = scratch_ext4(words, fold_start);
        let actual_exponent = (0..=708).find(|exponent| challenge.pow(*exponent) == actual_power);
        let actual_numerators = [
            scratch_ext4(
                words,
                COMPOSITION_ALL_ROWS_SCRATCH_START + evaluation * EXTENSION_WORDS,
            ),
            scratch_ext4(
                words,
                COMPOSITION_PAIR_ROWS_SCRATCH_START + evaluation * EXTENSION_WORDS,
            ),
            scratch_ext4(
                words,
                COMPOSITION_FIRST_ROW_SCRATCH_START + evaluation * EXTENSION_WORDS,
            ),
            scratch_ext4(words, fold_start + EXTENSION_WORDS),
        ];
        assert_eq!(
            (actual_power, actual_numerators),
            (expected_power, numerators),
            "GPU typed composition checkpoint must match at evaluation {evaluation}; actual exponent is {actual_exponent:?}",
        );
        assert_eq!(
            words[COMPOSITION_FOLD_VALID_SCRATCH_START + evaluation],
            1,
            "GPU composition fold must remain valid at evaluation {evaluation}",
        );
    }
}

fn reference_merkle_tree(round: usize, evaluations: &[Ext4]) -> Vec<[u32; 8]> {
    let tag = protocol_round_tag(*b"FR", round);
    let mut layer = evaluations
        .iter()
        .map(|evaluation| reference_field_commitment(&tag, &evaluation.0))
        .collect::<Vec<_>>();
    let mut tree = layer.clone();
    while layer.len() > 1 {
        assert_eq!(layer.len() % 2, 0);
        layer = layer
            .chunks_exact(2)
            .map(|children| reference_compress(&children[0], &children[1]))
            .collect();
        tree.extend_from_slice(&layer);
    }
    tree
}

fn reference_fri_pair(positive: Ext4, negative: Ext4, challenge: Ext4, point: u32) -> Ext4 {
    let inverse_two = pow_mod(2, MODULUS - 2);
    let inverse_point = pow_mod(u64::from(point), MODULUS - 2);
    let even = positive.add(negative).scale(inverse_two);
    let odd = positive
        .sub(negative)
        .scale(inverse_two)
        .scale(inverse_point);
    challenge.mul(odd).add(even)
}

#[derive(Debug)]
struct ReferenceFriReceipt {
    clean_evaluations: Vec<u32>,
    observed_evaluations: Vec<u32>,
    challenges: Vec<u32>,
    roots: Vec<u32>,
    transcripts: Vec<u32>,
    trees: Vec<u32>,
}

fn reference_fri_receipt(
    initial: &[Ext4],
    starting_transcript: &[u32; 8],
    tampered: bool,
) -> ReferenceFriReceipt {
    let maximal_root = pow_mod(31, 15);
    assert_eq!(initial.len(), LDE_ROWS);
    let mut receipt = ReferenceFriReceipt {
        clean_evaluations: Vec::with_capacity(FRI_EVALUATION_WORDS),
        observed_evaluations: Vec::with_capacity(FRI_EVALUATION_WORDS),
        challenges: Vec::with_capacity(FRI_CHALLENGE_WORDS),
        roots: Vec::with_capacity(FRI_ROOT_WORDS),
        transcripts: Vec::with_capacity(FRI_TRANSCRIPT_WORDS),
        trees: Vec::with_capacity(FRI_VARIANTS * 26 * 8),
    };
    for variant in 0..FRI_VARIANTS {
        let mut current = initial.to_vec();
        let mut transcript = *starting_transcript;
        receipt.transcripts.extend(transcript);
        let mut shift = 7;
        for round_index in 0..FRI_ROUNDS {
            let round = round_index + 1;
            let challenge_digest =
                reference_field_commitment(&protocol_round_tag(*b"FC", round), &transcript);
            let challenge = Ext4(challenge_digest[..4].try_into().expect("quartic challenge"));
            receipt.challenges.extend(challenge.0);
            let input_width = current.len();
            let output_width = input_width / 2;
            let root = pow_mod(
                u64::from(maximal_root),
                1 << (TWO_ADICITY - input_width.ilog2()),
            );
            let mut folded = vec![Ext4::zero(); output_width];
            for pair in 0..output_width {
                let mut positive = current[pair];
                if variant == 1 && tampered && round_index == 0 && pair == 1 {
                    positive.0[1] = add_mod(positive.0[1], 1);
                }
                let negative = current[pair + output_width];
                let point = mul_mod(shift, pow_mod(u64::from(root), pair as u32));
                folded[pair] = reference_fri_pair(positive, negative, challenge, point);
            }
            let destination = if variant == 0 {
                &mut receipt.clean_evaluations
            } else {
                &mut receipt.observed_evaluations
            };
            destination.extend(folded.iter().flat_map(|evaluation| evaluation.0));
            let layer_tree = reference_merkle_tree(round, &folded);
            let layer_root = *layer_tree.last().expect("nonempty FRI layer tree");
            receipt.trees.extend(layer_tree.into_iter().flatten());
            receipt.roots.extend(layer_root);
            let mut binding_fields = Vec::with_capacity(16);
            binding_fields.extend(transcript);
            binding_fields.extend(layer_root);
            transcript =
                reference_field_commitment(&protocol_round_tag(*b"FT", round), &binding_fields);
            receipt.transcripts.extend(transcript);
            current = folded;
            shift = mul_mod(shift, shift);
        }
    }
    receipt
}

#[derive(Debug)]
struct ReferenceFriQueryReceipt {
    indices: Vec<u32>,
    evaluations: Vec<u32>,
    siblings: Vec<u32>,
}

fn reference_fri_query_receipt(fri: &ReferenceFriReceipt) -> ReferenceFriQueryReceipt {
    let widths = [8usize, 4, 2, 1];
    let evaluation_offsets = [0usize, 8, 12, 14];
    let tree_offsets = [0usize, 15, 22, 25];
    let mut receipt = ReferenceFriQueryReceipt {
        indices: Vec::with_capacity(FRI_VARIANTS),
        evaluations: Vec::with_capacity(FRI_QUERY_EVALUATION_WORDS),
        siblings: Vec::with_capacity(FRI_QUERY_SIBLING_WORDS),
    };
    for variant in 0..FRI_VARIANTS {
        let transcript_start = (variant * (FRI_ROUNDS + 1) + FRI_ROUNDS) * 8;
        let mut message = fri.transcripts[transcript_start..transcript_start + 8].to_vec();
        message.push(1);
        let sampled = reference_field_commitment(b"FQ02", &message)[0] & 7;
        receipt.indices.push(sampled);

        let source = if variant == 0 {
            &fri.clean_evaluations
        } else {
            &fri.observed_evaluations
        };
        let mut query = sampled as usize;
        for (round, width) in widths.into_iter().enumerate() {
            let offset = evaluation_offsets[round];
            if width > 1 {
                let pair_width = width / 2;
                let pair = query & (pair_width - 1);
                for evaluation in [offset + pair, offset + pair + pair_width] {
                    receipt.evaluations.extend_from_slice(
                        &source[evaluation * EXTENSION_WORDS..(evaluation + 1) * EXTENSION_WORDS],
                    );
                }
                query = pair;
            } else {
                receipt.evaluations.extend_from_slice(
                    &source[offset * EXTENSION_WORDS..(offset + 1) * EXTENSION_WORDS],
                );
            }
        }

        query = sampled as usize;
        for (round, width) in widths.into_iter().enumerate() {
            if width > 2 {
                let pair_width = width / 2;
                let pair_depth = width.ilog2() as usize - 1;
                for side in 0..2 {
                    let leaf = (query & (pair_width - 1)) + side * pair_width;
                    let mut level_offset = 0usize;
                    let mut level_width = width;
                    for level in 0..pair_depth {
                        let node = tree_offsets[round] + level_offset + ((leaf >> level) ^ 1);
                        let start = (variant * 26 + node) * 8;
                        receipt
                            .siblings
                            .extend_from_slice(&fri.trees[start..start + 8]);
                        level_offset += level_width;
                        level_width /= 2;
                    }
                }
            }
            if width > 1 {
                query &= width / 2 - 1;
            }
        }
    }
    assert_eq!(receipt.indices.len(), FRI_VARIANTS);
    assert_eq!(receipt.evaluations.len(), FRI_QUERY_EVALUATION_WORDS);
    assert_eq!(receipt.siblings.len(), FRI_QUERY_SIBLING_WORDS);
    receipt
}

fn query_evaluation(receipt: &ReferenceFriQueryReceipt, variant: usize, slot: usize) -> Ext4 {
    let start = (variant * FRI_QUERY_EVALUATIONS_PER_VARIANT + slot) * EXTENSION_WORDS;
    Ext4(
        receipt.evaluations[start..start + EXTENSION_WORDS]
            .try_into()
            .expect("one queried extension evaluation"),
    )
}

fn verify_reference_fri_query(
    fri: &ReferenceFriReceipt,
    query: &ReferenceFriQueryReceipt,
    variant: usize,
) {
    let widths = [8usize, 4, 2, 1];
    let evaluation_slots = [0usize, 2, 4, 6];
    let mut query_index = query.indices[variant] as usize;
    let mut sibling_slot = variant * FRI_QUERY_SIBLINGS_PER_VARIANT;
    for (round_index, width) in widths.into_iter().enumerate() {
        let round = round_index + 1;
        let tag = protocol_round_tag(*b"FR", round);
        let slot = evaluation_slots[round_index];
        let root = if width > 1 {
            let pair_width = width / 2;
            let pair_depth = if width > 2 {
                width.ilog2() as usize - 1
            } else {
                0
            };
            let pair = query_index & (pair_width - 1);
            let mut sides = [
                reference_field_commitment(&tag, &query_evaluation(query, variant, slot).0),
                reference_field_commitment(&tag, &query_evaluation(query, variant, slot + 1).0),
            ];
            for side in 0..2 {
                let leaf = pair + side * pair_width;
                for level in 0..pair_depth {
                    let start = sibling_slot * 8;
                    let sibling: [u32; 8] = query.siblings[start..start + 8]
                        .try_into()
                        .expect("one queried sibling digest");
                    sides[side] = if ((leaf >> level) & 1) == 0 {
                        reference_compress(&sides[side], &sibling)
                    } else {
                        reference_compress(&sibling, &sides[side])
                    };
                    sibling_slot += 1;
                }
            }
            query_index = pair;
            reference_compress(&sides[0], &sides[1])
        } else {
            reference_field_commitment(&tag, &query_evaluation(query, variant, slot).0)
        };
        let root_start = (variant * FRI_ROUNDS + round_index) * 8;
        assert_eq!(
            root,
            fri.roots[root_start..root_start + 8],
            "queried authentication path must reconstruct FRI layer {round}",
        );
    }
    assert_eq!(sibling_slot, (variant + 1) * FRI_QUERY_SIBLINGS_PER_VARIANT,);

    query_index = query.indices[variant] as usize;
    let maximal_root = pow_mod(31, 15);
    for transition in 0..FRI_ROUNDS - 1 {
        let input_width = widths[transition];
        let pair_width = input_width / 2;
        let pair = query_index & (pair_width - 1);
        let slot = evaluation_slots[transition];
        let challenge_start = (variant * FRI_ROUNDS + transition + 1) * EXTENSION_WORDS;
        let challenge = Ext4(
            fri.challenges[challenge_start..challenge_start + EXTENSION_WORDS]
                .try_into()
                .expect("one inter-layer FRI challenge"),
        );
        let mut shift = 7;
        for _ in 0..transition + 1 {
            shift = mul_mod(shift, shift);
        }
        let root = pow_mod(
            u64::from(maximal_root),
            1 << (TWO_ADICITY - input_width.ilog2()),
        );
        let point = mul_mod(shift, pow_mod(u64::from(root), pair as u32));
        let folded = reference_fri_pair(
            query_evaluation(query, variant, slot),
            query_evaluation(query, variant, slot + 1),
            challenge,
            point,
        );
        let next_width = widths[transition + 1];
        let next_slot = evaluation_slots[transition + 1];
        let expected = if next_width == 1 || pair < next_width / 2 {
            query_evaluation(query, variant, next_slot)
        } else {
            query_evaluation(query, variant, next_slot + 1)
        };
        assert_eq!(
            folded,
            expected,
            "queried FRI transition {} must fold into the next authenticated layer",
            transition + 1,
        );
        query_index = pair;
    }
}

fn assert_commit_state(words: &[u32], offset: usize) {
    assert_eq!(
        &words[offset + COMMIT_CURSOR..offset + COMMIT_CURSOR + POSEIDON_WIDTH],
        &[0; POSEIDON_WIDTH],
        "all field lanes must finish at the transition boundary"
    );
    assert_eq!(
        &words[offset + COMMIT_BLOCK..offset + COMMIT_BLOCK + POSEIDON_WIDTH],
        &[DONE_BLOCK; POSEIDON_WIDTH],
        "all field lanes must consume the complete typed message"
    );
    assert_eq!(
        &words[offset + COMMIT_VALID..offset + COMMIT_VALID + POSEIDON_WIDTH],
        &[1; POSEIDON_WIDTH],
        "all field lanes must retain a valid checkpoint"
    );
}

fn assert_proof(proof: &[u32], tampered: bool, expected_lde: &[u32], expected_air_lde: &[u32]) {
    assert_eq!(proof.len(), PROOF_WORDS);
    let expected_trace = trace_columns().into_iter().flatten().collect::<Vec<_>>();
    assert_eq!(&proof[..LDE_START], &expected_trace);
    assert_eq!(
        &proof[LDE_START..LDE_START + expected_lde.len()],
        expected_lde,
        "GPU LDE must match the independent direct DFT"
    );
    let clean = reference_commitment(expected_lde);
    let mut observed_fields = expected_lde.to_vec();
    if tampered {
        observed_fields[TAMPER_LDE_FIELD] = (observed_fields[TAMPER_LDE_FIELD] + 1) % MODULUS;
    }
    let observed = reference_commitment(&observed_fields);
    assert_eq!(&proof[CLEAN_ROOT..CLEAN_ROOT + 8], &clean);
    assert_eq!(&proof[OBSERVED_ROOT..OBSERVED_ROOT + 8], &observed);
    assert_eq!(proof[TRACE_VALID], 1);
    assert_eq!(
        &proof[LDE_VALID_START..LDE_VALID_START + COLUMN_COUNT],
        &[1; COLUMN_COUNT]
    );
    assert_eq!(proof[ROOTS_EQUAL], u32::from(!tampered));
    assert_eq!(proof[MODE_CORRECT], 1);
    assert_commit_state(proof, CLEAN_COMMIT_STATE);
    assert_commit_state(proof, OBSERVED_COMMIT_STATE);
    assert_eq!(
        &proof[PARAMETER_START..PARAMETER_END],
        reference_montgomery_parameters(),
        "GPU parameter initialization must match Plonky3 exactly"
    );
    let (main_root, auxiliary_root) = reference_air_lde_roots(expected_air_lde);
    let production = reference_production_transcript(main_root, auxiliary_root);
    let composition = reference_composition_receipt(expected_air_lde, production.air_transcript);
    assert_eq!(
        &proof[CANONICAL_PUBLIC..COMPOSITION_CHALLENGE],
        &[0, 3_072, 0, 0, 4, 3, 4, 4],
        "the GPU composition must consume the canonical typed public claim",
    );
    assert_eq!(
        &proof[COMPOSITION_CHALLENGE..COMPOSITION_VALUES],
        &composition.challenge.0,
        "the GPU composition challenge must match the independent BC01 squeeze",
    );
    let composition_words = composition
        .values
        .iter()
        .flat_map(|value| value.0)
        .collect::<Vec<_>>();
    assert_eq!(
        &proof[COMPOSITION_VALUES..COMPOSITION_ROOT],
        composition_words,
        "all GPU composition evaluations must match the independent AIR model",
    );
    assert_eq!(
        &proof[COMPOSITION_ROOT..COMPOSITION_TRANSCRIPT],
        &composition.root,
        "the GPU composition root must match independent BC02 leaves",
    );
    assert_eq!(
        &proof[COMPOSITION_TRANSCRIPT..COMPOSITION_VALUE_VALID],
        &composition.transcript,
        "the GPU composition transcript must match the independent BC03 binding",
    );
    assert_eq!(
        &proof[COMPOSITION_VALUE_VALID..COMPOSITION_VALID],
        &[1; LDE_ROWS],
        "every production composition evaluation must be defined",
    );
    assert_eq!(
        &proof[COMPOSITION_VALID..PROOF_WORDS],
        &[1, 1, 1],
        "challenge, commitment tree, and composition transcript must complete",
    );
    let fri = reference_fri_receipt(&composition.values, &composition.transcript, tampered);
    assert_eq!(
        &proof[FRI_CLEAN..FRI_CLEAN + FRI_EVALUATION_WORDS],
        fri.clean_evaluations,
        "GPU clean FRI schedule must match the independent quartic folds",
    );
    assert_eq!(
        &proof[FRI_OBSERVED..FRI_OBSERVED + FRI_EVALUATION_WORDS],
        fri.observed_evaluations,
        "GPU observed FRI schedule must match the independently mutated folds",
    );
    assert_eq!(
        &proof[FRI_CHALLENGES..FRI_CHALLENGES + FRI_CHALLENGE_WORDS],
        fri.challenges,
        "every GPU FRI challenge must match the independent typed transcript",
    );
    assert_eq!(
        &proof[FRI_ROOTS..FRI_ROOTS + FRI_ROOT_WORDS],
        fri.roots,
        "every GPU FRI Merkle root must match independent Plonky3 compression",
    );
    assert_eq!(
        &proof[FRI_TRANSCRIPTS..FRI_TRANSCRIPTS + FRI_TRANSCRIPT_WORDS],
        fri.transcripts,
        "every GPU FRI transcript must match independent domain-separated binding",
    );
    assert_eq!(
        &proof[FRI_CHALLENGE_VALID..FRI_ROOT_VALID],
        &[1; FRI_VARIANTS * FRI_ROUNDS],
    );
    assert_eq!(
        &proof[FRI_ROOT_VALID..FRI_TRANSCRIPT_VALID],
        &[1; FRI_VARIANTS * FRI_ROUNDS],
    );
    assert_eq!(
        &proof[FRI_TRANSCRIPT_VALID..FRI_ROUND_VALID],
        &[1; FRI_VARIANTS * (FRI_ROUNDS + 1)],
    );
    assert_eq!(
        &proof[FRI_ROUND_VALID..FRI_EQUAL],
        &[1; FRI_VARIANTS * FRI_ROUNDS],
    );
    assert_eq!(proof[FRI_EQUAL], u32::from(!tampered));
    assert_eq!(proof[FRI_CORRECT], 1);
    assert_eq!(
        proof[FRI_COLOR].to_le_bytes(),
        if tampered {
            [255, 176, 222, 255]
        } else {
            [87, 117, 226, 255]
        },
    );
    let query = reference_fri_query_receipt(&fri);
    assert_eq!(
        &proof[FRI_QUERY_INDICES..FRI_QUERY_EVALUATIONS],
        query.indices,
        "GPU query indices must match independent indexed Poseidon squeezes",
    );
    assert_eq!(
        &proof[FRI_QUERY_EVALUATIONS..FRI_QUERY_SIBLINGS],
        query.evaluations,
        "GPU query extraction must select the independently derived FRI evaluations",
    );
    assert_eq!(
        &proof[FRI_QUERY_SIBLINGS..FRI_QUERY_STATUS],
        query.siblings,
        "GPU query extraction must select the independently derived Merkle siblings",
    );
    for variant in 0..FRI_VARIANTS {
        verify_reference_fri_query(&fri, &query, variant);
    }
    assert_eq!(
        &proof[FRI_QUERY_INDEX_VALID..FRI_QUERY_OPENING_VALID],
        &[1; FRI_VARIANTS],
    );
    assert_eq!(
        &proof[FRI_QUERY_OPENING_VALID..FRI_QUERY_EQUAL],
        &[1; FRI_VARIANTS],
    );
    let evaluation_words_per_variant = FRI_QUERY_EVALUATIONS_PER_VARIANT * EXTENSION_WORDS;
    let sibling_words_per_variant = FRI_QUERY_SIBLINGS_PER_VARIANT * 8;
    let query_equal = query.indices[0] == query.indices[1]
        && query.evaluations[..evaluation_words_per_variant]
            == query.evaluations[evaluation_words_per_variant..]
        && query.siblings[..sibling_words_per_variant]
            == query.siblings[sibling_words_per_variant..];
    assert_eq!(query_equal, !tampered);
    assert_eq!(proof[FRI_QUERY_EQUAL], u32::from(query_equal));
    assert_eq!(proof[FRI_QUERY_CORRECT], 1);
    assert_eq!(
        proof[FRI_QUERY_COLOR].to_le_bytes(),
        if tampered {
            [255, 176, 222, 255]
        } else {
            [87, 117, 226, 255]
        },
    );
    assert_eq!(
        &proof[AIR_TRACE_START..AIR_LDE_VALID_START],
        reference_air_trace(),
        "the receipt must retain every canonical production AIR trace word",
    );
    assert_eq!(
        &proof[AIR_LDE_VALID_START..MAIN_LDE_ROOT],
        &[1; AIR_COLUMN_COUNT],
        "the receipt must retain completion of every production AIR LDE column",
    );
    assert_eq!(
        &proof[MAIN_LDE_ROOT..AUXILIARY_LDE_ROOT],
        &main_root,
        "the GPU main AIR tree must match independent production-domain hashing",
    );
    assert_eq!(
        &proof[AUXILIARY_LDE_ROOT..AIR_LDE_ROOT_VALID],
        &auxiliary_root,
        "the GPU auxiliary AIR tree must match independent production-domain hashing",
    );
    assert_eq!(
        &proof[AIR_LDE_ROOT_VALID..PACKED_MAIN_TRACE],
        &[1, 1],
        "both production AIR commitment trees must complete",
    );
    assert_eq!(
        &proof[PACKED_MAIN_TRACE..PACKED_AUXILIARY_TRACE],
        production.packed_main,
        "GPU main-row packing must match the independent audited bit schema",
    );
    assert_eq!(
        &proof[PACKED_AUXILIARY_TRACE..PACKED_PUBLIC],
        production.packed_auxiliary,
        "GPU auxiliary packing must match all 411 independent witness bits",
    );
    assert_eq!(
        &proof[PACKED_PUBLIC..MAIN_TRACE_ROOT],
        production.packed_public,
        "GPU public packing must match the independent claim schema",
    );
    assert_eq!(
        &proof[MAIN_TRACE_ROOT..AUXILIARY_TRACE_ROOT],
        &production.main_trace_root,
        "GPU main trace tree must match independent packed-row hashing",
    );
    assert_eq!(
        &proof[AUXILIARY_TRACE_ROOT..PUBLIC_DIGEST],
        &production.auxiliary_trace_root,
        "GPU auxiliary trace tree must match independent packed-row hashing",
    );
    assert_eq!(
        &proof[PUBLIC_DIGEST..PRODUCTION_TRACE_DIGEST_VALID],
        &production.public_digest,
        "GPU public digest must match independent packed-claim hashing",
    );
    assert_eq!(
        &proof[PRODUCTION_TRACE_DIGEST_VALID..AIR_TRANSCRIPT],
        &[1, 1, 1],
        "all production trace commitments must complete",
    );
    assert_eq!(
        &proof[AIR_TRANSCRIPT..AIR_TRANSCRIPT_VALID],
        &production.air_transcript,
        "GPU AIR transcript must bind public, trace, auxiliary, and LDE roots in order",
    );
    assert_eq!(proof[AIR_TRANSCRIPT_VALID], 1);
}

fn assert_receipt(
    receipt: &ExecutionReceipt,
    tampered: bool,
    expected_lde: &[u32],
    expected_air_trace: &[u32],
    expected_air_lde: &[u32],
) {
    assert_proof(&receipt.proof, tampered, expected_lde, expected_air_lde);
    assert_eq!(
        receipt.air_trace, expected_air_trace,
        "GPU witness placement must match every independently expanded AIR word",
    );
    assert_eq!(
        receipt.air_lde_valid,
        vec![1; AIR_COLUMN_COUNT],
        "every production AIR column must complete the derived stage grid",
    );
    assert_eq!(
        receipt.air_lde, expected_air_lde,
        "all 6,848 production AIR LDE values must match the independent direct DFT",
    );
    assert!(
        receipt.traps.iter().all(|word| *word == 0),
        "every physical invocation lane must remain trap-free: {:?}",
        receipt.traps
    );
}

#[derive(Deserialize)]
struct BrowserProofReceipts {
    clean: Vec<u32>,
    tampered: Vec<u32>,
    recovered: Vec<u32>,
    #[serde(rename = "airLde")]
    air_lde: Vec<u32>,
    #[serde(rename = "friScratch")]
    fri_scratch: Vec<u32>,
}

#[test]
fn chromium_receipts_match_independent_oracles() {
    let Some(path) = std::env::var_os(BROWSER_RECEIPTS) else {
        eprintln!("  Chromium receipt oracle SKIPPED: {BROWSER_RECEIPTS} is unset");
        return;
    };
    let encoded = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("failed to read {}: {error}", PathBuf::from(&path).display())
    });
    let receipts: BrowserProofReceipts = serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("invalid Chromium receipt JSON: {error}"));
    let expected_lde = trace_columns()
        .iter()
        .flat_map(|column| direct_coset_lde(column, LDE_ROWS, 7))
        .collect::<Vec<_>>();
    let expected_air_trace = reference_air_trace();
    let expected_air_lde = reference_air_lde(&expected_air_trace);
    assert_eq!(
        receipts.air_lde, expected_air_lde,
        "browser WebGPU must match all 6,848 independently derived AIR LDE values",
    );
    let (main_root, auxiliary_root) = reference_air_lde_roots(&expected_air_lde);
    let production = reference_production_transcript(main_root, auxiliary_root);
    let composition = reference_composition_receipt(&expected_air_lde, production.air_transcript);
    assert_composition_scratch(
        &receipts.fri_scratch,
        &expected_air_lde,
        composition.challenge,
    );
    assert_proof(&receipts.clean, false, &expected_lde, &expected_air_lde);
    assert_proof(&receipts.tampered, true, &expected_lde, &expected_air_lde);
    assert_proof(&receipts.recovered, false, &expected_lde, &expected_air_lde);
    assert_eq!(receipts.recovered, receipts.clean);
}

#[test]
fn final_fri_schedule_lowers_to_browser_webgpu() {
    let artifact = compile_proof_compute_stage("finalize_fri_schedule", false);
    assert_eq!(artifact.layout.workgroup_size, [1, 1, 1]);
    assert!(
        artifact
            .wgsl
            .as_deref()
            .is_some_and(|wgsl| !wgsl.is_empty())
    );
}

fn assert_production_air_stages_lower(entries: &[&str]) {
    for entry in entries {
        let started = std::time::Instant::now();
        let artifact = compile_proof_compute_stage(entry, true);
        eprintln!(
            "  production AIR composition stage `{entry}` lowered in {:.2?}",
            started.elapsed(),
        );
        assert_eq!(artifact.layout.workgroup_size, [1, 1, 1], "{entry}");
        assert!(
            artifact
                .wgsl
                .as_deref()
                .is_some_and(|wgsl| !wgsl.is_empty()),
            "{entry}",
        );
    }
}

#[test]
fn production_air_composition_prefix_stages_lower_to_browser_webgpu() {
    assert_production_air_stages_lower(&[
        "evaluate_production_air_local_step",
        "evaluate_production_air_orbit_coordinates",
        "evaluate_production_air_real_square",
        "evaluate_production_air_imaginary_square",
        "evaluate_production_air_real_quotient",
        "evaluate_production_air_imaginary_quotient",
        "evaluate_production_air_magnitude_terminal",
    ]);
}

#[test]
fn production_air_composition_final_stages_lower_to_browser_webgpu() {
    assert_production_air_stages_lower(&[
        "evaluate_production_air_pair_rows",
        "evaluate_production_air_first_row",
        "evaluate_production_air_last_row",
        "project_production_air_composition",
    ]);
}

#[test]
fn complete_proof_graph_matches_independent_oracles_on_webgpu() {
    let bundle = compile_proof_graph();
    assert_eq!(bundle.manifest.resources.len(), 6);
    assert_eq!(
        bundle
            .manifest
            .resources
            .iter()
            .map(|resource| (resource.name.as_str(), resource.length))
            .collect::<Vec<_>>(),
        vec![
            ("proof", PROOF_WORDS as u32),
            ("lde_inverse_values", AIR_TRACE_WORDS as u32),
            ("lde_inverse_progress", AIR_INPUT_GRID_LANES as u32),
            ("lde_values", AIR_LDE_WORDS as u32),
            ("lde_progress", AIR_OUTPUT_GRID_LANES as u32),
            ("fri_scratch", 2_874),
        ]
    );
    assert_eq!(bundle.manifest.passes.len(), COMPUTE_PASSES + 1);
    assert!(
        bundle
            .manifest
            .passes
            .iter()
            .all(|pass| pass.layout.bindings.len() <= 8),
        "every pass must fit WebGPU's portable per-stage storage-buffer minimum"
    );
    assert_eq!(bundle.manifest.passes[2].repeat, 2);
    assert_eq!(bundle.manifest.passes[2].dispatch, Some([4, 1, 1]));
    assert_eq!(bundle.manifest.passes[2].layout.workgroup_size, [256, 1, 1]);
    assert_eq!(bundle.manifest.passes[4].repeat, 4);
    assert_eq!(bundle.manifest.passes[4].dispatch, Some([14, 1, 1]));
    assert_eq!(bundle.manifest.passes[4].layout.workgroup_size, [256, 1, 1]);
    assert_eq!(bundle.manifest.passes[MAIN_LDE_COMMITMENT_PASS].repeat, 132);
    assert_eq!(
        bundle.manifest.passes[MAIN_LDE_COMMITMENT_PASS]
            .layout
            .workgroup_size,
        [256, 1, 1],
    );
    assert_eq!(
        bundle.manifest.passes[AUXILIARY_LDE_COMMITMENT_PASS].repeat,
        2_288,
    );
    assert_eq!(
        bundle.manifest.passes[AUXILIARY_LDE_COMMITMENT_PASS]
            .layout
            .workgroup_size,
        [256, 1, 1],
    );
    assert_eq!(bundle.manifest.passes[AIR_LDE_TREE_PASS].repeat, 180);
    assert_eq!(
        bundle.manifest.passes[AIR_LDE_TREE_PASS]
            .layout
            .workgroup_size,
        [256, 1, 1],
    );
    assert_eq!(bundle.manifest.passes[TRACE_COMMITMENT_PASS].repeat, 88);
    assert_eq!(
        bundle.manifest.passes[TRACE_COMMITMENT_PASS]
            .layout
            .workgroup_size,
        [144, 1, 1],
    );
    assert_eq!(bundle.manifest.passes[TRACE_TREE_PASS].repeat, 90);
    assert_eq!(
        bundle.manifest.passes[TRACE_TREE_PASS]
            .layout
            .workgroup_size,
        [256, 1, 1],
    );
    assert_eq!(bundle.manifest.passes[AIR_TRANSCRIPT_PASS].repeat, 532);
    assert_eq!(
        bundle.manifest.passes[AIR_TRANSCRIPT_PASS]
            .layout
            .workgroup_size,
        [16, 1, 1],
    );
    assert_eq!(
        bundle.manifest.passes[COMPOSITION_CHALLENGE_PASS].repeat,
        89,
    );
    assert_eq!(
        bundle.manifest.passes[COMPOSITION_CHALLENGE_PASS]
            .layout
            .workgroup_size,
        [16, 1, 1],
    );
    for pass in &bundle.manifest.passes[COMPOSITION_EVALUATION_FIRST_PASS
        ..COMPOSITION_EVALUATION_FIRST_PASS + COMPOSITION_EVALUATION_PASSES]
    {
        assert_eq!(pass.repeat, 1);
        assert_eq!(pass.layout.workgroup_size, [16, 1, 1]);
    }
    assert_eq!(
        bundle.manifest.passes[COMPOSITION_COMMITMENT_PASS].repeat,
        44,
    );
    assert_eq!(
        bundle.manifest.passes[COMPOSITION_COMMITMENT_PASS]
            .layout
            .workgroup_size,
        [256, 1, 1],
    );
    assert_eq!(bundle.manifest.passes[COMPOSITION_TREE_PASS].repeat, 180);
    assert_eq!(
        bundle.manifest.passes[COMPOSITION_TREE_PASS]
            .layout
            .workgroup_size,
        [256, 1, 1],
    );
    assert_eq!(
        bundle.manifest.passes[COMPOSITION_TRANSCRIPT_PASS].repeat,
        133,
    );
    assert_eq!(
        bundle.manifest.passes[COMPOSITION_TRANSCRIPT_PASS]
            .layout
            .workgroup_size,
        [16, 1, 1],
    );
    assert_eq!(bundle.manifest.passes[COMMITMENT_PASS].repeat, 396);
    assert_eq!(
        bundle.manifest.passes[COMMITMENT_PASS]
            .layout
            .workgroup_size,
        [32, 1, 1]
    );
    for (round, expected_repeat) in FRI_ROUND_REPEATS.into_iter().enumerate() {
        let pass = &bundle.manifest.passes[FRI_FIRST_PASS + round];
        assert_eq!(pass.repeat, expected_repeat);
        assert_eq!(pass.layout.workgroup_size, [256, 1, 1]);
    }
    assert_eq!(bundle.manifest.passes[FRI_QUERY_SAMPLE_PASS].repeat, 89,);
    assert_eq!(
        bundle.manifest.passes[FRI_QUERY_SAMPLE_PASS]
            .layout
            .workgroup_size,
        [256, 1, 1],
    );
    assert_eq!(bundle.manifest.passes[FRI_QUERY_EXTRACT_PASS].repeat, 1,);
    assert_eq!(
        bundle.manifest.passes[FRI_QUERY_EXTRACT_PASS]
            .layout
            .workgroup_size,
        [256, 1, 1],
    );
    let Some((adapter, device, queue)) = request_browser_profile_device() else {
        return;
    };
    eprintln!(
        "  Mandelbrot proof WebGPU adapter (no required features): {}",
        adapter.get_info().name
    );
    eprintln!(
        "  Mandelbrot proof shader bytes: {:?}",
        bundle
            .manifest
            .passes
            .iter()
            .zip(&bundle.pass_wgsl)
            .map(|(pass, shader)| (pass.source_entry.as_str(), shader.source.len()))
            .collect::<Vec<_>>()
    );
    eprintln!(
        "  Mandelbrot proof largest commitment functions: {:?}",
        largest_wgsl_functions(&bundle.pass_wgsl[COMMITMENT_PASS].source, 12)
    );
    for round in 0..FRI_ROUNDS {
        eprintln!(
            "  Mandelbrot proof largest FRI round {} functions: {:?}",
            round + 1,
            largest_wgsl_functions(&bundle.pass_wgsl[FRI_FIRST_PASS + round].source, 12)
        );
    }
    let pipeline_started = std::time::Instant::now();
    let kernels = compile_kernels(&device, &bundle);
    eprintln!(
        "  Mandelbrot proof pipelines compiled in {:?}",
        pipeline_started.elapsed()
    );
    let expected_lde = trace_columns()
        .iter()
        .flat_map(|column| direct_coset_lde(column, LDE_ROWS, 7))
        .collect::<Vec<_>>();
    let expected_air_trace = reference_air_trace();
    let expected_air_lde = reference_air_lde(&expected_air_trace);
    for (compact, air) in [0usize, 7, 15, 16].into_iter().enumerate() {
        assert_eq!(
            &expected_lde[compact * LDE_ROWS..(compact + 1) * LDE_ROWS],
            &expected_air_lde[air * LDE_ROWS..(air + 1) * LDE_ROWS],
            "compact FRI input column must be a projection of the full AIR LDE",
        );
    }

    let clean_started = std::time::Instant::now();
    let clean = execute_case(&device, &queue, &bundle, &kernels, 0.0);
    eprintln!(
        "  Mandelbrot proof clean graph executed in {:?}",
        clean_started.elapsed()
    );
    assert_receipt(
        &clean,
        false,
        &expected_lde,
        &expected_air_trace,
        &expected_air_lde,
    );
    let tampered_started = std::time::Instant::now();
    let tampered = execute_case(&device, &queue, &bundle, &kernels, 1.0);
    eprintln!(
        "  Mandelbrot proof tampered graph executed in {:?}",
        tampered_started.elapsed()
    );
    assert_receipt(
        &tampered,
        true,
        &expected_lde,
        &expected_air_trace,
        &expected_air_lde,
    );
    assert_ne!(
        &clean.proof[OBSERVED_ROOT..OBSERVED_ROOT + 8],
        &tampered.proof[OBSERVED_ROOT..OBSERVED_ROOT + 8],
        "the Fe-authored mutation mode must alter the observed commitment"
    );
    assert_ne!(
        &clean.proof[FRI_OBSERVED..FRI_OBSERVED + FRI_EVALUATION_WORDS],
        &tampered.proof[FRI_OBSERVED..FRI_OBSERVED + FRI_EVALUATION_WORDS],
        "the Fe-authored mutation mode must alter the observed FRI schedule"
    );
}
