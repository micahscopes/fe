//! Executed oracle for Fe's nominal compute invocation context.
//!
//! The Fe actor owns the workgroup and fixed dispatch types. Its generated WGSL
//! writes one independently checkable receipt per invocation. This test submits
//! that shader through WebGPU and also reads every compiler-owned trap lane.

use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WebBindingAccess, WebBindingRole, WebBuildOptions, WebBundle, WebBundleMode, resolve_web_entry,
};
use hir::hir_def::HirIngot;
use url::Url;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repo-root ancestor")
        .to_path_buf()
}

fn compile_invocation_graph() -> WebBundle {
    let dir = repo_root().join("crates/codegen/tests/fixtures/actor_compute_invocation");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "compute invocation ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("compute invocation fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "compute invocation source diagnostics:\n{diagnostics}"
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("actor_compute_invocation".into())),
    )
    .expect("compute invocation fixture should compile into a WebBundle")
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
                "  compute invocation SKIPPED (MB2_ALLOW_GPU_SKIP): no WebGPU adapter: {error:?}"
            );
            return None;
        }
        Err(error) => panic!(
            "compute invocation has no WebGPU adapter ({error:?}). Set up Vulkan/lavapipe, or set \
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
                "  compute invocation SKIPPED (MB2_ALLOW_GPU_SKIP): device request failed: {error:?}"
            );
            return None;
        }
        Err(error) => panic!(
            "compute invocation browser-profile device request with no required features failed: \
             {error:?}"
        ),
    };
    Some((adapter, device, queue))
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one u32")))
        .collect()
}

fn expected_receipts() -> [u32; 16] {
    let mut expected = [0; 16];
    for gy in 0..4 {
        for gx in 0..4 {
            let lx = gx % 2;
            let ly = gy % 2;
            let wx = gx / 2;
            let wy = gy / 2;
            let local_index = ly * 2 + lx;
            expected[(gy * 4 + gx) as usize] = gx
                + gy * 10
                + lx * 1000
                + ly * 2000
                + wx * 10000
                + wy * 20000
                + 2 * 100000
                + 2 * 200000
                + 400000
                + local_index * 1000000;
        }
    }
    expected
}

fn expected_checkpoints() -> Vec<u32> {
    let mut expected = Vec::with_capacity(16 * 12);
    for slot in 0..16u32 {
        let seed = slot + 1;
        let challenge = [seed + 11, seed + 17, seed + 23, seed + 29];
        let mut power = [1, 0, 0, 0];
        let mut value = [0, 0, 0, 0];
        for round in 0..9u32 {
            let delta = round + 1;
            let old_power = power;
            for lane in 0..4 {
                power[lane] += challenge[lane];
                value[lane] += old_power[lane] + delta * (lane as u32 + 1);
            }
        }
        expected.extend(challenge);
        expected.extend(power);
        expected.extend(value);
    }
    expected
}

#[test]
fn typed_compute_invocation_executes_every_fixed_dispatch_lane_on_webgpu() {
    let bundle = compile_invocation_graph();
    let compute = &bundle.manifest.passes[0];
    assert_eq!(compute.source_entry, "stamp");
    assert_eq!(compute.dispatch, Some([2, 2, 1]));
    assert_eq!(compute.layout.workgroup_size, [2, 2, 1]);

    let receipt_binding = compute
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == WebBindingRole::Resource && binding.name == "receipts")
        .expect("receipt resource binding");
    let checkpoint_binding = compute
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == WebBindingRole::Resource && binding.name == "checkpoints")
        .expect("checkpoint resource binding");
    let trap_binding = compute
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == WebBindingRole::Output && binding.name == "trap")
        .expect("per-invocation trap binding");
    let receipt_resource = bundle
        .manifest
        .resources
        .iter()
        .find(|resource| resource.name == "receipts")
        .expect("receipt resource declaration");
    let checkpoint_resource = bundle
        .manifest
        .resources
        .iter()
        .find(|resource| resource.name == "checkpoints")
        .expect("checkpoint resource declaration");
    let receipt_bytes = receipt_resource
        .length
        .checked_mul(receipt_resource.stride)
        .expect("receipt resource byte span");
    let checkpoint_bytes = checkpoint_resource
        .length
        .checked_mul(checkpoint_resource.stride)
        .expect("checkpoint resource byte span");
    assert_eq!(receipt_binding.name, "receipts");
    assert_eq!(receipt_binding.span, receipt_resource.stride);
    assert_eq!(receipt_bytes, 16 * 4);
    assert_eq!(checkpoint_binding.span, checkpoint_resource.stride);
    assert_eq!(checkpoint_bytes, 16 * 12 * 4);
    assert_eq!(trap_binding.span, 16 * 4);

    let Some((adapter, device, queue)) = request_browser_profile_device() else {
        return;
    };
    eprintln!(
        "  compute invocation WebGPU adapter (no required features): {}",
        adapter.get_info().name
    );

    let receipts = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Fe compute invocation receipts"),
        size: u64::from(receipt_bytes),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let checkpoints = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Fe wide helper-return checkpoints"),
        size: u64::from(checkpoint_bytes),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let trap = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Fe compute invocation trap lanes"),
        size: u64::from(trap_binding.span),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("compute invocation test-only readback"),
        size: u64::from(receipt_bytes + checkpoint_bytes + trap_binding.span),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let layout_entries = compute
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
        label: Some("compute invocation bindings"),
        entries: &layout_entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("compute invocation pipeline layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Fe compute invocation WGSL"),
        source: wgpu::ShaderSource::Wgsl(bundle.pass_wgsl[0].source.as_str().into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Fe compute invocation pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Fe compute invocation resources"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: receipt_binding.binding,
                resource: receipts.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: checkpoint_binding.binding,
                resource: checkpoints.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: trap_binding.binding,
                resource: trap.as_entire_binding(),
            },
        ],
    });

    let dispatch = compute.dispatch.expect("fixed compute dispatch");
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Fe compute invocation execution"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("stamp all invocation receipts"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(dispatch[0], dispatch[1], dispatch[2]);
    }
    encoder.copy_buffer_to_buffer(&receipts, 0, &staging, 0, u64::from(receipt_bytes));
    encoder.copy_buffer_to_buffer(
        &checkpoints,
        0,
        &staging,
        u64::from(receipt_bytes),
        u64::from(checkpoint_bytes),
    );
    encoder.copy_buffer_to_buffer(
        &trap,
        0,
        &staging,
        u64::from(receipt_bytes + checkpoint_bytes),
        u64::from(trap_binding.span),
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
        .expect("compute invocation WebGPU submission should complete");
    rx.recv()
        .expect("map callback should fire")
        .expect("test-only staging buffer should map");
    let data = slice.get_mapped_range();
    let result = words(&data);
    assert_eq!(&result[..16], &expected_receipts());
    assert_eq!(&result[16..16 + 16 * 12], expected_checkpoints());
    assert_eq!(
        &result[16 + 16 * 12..],
        &[0; 16],
        "one clean trap lane per invocation"
    );
    drop(data);
    staging.unmap();
}
