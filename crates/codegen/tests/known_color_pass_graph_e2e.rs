//! Executed B4.0 gate for the smallest typed WebGPU pass graph.
//!
//! The Fe actor, generated WGSL, manifest-v6 graph, shared storage buffer, and
//! ordered compute then fragment execution are all exercised here. Readback is
//! test-only; the production runtime never maps the resource or rendered pixel.

use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WebBinding, WebBindingAccess, WebBindingRole, WebBuildOptions, WebBundle, WebBundleMode,
    WebFeResponsibility, resolve_web_entry,
};
use hir::hir_def::HirIngot;
use url::Url;

const RE_BITS: u32 = 1.0f32.to_bits();
const IM_BITS: u32 = (-2.0f32).to_bits();
const EXPECTED_PIXEL: [u8; 4] = [0, 0, 128, 255];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repo-root ancestor")
        .to_path_buf()
}

fn compile_known_color_graph() -> WebBundle {
    let dir = repo_root().join("demos/sketches/known_color");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "known-color ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("known-color should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "known-color source diagnostics:\n{diagnostics}"
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("demos/sketches/known_color".into())),
    )
    .expect("known-color should compile into a WebBundle")
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
                "  Known-color pass graph SKIPPED (MB2_ALLOW_GPU_SKIP): no WebGPU adapter: {error:?}"
            );
            return None;
        }
        Err(error) => panic!(
            "Known-color pass graph has no WebGPU adapter ({error:?}). Set up Vulkan/lavapipe, \
             or set MB2_ALLOW_GPU_SKIP on a genuinely GPU-less host."
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
                "  Known-color pass graph SKIPPED (MB2_ALLOW_GPU_SKIP): device request failed: {error:?}"
            );
            return None;
        }
        Err(error) => panic!("known-color browser-profile device request failed: {error:?}"),
    };
    Some((adapter, device, queue))
}

fn buffer_type(binding: &WebBinding) -> wgpu::BufferBindingType {
    wgpu::BufferBindingType::Storage {
        read_only: binding.access == WebBindingAccess::Read,
    }
}

fn layout_entries(
    bindings: &[WebBinding],
    visibility: wgpu::ShaderStages,
) -> Vec<wgpu::BindGroupLayoutEntry> {
    bindings
        .iter()
        .map(|binding| {
            assert_eq!(binding.role, WebBindingRole::Resource);
            wgpu::BindGroupLayoutEntry {
                binding: binding.binding,
                visibility,
                ty: wgpu::BindingType::Buffer {
                    ty: buffer_type(binding),
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }
        })
        .collect()
}

fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    bindings: &[WebBinding],
    resource: &wgpu::Buffer,
) -> wgpu::BindGroup {
    let entries = bindings
        .iter()
        .map(|binding| wgpu::BindGroupEntry {
            binding: binding.binding,
            resource: resource.as_entire_binding(),
        })
        .collect::<Vec<_>>();
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("known-color shared resource"),
        layout,
        entries: &entries,
    })
}

fn map_bytes(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
    let slice = buffer.slice(..);
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
        .expect("known-color WebGPU submission should complete");
    rx.recv()
        .expect("map callback should fire")
        .expect("test-only staging buffer should map");
    let data = slice.get_mapped_range().to_vec();
    buffer.unmap();
    data
}

