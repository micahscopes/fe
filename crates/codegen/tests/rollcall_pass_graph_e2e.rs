//! Executed browser-profile oracle for the Rollcall-shaped typed pass graph.
//!
//! The Fe actor is compiled through `WebBundle`, then its generated WGSL is
//! submitted to one WebGPU device as compute `collect`, compute `fold`, and
//! fragment `display`. Readback exists only in this test. The production
//! runtime keeps both actor resources on the GPU.

use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WebBinding, WebBindingAccess, WebBindingRole, WebBuildOptions, WebBundle, WebBundleMode,
    resolve_web_entry,
};
use hir::hir_def::HirIngot;
use url::Url;

const LEAVES: [u32; 8] = [3, 5, 7, 9, 11, 13, 15, 17];
const NODES: [u32; 8] = [896_599, 12_151, 30_583, 185, 377, 569, 761, 8];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repo-root ancestor")
        .to_path_buf()
}

fn compile_rollcall_graph() -> WebBundle {
    let dir = repo_root().join("demos/sketches/rollcall_pipeline");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "rollcall pipeline ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("rollcall pipeline should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "rollcall pipeline source diagnostics:\n{diagnostics}"
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("demos/sketches/rollcall_pipeline".into())),
    )
    .expect("rollcall pipeline should compile into a WebBundle")
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
            eprintln!(
                "  Rollcall pass graph SKIPPED (MB2_ALLOW_GPU_SKIP): no WebGPU adapter: {error:?}"
            );
            return None;
        }
        Err(error) => panic!(
            "Rollcall pass graph has no WebGPU adapter ({error:?}). Set up Vulkan/lavapipe, or set \
             MB2_ALLOW_GPU_SKIP to record an explicit non-execution on a genuinely GPU-less host."
        ),
    };
    let (device, queue) = match pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            ..Default::default()
        },
    )) {
        Ok(pair) => pair,
        Err(error) if allow_skip => {
            eprintln!(
                "  Rollcall pass graph SKIPPED (MB2_ALLOW_GPU_SKIP): device request failed: {error:?}"
            );
            return None;
        }
        Err(error) => panic!(
            "Rollcall browser-profile device request with no required features failed: {error:?}"
        ),
    };
    Some((adapter, device, queue))
}

fn buffer_type(binding: &WebBinding) -> wgpu::BufferBindingType {
    wgpu::BufferBindingType::Storage {
        read_only: binding.access == WebBindingAccess::Read,
    }
}

fn compute_pipeline_and_group(
    device: &wgpu::Device,
    label: &str,
    wgsl: &str,
    bindings: &[WebBinding],
    leaves: &wgpu::Buffer,
    nodes: &wgpu::Buffer,
    trap: &wgpu::Buffer,
) -> (wgpu::ComputePipeline, wgpu::BindGroup) {
    let layout_entries = bindings
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
        label: Some(label),
        entries: &layout_entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let group_entries = bindings
        .iter()
        .map(|binding| {
            let buffer = match binding.role {
                WebBindingRole::Resource if binding.name == "leaves" => leaves,
                WebBindingRole::Resource if binding.name == "nodes" => nodes,
                WebBindingRole::Output if binding.name == "trap" => trap,
                _ => panic!(
                    "unexpected Rollcall binding {} ({:?})",
                    binding.name, binding.role
                ),
            };
            wgpu::BindGroupEntry {
                binding: binding.binding,
                resource: buffer.as_entire_binding(),
            }
        })
        .collect::<Vec<_>>();
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout: &layout,
        entries: &group_entries,
    });
    (pipeline, group)
}

fn render_pipeline_and_group(
    device: &wgpu::Device,
    wgsl: &str,
    bindings: &[WebBinding],
    leaves: &wgpu::Buffer,
    nodes: &wgpu::Buffer,
) -> (wgpu::RenderPipeline, wgpu::BindGroup) {
    let layout_entries = bindings
        .iter()
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding: binding.binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: buffer_type(binding),
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect::<Vec<_>>();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rollcall display"),
        entries: &layout_entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rollcall display"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rollcall display"),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rollcall display"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_fullscreen"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let group_entries = bindings
        .iter()
        .map(|binding| {
            let buffer = match binding.name.as_str() {
                "leaves" => leaves,
                "nodes" => nodes,
                _ => panic!("unexpected Rollcall display binding {}", binding.name),
            };
            wgpu::BindGroupEntry {
                binding: binding.binding,
                resource: buffer.as_entire_binding(),
            }
        })
        .collect::<Vec<_>>();
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rollcall display"),
        layout: &layout,
        entries: &group_entries,
    });
    (pipeline, group)
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one u32")))
        .collect()
}

