//! Executed exactness gate for the Fe-authored Mandelbrot proof pass graph.
//!
//! The test compiles the real actor, executes every compute pass in manifest
//! order, honors each Fe-derived repeat count, and reads back only test
//! evidence. The expected LDE uses a direct DFT and the expected roots use the
//! independent Plonky3 Poseidon2 implementation.

use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    resolve_web_entry, WebBinding, WebBindingAccess, WebBindingRole, WebBuildOptions, WebBundle,
    WebBundleMode, WebScalarKind,
};
use hir::hir_def::HirIngot;
use p3_baby_bear::{
    default_babybear_poseidon2_16, BabyBear, BABYBEAR_POSEIDON2_RC_16_EXTERNAL_FINAL,
    BABYBEAR_POSEIDON2_RC_16_EXTERNAL_INITIAL, BABYBEAR_POSEIDON2_RC_16_INTERNAL,
};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_symmetric::Permutation;
use url::Url;

const MODULUS: u32 = 2_013_265_921;
const TWO_ADICITY: u32 = 27;
const TRACE_ROWS: usize = 4;
const LDE_ROWS: usize = 16;
const COLUMN_COUNT: usize = 4;
const LDE_START: usize = 16;
const CLEAN_ROOT: usize = 80;
const OBSERVED_ROOT: usize = 88;
const TRACE_VALID: usize = 96;
const LDE_VALID_START: usize = 97;
const ROOTS_EQUAL: usize = 101;
const MODE_CORRECT: usize = 102;
const CLEAN_COMMIT_STATE: usize = 107;
const OBSERVED_COMMIT_STATE: usize = 187;
const COMMIT_CURSOR: usize = 32;
const COMMIT_BLOCK: usize = 48;
const COMMIT_VALID: usize = 64;
const POSEIDON_WIDTH: usize = 16;
const DONE_BLOCK: u32 = 9;
const ROUND_CONSTANT_COUNT: usize = 8 * POSEIDON_WIDTH + 13;
const PARAMETER_START: usize = 267;
const PARAMETER_END: usize = PARAMETER_START + ROUND_CONSTANT_COUNT;
const FRI_CLEAN: usize = PARAMETER_END;
const FRI_FOLD_WORDS: usize = 32;
const FRI_OBSERVED: usize = FRI_CLEAN + FRI_FOLD_WORDS;
const FRI_STATUS: usize = FRI_OBSERVED + FRI_FOLD_WORDS;
const FRI_CLEAN_VALID: usize = FRI_STATUS;
const FRI_OBSERVED_VALID: usize = FRI_CLEAN_VALID + LDE_ROWS / 2;
const FRI_EQUAL: usize = FRI_OBSERVED_VALID + LDE_ROWS / 2;
const FRI_CORRECT: usize = FRI_EQUAL + 1;
const PROOF_WORDS: usize = FRI_CORRECT + 2;
const TAMPER_LDE_FIELD: usize = 17;
const DOMAIN: [u8; 4] = *b"MGDL";
const COMPUTE_PASSES: usize = 11;
const COMMITMENT_PASS: usize = 7;
const FRI_PASS: usize = 9;
const EXT_NONRESIDUE: u32 = 11;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repo-root ancestor")
        .to_path_buf()
}

fn compile_proof_graph() -> WebBundle {
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
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("demos/sketches/mandelbrot_proof_gpu".into())),
    )
    .expect("Mandelbrot proof actor should compile into a WebBundle")
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
            eprintln!("  Mandelbrot proof graph SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
            return None;
        }
        Err(error) => panic!(
            "Mandelbrot proof graph has no WebGPU adapter ({error:?}). Set up Vulkan/lavapipe, \
             or set MB2_ALLOW_GPU_SKIP to record an explicit non-execution."
        ),
    };
    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            ..Default::default()
        })) {
            Ok(pair) => pair,
            Err(error) if allow_skip => {
                eprintln!("  Mandelbrot proof graph SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
                return None;
            }
            Err(error) => panic!("Mandelbrot proof device request failed: {error:?}"),
        };
    Some((adapter, device, queue))
}

