//! Independent execution gate for reflection-derived typed proof regions.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use sonatina_codegen::isa::spirv::{
    Access, Role, SpirvExternalResource, SpirvResourceElement, SpirvScalarKind,
};
use std::path::{Path, PathBuf};
use url::Url;
use wasmtime::Val;

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/region_layout_oracle_ingot")
        .canonicalize()
        .unwrap()
}

fn rejected_fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/region_layout_forge_rejected_ingot")
        .canonicalize()
        .unwrap()
}

fn webgpu_fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/region_layout_webgpu_oracle_ingot")
        .canonicalize()
        .unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = Url::from_directory_path(fixture_path()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "typed region fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("typed region fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected typed region diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("typed region fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("typed region Wasm should validate");
    bytes
}

fn call(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    arguments: &[Val],
    result_count: usize,
) -> Vec<u32> {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("missing `{name}` export"));
    let mut results = vec![Val::I32(0); result_count];
    function
        .call(&mut *store, arguments, &mut results)
        .unwrap_or_else(|error| panic!("`{name}` should execute: {error:?}"));
    results
        .into_iter()
        .map(|value| match value {
            Val::I32(word) => word as u32,
            other => panic!("`{name}` returned non-u32 lane {other:?}"),
        })
        .collect()
}

#[test]
fn reflected_regions_match_an_independent_declaration_order_decoder() {
    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, compile_wasm()).expect("typed region module should load");
    assert_eq!(module.imports().len(), 0, "fixture must remain zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("typed region module should instantiate");

    // Header is two words. Each query is four words, and the digest is eight.
    assert_eq!(
        call(&mut store, &instance, "layout_receipt", &[], 7),
        vec![0, 2, 2, 8, 10, 8, 18],
    );
    for (index, expected) in [
        (0, vec![1, 2]),
        (1, vec![1, 3]),
        (7, vec![1, 9]),
        (8, vec![0, 0]),
        (u32::MAX, vec![0, 0]),
    ] {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "query_address",
                &[Val::I32(index as i32)],
                2,
            ),
            expected,
            "relative query coordinate {index}",
        );
    }
    for (index, expected) in [
        (0, vec![1, 6]),
        (3, vec![1, 9]),
        (4, vec![0, 0]),
        (u32::MAX, vec![0, 0]),
    ] {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "second_query_address",
                &[Val::I32(index as i32)],
                2,
            ),
            expected,
            "relative coordinate in a nested query region {index}",
        );
    }
    assert_eq!(
        call(
            &mut store,
            &instance,
            "oversized_query_region_valid",
            &[],
            1,
        ),
        vec![0],
        "an oversized child layout must fail closed",
    );
}

#[test]
fn raw_offsets_and_cross_region_type_confusion_are_rejected() {
    let url = Url::from_directory_path(rejected_fixture_path()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "negative typed region fixture should initialize before semantic checking",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("negative typed region fixture ingot");
    let diagnostics = db.run_on_top_mod(ingot.root_mod(&db)).format_diags(&db);
    assert!(
        diagnostics.contains("not visible") && diagnostics.contains("offset"),
        "raw offset construction must fail through field privacy:\n{diagnostics}",
    );
    assert!(
        diagnostics.contains("from_derived_offset") && diagnostics.contains("not visible"),
        "the provider's constructor must remain invisible to ordinary Fe:\n{diagnostics}",
    );
    assert!(
        diagnostics.contains("Region<Header>") && diagnostics.contains("Region<Query>"),
        "different semantic regions must not be interchangeable:\n{diagnostics}",
    );
}

#[test]
fn typed_region_writes_execute_on_browser_profile_webgpu() {
    let url = Url::from_directory_path(webgpu_fixture_path()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "typed region WebGPU fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("typed region WebGPU fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected typed region WebGPU diagnostics:\n{diagnostics}",
    );
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "write_regions")
        .expect("typed region writer should build a runtime package");
    let tape = SpirvExternalResource {
        arg_index: 0,
        group: 0,
        binding: 0,
        name: "tape".to_owned(),
        access: Access::ReadWrite,
        element: SpirvResourceElement::Scalar(SpirvScalarKind::U32),
        stride: 4,
        length: 18,
    };
    let artifact = fe_codegen::compile_runtime_package_spirv_compute_with_interface(
        &db,
        &package,
        [1, 1, 1],
        [1, 1, 1],
        &[tape],
        &[],
    )
    .expect("typed region writer should lower to browser WebGPU");
    let wgsl = artifact.wgsl.as_deref().expect("typed region browser WGSL");
    let module = naga::front::wgsl::parse_str(wgsl).expect("typed region WGSL should parse");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("typed region WGSL should validate in the browser profile");
    assert_eq!(artifact.layout.workgroup_size, [1, 1, 1]);
    let binding = artifact
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == Role::Resource && binding.name == "tape")
        .expect("typed tape storage binding");

    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        ..Default::default()
    }))
    .expect("typed region gate requires Vulkan/lavapipe or real WebGPU hardware");
    eprintln!("  typed region WebGPU adapter: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::empty(),
        ..Default::default()
    }))
    .expect("typed region browser-profile device request");
    let tape_bytes = 18 * 4;
    let tape_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("typed region tape"),
        size: tape_bytes,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("typed region readback"),
        size: tape_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("typed region layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: binding.binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("typed region pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("typed region WGSL"),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("typed region pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("typed region group"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: binding.binding,
            resource: tape_buffer.as_entire_binding(),
        }],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("typed region execution"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("typed region writer"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&tape_buffer, 0, &readback, 0, tape_bytes);
    queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(180)),
        })
        .expect("typed region submission should complete");
    rx.recv()
        .expect("typed region map callback")
        .expect("typed region readback should map");
    let bytes = slice.get_mapped_range();
    let words = bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one u32")))
        .collect::<Vec<_>>();
    assert_eq!(words[0], 101);
    assert_eq!(words[1], 102);
    assert_eq!(words[2], 201);
    assert_eq!(words[9], 208);
    assert_eq!(words[10], 301);
    assert_eq!(words[11], 1, "all typed stores and rejection must succeed");
    assert_eq!(words[17], 308, "rejected query write must not reach digest");
    assert!(
        words[3..9].iter().all(|word| *word == 0) && words[12..17].iter().all(|word| *word == 0),
    );
    drop(bytes);
    readback.unmap();
}
