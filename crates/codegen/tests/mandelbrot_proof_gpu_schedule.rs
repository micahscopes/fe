//! Executed exactness gate for the first balanced Mandelbrot proof workgroup.
//!
//! The Fe schedule type derives four independent LDE tasks. This test compiles
//! only that actor behavior, executes its four physical lanes through WebGPU,
//! and compares every output against a direct inverse DFT plus polynomial
//! evaluation. The oracle deliberately does not replay Fe's radix-2
//! butterflies.

use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::compile_runtime_package_spirv_compute_with_interface;
use hir::hir_def::HirIngot;
use sonatina_codegen::isa::spirv::{
    Access, Role, SpirvBuiltinArgument, SpirvBuiltinSource, SpirvExternalResource,
    SpirvResourceElement, SpirvScalarKind,
};
use url::Url;

const MODULUS: u32 = 2_013_265_921;
const TWO_ADICITY: u32 = 27;
const TRACE_ROWS: usize = 4;
const LDE_ROWS: usize = 16;
const COLUMN_COUNT: usize = 4;
const PROOF_WORDS: usize = 139;
const LDE_START: usize = 16;
const LDE_VALID_START: usize = 97;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repo-root ancestor")
        .to_path_buf()
}

fn invocation_builtins() -> Vec<SpirvBuiltinArgument> {
    use SpirvBuiltinSource as Source;
    [
        Source::GlobalInvocationIdX,
        Source::GlobalInvocationIdY,
        Source::GlobalInvocationIdZ,
        Source::LocalInvocationIdX,
        Source::LocalInvocationIdY,
        Source::LocalInvocationIdZ,
        Source::WorkgroupIdX,
        Source::WorkgroupIdY,
        Source::WorkgroupIdZ,
        Source::NumWorkgroupsX,
        Source::NumWorkgroupsY,
        Source::NumWorkgroupsZ,
        Source::LocalInvocationIndex,
    ]
    .into_iter()
    .enumerate()
    .map(|(arg_index, source)| SpirvBuiltinArgument {
        arg_index: arg_index as u32,
        source,
    })
    .collect()
}

fn compile_lde_workgroup() -> sonatina_codegen::isa::spirv::SpirvArtifact {
    let dir = repo_root().join("demos/sketches/mandelbrot_proof_gpu");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "Mandelbrot proof GPU ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("Mandelbrot proof GPU ingot should resolve");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "Mandelbrot proof GPU source diagnostics:\n{diagnostics}"
    );

    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "extend_columns")
        .expect("balanced LDE behavior should build a runtime package");
    let proof = SpirvExternalResource {
        arg_index: 13,
        group: 0,
        binding: 0,
        name: "proof".to_owned(),
        access: Access::ReadWrite,
        element: SpirvResourceElement::Scalar(SpirvScalarKind::U32),
        stride: 4,
        length: PROOF_WORDS as u32,
    };
    compile_runtime_package_spirv_compute_with_interface(
        &db,
        &package,
        [COLUMN_COUNT as u32, 1, 1],
        [1, 1, 1],
        &[proof],
        &invocation_builtins(),
    )
    .expect("balanced LDE behavior should lower to browser WebGPU")
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
            eprintln!("  balanced LDE SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
            return None;
        }
        Err(error) => panic!(
            "balanced LDE has no WebGPU adapter ({error:?}). Set up Vulkan/lavapipe, or set \
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
                eprintln!("  balanced LDE SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
                return None;
            }
            Err(error) => panic!("balanced LDE browser-profile device request failed: {error:?}"),
        };
    Some((adapter, device, queue))
}

fn pow_mod(mut base: u64, mut exponent: u32) -> u32 {
    let modulus = u64::from(MODULUS);
    base %= modulus;
    let mut result = 1u64;
    while exponent != 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus;
        }
        base = base * base % modulus;
        exponent >>= 1;
    }
    result as u32
}

fn direct_ntt(values: &[u32], inverse: bool) -> Vec<u32> {
    let log_n = values.len().ilog2();
    let maximal_root = pow_mod(31, 15);
    let mut root = pow_mod(u64::from(maximal_root), 1 << (TWO_ADICITY - log_n));
    if inverse {
        root = pow_mod(u64::from(root), MODULUS - 2);
    }
    let modulus = u64::from(MODULUS);
    let mut output = vec![0u32; values.len()];
    for (index, slot) in output.iter_mut().enumerate() {
        let point = pow_mod(u64::from(root), index as u32);
        let mut power = 1u64;
        let mut sum = 0u64;
        for value in values {
            sum = (sum + u64::from(*value) * power) % modulus;
            power = power * u64::from(point) % modulus;
        }
        *slot = sum as u32;
    }
    if inverse {
        let scale = pow_mod(values.len() as u64, MODULUS - 2);
        for value in &mut output {
            *value = (u64::from(*value) * u64::from(scale) % modulus) as u32;
        }
    }
    output
}

