//! Executed browser-profile gate for the production perturbation Mandelbrot.
//!
//! The generated compute and fragment WGSL run in one ordered command buffer.
//! A shallow zero-reference frame must contain both interior and escaped
//! pixels. A second frame uses an escaping reference plus nearby bounded
//! pixels and proves center-orbit reanchoring prevents sentinel-sized blobs.

use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WebBinding, WebBindingAccess, WebBindingRole, WebBuildOptions, WebBundle, WebBundleMode,
    resolve_web_entry,
};
use hir::hir_def::HirIngot;
use num_bigint::{BigInt, BigUint};
use url::Url;

const WIDTH: u32 = 8;
const HEIGHT: u32 = 8;
const ROW_BYTES: u32 = 256;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repo-root ancestor")
        .to_path_buf()
}

fn compile_graph() -> WebBundle {
    let dir = repo_root().join("demos/sketches/perturbational_mandelbrot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "perturbation Mandelbrot ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("perturbation Mandelbrot should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "perturbation Mandelbrot source diagnostics:\n{diagnostics}"
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(
            entry,
            Some("demos/sketches/perturbational_mandelbrot".into()),
        ),
    )
    .expect("perturbation Mandelbrot should compile into a WebBundle")
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
            eprintln!("  Perturbation Mandelbrot GPU gate SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
            return None;
        }
        Err(error) => panic!(
            "Perturbation Mandelbrot has no WebGPU adapter ({error:?}). Set up Vulkan/lavapipe, \
             or set MB2_ALLOW_GPU_SKIP on a genuinely GPU-less host."
        ),
    };
    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            ..Default::default()
        })) {
            Ok(pair) => pair,
            Err(error) if allow_skip => {
                eprintln!(
                    "  Perturbation Mandelbrot GPU gate SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}"
                );
                return None;
            }
            Err(error) => panic!("browser-profile device request failed: {error:?}"),
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
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding: binding.binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: buffer_type(binding),
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect()
}

fn group_entries<'a>(
    bindings: &'a [WebBinding],
    orbit: &'a wgpu::Buffer,
    params: &'a wgpu::Buffer,
) -> Vec<wgpu::BindGroupEntry<'a>> {
    bindings
        .iter()
        .map(|binding| {
            let buffer = match binding.role {
                WebBindingRole::Resource if binding.name == "orbit" => orbit,
                WebBindingRole::Input => params,
                _ => panic!(
                    "unexpected perturbation binding {} ({:?})",
                    binding.name, binding.role
                ),
            };
            wgpu::BindGroupEntry {
                binding: binding.binding,
                resource: buffer.as_entire_binding(),
            }
        })
        .collect()
}

fn params_bytes(values: [f32; 10]) -> Vec<u8> {
    values
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>()
}

const FIXED8_FRACTIONAL_BITS: i32 = 91;

/// Decode one finite binary32 value into the integer units used by Fixed<8>.
/// This mirrors the mathematical result of Fe's `from_f32<8>` but shares no
/// limb code with it: bits below the 2^-91 floor are truncated.
fn fixed8_integer(value: f32) -> BigInt {
    let bits = value.to_bits();
    if bits & 0x7fff_ffff == 0 {
        return BigInt::from(0u32);
    }
    let exponent = ((bits >> 23) & 0xff) as i32 - 127;
    let significand = BigInt::from((1u32 << 23) | (bits & 0x7f_ffff));
    let shift = exponent - 23 + FIXED8_FRACTIONAL_BITS;
    let magnitude = if shift >= 0 {
        significand << shift as usize
    } else {
        significand >> (-shift) as usize
    };
    if bits >> 31 == 0 {
        magnitude
    } else {
        -magnitude
    }
}

/// Independent round-to-nearest-ties-up Fixed<8> multiplication over one
/// BigInt, rather than Fe's recursive 13-bit limb implementation.
fn fixed8_mul(a: &BigInt, b: &BigInt) -> BigInt {
    let product = a * b;
    let scale = BigUint::from(1u32) << FIXED8_FRACTIONAL_BITS as usize;
    let half = &scale >> 1usize;
    let rounded = (product.magnitude() + half) / scale;
    BigInt::from_biguint(product.sign(), rounded)
}

fn fixed8_coordinate(words: &[f32], offset: f32) -> BigInt {
    words
        .iter()
        .copied()
        .chain(std::iter::once(offset))
        .map(fixed8_integer)
        .sum()
}

