//! Executed WebGPU gate for the production Mandelbrot FRI query grid.
//!
//! Fe derives the 114-query Cartesian schedule from the security policy and
//! the thirteen-round FRI tree. This test checks the emitted browser contract,
//! executes every work item, and compares the full receipts with an
//! independently expanded Rust product space.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WebBindingAccess, WebBindingRole, WebBuildOptions, WebBundle, WebBundleMode, resolve_web_entry,
};
use hir::hir_def::HirIngot;
use url::Url;

const QUERY_COUNT: usize = 114;
const EVALUATIONS_PER_QUERY: usize = 25;
const SIBLINGS_PER_QUERY: usize = 132;
const EVALUATION_ITEMS: usize = QUERY_COUNT * EVALUATIONS_PER_QUERY;
const SIBLING_ITEMS: usize = QUERY_COUNT * SIBLINGS_PER_QUERY;
const RECEIPT_WORDS: usize = 4;
const THREADS: u32 = 64;
const EVALUATION_GROUPS: u32 = 45;
const SIBLING_GROUPS: u32 = 236;
const EVALUATION_PADDING: usize = EVALUATION_GROUPS as usize * THREADS as usize - EVALUATION_ITEMS;
const SIBLING_PADDING: usize = SIBLING_GROUPS as usize * THREADS as usize - SIBLING_ITEMS;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repository root")
        .to_path_buf()
}

fn compile_query_grid() -> WebBundle {
    let dir = repo_root().join("crates/codegen/tests/fixtures/mandelbrot_proof_query_grid_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "query-grid ingot initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("query-grid fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "query-grid source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("mandelbrot_proof_query_grid".into())),
    )
    .expect("query-grid fixture should compile into a WebBundle")
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
            eprintln!("  production query grid SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
            return None;
        }
        Err(error) => panic!(
            "production query grid has no WebGPU adapter ({error:?}). Set up Vulkan/lavapipe, or \
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
                eprintln!("  production query grid SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
                return None;
            }
            Err(error) => panic!("production query-grid device request failed: {error:?}"),
        };
    Some((adapter, device, queue))
}