fn buffer_type(binding: &WebBinding) -> wgpu::BufferBindingType {
    wgpu::BufferBindingType::Storage {
        read_only: binding.access == WebBindingAccess::Read,
    }
}

struct ComputeKernel {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

struct BoundPass {
    group: wgpu::BindGroup,
    buffers: Vec<(u32, WebBindingRole, wgpu::Buffer)>,
}

struct DeviceResource {
    name: String,
    buffer: wgpu::Buffer,
}

fn compile_kernels(device: &wgpu::Device, bundle: &WebBundle) -> Vec<ComputeKernel> {
    bundle.manifest.passes[..COMPUTE_PASSES]
        .iter()
        .zip(&bundle.pass_wgsl[..COMPUTE_PASSES])
        .map(|(pass, shader)| {
            let entries = pass
                .layout
                .bindings
                .iter()
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding: binding.binding,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: buffer_type(binding),
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                })
                .collect::<Vec<_>>();
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(pass.source_entry.as_str()),
                entries: &entries,
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(pass.source_entry.as_str()),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(pass.source_entry.as_str()),
                source: wgpu::ShaderSource::Wgsl(shader.source.as_str().into()),
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(pass.source_entry.as_str()),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
            ComputeKernel { layout, pipeline }
        })
        .collect()
}

fn allocate_resources(device: &wgpu::Device, bundle: &WebBundle) -> Vec<DeviceResource> {
    bundle
        .manifest
        .resources
        .iter()
        .map(|resource| DeviceResource {
            name: resource.name.clone(),
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(resource.name.as_str()),
                size: u64::from(resource.length) * 4,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        })
        .collect()
}

fn resource<'a>(resources: &'a [DeviceResource], name: &str) -> &'a wgpu::Buffer {
    &resources
        .iter()
        .find(|resource| resource.name == name)
        .unwrap_or_else(|| panic!("missing actor resource `{name}`"))
        .buffer
}

fn largest_wgsl_functions(source: &str, count: usize) -> Vec<(&str, usize)> {
    let mut starts = Vec::new();
    if source.starts_with("fn ") {
        starts.push(0);
    }
    starts.extend(source.match_indices("\nfn ").map(|(index, _)| index + 1));
    let mut functions = starts
        .iter()
        .enumerate()
        .map(|(index, start)| {
            let end = starts.get(index + 1).copied().unwrap_or(source.len());
            let name_start = start + 3;
            let name_end = source[name_start..]
                .find('(')
                .map(|offset| name_start + offset)
                .unwrap_or(end);
            (&source[name_start..name_end], end - start)
        })
        .collect::<Vec<_>>();
    functions.sort_unstable_by_key(|(_, bytes)| std::cmp::Reverse(*bytes));
    functions.truncate(count);
    functions
}

fn scalar_input(binding: &WebBinding, tamper: f32) -> Vec<u8> {
    let mut bytes = vec![0u8; binding.span as usize];
    for member in &binding.members {
        assert_eq!(member.scalar, WebScalarKind::F32);
        assert_eq!(member.width, 4);
        let value = match member.name.as_str() {
            "tamper" => tamper,
            "res" => 512.0,
            other => panic!("unexpected proof actor scalar input `{other}`"),
        };
        let start = member.offset as usize;
        bytes[start..start + 4].copy_from_slice(&value.to_bits().to_le_bytes());
    }
    bytes
}

