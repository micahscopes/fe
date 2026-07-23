use common::InputDb;
use driver::DriverDataBase;
use url::Url;

const SOURCE: &str = include_str!("fixtures/spirv/cga_schedule_ctfe_specialized_render.fe");

#[test]
fn ctfe_specialized_cga_schedule_compiles_to_render_spirv() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///cga_schedule_ctfe_specialized_render.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let package = mir::build_wasm_runtime_package(&db, db.top_mod(file))
        .expect("typed CGA schedule should build a runtime package");
    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("typed CGA schedule should compile to render SPIR-V");
    assert_eq!(artifact.words.first().copied(), Some(0x0723_0203));
    assert!(artifact.wgsl.is_some());
    assert_eq!(artifact.layout.builtin_inputs.len(), 2);
    let input = artifact.layout.bindings.iter()
        .find(|binding| binding.role == sonatina_codegen::isa::spirv::Role::Input)
        .expect("nine broadcast f32 coefficients need an input binding");
    assert_eq!(input.members.len(), 9);
    assert!(input.members.iter().all(|member|
        member.scalar == sonatina_codegen::isa::spirv::SpirvScalarKind::F32));

    let wgsl = artifact.wgsl.as_deref().unwrap();
    assert_eq!(
        wgsl.matches("fn ").count(),
        2,
        "call-free render WGSL should contain only vertex and fragment entries:\n{wgsl}",
    );
    for forbidden in ["i64", "u64", "i256"] {
        assert!(!wgsl.contains(forbidden), "browser profile forbids `{forbidden}`:\n{wgsl}");
    }
    let reparsed = naga::front::wgsl::parse_str(wgsl)
        .expect("typed Schedule<32> WGSL must reparse through Naga");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&reparsed)
    .expect("typed Schedule<32> WGSL must validate with browser-default capabilities");
    for (sphere, point) in [
        ([0.5, -0.25, -0.875, 0.125], [2.0, 0.5, -0.75, 1.25, 1.75]),
        ([1.0, 0.5, -1.0, 0.25], [-0.5, 2.0, 0.25, -1.5, 1.0]),
    ] {
        let params = [
            sphere[0], sphere[1], sphere[2], sphere[3],
            point[0], point[1], point[2], point[3], point[4],
        ];
        let mut input_bytes = vec![0u8; input.span as usize];
        for member in &input.members {
            let value: f32 = params[(member.arg_index - 2) as usize];
            input_bytes[member.offset as usize..member.offset as usize + 4]
                .copy_from_slice(&value.to_bits().to_le_bytes());
        }
        let actual = run_render_rgba8_on_lavapipe(wgsl, 8, 4, &input_bytes)
            .expect("typed CGA schedule requires lavapipe execution");
        let expected = raw_80_oracle(sphere, point);
        for (blade, coefficient) in expected.into_iter().enumerate() {
            assert_eq!(
                &actual[blade * 4..blade * 4 + 4],
                &((coefficient * 256.0) as i32 as u32).to_le_bytes(),
                "CTFE-specialized CGA blade {blade}",
            );
        }
    }
}