fn read_words(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &wgpu::Buffer,
    words: usize,
) -> Vec<u32> {
    let size = (words * 4) as u64;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("production query-grid readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("production query-grid readback encoder"),
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
        .expect("production query-grid submission should complete");
    rx.recv()
        .expect("map callback should fire")
        .expect("query-grid staging buffer should map");
    let mapped = slice.get_mapped_range();
    let result = mapped
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one u32")))
        .collect();
    drop(mapped);
    staging.unmap();
    result
}

fn expected_receipts(queries: usize, openings: usize) -> Vec<u32> {
    let mut expected = Vec::with_capacity(queries * openings * RECEIPT_WORDS);
    for query in 0..queries {
        for opening in 0..openings {
            expected.extend_from_slice(&[1, query as u32, query as u32 + 1, opening as u32]);
        }
    }
    expected
}

struct ExecutablePass {
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    dispatch: [u32; 3],
}

#[test]
fn production_policy_query_grid_is_derived_and_exact_on_webgpu() {
    let compile_started = Instant::now();
    let bundle = compile_query_grid();
    let compile_elapsed = compile_started.elapsed();
    assert_eq!(bundle.manifest.passes.len(), 3);
    let evaluation_pass = &bundle.manifest.passes[0];
    let sibling_pass = &bundle.manifest.passes[1];
    assert_eq!(evaluation_pass.source_entry, "place_evaluations");
    assert_eq!(evaluation_pass.layout.workgroup_size, [THREADS, 1, 1]);
    assert_eq!(evaluation_pass.dispatch, Some([EVALUATION_GROUPS, 1, 1]));
    assert_eq!(evaluation_pass.repeat, 1);
    assert_eq!(sibling_pass.source_entry, "place_siblings");
    assert_eq!(sibling_pass.layout.workgroup_size, [THREADS, 1, 1]);
    assert_eq!(sibling_pass.dispatch, Some([SIBLING_GROUPS, 1, 1]));
    assert_eq!(sibling_pass.repeat, 1);

    for (pass, shader) in bundle.manifest.passes[..2]
        .iter()
        .zip(&bundle.pass_wgsl[..2])
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

    let Some((adapter, device, queue)) = request_browser_profile_device() else {
        return;
    };
    let adapter_name = adapter.get_info().name;
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
    assert_eq!(
        bundle
            .manifest
            .resources
            .iter()
            .find(|resource| resource.name == "evaluation_receipts")
            .map(|resource| resource.length),
        Some((EVALUATION_ITEMS * RECEIPT_WORDS) as u32),
    );
    assert_eq!(
        bundle
            .manifest
            .resources
            .iter()
            .find(|resource| resource.name == "sibling_receipts")
            .map(|resource| resource.length),
        Some((SIBLING_ITEMS * RECEIPT_WORDS) as u32),
    );
    assert_eq!(
        bundle
            .manifest
            .resources
            .iter()
            .find(|resource| resource.name == "padding_receipts")
            .map(|resource| resource.length),
        Some((EVALUATION_PADDING + SIBLING_PADDING) as u32),
    );

    let mut executable = Vec::new();
    for (pass, shader) in bundle.manifest.passes[..2]
        .iter()
        .zip(&bundle.pass_wgsl[..2])
    {
        assert!(
            pass.layout
                .bindings
                .iter()
                .all(|binding| binding.role == WebBindingRole::Resource),
            "query-grid compute passes should need only actor resources",
        );
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
            label: Some(pass.source_entry.as_str()),
            entries: &layout_entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(pass.source_entry.as_str()),
            bind_group_layouts: &[Some(&bind_group_layout)],
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
        let entries = pass
            .layout
            .bindings
            .iter()
            .map(|binding| wgpu::BindGroupEntry {
                binding: binding.binding,
                resource: resources
                    .get(&binding.name)
                    .unwrap_or_else(|| panic!("missing resource {}", binding.name))
                    .as_entire_binding(),
            })
            .collect::<Vec<_>>();
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(pass.source_entry.as_str()),
            layout: &bind_group_layout,
            entries: &entries,
        });
        executable.push(ExecutablePass {
            pipeline,
            bind_group,
            dispatch: pass.dispatch.expect("fixed compute dispatch"),
        });
    }

    let execution_started = Instant::now();
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("production query-grid execution"),
    });
    for pass in &executable {
        let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Fe-derived production query grid"),
            timestamp_writes: None,
        });
        compute.set_pipeline(&pass.pipeline);
        compute.set_bind_group(0, &pass.bind_group, &[]);
        compute.dispatch_workgroups(pass.dispatch[0], pass.dispatch[1], pass.dispatch[2]);
    }
    queue.submit(Some(encoder.finish()));

    let evaluation_receipts = read_words(
        &device,
        &queue,
        &resources["evaluation_receipts"],
        EVALUATION_ITEMS * RECEIPT_WORDS,
    );
    let sibling_receipts = read_words(
        &device,
        &queue,
        &resources["sibling_receipts"],
        SIBLING_ITEMS * RECEIPT_WORDS,
    );
    let padding_receipts = read_words(
        &device,
        &queue,
        &resources["padding_receipts"],
        EVALUATION_PADDING + SIBLING_PADDING,
    );
    let execution_elapsed = execution_started.elapsed();

    assert_eq!(
        evaluation_receipts,
        expected_receipts(QUERY_COUNT, EVALUATIONS_PER_QUERY),
        "every production evaluation work item must match the independent product space",
    );
    assert_eq!(
        sibling_receipts,
        expected_receipts(QUERY_COUNT, SIBLINGS_PER_QUERY),
        "every production sibling work item must match the independent product space",
    );
    let expected_padding = (EVALUATION_ITEMS..EVALUATION_GROUPS as usize * THREADS as usize)
        .chain(SIBLING_ITEMS..SIBLING_GROUPS as usize * THREADS as usize)
        .map(|lane| lane as u32 + 1)
        .collect::<Vec<_>>();
    assert_eq!(
        padding_receipts, expected_padding,
        "all padded GPU lanes must resolve to invalid work items",
    );

    eprintln!(
        "  production query grid: adapter={adapter_name:?}, compile={compile_elapsed:?}, \
         execute_and_read={execution_elapsed:?}, evaluation_items={EVALUATION_ITEMS}, \
         sibling_items={SIBLING_ITEMS}, wgsl_bytes={}",
        bundle.pass_wgsl[..2]
            .iter()
            .map(|shader| shader.source.len())
            .sum::<usize>(),
    );
}