fn bind_case(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bundle: &WebBundle,
    kernels: &[ComputeKernel],
    resources: &[DeviceResource],
    tamper: f32,
) -> Vec<BoundPass> {
    bundle.manifest.passes[..COMPUTE_PASSES]
        .iter()
        .zip(kernels)
        .map(|(pass, kernel)| {
            let buffers = pass
                .layout
                .bindings
                .iter()
                .filter(|binding| binding.role != WebBindingRole::Resource)
                .map(|binding| {
                    let usage = match binding.role {
                        WebBindingRole::Input => {
                            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST
                        }
                        WebBindingRole::Output => {
                            wgpu::BufferUsages::STORAGE
                                | wgpu::BufferUsages::COPY_SRC
                                | wgpu::BufferUsages::COPY_DST
                        }
                        WebBindingRole::Resource => unreachable!(),
                    };
                    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(binding.name.as_str()),
                        size: u64::from(binding.span),
                        usage,
                        mapped_at_creation: false,
                    });
                    if binding.role == WebBindingRole::Input {
                        queue.write_buffer(&buffer, 0, &scalar_input(binding, tamper));
                    }
                    (binding.binding, binding.role, buffer)
                })
                .collect::<Vec<_>>();
            let entries = pass
                .layout
                .bindings
                .iter()
                .map(|binding| {
                    let resource = if binding.role == WebBindingRole::Resource {
                        resource(resources, binding.name.as_str()).as_entire_binding()
                    } else {
                        buffers
                            .iter()
                            .find(|(slot, _, _)| *slot == binding.binding)
                            .expect("owned pass binding")
                            .2
                            .as_entire_binding()
                    };
                    wgpu::BindGroupEntry {
                        binding: binding.binding,
                        resource,
                    }
                })
                .collect::<Vec<_>>();
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(pass.source_entry.as_str()),
                layout: &kernel.layout,
                entries: &entries,
            });
            BoundPass { group, buffers }
        })
        .collect()
}

fn map_bytes(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(1_800)),
        })
        .expect("Mandelbrot proof WebGPU submission should complete");
    rx.recv()
        .expect("map callback should fire")
        .expect("test-only staging buffer should map");
    let bytes = slice.get_mapped_range().to_vec();
    buffer.unmap();
    bytes
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one u32")))
        .collect()
}

struct ExecutionReceipt {
    proof: Vec<u32>,
    traps: Vec<u32>,
}

fn execute_case(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bundle: &WebBundle,
    kernels: &[ComputeKernel],
    tamper: f32,
) -> ExecutionReceipt {
    let proof_bytes = (PROOF_WORDS * 4) as u64;
    let resources = allocate_resources(device, bundle);
    let proof = resource(&resources, "proof");
    let bound = bind_case(device, queue, bundle, kernels, &resources, tamper);
    let trap_bytes = bound
        .iter()
        .flat_map(|pass| &pass.buffers)
        .filter(|(_, role, _)| *role == WebBindingRole::Output)
        .map(|(_, _, buffer)| buffer.size())
        .sum::<u64>();
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Mandelbrot proof test-only readback"),
        size: proof_bytes + trap_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Fe Mandelbrot proof graph"),
    });
    for ((manifest_pass, kernel), resources) in bundle.manifest.passes[..COMPUTE_PASSES]
        .iter()
        .zip(kernels)
        .zip(&bound)
    {
        let dispatch = manifest_pass.dispatch.expect("compute dispatch");
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(manifest_pass.source_entry.as_str()),
            timestamp_writes: None,
        });
        pass.set_pipeline(&kernel.pipeline);
        pass.set_bind_group(0, &resources.group, &[]);
        for _ in 0..manifest_pass.repeat {
            pass.dispatch_workgroups(dispatch[0], dispatch[1], dispatch[2]);
        }
        drop(pass);
    }
    encoder.copy_buffer_to_buffer(&proof, 0, &staging, 0, proof_bytes);
    let mut trap_offset = proof_bytes;
    for (_, _, buffer) in bound
        .iter()
        .flat_map(|pass| &pass.buffers)
        .filter(|(_, role, _)| *role == WebBindingRole::Output)
    {
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, trap_offset, buffer.size());
        trap_offset += buffer.size();
    }
    assert_eq!(trap_offset, proof_bytes + trap_bytes);
    queue.submit(Some(encoder.finish()));

    let bytes = map_bytes(device, &staging);
    ExecutionReceipt {
        proof: words(&bytes[..proof_bytes as usize]),
        traps: words(&bytes[proof_bytes as usize..]),
    }
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

fn add_mod(left: u32, right: u32) -> u32 {
    ((u64::from(left) + u64::from(right)) % u64::from(MODULUS)) as u32
}

fn sub_mod(left: u32, right: u32) -> u32 {
    ((u64::from(left) + u64::from(MODULUS) - u64::from(right))
        % u64::from(MODULUS)) as u32
}

