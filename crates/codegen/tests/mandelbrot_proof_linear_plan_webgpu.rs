//! Focused lowering gate for the largest sparse base-plan pass.

use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WebBuildOptions, WebBundleMode, resolve_web_entry};
use hir::hir_def::HirIngot;
use url::Url;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repository root")
        .to_path_buf()
}

#[test]
fn production_sparse_linear_plan_lowers_in_isolation() {
    let dir =
        repo_root().join("crates/codegen/tests/fixtures/mandelbrot_proof_linear_plan_webgpu_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "linear-plan fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("linear-plan fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "linear-plan source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    let lowering_started = std::time::Instant::now();
    let bundle = fe_codegen::WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("mandelbrot_sparse_linear_plan".into())),
    )
    .expect("the isolated linear-plan pass should lower");
    let lowering_elapsed = lowering_started.elapsed();

    assert_eq!(bundle.manifest.passes.len(), 2);
    let pass = &bundle.manifest.passes[0];
    assert_eq!(pass.source_entry, "write_linear_plan");
    assert_eq!(pass.layout.workgroup_size, [64, 1, 1]);
    assert_eq!(pass.dispatch, Some([64, 1, 1]));
    let module = naga::front::wgsl::parse_str(&bundle.pass_wgsl[0].source)
        .expect("linear-plan WGSL should parse");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("linear-plan WGSL should validate");
    let expression_count: usize = module.functions.iter()
        .map(|(_, function)| function.expressions.len())
        .chain(module.entry_points.iter().map(|entry| entry.function.expressions.len()))
        .sum();
    eprintln!(
        "linear-plan artifact: wgsl_bytes={}, naga_expressions={}, naga_helpers={}, bundle_compile_ms={}",
        bundle.pass_wgsl[0].source.len(), expression_count, module.functions.len(),
        lowering_elapsed.as_millis(),
    );
    // Export the exact compiled manifest and artifacts for external browser
    // diagnosis. Publication rejects an existing destination; the browser
    // must not accidentally test a stale or mixed bundle. This gate itself
    // validates artifacts only, not dispatch inputs or proof correctness.
    if let Some(destination) = std::env::var_os("FE_TEST_BUNDLE_DIR") {
        bundle.write_atomic(destination).expect("publish isolated linear-plan bundle");
    }
}

/// Execute saved artifacts, never silently recompile a different shader.
/// Inputs come from export_production_linear_plan_browser_inputs in the
/// independent fixed oracle. This is one partition, not a proof receipt.
/// Run alone with --ignored --exact; a completion timeout exits the process
/// rather than unwinding through an outstanding driver submission.
#[test]
#[ignore = "requires explicit saved shader and independent input directory"]
fn saved_linear_plan_matches_independent_inputs() {
    use sha2::{Digest, Sha256};
    let shader = std::fs::read_to_string(std::env::var_os("MB2_LINEAR_PLAN_WGSL").unwrap()).unwrap();
    let directory = PathBuf::from(std::env::var_os("MB2_LINEAR_PLAN_INPUT_DIR").unwrap());
    let input = std::fs::read(directory.join("input.u32le")).unwrap();
    let expected = std::fs::read(directory.join("expected.u32le")).unwrap();
    let metadata: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.join("metadata.json")).unwrap(),
    ).unwrap();
    assert_eq!(metadata["schema"], "mb2-linear-plan-execution-input/1");
    assert_eq!(metadata["executed_rows"], 64);
    assert_eq!(metadata["trace_rows"], 4096);
    assert_eq!(metadata["fields"], 260);
    assert_eq!(input.len(), 4096 * 260 * 4);
    assert_eq!(input.len(), expected.len());
    assert_eq!(input.chunks_exact(4).zip(expected.chunks_exact(4))
        .filter(|(a, b)| a != b).count(), 3328);
    eprintln!("saved shader sha256={:x}; input={:x}; expected={:x}",
        Sha256::digest(shader.as_bytes()), Sha256::digest(&input), Sha256::digest(&expected));
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("execution gate requires an adapter; no skip success");
    eprintln!("adapter: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let started = std::time::Instant::now();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("saved linear-plan"), source: wgpu::ShaderSource::Wgsl(shader.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("saved linear-plan"), layout: None, module: &module,
        entry_point: Some("main"), compilation_options: Default::default(), cache: None,
    });
    eprintln!("module and pipeline ready: {:?}", started.elapsed());
    let make_buffer = |size, usage| device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("linear-plan oracle"), size, usage, mapped_at_creation: false,
    });
    let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
    let base = make_buffer(input.len() as u64, usage);
    let validity = make_buffer(4096 * 4, usage);
    let trap = make_buffer(4096 * 4, usage);
    let staging = make_buffer(input.len() as u64 + 4096 * 8,
        wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
    queue.write_buffer(&base, 0, &input);
    queue.write_buffer(&validity, 0, &1u32.to_le_bytes().repeat(4096));
    let layout = pipeline.get_bind_group_layout(0);
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None, layout: &layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: base.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: validity.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: trap.as_entire_binding() },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&base, 0, &staging, 0, input.len() as u64);
    encoder.copy_buffer_to_buffer(&validity, 0, &staging, input.len() as u64, 4096 * 4);
    encoder.copy_buffer_to_buffer(&trap, 0, &staging, input.len() as u64 + 4096 * 4, 4096 * 4);
    queue.submit([encoder.finish()]);
    eprintln!("submitted: {:?}", started.elapsed());
    let (tx, rx) = std::sync::mpsc::channel();
    staging.slice(..).map_async(wgpu::MapMode::Read, move |result| { let _ = tx.send(result); });
    if let Err(error) = device.poll(wgpu::PollType::Wait {
        submission_index: None, timeout: Some(std::time::Duration::from_secs(30)),
    }) {
        // This opt-in probe must be run alone. Unwinding an outstanding Vulkan
        // submission can panic again in wgpu queue destruction. Fail the test
        // process without pretending a timeout canceled driver work.
        eprintln!("saved-kernel completion failed after {:?}: {error:?}", started.elapsed());
        std::process::exit(1);
    }
    rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap().unwrap();
    let actual = staging.slice(..).get_mapped_range();
    let mismatch = actual[..expected.len()].chunks_exact(4).zip(expected.chunks_exact(4))
        .enumerate().find(|(_, (a, b))| a != b);
    assert!(mismatch.is_none(), "first mismatched word: {mismatch:?}");
    assert!(actual[expected.len()..expected.len() + 4096 * 4].chunks_exact(4)
        .all(|bytes| bytes == 1u32.to_le_bytes()));
    assert!(actual[expected.len() + 4096 * 4..].iter().all(|byte| *byte == 0));
    eprintln!("matched {} words, 64 executed rows, validity intact, traps clear", expected.len() / 4);
    drop(actual);
    staging.unmap();
}