#[test]
fn rollcall_pass_graph_executes_exact_u32_and_pixel_oracles_on_webgpu() {
    let bundle = compile_rollcall_graph();
    assert_eq!(bundle.manifest.passes.len(), 3);
    assert_eq!(bundle.pass_wgsl.len(), 3);
    let Some((adapter, device, queue)) = request_browser_profile_device() else {
        return;
    };
    eprintln!(
        "  Rollcall pass graph WebGPU adapter (no required features): {}",
        adapter.get_info().name
    );

    let leaves = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rollcall leaves"),
        size: 32,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let nodes = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rollcall nodes"),
        size: 32,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let trap = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rollcall private-Mem trap"),
        size: 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rollcall test-only readback"),
        size: 132,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let render_staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rollcall test-only pixel readback"),
        size: 256,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let render_target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rollcall offscreen target"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let render_view = render_target.create_view(&Default::default());

    let collect_pass = &bundle.manifest.passes[0];
    let fold_pass = &bundle.manifest.passes[1];
    let display_pass = &bundle.manifest.passes[2];
    assert_eq!(collect_pass.source_entry, "collect");
    assert_eq!(fold_pass.source_entry, "fold");
    assert_eq!(display_pass.source_entry, "display");
    let (collect, collect_group) = compute_pipeline_and_group(
        &device,
        "rollcall collect",
        &bundle.pass_wgsl[0].source,
        &collect_pass.layout.bindings,
        &leaves,
        &nodes,
        &trap,
    );
    let (fold, fold_group) = compute_pipeline_and_group(
        &device,
        "rollcall fold",
        &bundle.pass_wgsl[1].source,
        &fold_pass.layout.bindings,
        &leaves,
        &nodes,
        &trap,
    );
    let (display, display_group) = render_pipeline_and_group(
        &device,
        &bundle.pass_wgsl[2].source,
        &display_pass.layout.bindings,
        &leaves,
        &nodes,
    );

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("rollcall ordered compute graph"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("collect"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&collect);
        pass.set_bind_group(0, &collect_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&leaves, 0, &staging, 0, 32);
    encoder.copy_buffer_to_buffer(&nodes, 0, &staging, 32, 32);
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("fold"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&fold);
        pass.set_bind_group(0, &fold_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&leaves, 0, &staging, 64, 32);
    encoder.copy_buffer_to_buffer(&nodes, 0, &staging, 96, 32);
    encoder.copy_buffer_to_buffer(&trap, 0, &staging, 128, 4);
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("display"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &render_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&display);
        pass.set_bind_group(0, &display_group, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &render_target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &render_staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(1),
            },
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
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
            timeout: Some(std::time::Duration::from_secs(30)),
        })
        .expect("Rollcall WebGPU submission should complete");
    rx.recv()
        .expect("map callback should fire")
        .expect("test-only staging buffer should map");
    let data = slice.get_mapped_range();
    let result = words(&data);
    assert_eq!(&result[0..8], &LEAVES, "collect leaves checkpoint");
    assert_eq!(&result[8..16], &[0; 8], "collect nodes checkpoint");
    assert_eq!(&result[16..24], &LEAVES, "fold must preserve leaves");
    assert_eq!(&result[24..32], &NODES, "fold nodes checkpoint");
    assert_eq!(result[32], 0, "private Mem trap word");
    drop(data);
    staging.unmap();

    let pixel_slice = render_staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    pixel_slice.map_async(wgpu::MapMode::Read, move |result| {
        tx.send(result)
            .expect("pixel map callback receiver should remain open");
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(30)),
        })
        .expect("Rollcall pixel readback should complete");
    rx.recv()
        .expect("pixel map callback should fire")
        .expect("test-only pixel staging buffer should map");
    let pixel = pixel_slice.get_mapped_range();
    assert_eq!(&pixel[0..4], &[95, 166, 5, 255], "display pixel");
    drop(pixel);
    render_staging.unmap();
}