fn mul_mod(left: u32, right: u32) -> u32 {
    (u64::from(left) * u64::from(right) % u64::from(MODULUS)) as u32
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Ext4([u32; 4]);

impl Ext4 {
    fn add(self, other: Self) -> Self {
        Self(std::array::from_fn(|index| {
            add_mod(self.0[index], other.0[index])
        }))
    }

    fn sub(self, other: Self) -> Self {
        Self(std::array::from_fn(|index| {
            sub_mod(self.0[index], other.0[index])
        }))
    }

    fn scale(self, scalar: u32) -> Self {
        Self(self.0.map(|coefficient| mul_mod(coefficient, scalar)))
    }

    fn mul(self, other: Self) -> Self {
        let mut coefficients = [0u32; 7];
        for left in 0..4 {
            for right in 0..4 {
                coefficients[left + right] = add_mod(
                    coefficients[left + right],
                    mul_mod(self.0[left], other.0[right]),
                );
            }
        }
        for degree in (4..=6).rev() {
            coefficients[degree - 4] = add_mod(
                coefficients[degree - 4],
                mul_mod(coefficients[degree], EXT_NONRESIDUE),
            );
        }
        Self(coefficients[..4].try_into().expect("four extension coefficients"))
    }
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

fn reference_permutation(input: [u32; POSEIDON_WIDTH]) -> [u32; POSEIDON_WIDTH] {
    let mut state = input.map(BabyBear::from_u32);
    default_babybear_poseidon2_16().permute_mut(&mut state);
    state.map(|value| value.as_canonical_u32())
}

fn reference_montgomery_parameters() -> Vec<u32> {
    let mut parameters = Vec::with_capacity(ROUND_CONSTANT_COUNT);
    for round in BABYBEAR_POSEIDON2_RC_16_EXTERNAL_INITIAL {
        parameters.extend(round.map(|value| value.as_canonical_u32()));
    }
    parameters.extend(BABYBEAR_POSEIDON2_RC_16_INTERNAL.map(|value| value.as_canonical_u32()));
    for round in BABYBEAR_POSEIDON2_RC_16_EXTERNAL_FINAL {
        parameters.extend(round.map(|value| value.as_canonical_u32()));
    }
    let radix = (1u64 << 32) % u64::from(MODULUS);
    parameters
        .into_iter()
        .map(|value| (u64::from(value) * radix % u64::from(MODULUS)) as u32)
        .collect()
}

fn reference_commitment(fields: &[u32]) -> [u32; 8] {
    let mut message = vec![u32::from_be_bytes(DOMAIN), fields.len() as u32];
    message.extend_from_slice(fields);
    let mut state = [0u32; POSEIDON_WIDTH];
    for block in message.chunks(8) {
        state[..block.len()].copy_from_slice(block);
        state = reference_permutation(state);
    }
    state[..8].try_into().expect("eight digest fields")
}

fn reference_fri_fold(fields: &[u32], clean_root: &[u32; 8], tampered: bool) -> Vec<u32> {
    let maximal_root = pow_mod(31, 15);
    let root = pow_mod(
        u64::from(maximal_root),
        1 << (TWO_ADICITY - LDE_ROWS.ilog2()),
    );
    let inverse_two = pow_mod(2, MODULUS - 2);
    let challenge = Ext4(clean_root[..4].try_into().expect("quartic challenge"));
    let mut output = Vec::with_capacity(FRI_FOLD_WORDS);
    for pair in 0..LDE_ROWS / 2 {
        let mut positive = Ext4(std::array::from_fn(|column| {
            fields[column * LDE_ROWS + pair]
        }));
        if tampered && pair == 1 {
            positive.0[1] = add_mod(positive.0[1], 1);
        }
        let negative = Ext4(std::array::from_fn(|column| {
            fields[column * LDE_ROWS + pair + LDE_ROWS / 2]
        }));
        let point = mul_mod(7, pow_mod(u64::from(root), pair as u32));
        let inverse_point = pow_mod(u64::from(point), MODULUS - 2);
        let even = positive.add(negative).scale(inverse_two);
        let odd = positive
            .sub(negative)
            .scale(inverse_two)
            .scale(inverse_point);
        output.extend(challenge.mul(odd).add(even).0);
    }
    output
}

fn assert_commit_state(words: &[u32], offset: usize) {
    assert_eq!(
        &words[offset + COMMIT_CURSOR..offset + COMMIT_CURSOR + POSEIDON_WIDTH],
        &[0; POSEIDON_WIDTH],
        "all field lanes must finish at the transition boundary"
    );
    assert_eq!(
        &words[offset + COMMIT_BLOCK..offset + COMMIT_BLOCK + POSEIDON_WIDTH],
        &[DONE_BLOCK; POSEIDON_WIDTH],
        "all field lanes must consume the complete typed message"
    );
    assert_eq!(
        &words[offset + COMMIT_VALID..offset + COMMIT_VALID + POSEIDON_WIDTH],
        &[1; POSEIDON_WIDTH],
        "all field lanes must retain a valid checkpoint"
    );
}

fn assert_receipt(receipt: &ExecutionReceipt, tampered: bool, expected_lde: &[u32]) {
    assert_eq!(receipt.proof.len(), PROOF_WORDS);
    let expected_trace = trace_columns().into_iter().flatten().collect::<Vec<_>>();
    assert_eq!(&receipt.proof[..LDE_START], &expected_trace);
    assert_eq!(
        &receipt.proof[LDE_START..LDE_START + expected_lde.len()],
        expected_lde,
        "GPU LDE must match the independent direct DFT"
    );
    let clean = reference_commitment(expected_lde);
    let mut observed_fields = expected_lde.to_vec();
    if tampered {
        observed_fields[TAMPER_LDE_FIELD] = (observed_fields[TAMPER_LDE_FIELD] + 1) % MODULUS;
    }
    let observed = reference_commitment(&observed_fields);
    assert_eq!(&receipt.proof[CLEAN_ROOT..CLEAN_ROOT + 8], &clean);
    assert_eq!(&receipt.proof[OBSERVED_ROOT..OBSERVED_ROOT + 8], &observed);
    assert_eq!(receipt.proof[TRACE_VALID], 1);
    assert_eq!(
        &receipt.proof[LDE_VALID_START..LDE_VALID_START + COLUMN_COUNT],
        &[1; COLUMN_COUNT]
    );
    assert_eq!(receipt.proof[ROOTS_EQUAL], u32::from(!tampered));
    assert_eq!(receipt.proof[MODE_CORRECT], 1);
    assert_commit_state(&receipt.proof, CLEAN_COMMIT_STATE);
    assert_commit_state(&receipt.proof, OBSERVED_COMMIT_STATE);
    assert_eq!(
        &receipt.proof[PARAMETER_START..PARAMETER_END],
        reference_montgomery_parameters(),
        "GPU parameter initialization must match Plonky3 exactly"
    );
    let expected_clean_fri = reference_fri_fold(expected_lde, &clean, false);
    let expected_observed_fri = reference_fri_fold(expected_lde, &clean, tampered);
    assert_eq!(
        &receipt.proof[FRI_CLEAN..FRI_CLEAN + FRI_FOLD_WORDS],
        expected_clean_fri,
        "GPU clean FRI pairs must match the independent quartic-field formula",
    );
    assert_eq!(
        &receipt.proof[FRI_OBSERVED..FRI_OBSERVED + FRI_FOLD_WORDS],
        expected_observed_fri,
        "GPU observed FRI pairs must match the independently mutated formula",
    );
    assert_eq!(
        &receipt.proof[FRI_CLEAN_VALID..FRI_CLEAN_VALID + LDE_ROWS / 2],
        &[1; LDE_ROWS / 2],
    );
    assert_eq!(
        &receipt.proof[FRI_OBSERVED_VALID..FRI_OBSERVED_VALID + LDE_ROWS / 2],
        &[1; LDE_ROWS / 2],
    );
    assert_eq!(receipt.proof[FRI_EQUAL], u32::from(!tampered));
    assert_eq!(receipt.proof[FRI_CORRECT], 1);
    assert!(
        receipt.traps.iter().all(|word| *word == 0),
        "every physical invocation lane must remain trap-free: {:?}",
        receipt.traps
    );
}

#[test]
fn complete_proof_graph_matches_independent_oracles_on_webgpu() {
    let bundle = compile_proof_graph();
    assert_eq!(bundle.manifest.resources.len(), 6);
    assert_eq!(
        bundle
            .manifest
            .resources
            .iter()
            .map(|resource| (resource.name.as_str(), resource.length))
            .collect::<Vec<_>>(),
        vec![
            ("proof", PROOF_WORDS as u32),
            ("lde_inverse_values", 16),
            ("lde_inverse_progress", 8),
            ("lde_values", 64),
            ("lde_progress", 32),
            ("lde_coset_valid", 4),
        ]
    );
    assert_eq!(bundle.manifest.passes.len(), COMPUTE_PASSES + 1);
    assert_eq!(bundle.manifest.passes[2].repeat, 2);
    assert_eq!(bundle.manifest.passes[2].layout.workgroup_size, [8, 1, 1]);
    assert_eq!(bundle.manifest.passes[4].repeat, 4);
    assert_eq!(bundle.manifest.passes[4].layout.workgroup_size, [32, 1, 1]);
    assert_eq!(bundle.manifest.passes[COMMITMENT_PASS].repeat, 396);
    assert_eq!(
        bundle.manifest.passes[COMMITMENT_PASS]
            .layout
            .workgroup_size,
        [32, 1, 1]
    );
    assert_eq!(bundle.manifest.passes[FRI_PASS].repeat, 1);
    assert_eq!(
        bundle.manifest.passes[FRI_PASS].layout.workgroup_size,
        [16, 1, 1]
    );

    let Some((adapter, device, queue)) = request_browser_profile_device() else {
        return;
    };
    eprintln!(
        "  Mandelbrot proof WebGPU adapter (no required features): {}",
        adapter.get_info().name
    );
    eprintln!(
        "  Mandelbrot proof shader bytes: {:?}",
        bundle
            .manifest
            .passes
            .iter()
            .zip(&bundle.pass_wgsl)
            .map(|(pass, shader)| (pass.source_entry.as_str(), shader.source.len()))
            .collect::<Vec<_>>()
    );
    eprintln!(
        "  Mandelbrot proof largest commitment functions: {:?}",
        largest_wgsl_functions(&bundle.pass_wgsl[COMMITMENT_PASS].source, 12)
    );
    eprintln!(
        "  Mandelbrot proof largest FRI functions: {:?}",
        largest_wgsl_functions(&bundle.pass_wgsl[FRI_PASS].source, 12)
    );
    let pipeline_started = std::time::Instant::now();
    let kernels = compile_kernels(&device, &bundle);
    eprintln!(
        "  Mandelbrot proof pipelines compiled in {:?}",
        pipeline_started.elapsed()
    );
    let expected_lde = trace_columns()
        .iter()
        .flat_map(|column| direct_coset_lde(column, LDE_ROWS, 7))
        .collect::<Vec<_>>();

    let clean_started = std::time::Instant::now();
    let clean = execute_case(&device, &queue, &bundle, &kernels, 0.0);
    eprintln!(
        "  Mandelbrot proof clean graph executed in {:?}",
        clean_started.elapsed()
    );
    assert_receipt(&clean, false, &expected_lde);
    let tampered_started = std::time::Instant::now();
    let tampered = execute_case(&device, &queue, &bundle, &kernels, 1.0);
    eprintln!(
        "  Mandelbrot proof tampered graph executed in {:?}",
        tampered_started.elapsed()
    );
    assert_receipt(&tampered, true, &expected_lde);
    assert_ne!(
        &clean.proof[OBSERVED_ROOT..OBSERVED_ROOT + 8],
        &tampered.proof[OBSERVED_ROOT..OBSERVED_ROOT + 8],
        "the Fe-authored mutation mode must alter the observed commitment"
    );
    assert_ne!(
        &clean.proof[FRI_OBSERVED..FRI_OBSERVED + FRI_FOLD_WORDS],
        &tampered.proof[FRI_OBSERVED..FRI_OBSERVED + FRI_FOLD_WORDS],
        "the Fe-authored mutation mode must alter the observed FRI fold"
    );
}