fn adaptive_iteration_limit(zoom: f32) -> u32 {
    let mut z = zoom;
    let mut depth = 0u32;
    for _ in 0..96 {
        if z < 1.0 {
            z *= 2.0;
            depth += 1;
        }
    }
    (256 + depth * 64).min(2000)
}

/// Exact Fixed<8> classification for the same four-word center and f32 pixel
/// offset consumed by the production graph. This is intentionally a separate
/// BigInt recurrence, not a Rust translation of perturbation arithmetic.
fn exact_fixed8_pixel_escapes(values: &[f32; 10], px: u32, py: u32) -> bool {
    let res = values[9];
    let u = (px as f32 + 0.5) / res * 2.0 - 1.0;
    let v = 1.0 - (py as f32 + 0.5) / res * 2.0;
    let cx = fixed8_coordinate(&values[0..4], u * values[8]);
    let cy = fixed8_coordinate(&values[4..8], v * values[8]);
    let mut zx = BigInt::from(0u32);
    let mut zy = BigInt::from(0u32);
    let four = BigInt::from(4u32) << FIXED8_FRACTIONAL_BITS as usize;
    for _ in 0..adaptive_iteration_limit(values[8]) {
        let xx = fixed8_mul(&zx, &zx);
        let yy = fixed8_mul(&zy, &zy);
        let xy = fixed8_mul(&zx, &zy);
        zx = &xx - &yy + &cx;
        zy = (&xy << 1usize) + &cy;
        let magnitude = fixed8_mul(&zx, &zx) + fixed8_mul(&zy, &zy);
        if magnitude > four {
            return true;
        }
    }
    false
}

struct Pipelines {
    compute: wgpu::ComputePipeline,
    compute_group: wgpu::BindGroup,
    fragment: wgpu::RenderPipeline,
    fragment_group: wgpu::BindGroup,
}

fn build_pipelines(
    device: &wgpu::Device,
    bundle: &WebBundle,
    orbit: &wgpu::Buffer,
    compute_params: &wgpu::Buffer,
    fragment_params: &wgpu::Buffer,
) -> Pipelines {
    let compute_pass = &bundle.manifest.passes[0];
    let compute_entries =
        layout_entries(&compute_pass.layout.bindings, wgpu::ShaderStages::COMPUTE);
    let compute_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("perturbation reference layout"),
        entries: &compute_entries,
    });
    let compute_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("perturbation reference pipeline layout"),
        bind_group_layouts: &[Some(&compute_layout)],
        immediate_size: 0,
    });
    let compute_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("generated Fixed reference shader"),
        source: wgpu::ShaderSource::Wgsl(bundle.pass_wgsl[0].source.as_str().into()),
    });
    let compute = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Fixed reference producer"),
        layout: Some(&compute_pipeline_layout),
        module: &compute_module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let compute_group_entries = group_entries(&compute_pass.layout.bindings, orbit, compute_params);
    let compute_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Fixed reference bindings"),
        layout: &compute_layout,
        entries: &compute_group_entries,
    });

    let fragment_pass = &bundle.manifest.passes[1];
    let fragment_entries =
        layout_entries(&fragment_pass.layout.bindings, wgpu::ShaderStages::FRAGMENT);
    let fragment_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("perturbation fragment layout"),
        entries: &fragment_entries,
    });
    let fragment_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("perturbation fragment pipeline layout"),
        bind_group_layouts: &[Some(&fragment_layout)],
        immediate_size: 0,
    });
    let fragment_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("generated f32 perturbation shader"),
        source: wgpu::ShaderSource::Wgsl(bundle.pass_wgsl[1].source.as_str().into()),
    });
    let fragment = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("f32 perturbation pixels"),
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
    let fragment_group_entries =
        group_entries(&fragment_pass.layout.bindings, orbit, fragment_params);
    let fragment_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("perturbation fragment bindings"),
        layout: &fragment_layout,
        entries: &fragment_group_entries,
    });
    Pipelines {
        compute,
        compute_group,
        fragment,
        fragment_group,
    }
}