#[test]
fn known_color_graph_preserves_typed_bits_and_exact_pixel_on_webgpu() {
    let bundle = compile_known_color_graph();
    assert_eq!(bundle.manifest.protocol_version, 6);
    assert!(
        !bundle.wasm.is_empty(),
        "the Fe-authored surface quality and recovery policies need their control Wasm"
    );
    assert_eq!(
        bundle.manifest.artifacts.wasm_bytes,
        Some(bundle.wasm.len() as u64)
    );
    assert!(
        bundle
            .manifest
            .provenance
            .fe_responsibilities
            .contains(&WebFeResponsibility::BackingQualityPolicy)
    );
    assert!(
        bundle
            .manifest
            .provenance
            .fe_responsibilities
            .contains(&WebFeResponsibility::DeviceRecoveryPolicy)
    );
    assert!(
        bundle
            .manifest
            .canonical_status
            .omission_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("no CPU fallback")),
        "control Wasm must not be mislabeled as a CPU rendering fallback"
    );
    assert_eq!(bundle.manifest.resources.len(), 1);
    assert_eq!(bundle.manifest.passes.len(), 2);
    assert_eq!(bundle.pass_wgsl.len(), 2);

    let resource_manifest = &bundle.manifest.resources[0];
    assert_eq!(resource_manifest.length, 1);
    assert_eq!(resource_manifest.stride, 8);
    assert_eq!(resource_manifest.span, 8);
    let compute_pass = &bundle.manifest.passes[0];
    let fragment_pass = &bundle.manifest.passes[1];
    assert_eq!(compute_pass.source_entry, "seed");
    assert_eq!(fragment_pass.source_entry, "paint");
    assert_eq!(compute_pass.layout.bindings.len(), 1);
    assert_eq!(fragment_pass.layout.bindings.len(), 1);
    let compute_binding = &compute_pass.layout.bindings[0];
    let fragment_binding = &fragment_pass.layout.bindings[0];
    assert_eq!(compute_binding.name, resource_manifest.name);
    assert_eq!(fragment_binding.name, resource_manifest.name);
    assert_eq!(compute_binding.group, resource_manifest.group);
    assert_eq!(fragment_binding.group, resource_manifest.group);
    assert_eq!(compute_binding.binding, resource_manifest.binding);
    assert_eq!(fragment_binding.binding, resource_manifest.binding);
    assert_eq!(compute_binding.access, WebBindingAccess::ReadWrite);
    assert_eq!(fragment_binding.access, WebBindingAccess::Read);

    let Some((adapter, device, queue)) = request_browser_profile_device() else {
        return;
    };
    eprintln!(
        "  Known-color pass graph WebGPU adapter (no required features): {}",
        adapter.get_info().name
    );

    let resource = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("known-color typed storage"),
        size: 8,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let resource_staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("known-color test-only resource readback"),
        size: 8,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let pixel_staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("known-color test-only pixel readback"),
        size: 256,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("known-color offscreen target"),
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
    let target_view = target.create_view(&Default::default());

    let compute_entries =
        layout_entries(&compute_pass.layout.bindings, wgpu::ShaderStages::COMPUTE);
    let compute_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("known-color compute layout"),
        entries: &compute_entries,
    });
    let compute_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("known-color compute pipeline layout"),
        bind_group_layouts: &[Some(&compute_layout)],
        immediate_size: 0,
    });
    let compute_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("generated known-color compute WGSL"),
        source: wgpu::ShaderSource::Wgsl(bundle.pass_wgsl[0].source.as_str().into()),
    });
    let compute = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("known-color seed"),
        layout: Some(&compute_pipeline_layout),
        module: &compute_module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let compute_group = bind_group(
        &device,
        &compute_layout,
        &compute_pass.layout.bindings,
        &resource,
    );

    let fragment_entries =
        layout_entries(&fragment_pass.layout.bindings, wgpu::ShaderStages::FRAGMENT);
    let fragment_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("known-color fragment layout"),
        entries: &fragment_entries,
    });
    let fragment_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("known-color fragment pipeline layout"),
        bind_group_layouts: &[Some(&fragment_layout)],
        immediate_size: 0,
    });
    let fragment_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("generated known-color fragment WGSL"),
        source: wgpu::ShaderSource::Wgsl(bundle.pass_wgsl[1].source.as_str().into()),
    });
    let fragment = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("known-color paint"),
        layout: Some(&fragment_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &fragment_module,
            entry_point: Some("vs_fullscreen"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &fragment_module,
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
    let fragment_group = bind_group(
        &device,
        &fragment_layout,
        &fragment_pass.layout.bindings,
        &resource,
    );

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("known-color ordered pass graph"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("seed"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&compute);
        pass.set_bind_group(0, &compute_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&resource, 0, &resource_staging, 0, 8);
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("paint"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
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
        pass.set_pipeline(&fragment);
        pass.set_bind_group(0, &fragment_group, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &pixel_staging,
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

    let resource_bytes = map_bytes(&device, &resource_staging);
    assert_eq!(
        &resource_bytes,
        &[RE_BITS.to_le_bytes(), IM_BITS.to_le_bytes()].concat(),
        "compute must preserve the two exact packed f32 bit patterns"
    );
    let pixel_bytes = map_bytes(&device, &pixel_staging);
    assert_eq!(&pixel_bytes[0..4], &EXPECTED_PIXEL, "exact painted pixel");
    eprintln!(
        "  Known-color receipt: storage=[0x{RE_BITS:08x}, 0x{IM_BITS:08x}], pixel={EXPECTED_PIXEL:?}"
    );
}
