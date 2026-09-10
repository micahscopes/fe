//! Fe generics must preserve actor resource identity through shader helpers.
use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WebBuildOptions, WebBundle};
use hir::hir_def::HirIngot;
use url::Url;

#[test]
fn generic_helpers_specialize_distinct_resources_and_share_repeated_calls() {
    let bundle = compile(None).expect("generic resource helper should compile");
    let shader = &bundle.pass_wgsl[0].source;
    assert_eq!(shader.matches("loop {").count(), 2,
        "one shared loop per distinct binding, not one per call:\n{shader}");
    assert!(shader.contains("left[") && shader.contains("right["),
        "both resource identities must survive:\n{shader}");
    assert_eq!(shader.matches("fn total").count(), 2,
        "two physical resource variants of the generic helper:\n{shader}");
}

fn compile(source: Option<String>) -> Result<WebBundle, String> {
    let mut db = DriverDataBase::default();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/actor_generic_resources");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    assert!(!driver::init_ingot(&mut db, &url));
    let top = if let Some(source) = source {
        let source_url = Url::parse("file:///generic_resource_selection.fe").unwrap();
        db.workspace().touch(&mut db, source_url.clone(), Some(source));
        let file = db.workspace().get(&db, &source_url).unwrap();
        db.top_mod(file)
    } else {
        db.workspace().containing_ingot(&db, url).unwrap().root_mod(&db)
    };
    let diagnostics = db.run_on_top_mod(top).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    WebBundle::compile(&db, top, WebBuildOptions::render("paint", None))
        .map_err(|error| error.to_string())
}

#[test]
fn generic_helper_runtime_selection_preserves_identity_or_fails_closed() {
    let source = include_str!("fixtures/actor_generic_resources/src/lib.fe").replace(
        "let a = total(words: self.left, count: 3)",
        "let chosen = if lane.global.x == 0 { self.left } else { self.right }\n        let a = total(words: chosen, count: 3)",
    );
    match compile(Some(source)) {
        Err(error) => {
            eprintln!("runtime resource choice rejected: {error}");
            assert!(error.contains("resource") || error.contains("custody"), "{error}");
        }
        Ok(bundle) => {
            let mut expected = [87; 64];
            expected[0] = 72;
            execute(&bundle, &expected);
        }
    }
}

#[test]
fn generic_resource_variants_execute_with_distinct_contents() {
    let bundle = compile(None).unwrap();
    execute(&bundle, &[72; 64]);
}

fn execute(bundle: &WebBundle, expected: &[u32; 64]) {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("resource identity execution requires a GPU adapter");
    eprintln!("resource boundary adapter: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("generic resource identity"),
        source: wgpu::ShaderSource::Wgsl(bundle.pass_wgsl[0].source.as_str().into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None, layout: None, module: &shader, entry_point: Some("main"),
        compilation_options: Default::default(), cache: None,
    });
    // This fixture owns three 64-word buffers plus the compiler trap buffer.
    let buffers = (0..4).map(|_| device.create_buffer(&wgpu::BufferDescriptor {
        label: None, size: 256,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })).collect::<Vec<_>>();
    for (index, value) in [2u32, 7, 0, 0].into_iter().enumerate() {
        let bytes = (0..64).flat_map(|_| value.to_le_bytes()).collect::<Vec<_>>();
        queue.write_buffer(&buffers[index], 0, &bytes);
    }
    let entries = buffers.iter().enumerate().map(|(binding, buffer)| wgpu::BindGroupEntry {
        binding: binding as u32, resource: buffer.as_entire_binding(),
    }).collect::<Vec<_>>();
    let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None, layout: &pipeline.get_bind_group_layout(0), entries: &entries,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None, size: 512, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&buffers[2], 0, &readback, 0, 256);
    encoder.copy_buffer_to_buffer(&buffers[3], 0, &readback, 256, 256);
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| sender.send(result).unwrap());
    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
    receiver.recv().unwrap().unwrap();
    let data = slice.get_mapped_range();
    let words = data.chunks_exact(4).map(|word| u32::from_le_bytes(word.try_into().unwrap())).collect::<Vec<_>>();
    assert_eq!(&words[..64], expected, "resource identities must select the right data; shader:\n{}", bundle.pass_wgsl[0].source);
    assert_eq!(&words[64..], &[0; 64], "no shader traps");
}
