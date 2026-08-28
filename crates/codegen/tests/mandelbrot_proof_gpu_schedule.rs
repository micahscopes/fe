//! Focused execution gate for the staged Mandelbrot proof LDE.
//!
//! The test initializes only the four trace columns, executes the five
//! Fe-authored LDE passes in manifest order, and compares every output with a
//! direct inverse DFT plus polynomial evaluation. It also checks the exact
//! Conal-derived repeat counts, private stage cursors, coset predicates, and
//! compiler trap lanes. The oracle deliberately does not replay Fe's radix-two
//! butterflies.

use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    resolve_web_entry, WebBinding, WebBindingAccess, WebBindingRole, WebBuildOptions, WebBundle,
    WebBundleMode,
};
use hir::hir_def::HirIngot;
use url::Url;

const MODULUS: u32 = 2_013_265_921;
const TWO_ADICITY: u32 = 27;
const TRACE_ROWS: usize = 4;
const LDE_ROWS: usize = 16;
const COLUMN_COUNT: usize = 4;
const PROOF_WORDS: usize = 578;
const LDE_START: usize = 16;
const LDE_VALID_START: usize = 97;
const LDE_FIRST_PASS: usize = 1;
const LDE_PASS_COUNT: usize = 5;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repo-root ancestor")
        .to_path_buf()
}

fn compile_lde_graph() -> WebBundle {
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
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("demos/sketches/mandelbrot_proof_gpu".into())),
    )
    .expect("Mandelbrot proof actor should compile into a WebBundle")
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
            eprintln!("  staged LDE SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
            return None;
        }
        Err(error) => panic!(
            "staged LDE has no WebGPU adapter ({error:?}). Set up Vulkan/lavapipe, or set \
             MB2_ALLOW_GPU_SKIP to record an explicit non-execution."
        ),
    };
    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            ..Default::default()
        })) {
            Ok(pair) => pair,
            Err(error) if allow_skip => {
                eprintln!("  staged LDE SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
                return None;
            }
            Err(error) => panic!("staged LDE browser-profile device request failed: {error:?}"),
        };
    Some((adapter, device, queue))
}

fn buffer_type(binding: &WebBinding) -> wgpu::BufferBindingType {
    wgpu::BufferBindingType::Storage {
        read_only: binding.access == WebBindingAccess::Read,
    }
}

struct DeviceResource {
    name: String,
    buffer: wgpu::Buffer,
}

struct LdePass {
    pipeline: wgpu::ComputePipeline,
    group: wgpu::BindGroup,
    auxiliary: Vec<(WebBindingRole, wgpu::Buffer)>,
}

fn resource<'a>(resources: &'a [DeviceResource], name: &str) -> &'a wgpu::Buffer {
    &resources
        .iter()
        .find(|resource| resource.name == name)
        .unwrap_or_else(|| panic!("missing actor resource `{name}`"))
        .buffer
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

fn compile_lde_passes(
    device: &wgpu::Device,
    bundle: &WebBundle,
    resources: &[DeviceResource],
) -> Vec<LdePass> {
    let end = LDE_FIRST_PASS + LDE_PASS_COUNT;
    bundle.manifest.passes[LDE_FIRST_PASS..end]
        .iter()
        .zip(&bundle.pass_wgsl[LDE_FIRST_PASS..end])
        .map(|(pass, shader)| {
            let module = naga::front::wgsl::parse_str(&shader.source)
                .unwrap_or_else(|error| panic!("{} WGSL should parse: {error}", pass.source_entry));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::default(),
            )
            .validate(&module)
            .unwrap_or_else(|error| panic!("{} WGSL should validate: {error}", pass.source_entry));

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
            let auxiliary = pass
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
                    (
                        binding.role,
                        device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some(binding.name.as_str()),
                            size: u64::from(binding.span),
                            usage,
                            mapped_at_creation: false,
                        }),
                    )
                })
                .collect::<Vec<_>>();
            let bind_entries =
                pass.layout
                    .bindings
                    .iter()
                    .map(|binding| {
                        let buffer =
                            if binding.role == WebBindingRole::Resource {
                                resource(resources, binding.name.as_str())
                            } else {
                                &auxiliary
                                    .iter()
                                    .zip(pass.layout.bindings.iter().filter(|candidate| {
                                        candidate.role != WebBindingRole::Resource
                                    }))
                                    .find(|(_, candidate)| candidate.binding == binding.binding)
                                    .expect("owned pass binding")
                                    .0
                                     .1
                            };
                        wgpu::BindGroupEntry {
                            binding: binding.binding,
                            resource: buffer.as_entire_binding(),
                        }
                    })
                    .collect::<Vec<_>>();
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(pass.source_entry.as_str()),
                layout: &layout,
                entries: &bind_entries,
            });
            LdePass {
                pipeline,
                group,
                auxiliary,
            }
        })
        .collect()
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

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one u32")))
        .collect()
}

fn mapped_bytes(device: &wgpu::Device, staging: &wgpu::Buffer) -> Vec<u8> {
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(180)),
        })
        .expect("staged LDE submission should complete");
    rx.recv()
        .expect("map callback should fire")
        .expect("test-only staging buffer should map");
    let bytes = slice.get_mapped_range().to_vec();
    staging.unmap();
    bytes
}

