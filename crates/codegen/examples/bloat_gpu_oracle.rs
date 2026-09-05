//! Execute saved scalar-helper pilot shaders against an independent pixel oracle.
//! This checks a finite input domain on the named adapter, not general equivalence.

use sha2::{Digest, Sha256};
use std::{error::Error, path::Path, time::Duration};

const WIDTH: u32 = 257;
const HEIGHT: u32 = 9;

fn main() -> Result<(), Box<dyn Error>> {
    let paths = std::env::args().skip(1).collect::<Vec<_>>();
    if paths.is_empty() {
        return Err("usage: bloat_gpu_oracle BASELINE.wgsl [VARIANT.wgsl ...]".into());
    }
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        ..Default::default()
    }))?;
    let info = adapter.get_info();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::empty(),
        ..Default::default()
    }))?;
    let mut results = Vec::new();
    for path in paths {
        let path = Path::new(&path).canonicalize()?;
        if path.metadata()?.len() > 64 * 1024 * 1024 {
            return Err("shader exceeds the 64 MiB pilot input limit".into());
        }
        let source = std::fs::read_to_string(&path)?;
        let module = naga::front::wgsl::parse_str(&source)?;
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)?;
        let pixels = render(&device, &queue, &source)?;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                // Independent of the compiler IR and emitted WGSL. The fixture
                // returns one packed RGBA word from mix_words(x, y) = 3*x + y.
                let expected = (3 * x + y).to_le_bytes();
                let offset = ((y * WIDTH + x) * 4) as usize;
                if pixels[offset..offset + 4] != expected {
                    return Err(format!(
                        "{}: pixel ({x},{y}) is {:?}, expected {:?}",
                        path.display(),
                        &pixels[offset..offset + 4],
                        expected
                    )
                    .into());
                }
            }
        }
        results.push(serde_json::json!({
            "shader": path,
            "shader_sha256": hex::encode(Sha256::digest(source.as_bytes())),
            "shader_bytes": source.len(),
            "wgsl_parse_and_validate": "passed",
            "executed_oracle": "passed",
            "pixels_checked": WIDTH * HEIGHT,
            "rgba_sha256": hex::encode(Sha256::digest(&pixels)),
        }));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "riffcat-bloat-scalar-gpu-oracle/1",
            "oracle": "packed_rgba_u32(3*x+y)",
            "width": WIDTH, "height": HEIGHT,
            "adapter": info.name,
            "device_type": format!("{:?}", info.device_type),
            "backend": format!("{:?}", info.backend),
            "driver": info.driver,
            "driver_info": info.driver_info,
            "required_features": "empty",
            "results": results,
            "limits": "Finite nonnegative pixel domain on this adapter only; no performance or general equivalence claim."
        }))?
    );
    Ok(())
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &str,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("bloat-pilot-saved-shader"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let input = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("unused-broadcast-input"),
        size: 4,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let bindings = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&bindings)],
        immediate_size: 0,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &bindings,
        entries: &[wgpu::BindGroupEntry {
            binding: 1,
            resource: input.as_entire_binding(),
        }],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("bloat-pilot-render"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_fullscreen"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
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
    let extent = wgpu::Extent3d {
        width: WIDTH,
        height: HEIGHT,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let row_bytes = (WIDTH * 4).div_ceil(256) * 256;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row_bytes * HEIGHT),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
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
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(HEIGHT),
            },
        },
        extent,
    );
    queue.submit(Some(encoder.finish()));
    let slice = staging.slice(..);
    let (send, receive) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = send.send(result);
    });
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(30)),
    })?;
    receive.recv_timeout(Duration::from_secs(30))??;
    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for y in 0..HEIGHT {
        let start = (y * row_bytes) as usize;
        pixels.extend_from_slice(&mapped[start..start + (WIDTH * 4) as usize]);
    }
    drop(mapped);
    staging.unmap();
    Ok(pixels)
}