fn run_render_rgba8_on_lavapipe(wgsl: &str, w: u32, h: u32, input: &[u8]) -> Option<Vec<u8>> {
    let allow_skip = std::env::var_os("MB2_ALLOW_GPU_SKIP").is_some();

    let instance = wgpu::Instance::default();
    let adapter = match pollster::block_on(instance.request_adapter(
        &wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            ..Default::default()
        },
    )) {
        Ok(a) => a,
        Err(e) => {
            if allow_skip {
                eprintln!(
                    "  render SPIR-V leg SKIPPED (MB2_ALLOW_GPU_SKIP): no Vulkan adapter: {e:?}"
                );
                return None;
            }
            panic!(
                "render SPIR-V leg: no GPU/Vulkan adapter available ({e:?}). The render rung \
                 requires lavapipe to EXECUTE; a missing device is a hard failure, not a skip. \
                 Set VK_ICD_FILENAMES / LD_LIBRARY_PATH / WGPU_BACKEND=vulkan for lavapipe, or \
                 MB2_ALLOW_GPU_SKIP to downgrade the rung on a genuinely GPU-less host."
            );
        }
    };

    // BROWSER PROFILE: NO required features (drop SHADER_INT64), exactly what a
    // WebGPU browser offers. A real failure here means the fragment is NOT
    // browser-viable, a STOP condition, not a skip.
    let (device, queue) = match pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            ..Default::default()
        },
    )) {
        Ok(dq) => dq,
        Err(e) => {
            if allow_skip {
                eprintln!(
                    "  render SPIR-V leg SKIPPED (MB2_ALLOW_GPU_SKIP): device request failed: {e:?}"
                );
                return None;
            }
            panic!(
                "render SPIR-V leg: browser-profile device request (NO required features) failed \
                 ({e:?}). This is a hard failure, not a skip."
            );
        }
    };

    eprintln!(
        "  render SPIR-V leg GPU adapter (BROWSER PROFILE, no required features): {}",
        adapter.get_info().name
    );

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("ctfe_cga_schedule_render"),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });

    // The broadcast input storage buffer at @group(0) @binding(1), FRAGMENT
    // visibility. v1 fragments take no broadcast params (`input.is_empty()`), so
    // the 4-byte dummy floor keeps the unused binding valid; a param-carrying
    // fragment writes its words before the draw.
    let input_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("render_input"),
        size: input.len().max(4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    if !input.is_empty() {
        queue.write_buffer(&input_buf, 0, input);
    }
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("render_bgl"),
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
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("render_pl"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("render_bg"),
        layout: &bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 1,
            resource: input_buf.as_entire_binding(),
        }],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("fullscreen"),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_fullscreen"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
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

    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("render_target"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());

    // 256-aligned bytes_per_row (COPY_BYTES_PER_ROW_ALIGNMENT). For 512x4 = 2048
    // this is already aligned; assert to document the invariant.
    let bytes_per_row = ((w * 4 + 255) / 256) * 256;
    assert_eq!(
        bytes_per_row % 256,
        0,
        "copy_texture_to_buffer bytes_per_row must be 256-aligned"
    );
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("render_staging"),
        size: u64::from(bytes_per_row * h),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("render_pass"),
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
        pass.set_bind_group(0, &bg, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).expect("map_async callback channel should be open");
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(30)),
    });
    rx.recv()
        .expect("map_async callback should fire")
        .expect("staging buffer should map for read");
    let data = slice.get_mapped_range();
    let row = (w * 4) as usize;
    let mut out = Vec::with_capacity(row * h as usize);
    for y in 0..h {
        let off = (y * bytes_per_row) as usize;
        out.extend_from_slice(&data[off..off + row]);
    }
    drop(data);
    staging.unmap();

    Some(out)
}

fn gp_sign_cl41(a: usize, b: usize) -> f32 {
    let mut negative = false;
    for bit in 0..5 {
        if a & (1 << bit) != 0 {
            if (b & ((1 << bit) - 1)).count_ones() & 1 != 0 { negative = !negative; }
            if bit == 4 && b & (1 << bit) != 0 { negative = !negative; }
        }
    }
    if negative { -1.0 } else { 1.0 }
}

fn raw_80_oracle(sphere: [f32; 4], point: [f32; 5]) -> [f32; 32] {
    let sb = [1usize, 2, 8, 16];
    let pb = [1usize, 2, 4, 8, 16];
    let mut out = [0.0; 32];
    for (li, &l) in sb.iter().enumerate() {
        for (pi, &p) in pb.iter().enumerate() {
            for (ri, &r) in sb.iter().enumerate() {
                out[l ^ p ^ r] += gp_sign_cl41(l, p) * gp_sign_cl41(l ^ p, r)
                    * sphere[li] * point[pi] * sphere[ri];
            }
        }
    }
    out
}
