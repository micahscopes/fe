//! Executed semantic gate for Fe-authored vertex + fragment raster lowering.
//!
//! A 2x2 target observes a varying emitted at one triangle vertex. Exact color
//! counts prove that the generated GPU pipeline executes both Fe bodies,
//! interpolates their typed interface, shares actor state, and honors the
//! Fe-derived draw count. WGSL text alone cannot establish those properties.

use std::path::Path;

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WasmCompileOptions, WebBindingAccess, WebBuildOptions, WebBundle,
    compile_runtime_package_wasm_with_options,
};
use hir::hir_def::HirIngot;
use url::Url;

fn compile_bundle() -> WebBundle {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/actor_raster_typed");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = db
        .workspace()
        .containing_ingot(&db, url)
        .unwrap()
        .root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let bundle = WebBundle::compile(&db, top_mod, WebBuildOptions::render("shade", None)).unwrap();
    assert!(
        bundle.wgsl.contains("fn tinted_heat")
            && bundle.wgsl.contains("fn shade_color")
            && bundle.wgsl.matches("shade_color").count() >= 2,
        "paired authored raster stages must retain the closed scalar helper graph:\n{}",
        bundle.wgsl,
    );
    bundle
}

fn device() -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let allow_skip = std::env::var_os("MB2_ALLOW_GPU_SKIP").is_some();
    let instance = wgpu::Instance::default();
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        ..Default::default()
    })) {
        Ok(adapter) => adapter,
        Err(error) if allow_skip => {
            eprintln!("authored raster GPU gate skipped (MB2_ALLOW_GPU_SKIP): {error:?}");
            return None;
        }
        Err(error) => panic!("authored raster GPU gate has no adapter: {error:?}"),
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::empty(),
        ..Default::default()
    }))
    .unwrap();
    Some((adapter, device, queue))
}

fn readback(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(30)),
        })
        .unwrap();
    rx.recv().unwrap().unwrap();
    let bytes = slice.get_mapped_range().to_vec();
    buffer.unmap();
    bytes
}

#[test]
fn fe_vertex_varying_and_fragment_execute_as_one_gpu_pipeline() {
    let bundle = compile_bundle();
    let pass = &bundle.manifest.passes[0];
    assert_eq!(pass.draw_vertices, Some(3));
    let Some((adapter, device, queue)) = device() else {
        return;
    };
    eprintln!("authored raster adapter: {}", adapter.get_info().name);

    let binding = &pass.layout.bindings[0];
    assert_eq!(binding.members[0].name, "tint");
    assert_eq!(binding.access, WebBindingAccess::Read);
    let state = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Fe authored raster state"),
        size: 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&state, 0, &0.4f32.to_le_bytes());
    let group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Fe authored raster group layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: binding.binding,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Fe authored raster group"),
        layout: &group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: binding.binding,
            resource: state.as_entire_binding(),
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Fe authored raster pipeline layout"),
        bind_group_layouts: &[Some(&group_layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Fe authored raster WGSL"),
        source: wgpu::ShaderSource::Wgsl(bundle.wgsl.as_str().into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Fe authored raster pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: pass.layout.vertex_entry.as_deref(),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: pass.layout.fragment_entry.as_deref(),
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
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Fe authored raster target"),
        size: wgpu::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Fe authored raster readback"),
        size: 512,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let view = target.create_view(&Default::default());
        let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Fe authored raster draw"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
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
        render.set_pipeline(&pipeline);
        render.set_bind_group(0, &group, &[]);
        render.draw(0..pass.draw_vertices.unwrap(), 0..1);
    }
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(2),
            },
        },
        wgpu::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let bytes = readback(&device, &staging);
    let pixels = [
        &bytes[0..4],
        &bytes[4..8],
        &bytes[256..260],
        &bytes[260..264],
    ];
    let hot = pixels
        .iter()
        .filter(|pixel| **pixel == [0, 0, 255, 255])
        .count();
    let cool = pixels
        .iter()
        .filter(|pixel| **pixel == [255, 0, 0, 255])
        .count();
    assert_eq!(
        (hot, cool),
        (1, 3),
        "interpolated Fe varying pixels: {pixels:?}"
    );
}

#[test]
fn fe_vertex_and_fragment_bodies_match_the_source_oracle_in_wasmtime() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/actor_raster_typed");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = db
        .workspace()
        .containing_ingot(&db, url)
        .unwrap()
        .root_mod(&db);
    let package = mir::build_wasm_runtime_package_for_entries(
        &db,
        top_mod,
        &["vertices".to_string(), "shade".to_string()],
    )
    .unwrap();
    let wasm = compile_runtime_package_wasm_with_options(
        &db,
        &package,
        WasmCompileOptions::default().with_optimization(),
    )
    .unwrap()
    .bytes;
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let vertices = instance
        .get_typed_func::<(i32, f32), (f32, f32, f32, f32, f32, f32, f32, f32)>(
            &mut store, "vertices",
        )
        .unwrap();
    let shade = instance
        .get_typed_func::<(f32, f32, f32, f32, f32), i32>(&mut store, "shade")
        .unwrap();

    let expected = [
        (-1.0, -1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0),
        (3.0, -1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0),
        (-1.0, 3.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0),
    ];
    for (index, oracle) in expected.into_iter().enumerate() {
        assert_eq!(
            vertices.call(&mut store, (index as i32, 0.4)).unwrap(),
            oracle
        );
    }
    assert_eq!(
        shade.call(&mut store, (0.0, 0.0, 1.0, 0.75, 0.4)).unwrap() as u32,
        0xffff_0000
    );
    assert_eq!(
        shade.call(&mut store, (0.0, 0.0, 1.0, 0.25, 0.4)).unwrap() as u32,
        0xff00_00ff
    );
}