fn render_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipelines: &Pipelines,
    target: &wgpu::Texture,
    target_view: &wgpu::TextureView,
    readback: &wgpu::Buffer,
) -> Vec<[u8; 4]> {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("ordered reference then perturbation frame"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("build reference"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipelines.compute);
        pass.set_bind_group(0, &pipelines.compute_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("perturb pixels"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
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
        pass.set_pipeline(&pipelines.fragment);
        pass.set_bind_group(0, &pipelines.fragment_group, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ROW_BYTES),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        tx.send(result)
            .expect("pixel map callback receiver should remain open");
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .expect("perturbation frame should complete");
    rx.recv()
        .expect("pixel map callback should fire")
        .expect("pixel readback should map");
    let data = slice.get_mapped_range();
    let pixels = (0..HEIGHT as usize)
        .flat_map(|row| {
            let start = row * ROW_BYTES as usize;
            data[start..start + WIDTH as usize * 4]
                .chunks_exact(4)
                .map(|pixel| pixel.try_into().expect("RGBA8 pixel"))
                .collect::<Vec<[u8; 4]>>()
        })
        .collect();
    drop(data);
    readback.unmap();
    pixels
}

fn render_params(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipelines: &Pipelines,
    compute_params: &wgpu::Buffer,
    fragment_params: &wgpu::Buffer,
    target: &wgpu::Texture,
    target_view: &wgpu::TextureView,
    readback: &wgpu::Buffer,
    values: [f32; 10],
) -> Vec<[u8; 4]> {
    let bytes = params_bytes(values);
    queue.write_buffer(compute_params, 0, &bytes);
    queue.write_buffer(fragment_params, 0, &bytes);
    render_frame(device, queue, pipelines, target, target_view, readback)
}

fn assert_exact_or_visible_glitch(values: [f32; 10], pixels: &[[u8; 4]], label: &str) -> usize {
    let black = [0, 0, 0, 255];
    let magenta = [255, 0, 255, 255];
    let mut inside = 0usize;
    let mut escaped = 0usize;
    let mut glitches = 0usize;
    for py in 0..HEIGHT {
        for px in 0..WIDTH {
            let pixel = pixels[(py * WIDTH + px) as usize];
            let want_escaped = exact_fixed8_pixel_escapes(&values, px, py);
            if pixel == magenta {
                glitches += 1;
                continue;
            }
            let got_escaped = pixel != black && pixel != magenta;
            assert_eq!(
                got_escaped, want_escaped,
                "{label} pixel ({px},{py}) classification mismatch: RGBA={pixel:?}"
            );
            if want_escaped {
                escaped += 1;
            } else {
                inside += 1;
            }
        }
    }
    eprintln!(
        "  exact-or-visible-glitch receipt {label}: {inside} inside, {escaped} escaped, \
         0 false classifications, {glitches} magenta"
    );
    glitches
}

#[test]
fn perturbation_graph_executes_reference_before_pixels_and_exposes_glitches() {
    let bundle = compile_graph();
    assert_eq!(bundle.manifest.passes.len(), 2);
    assert_eq!(bundle.manifest.passes[0].source_entry, "build_reference");
    assert_eq!(bundle.manifest.passes[1].source_entry, "display_reference");
    let Some((adapter, device, queue)) = request_browser_profile_device() else {
        return;
    };
    eprintln!(
        "  Perturbation Mandelbrot WebGPU adapter (no required features): {}",
        adapter.get_info().name
    );

    let orbit = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shared reference orbit"),
        size: 2003 * 32,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let compute_params = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("reference params"),
        size: 40,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let fragment_params = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("perturbation params"),
        size: 40,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("perturbation offscreen target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
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
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only perturbation pixel readback"),
        size: u64::from(ROW_BYTES * HEIGHT),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let pipelines = build_pipelines(&device, &bundle, &orbit, &compute_params, &fragment_params);

    let ordinary = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 8.0];
    let pixels = render_params(
        &device,
        &queue,
        &pipelines,
        &compute_params,
        &fragment_params,
        &target,
        &target_view,
        &readback,
        ordinary,
    );
    let black = [0, 0, 0, 255];
    let magenta = [255, 0, 255, 255];
    assert!(
        pixels.contains(&black),
        "zero reference should retain interior pixels"
    );
    assert!(
        pixels
            .iter()
            .any(|pixel| *pixel != black && *pixel != magenta),
        "zero reference should also produce escaped palette pixels"
    );

    // The reference c=(1,1) escapes. At the lower-left sample, zoom=8/7
    // contributes approximately (-1,-1), keeping that perturbed pixel near
    // c=(0,0). Exhausting the short reference must reanchor at z=0 before
    // any invalid-reference sentinel is consumed, without producing a pink
    // blob or changing exact Fixed<8> escape classification.
    let sentinel = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.1428572, 8.0];
    let pixels = render_params(
        &device,
        &queue,
        &pipelines,
        &compute_params,
        &fragment_params,
        &target,
        &target_view,
        &readback,
        sentinel,
    );
    let magenta_count = pixels.iter().filter(|pixel| **pixel == magenta).count();
    assert_eq!(
        magenta_count, 0,
        "reference exhaustion must reanchor against the center orbit, not produce a pink blob"
    );
    assert_eq!(
        assert_exact_or_visible_glitch(sentinel, &pixels, "escaping reference overlap"),
        0,
        "center-orbit reanchoring should resolve this overlap without a visible glitch"
    );
    eprintln!(
        "  sentinel-overlap receipt: {magenta_count}/{} pixels ambiguous after exact \
         center-orbit reanchoring",
        pixels.len()
    );

    // A separate directed frame proves that real numerical ambiguity remains
    // visible instead of being silently classified inside. For a zero
    // reference, each corner has u^2 + v^2 = 2 * 0.875^2. This adjacent-f32
    // zoom places the first iterate just above the escape boundary and inside
    // the shader's explicit ambiguity band. Only those four symmetric samples
    // are deliberately ambiguous.
    let boundary = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.6162442, 8.0];
    let corner_delta = -0.875f32 * boundary[8];
    let corner_magnitude = corner_delta * corner_delta + corner_delta * corner_delta;
    assert!(
        corner_magnitude > 4.0
            && (corner_magnitude - 4.0).abs() <= 0.000004 * 1.0f32.max(corner_magnitude),
        "the directed frame must actually land in the declared ambiguity band"
    );
    let pixels = render_params(
        &device,
        &queue,
        &pipelines,
        &compute_params,
        &fragment_params,
        &target,
        &target_view,
        &readback,
        boundary,
    );
    let corner_indices = [0usize, 7, 56, 63];
    for (index, pixel) in pixels.iter().enumerate() {
        assert_eq!(
            *pixel == magenta,
            corner_indices.contains(&index),
            "only the four directed boundary samples may be visibly ambiguous: \
             pixel {index} is {pixel:?}"
        );
    }
    eprintln!("  visible-glitch receipt: 4/64 directed boundary pixels are magenta");

    // Directed classification receipts. The first frame is wholly inside the
    // main cardioid. The next two exercise the cancellation-heavy seahorse
    // reference at increasing depth. Every pixel is compared against a wholly
    // independent BigInt Fixed<8> orbit at the identical adaptive budget.
    let interior = [-0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.05, 8.0];
    let pixels = render_params(
        &device,
        &queue,
        &pipelines,
        &compute_params,
        &fragment_params,
        &target,
        &target_view,
        &readback,
        interior,
    );
    assert_eq!(
        assert_exact_or_visible_glitch(interior, &pixels, "cardioid zoom=5e-2"),
        0,
        "the directed cardioid frame should be unambiguous"
    );

    let seahorse_center = [
        -0.7436438798904419,
        -0.0000000071467170,
        -0.0000000000000003,
        -0.00000000000000000000002,
        0.1318259090185165,
        -0.0000000048132045,
        0.00000000000000025,
        0.000000000000000000000015,
        0.0000001,
        8.0,
    ];
    let pixels = render_params(
        &device,
        &queue,
        &pipelines,
        &compute_params,
        &fragment_params,
        &target,
        &target_view,
        &readback,
        seahorse_center,
    );
    assert_eq!(
        assert_exact_or_visible_glitch(seahorse_center, &pixels, "seahorse zoom=1e-7"),
        0,
        "the directed seahorse frame should need no visible fallback at zoom=1e-7"
    );

    let mut deep_seahorse = seahorse_center;
    deep_seahorse[8] = 0.0000000001;
    let pixels = render_params(
        &device,
        &queue,
        &pipelines,
        &compute_params,
        &fragment_params,
        &target,
        &target_view,
        &readback,
        deep_seahorse,
    );
    assert_eq!(
        assert_exact_or_visible_glitch(deep_seahorse, &pixels, "seahorse zoom=1e-10"),
        0,
        "the directed seahorse frame should need no visible fallback at zoom=1e-10"
    );
}
