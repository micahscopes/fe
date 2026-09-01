//! Executed value and schedule gate for the factor-tree WebGPU NTT.
//!
//! The Fe actor derives repeated dispatches from `Dit<4>` and `Dit<5>`, places
//! their butterfly matchings across workgroups, and passes the inverse result
//! into a validated coset extension. This test validates browser-profile WGSL,
//! executes it through wgpu, and compares every value to direct polynomial
//! evaluation rather than another butterfly implementation. A mutated private
//! cursor additionally proves that the validation receipt fails closed before
//! the next phase may consume it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WebBindingAccess, WebBindingRole, WebBuildOptions, WebBundle, WebBundleMode, resolve_web_entry,
};
use hir::hir_def::HirIngot;
use url::Url;

const MODULUS: u32 = 2_013_265_921;
const TWO_ADICITY: u32 = 27;
const POINTS: usize = 16;
const BUTTERFLIES: usize = POINTS / 2;
const STAGES: u32 = 4;
const LDE_POINTS: usize = POINTS * 2;
const LDE_BUTTERFLIES: usize = LDE_POINTS / 2;
const LDE_STAGES: u32 = 5;
const COMPUTE_PASSES: usize = 9;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repo-root ancestor")
        .to_path_buf()
}

fn compile_stage_grid() -> WebBundle {
    let dir = repo_root().join("crates/codegen/tests/fixtures/parallel_ntt_webgpu_oracle_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "stage-grid ingot initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("stage-grid fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "stage-grid source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("parallel_ntt_webgpu_oracle".into())),
    )
    .expect("stage-grid fixture should compile into a WebBundle")
}

#[test]
fn production_depth_factor_tree_stage_is_call_free() {
    let dir = repo_root().join("crates/codegen/tests/fixtures/parallel_ntt_webgpu_deep_plan_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "deep stage-grid ingot initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("deep stage-grid fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "deep stage-grid source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the deep actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    let bundle = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("parallel_ntt_webgpu_deep_plan".into())),
    )
    .expect("the production-depth stage should flatten and validate");

    assert_eq!(bundle.manifest.passes.len(), 3);
    let advance = &bundle.manifest.passes[0];
    assert_eq!(advance.source_entry, "advance");
    assert_eq!(advance.layout.workgroup_size, [64, 1, 1]);
    assert_eq!(advance.dispatch, Some([32, 1, 1]));
    assert_eq!(advance.repeat, 12);
    let validate = &bundle.manifest.passes[1];
    assert_eq!(validate.source_entry, "validate");
    assert_eq!(validate.layout.workgroup_size, [1, 1, 1]);
    assert_eq!(validate.dispatch, Some([1, 1, 1]));
    assert_eq!(bundle.manifest.passes[2].source_entry, "paint");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(
        &naga::front::wgsl::parse_str(&bundle.pass_wgsl[0].source)
            .expect("deep stage WGSL should parse"),
    )
    .expect("deep stage WGSL should validate for the browser profile");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(
        &naga::front::wgsl::parse_str(&bundle.pass_wgsl[1].source)
            .expect("deep validation WGSL should parse"),
    )
    .expect("deep validation WGSL should validate for the browser profile");
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
            eprintln!("  factor-tree stage grid SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
            return None;
        }
        Err(error) => panic!(
            "factor-tree stage grid has no WebGPU adapter ({error:?}). Set up Vulkan/lavapipe, or \
             set MB2_ALLOW_GPU_SKIP to record an explicit non-execution."
        ),
    };
    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            ..Default::default()
        })) {
            Ok(pair) => pair,
            Err(error) if allow_skip => {
                eprintln!("  factor-tree stage grid SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
                return None;
            }
            Err(error) => panic!("factor-tree stage-grid device request failed: {error:?}"),
        };
    Some((adapter, device, queue))
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

fn direct_ntt(values: &[u32]) -> Vec<u32> {
    let log_n = values.len().ilog2();
    let maximal_root = pow_mod(31, 15);
    let root = pow_mod(u64::from(maximal_root), 1 << (TWO_ADICITY - log_n));
    let modulus = u64::from(MODULUS);
    (0..values.len())
        .map(|index| {
            let point = pow_mod(u64::from(root), index as u32);
            values
                .iter()
                .fold((0u64, 1u64), |(sum, power), value| {
                    (
                        (sum + u64::from(*value % MODULUS) * power) % modulus,
                        power * u64::from(point) % modulus,
                    )
                })
                .0 as u32
        })
        .collect()
}

fn direct_coset_evaluation(coefficients: &[u32], points: usize, shift: u32) -> Vec<u32> {
    let log_n = points.ilog2();
    let maximal_root = pow_mod(31, 15);
    let root = pow_mod(u64::from(maximal_root), 1 << (TWO_ADICITY - log_n));
    let modulus = u64::from(MODULUS);
    (0..points)
        .map(|index| {
            let point =
                u64::from(shift) * u64::from(pow_mod(u64::from(root), index as u32)) % modulus;
            coefficients
                .iter()
                .fold((0u64, 1u64), |(sum, power), coefficient| {
                    (
                        (sum + u64::from(*coefficient % MODULUS) * power) % modulus,
                        power * point % modulus,
                    )
                })
                .0 as u32
        })
        .collect()
}

fn validation_marker(points: u32, stages: u32) -> u32 {
    0x8000_0000 | (stages << 24) | points
}

fn vectors() -> Vec<Vec<u32>> {
    let mut pseudo_random = Vec::with_capacity(POINTS);
    let mut state = 0x3141_5926u32;
    for _ in 0..POINTS {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        pseudo_random.push(state);
    }
    vec![
        (0..POINTS as u32).collect(),
        vec![
            0,
            1,
            MODULUS - 1,
            MODULUS,
            MODULUS + 1,
            u32::MAX,
            17,
            31,
            65_535,
            65_536,
            1_000_000,
            0x8000_0000,
            123_456_789,
            987_654_321,
            42,
            7,
        ],
        pseudo_random,
    ]
}

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn read_words(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &wgpu::Buffer,
    words: usize,
) -> Vec<u32> {
    let size = (words * 4) as u64;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("factor-tree stage-grid readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("factor-tree stage-grid readback encoder"),
    });
    encoder.copy_buffer_to_buffer(source, 0, &staging, 0, size);
    queue.submit(Some(encoder.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        tx.send(result)
            .expect("map callback receiver should remain open");
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(180)),
        })
        .expect("factor-tree stage-grid submission should complete");
    rx.recv()
        .expect("map callback should fire")
        .expect("stage-grid staging buffer should map");
    let mapped = slice.get_mapped_range();
    let result = mapped
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one u32")))
        .collect();
    drop(mapped);
    staging.unmap();
    result
}