#[test]
fn staged_lde_schedule_matches_direct_dft_on_webgpu() {
    let bundle = compile_lde_graph();
    let end = LDE_FIRST_PASS + LDE_PASS_COUNT;
    let passes = &bundle.manifest.passes[LDE_FIRST_PASS..end];
    assert_eq!(
        passes
            .iter()
            .map(|pass| (
                pass.source_entry.as_str(),
                pass.repeat,
                pass.layout.workgroup_size
            ))
            .collect::<Vec<_>>(),
        vec![
            ("prepare_lde_inverse", 1, [8, 1, 1]),
            ("advance_lde_inverse", 2, [8, 1, 1]),
            ("prepare_lde_forward", 1, [32, 1, 1]),
            ("advance_lde_forward", 4, [32, 1, 1]),
            ("finish_lde", 1, [32, 1, 1]),
        ]
    );

    let Some((adapter, device, queue)) = request_browser_profile_device() else {
        return;
    };
    eprintln!(
        "  staged LDE WebGPU adapter (no required features): {}",
        adapter.get_info().name
    );
    let resources = allocate_resources(&device, &bundle);
    let lde_passes = compile_lde_passes(&device, &bundle, &resources);

    let trace_words = trace_columns().into_iter().flatten().collect::<Vec<_>>();
    let trace_bytes = trace_words
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect::<Vec<_>>();
    queue.write_buffer(resource(&resources, "proof"), 0, &trace_bytes);

    let copied = [
        ("proof", PROOF_WORDS),
        ("lde_inverse_progress", 8),
        ("lde_values", 64),
        ("lde_progress", 32),
        ("lde_coset_valid", 4),
    ];
    let resource_bytes = copied.iter().map(|(_, words)| words * 4).sum::<usize>();
    let trap_bytes = lde_passes
        .iter()
        .flat_map(|pass| &pass.auxiliary)
        .filter(|(role, _)| *role == WebBindingRole::Output)
        .map(|(_, buffer)| buffer.size() as usize)
        .sum::<usize>();
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("staged LDE test-only readback"),
        size: (resource_bytes + trap_bytes) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("staged LDE execution"),
    });
    for ((manifest_pass, shader_pass), pass) in passes
        .iter()
        .zip(&bundle.pass_wgsl[LDE_FIRST_PASS..end])
        .zip(&lde_passes)
    {
        eprintln!(
            "  staged LDE pass {}: {} bytes, repeat {}",
            manifest_pass.source_entry,
            shader_pass.source.len(),
            manifest_pass.repeat
        );
        let dispatch = manifest_pass.dispatch.expect("compute dispatch");
        let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(manifest_pass.source_entry.as_str()),
            timestamp_writes: None,
        });
        compute.set_pipeline(&pass.pipeline);
        compute.set_bind_group(0, &pass.group, &[]);
        for _ in 0..manifest_pass.repeat {
            compute.dispatch_workgroups(dispatch[0], dispatch[1], dispatch[2]);
        }
    }

    let mut offset = 0u64;
    for (name, word_count) in copied {
        let bytes = (word_count * 4) as u64;
        encoder.copy_buffer_to_buffer(resource(&resources, name), 0, &staging, offset, bytes);
        offset += bytes;
    }
    for (_, buffer) in lde_passes
        .iter()
        .flat_map(|pass| &pass.auxiliary)
        .filter(|(role, _)| *role == WebBindingRole::Output)
    {
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, offset, buffer.size());
        offset += buffer.size();
    }
    assert_eq!(offset as usize, resource_bytes + trap_bytes);
    queue.submit(Some(encoder.finish()));

    let result = words(&mapped_bytes(&device, &staging));
    let expected = trace_columns()
        .iter()
        .flat_map(|column| direct_coset_lde(column, LDE_ROWS, 7))
        .collect::<Vec<_>>();
    let mut cursor = 0usize;
    let proof = &result[cursor..cursor + PROOF_WORDS];
    cursor += PROOF_WORDS;
    let inverse_progress = &result[cursor..cursor + 8];
    cursor += 8;
    let lde_values = &result[cursor..cursor + 64];
    cursor += 64;
    let forward_progress = &result[cursor..cursor + 32];
    cursor += 32;
    let coset_valid = &result[cursor..cursor + 4];
    cursor += 4;
    let traps = &result[cursor..];

    assert_eq!(
        &proof[LDE_START..LDE_START + COLUMN_COUNT * LDE_ROWS],
        expected.as_slice(),
        "staged LDE proof words must match the independent direct DFT"
    );
    assert_eq!(lde_values, expected.as_slice());
    assert_eq!(inverse_progress, &[2; 8]);
    assert_eq!(forward_progress, &[4; 32]);
    assert_eq!(coset_valid, &[1; COLUMN_COUNT]);
    assert_eq!(
        &proof[LDE_VALID_START..LDE_VALID_START + COLUMN_COUNT],
        &[1; COLUMN_COUNT]
    );
    assert!(
        traps.iter().all(|word| *word == 0),
        "every staged LDE invocation must remain trap-free: {traps:?}"
    );
}