fn direct_coset_lde(values: &[u32], output_len: usize, shift: u32) -> Vec<u32> {
    let coefficients = direct_ntt(values, true);
    let maximal_root = pow_mod(31, 15);
    let root = pow_mod(
        u64::from(maximal_root),
        1 << (TWO_ADICITY - output_len.ilog2()),
    );
    let modulus = u64::from(MODULUS);
    (0..output_len)
        .map(|index| {
            let point =
                u64::from(shift) * u64::from(pow_mod(u64::from(root), index as u32)) % modulus;
            coefficients
                .iter()
                .fold((0u64, 1u64), |(sum, power), value| {
                    (
                        (sum + u64::from(*value) * power) % modulus,
                        power * point % modulus,
                    )
                })
                .0 as u32
        })
        .collect()
}

fn trace_columns() -> [[u32; TRACE_ROWS]; COLUMN_COUNT] {
    [
        [0, 1, 2, 3],
        [0, 9_437_184, 28_901_376, 102_576_384],
        [1, 1, 1, 1],
        [0, 0, 0, 1],
    ]
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one u32")))
        .collect()
}

#[test]
fn balanced_lde_schedule_matches_direct_dft_on_webgpu() {
    let artifact = compile_lde_workgroup();
    assert_eq!(artifact.layout.workgroup_size, [4, 1, 1]);
    assert_eq!(artifact.layout.builtin_inputs.len(), 13);
    let wgsl = artifact.wgsl.as_deref().expect("balanced LDE browser WGSL");
    eprintln!("  balanced LDE browser WGSL: {} bytes", wgsl.len());
    let module = naga::front::wgsl::parse_str(wgsl).expect("balanced LDE WGSL should parse");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("balanced LDE WGSL should validate in the browser profile");

    let proof_binding = artifact
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == Role::Resource && binding.name == "proof")
        .expect("Fe proof tape storage binding");
    let trap_binding = artifact
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == Role::Output && binding.name == "trap")
        .expect("one compiler trap word per physical LDE lane");
    let input_binding = artifact
        .layout
        .bindings
        .iter()
        .find(|binding| binding.role == Role::Input)
        .expect("actor scalar-state broadcast binding");
    assert_eq!(proof_binding.resource_length, Some(PROOF_WORDS as u32));
    assert_eq!(trap_binding.span, (COLUMN_COUNT * 4) as u32);
    assert_eq!(input_binding.span, 8, "tamper and resolution state words");

    let Some((adapter, device, queue)) = request_browser_profile_device() else {
        return;
    };
    eprintln!(
        "  balanced LDE WebGPU adapter (no required features): {}",
        adapter.get_info().name
    );

    let proof_bytes = (PROOF_WORDS * 4) as u64;
    let trap_bytes = u64::from(trap_binding.span);
    let proof = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Fe balanced LDE proof tape"),
        size: proof_bytes,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let trap = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Fe balanced LDE trap lanes"),
        size: trap_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let input = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Fe balanced LDE scalar state"),
        size: u64::from(input_binding.span),
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("balanced LDE test-only readback"),
        size: proof_bytes + trap_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let columns = trace_columns();
    let trace_words = columns.into_iter().flatten().collect::<Vec<_>>();
    let trace_bytes = trace_words
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect::<Vec<_>>();
    queue.write_buffer(&proof, 0, &trace_bytes);

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("balanced LDE bindings"),
        entries: &artifact
            .layout
            .bindings
            .iter()
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding: binding.binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage {
                        read_only: binding.access == Access::Read,
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect::<Vec<_>>(),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("balanced LDE pipeline layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Fe balanced LDE WGSL"),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Fe balanced LDE pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Fe balanced LDE resources"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: proof_binding.binding,
                resource: proof.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: trap_binding.binding,
                resource: trap.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: input_binding.binding,
                resource: input.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("balanced LDE execution"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("four Fe-derived LDE tasks"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&proof, 0, &staging, 0, proof_bytes);
    encoder.copy_buffer_to_buffer(&trap, 0, &staging, proof_bytes, trap_bytes);
    queue.submit(Some(encoder.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(180)),
        })
        .expect("balanced LDE WebGPU submission should complete");
    rx.recv()
        .expect("map callback should fire")
        .expect("test-only staging buffer should map");
    let data = slice.get_mapped_range();
    let result = words(&data);

    let expected = trace_columns()
        .iter()
        .flat_map(|column| direct_coset_lde(column, LDE_ROWS, 7))
        .collect::<Vec<_>>();
    assert_eq!(
        &result[LDE_START..LDE_START + COLUMN_COUNT * LDE_ROWS],
        expected.as_slice(),
        "workgroup LDE words must match the independent direct DFT"
    );
    assert_eq!(
        &result[LDE_VALID_START..LDE_VALID_START + COLUMN_COUNT],
        &[1; COLUMN_COUNT],
        "each Fe-derived schedule leaf must report a valid radix-2 plan"
    );
    assert_eq!(
        &result[PROOF_WORDS..],
        &[0; COLUMN_COUNT],
        "one clean trap lane per physical LDE task"
    );
    drop(data);
    staging.unmap();
}