struct ExecutablePass {
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    dispatch: [u32; 3],
    repeat: u32,
}

fn submit_passes(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    passes: &[ExecutablePass],
    label: &'static str,
) {
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });
    for pass in passes {
        let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(label),
            timestamp_writes: None,
        });
        compute.set_pipeline(&pass.pipeline);
        compute.set_bind_group(0, &pass.bind_group, &[]);
        for _ in 0..pass.repeat {
            compute.dispatch_workgroups(pass.dispatch[0], pass.dispatch[1], pass.dispatch[2]);
        }
    }
    queue.submit(Some(encoder.finish()));
}

#[test]
fn factor_tree_stage_grid_matches_direct_polynomial_evaluation_on_webgpu() {
    let bundle = compile_stage_grid();
    assert_eq!(bundle.manifest.passes.len(), COMPUTE_PASSES + 1);
    let prepare = &bundle.manifest.passes[0];
    let advance = &bundle.manifest.passes[1];
    let prepare_inverse = &bundle.manifest.passes[2];
    let advance_inverse = &bundle.manifest.passes[3];
    let validate_inverse = &bundle.manifest.passes[4];
    let prepare_lde = &bundle.manifest.passes[5];
    let advance_lde = &bundle.manifest.passes[6];
    let validate_lde = &bundle.manifest.passes[7];
    let finish_inverse = &bundle.manifest.passes[8];
    assert_eq!(prepare.source_entry, "prepare");
    assert_eq!(prepare.layout.workgroup_size, [4, 1, 1]);
    assert_eq!(prepare.dispatch, Some([2, 1, 1]));
    assert_eq!(prepare.repeat, 1);
    assert_eq!(advance.source_entry, "advance");
    assert_eq!(advance.layout.workgroup_size, [4, 1, 1]);
    assert_eq!(advance.dispatch, Some([2, 1, 1]));
    assert_eq!(advance.repeat, STAGES);
    assert_eq!(prepare_inverse.source_entry, "prepare_inverse");
    assert_eq!(prepare_inverse.layout.workgroup_size, [4, 1, 1]);
    assert_eq!(prepare_inverse.dispatch, Some([2, 1, 1]));
    assert_eq!(prepare_inverse.repeat, 1);
    assert_eq!(advance_inverse.source_entry, "advance_inverse");
    assert_eq!(advance_inverse.layout.workgroup_size, [4, 1, 1]);
    assert_eq!(advance_inverse.dispatch, Some([2, 1, 1]));
    assert_eq!(advance_inverse.repeat, STAGES);
    assert_eq!(validate_inverse.source_entry, "validate_inverse");
    assert_eq!(validate_inverse.layout.workgroup_size, [1, 1, 1]);
    assert_eq!(validate_inverse.dispatch, Some([1, 1, 1]));
    assert_eq!(validate_inverse.repeat, 1);
    assert_eq!(prepare_lde.source_entry, "prepare_lde");
    assert_eq!(prepare_lde.layout.workgroup_size, [4, 1, 1]);
    assert_eq!(prepare_lde.dispatch, Some([4, 1, 1]));
    assert_eq!(prepare_lde.repeat, 1);
    assert_eq!(advance_lde.source_entry, "advance_lde");
    assert_eq!(advance_lde.layout.workgroup_size, [4, 1, 1]);
    assert_eq!(advance_lde.dispatch, Some([4, 1, 1]));
    assert_eq!(advance_lde.repeat, LDE_STAGES);
    assert_eq!(validate_lde.source_entry, "validate_lde");
    assert_eq!(validate_lde.layout.workgroup_size, [1, 1, 1]);
    assert_eq!(validate_lde.dispatch, Some([1, 1, 1]));
    assert_eq!(validate_lde.repeat, 1);
    assert_eq!(finish_inverse.source_entry, "finish_inverse");
    assert_eq!(finish_inverse.layout.workgroup_size, [4, 1, 1]);
    assert_eq!(finish_inverse.dispatch, Some([2, 1, 1]));
    assert_eq!(finish_inverse.repeat, 1);

    for (pass, shader) in bundle.manifest.passes[..COMPUTE_PASSES]
        .iter()
        .zip(&bundle.pass_wgsl[..COMPUTE_PASSES])
    {
        let module = naga::front::wgsl::parse_str(&shader.source)
            .unwrap_or_else(|error| panic!("{} WGSL parse failed: {error:?}", pass.source_entry));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap_or_else(|error| {
            panic!(
                "{} WGSL browser validation failed: {error:?}",
                pass.source_entry
            )
        });
    }
    let advance_wgsl = &bundle.pass_wgsl[1].source;
    assert!(
        !advance_wgsl.contains("var<workgroup>"),
        "the portable baseline must not depend on fused shared memory",
    );
    assert!(
        !advance_wgsl.contains("workgroupBarrier") && !advance_wgsl.contains("storageBarrier"),
        "dispatch boundaries, not intra-kernel barriers, order portable stages",
    );
    let inverse_wgsl = &bundle.pass_wgsl[3].source;
    assert!(
        !inverse_wgsl.contains("var<workgroup>")
            && !inverse_wgsl.contains("workgroupBarrier")
            && !inverse_wgsl.contains("storageBarrier"),
        "the portable inverse must retain the same dispatch-boundary policy",
    );

    let Some((adapter, device, queue)) = request_browser_profile_device() else {
        return;
    };
    eprintln!(
        "  factor-tree stage-grid WebGPU adapter (no required features): {}",
        adapter.get_info().name,
    );
    eprintln!(
        "  factor-tree stage-grid WGSL: prepare={} bytes, advance={} bytes, inverse={} bytes",
        bundle.pass_wgsl[0].source.len(),
        advance_wgsl.len(),
        inverse_wgsl.len(),
    );

    let resource_shapes = bundle
        .manifest
        .resources
        .iter()
        .map(|resource| (resource.name.as_str(), (resource.length, resource.stride)))
        .collect::<HashMap<_, _>>();
    assert_eq!(resource_shapes["source"], (POINTS as u32, 4));
    assert_eq!(resource_shapes["values"], (POINTS as u32, 4));
    assert_eq!(resource_shapes["progress"], (BUTTERFLIES as u32, 4));
    assert_eq!(resource_shapes["valid"], (BUTTERFLIES as u32, 4));
    assert_eq!(
        resource_shapes["stage_receipt"],
        (BUTTERFLIES as u32 * STAGES, 4),
    );
    assert_eq!(resource_shapes["roundtrip"], (POINTS as u32, 4));
    assert_eq!(resource_shapes["inverse_progress"], (BUTTERFLIES as u32, 4),);
    assert_eq!(resource_shapes["inverse_valid"], (BUTTERFLIES as u32, 4),);
    assert_eq!(resource_shapes.len(), 8);
    let resources = bundle
        .manifest
        .resources
        .iter()
        .map(|resource| {
            (
                resource.name.clone(),
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(resource.name.as_str()),
                    size: u64::from(resource.length) * u64::from(resource.stride),
                    usage: wgpu::BufferUsages::STORAGE
                        | wgpu::BufferUsages::COPY_SRC
                        | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
            )
        })
        .collect::<HashMap<_, _>>();

    let mut extras = HashMap::<(usize, u32), wgpu::Buffer>::new();
    for (pass_index, pass) in bundle.manifest.passes[..COMPUTE_PASSES].iter().enumerate() {
        for binding in &pass.layout.bindings {
            if binding.role != WebBindingRole::Resource {
                extras.insert(
                    (pass_index, binding.binding),
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(&format!(
                            "{} extra binding {}",
                            pass.source_entry, binding.name
                        )),
                        size: u64::from(binding.span),
                        usage: wgpu::BufferUsages::STORAGE
                            | wgpu::BufferUsages::COPY_SRC
                            | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                );
            }
        }
    }

    let mut executable = Vec::new();
    for (pass_index, (pass, shader)) in bundle.manifest.passes[..COMPUTE_PASSES]
        .iter()
        .zip(&bundle.pass_wgsl[..COMPUTE_PASSES])
        .enumerate()
    {
        let layout_entries = pass
            .layout
            .bindings
            .iter()
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding: binding.binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage {
                        read_only: binding.access == WebBindingAccess::Read,
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect::<Vec<_>>();
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("{} bind-group layout", pass.source_entry)),
            entries: &layout_entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&format!("{} pipeline layout", pass.source_entry)),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&format!("{} Fe stage-grid WGSL", pass.source_entry)),
            source: wgpu::ShaderSource::Wgsl(shader.source.as_str().into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(&format!("{} Fe stage-grid pipeline", pass.source_entry)),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let entries = pass
            .layout
            .bindings
            .iter()
            .map(|binding| {
                let buffer = if binding.role == WebBindingRole::Resource {
                    resources
                        .get(&binding.name)
                        .unwrap_or_else(|| panic!("missing resource {}", binding.name))
                } else {
                    extras
                        .get(&(pass_index, binding.binding))
                        .unwrap_or_else(|| panic!("missing extra binding {}", binding.name))
                };
                wgpu::BindGroupEntry {
                    binding: binding.binding,
                    resource: buffer.as_entire_binding(),
                }
            })
            .collect::<Vec<_>>();
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("{} stage-grid bindings", pass.source_entry)),
            layout: &bind_group_layout,
            entries: &entries,
        });
        executable.push(ExecutablePass {
            pipeline,
            bind_group,
            dispatch: pass.dispatch.expect("fixed compute dispatch"),
            repeat: pass.repeat,
        });
    }

    let zero_values = vec![0u32; POINTS];
    let zero_lanes = vec![0u32; BUTTERFLIES];
    let zero_receipt = vec![0u32; BUTTERFLIES * STAGES as usize];
    for input in vectors() {
        queue.write_buffer(&resources["source"], 0, &bytes(&input));
        queue.write_buffer(&resources["values"], 0, &bytes(&zero_values));
        queue.write_buffer(&resources["progress"], 0, &bytes(&zero_lanes));
        queue.write_buffer(&resources["valid"], 0, &bytes(&zero_lanes));
        queue.write_buffer(&resources["stage_receipt"], 0, &bytes(&zero_receipt));
        queue.write_buffer(&resources["roundtrip"], 0, &bytes(&zero_values));
        queue.write_buffer(&resources["inverse_progress"], 0, &bytes(&zero_lanes));
        queue.write_buffer(&resources["inverse_valid"], 0, &bytes(&zero_lanes));

        submit_passes(
            &device,
            &queue,
            &executable[..5],
            "factor-tree transform and inverse validation",
        );

        assert_eq!(
            read_words(&device, &queue, &resources["values"], POINTS),
            direct_ntt(&input),
            "WebGPU factor-tree stages must equal direct polynomial evaluation",
        );
        assert_eq!(
            read_words(&device, &queue, &resources["progress"], BUTTERFLIES,),
            vec![STAGES; BUTTERFLIES],
        );
        assert_eq!(
            read_words(&device, &queue, &resources["valid"], BUTTERFLIES),
            vec![1; BUTTERFLIES],
        );
        let expected_receipt = [2u32, 4, 8, 16]
            .into_iter()
            .enumerate()
            .flat_map(|(stage, width)| {
                std::iter::repeat_n((stage as u32) * 65_536 + width, BUTTERFLIES)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            read_words(
                &device,
                &queue,
                &resources["stage_receipt"],
                BUTTERFLIES * STAGES as usize,
            ),
            expected_receipt,
            "every GPU lane must observe the type-derived stage order",
        );
        assert_eq!(
            read_words(&device, &queue, &resources["inverse_progress"], BUTTERFLIES,),
            std::iter::once(validation_marker(POINTS as u32, STAGES))
                .chain(std::iter::repeat_n(STAGES, BUTTERFLIES - 1))
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            read_words(&device, &queue, &resources["inverse_valid"], BUTTERFLIES,),
            vec![1; BUTTERFLIES],
        );

        submit_passes(
            &device,
            &queue,
            &executable[5..],
            "validated coset extension and inverse finish",
        );
        assert_eq!(
            read_words(&device, &queue, &resources["roundtrip"], POINTS),
            input
                .iter()
                .map(|value| value % MODULUS)
                .collect::<Vec<_>>(),
            "the inverse stage grid and N^-1 pass must recover canonical inputs",
        );
        assert_eq!(
            read_words(&device, &queue, &resources["stage_receipt"], LDE_POINTS),
            direct_coset_evaluation(&input, LDE_POINTS, 7),
            "validated inverse coefficients must extend on the requested coset",
        );
        assert_eq!(
            read_words(&device, &queue, &resources["source"], LDE_BUTTERFLIES,),
            std::iter::once(validation_marker(LDE_POINTS as u32, LDE_STAGES))
                .chain(std::iter::repeat_n(LDE_STAGES, LDE_BUTTERFLIES - 1,))
                .collect::<Vec<_>>(),
        );

        let mut incomplete_inverse = vec![STAGES; BUTTERFLIES];
        incomplete_inverse[3] -= 1;
        queue.write_buffer(
            &resources["inverse_progress"],
            0,
            &bytes(&incomplete_inverse),
        );
        queue.write_buffer(
            &resources["stage_receipt"],
            0,
            &bytes(&vec![u32::MAX; LDE_POINTS]),
        );
        queue.write_buffer(
            &resources["source"],
            0,
            &bytes(&vec![u32::MAX; LDE_BUTTERFLIES]),
        );
        submit_passes(
            &device,
            &queue,
            &executable[4..=5],
            "validate incomplete inverse before coset preparation",
        );
        assert_eq!(
            read_words(&device, &queue, &resources["inverse_progress"], 1),
            vec![0],
            "one incomplete cursor must invalidate the batch receipt",
        );
        assert_eq!(
            read_words(&device, &queue, &resources["stage_receipt"], LDE_POINTS),
            vec![0; LDE_POINTS],
            "an invalid batch receipt must fail closed before coset extension",
        );
        assert_eq!(
            read_words(&device, &queue, &resources["source"], LDE_BUTTERFLIES,),
            vec![0; LDE_BUTTERFLIES],
        );
    }

    for ((pass_index, binding), buffer) in &extras {
        let pass = &bundle.manifest.passes[*pass_index];
        let layout = pass
            .layout
            .bindings
            .iter()
            .find(|candidate| candidate.binding == *binding)
            .expect("extra binding layout");
        assert_eq!(
            read_words(&device, &queue, buffer, (layout.span / 4) as usize),
            vec![0; (layout.span / 4) as usize],
            "{} {} must retain a clean compiler receipt",
            pass.source_entry,
            layout.name,
        );
    }
}
