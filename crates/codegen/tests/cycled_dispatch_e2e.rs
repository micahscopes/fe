//! Executed gate for Fe-derived actor pass cycles.
//!
//! Two authored compute phases form one nominal three-round body. The second
//! phase tapers its inner repeats from three to two to one, so correct
//! interleaving leaves the receipt at `111223`. Executing each pass's outer
//! repeat independently cannot produce that value.

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

fn compile_cycled_graph() -> WebBundle {
    let dir = repo_root().join("crates/codegen/tests/fixtures/actor_cycled_dispatch");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "cycled dispatch ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("cycled dispatch fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "cycled dispatch source diagnostics:\n{diagnostics}"
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("actor_cycled_dispatch".into())),
    )
    .expect("cycled dispatch fixture should compile into a WebBundle")
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
            eprintln!("  actor pass cycle SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
            return None;
        }
        Err(error) => panic!(
            "actor pass cycle has no WebGPU adapter ({error:?}). Set up Vulkan/lavapipe, or set \
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
                eprintln!("  actor pass cycle SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
                return None;
            }
            Err(error) => panic!("actor pass cycle device request failed: {error:?}"),
        };
    Some((adapter, device, queue))
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one u32")))
        .collect()
}

#[test]
fn nominal_cycle_executes_the_complete_actor_body_in_storage_order() {
    let bundle = compile_cycled_graph();
    let compute = &bundle.manifest.passes[..2];
    assert_eq!(compute[0].source_entry, "begin_round");
    assert_eq!(compute[1].source_entry, "record_round");
    assert_eq!(compute[0].repeat, 1);
    assert_eq!(compute[1].repeat, 3);
    assert_eq!(
        compute[1].cooperation,
        Some(fe_codegen::WebDispatchCooperation { repeat_batch: 2 })
    );
    assert_eq!(
        compute[1].taper,
        Some(fe_codegen::WebDispatchTaper {
            shifts: [0, 0, 0],
            repeat_decrement: 1,
        })
    );
    let cycle = compute[0].cycle.as_ref().expect("first cycle member");
    assert_eq!(cycle.repeat, 3);
    assert_eq!(compute[1].cycle.as_ref(), Some(cycle));
    assert_eq!(bundle.manifest.passes[2].cycle, None);

    let receipt_binding = compute[0]
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == WebBindingRole::Resource)
        .expect("receipt resource binding");
    assert_eq!(receipt_binding.name, "receipt");
    assert_eq!(receipt_binding.span, 4);
    let recording_binding = compute[1]
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == WebBindingRole::Resource)
        .expect("recording receipt resource binding");
    assert_eq!(recording_binding.binding, receipt_binding.binding);
    assert_eq!(recording_binding.span, 4);
    let receipt_resource = bundle
        .manifest
        .resources
        .iter()
        .find(|resource| resource.name == "receipt")
        .expect("receipt resource manifest");
    assert_eq!(receipt_resource.length, 2);
    assert_eq!(receipt_resource.stride, 4);

    let Some((adapter, device, queue)) = request_browser_profile_device() else {
        return;
    };
    eprintln!(
        "  actor pass cycle WebGPU adapter (no required features): {}",
        adapter.get_info().name
    );

    let receipt = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Fe actor pass cycle receipt"),
        size: 8,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("actor pass cycle test-only readback"),
        size: 8,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let layout_entries = compute[0]
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
        label: Some("actor pass cycle bindings"),
        entries: &layout_entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("actor pass cycle pipeline layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let pipelines = bundle.pass_wgsl[..2]
        .iter()
        .map(|pass| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Fe actor pass cycle WGSL"),
                source: wgpu::ShaderSource::Wgsl(pass.source.as_str().into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Fe actor pass cycle pipeline"),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            })
        })
        .collect::<Vec<_>>();
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Fe actor pass cycle resources"),
        layout: &bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: receipt_binding.binding,
            resource: receipt.as_entire_binding(),
        }],
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("actor pass cycle execution"),
    });
    for cycle_iteration in 0..cycle.repeat {
        for (stage, pipeline) in compute.iter().zip(&pipelines) {
            let dispatch = stage.dispatch.expect("fixed compute dispatch");
            let repeat = stage.repeat
                - cycle_iteration * stage.taper.map_or(0, |taper| taper.repeat_decrement);
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("compiler-derived actor cycle member"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            for _ in 0..repeat {
                pass.dispatch_workgroups(dispatch[0], dispatch[1], dispatch[2]);
            }
        }
    }
    encoder.copy_buffer_to_buffer(&receipt, 0, &staging, 0, 8);
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
        .expect("actor pass cycle submission should complete");
    rx.recv()
        .expect("map callback should fire")
        .expect("test-only staging buffer should map");
    let data = slice.get_mapped_range();
    assert_eq!(words(&data), vec![3, 111223]);
    drop(data);
    staging.unmap();
}
