//! Executed exactness gate for canonical BabyBear row commitments, ordered
//! Poseidon2 Merkle placement, and root-derived interaction challenges.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WebBindingAccess, WebBindingRole, WebBuildOptions, WebBundle, WebBundleMode, resolve_web_entry,
};
use hir::hir_def::HirIngot;
use p3_baby_bear::{BabyBear, default_babybear_poseidon2_16};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_symmetric::Permutation;
use url::Url;

const MODULUS: u32 = 2_013_265_921;
const WIDTH: usize = 16;
const DIGEST_FIELDS: usize = 8;
const LEAVES: usize = 8;
const FIELDS: usize = 5;
const VALUE_WORDS: usize = LEAVES * FIELDS;
const COMMITMENT_WORKSPACE_WORDS: usize = LEAVES * (WIDTH + 2);
const COMMITMENT_STEPS: u32 = 2;
const TREE_NODES: usize = LEAVES * 2 - 1;
const TREE_WORDS: usize = TREE_NODES * DIGEST_FIELDS;
const PARENT_TASKS: usize = LEAVES / 2;
const LEVELS: u32 = 3;
const CHALLENGE_COUNT: usize = 8;
const CHALLENGE_WORKSPACE_WORDS: usize = 408;
const CHALLENGE_PROGRESS_START: usize = 280;
const CHALLENGE_OUTPUT_WORDS: usize = 40;
const CHALLENGE_STEPS: u32 = 89;
const COMPUTE_PASSES: usize = 8;
const CHALLENGE_TAGS: [[u8; 4]; CHALLENGE_COUNT] = [
    *b"PB02", *b"PG02", *b"RB02", *b"RG02", *b"LB02", *b"LG02", *b"BB02", *b"BG02",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repo-root ancestor")
        .to_path_buf()
}

fn compile_oracle() -> WebBundle {
    let dir = repo_root().join("crates/codegen/tests/fixtures/poseidon_merkle_webgpu_oracle_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "Poseidon Merkle fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("Poseidon Merkle fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "Poseidon Merkle fixture diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("poseidon_merkle_webgpu_oracle".into())),
    )
    .expect("Poseidon Merkle fixture should compile into a WebBundle")
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
            eprintln!("  Poseidon Merkle WebGPU oracle SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
            return None;
        }
        Err(error) => panic!(
            "Poseidon Merkle oracle has no WebGPU adapter ({error:?}). Set up Vulkan/lavapipe, or \
             set MB2_ALLOW_GPU_SKIP to record an explicit non-execution."
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
                    "  Poseidon Merkle WebGPU oracle SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}"
                );
                return None;
            }
            Err(error) => panic!("Poseidon Merkle oracle device request failed: {error:?}"),
        };
    Some((adapter, device, queue))
}

fn reference_permutation(input: [u32; WIDTH]) -> [u32; WIDTH] {
    let mut state = input.map(BabyBear::from_u32);
    default_babybear_poseidon2_16().permute_mut(&mut state);
    state.map(|value| value.as_canonical_u32())
}

fn reference_sponge(message: &[u32]) -> [u32; DIGEST_FIELDS] {
    let mut state = [0u32; WIDTH];
    for block in message.chunks(8) {
        state[..block.len()].copy_from_slice(block);
        state = reference_permutation(state);
    }
    state[..DIGEST_FIELDS]
        .try_into()
        .expect("eight digest fields")
}

fn reference_leaf(row: usize, values: &[u32]) -> [u32; DIGEST_FIELDS] {
    let mut fields = vec![4, 4, LEAVES as u32, row as u32];
    fields.extend((0..FIELDS).map(|field| values[field * LEAVES + row]));
    let mut message = vec![u32::from_be_bytes(*b"LD01"), fields.len() as u32];
    message.extend(fields);
    reference_sponge(&message)
}

fn reference_compress(
    left: [u32; DIGEST_FIELDS],
    right: [u32; DIGEST_FIELDS],
) -> [u32; DIGEST_FIELDS] {
    let mut state = [0u32; WIDTH];
    state[..DIGEST_FIELDS].copy_from_slice(&left);
    state[DIGEST_FIELDS..].copy_from_slice(&right);
    reference_permutation(state)[..DIGEST_FIELDS]
        .try_into()
        .expect("eight parent fields")
}

fn reference_tree(values: &[u32]) -> Vec<[u32; DIGEST_FIELDS]> {
    let mut tree = (0..LEAVES)
        .map(|row| reference_leaf(row, values))
        .collect::<Vec<_>>();
    let mut offset = 0;
    let mut width = LEAVES;
    while width > 1 {
        for parent in 0..width / 2 {
            tree.push(reference_compress(
                tree[offset + parent * 2],
                tree[offset + parent * 2 + 1],
            ));
        }
        offset += width;
        width /= 2;
    }
    tree
}

fn reference_challenges(root: [u32; DIGEST_FIELDS]) -> Vec<u32> {
    CHALLENGE_TAGS
        .iter()
        .flat_map(|tag| {
            let mut message = vec![u32::from_be_bytes(*tag), DIGEST_FIELDS as u32];
            message.extend(root);
            reference_sponge(&message)[..4].to_vec()
        })
        .collect()
}

fn input_values() -> Vec<u32> {
    (0..FIELDS)
        .flat_map(|field| {
            (0..LEAVES)
                .map(move |row| ((field as u32 + 1) * 10_003 + (row as u32 + 1) * 257) % MODULUS)
        })
        .collect()
}

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn read_words(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &wgpu::Buffer,
    words: usize,
) -> Vec<u32> {
    let size = (words * 4) as u64;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Poseidon Merkle readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Poseidon Merkle readback encoder"),
    });
    encoder.copy_buffer_to_buffer(source, 0, &staging, 0, size);
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
            timeout: Some(std::time::Duration::from_secs(180)),
        })
        .expect("Poseidon Merkle submission should complete");
    rx.recv()
        .expect("map callback should fire")
        .expect("Poseidon Merkle staging buffer should map");
    let mapped = slice.get_mapped_range();
    let result = mapped
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one u32")))
        .collect();
    drop(mapped);
    staging.unmap();
    result
}

struct ExecutablePass {
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    dispatch: [u32; 3],
    repeat: u32,
}

fn submit_passes(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    passes: &[ExecutablePass],
    label: &'static str,
) {
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });
    for pass in passes {
        let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(label),
            timestamp_writes: None,
        });
        compute.set_pipeline(&pass.pipeline);
        compute.set_bind_group(0, &pass.bind_group, &[]);
        for _ in 0..pass.repeat {
            compute.dispatch_workgroups(pass.dispatch[0], pass.dispatch[1], pass.dispatch[2]);
        }
    }
    queue.submit(Some(encoder.finish()));
}

#[test]
fn canonical_rows_ordered_tree_and_challenges_match_plonky3_on_webgpu() {
    let bundle = compile_oracle();
    assert_eq!(bundle.manifest.passes.len(), COMPUTE_PASSES + 1);
    let expected_passes = [
        ("prepare_commitments", [4, 1, 1], [2, 1, 1], 1),
        (
            "advance_commitments",
            [4, 1, 1],
            [2, 1, 1],
            COMMITMENT_STEPS,
        ),
        ("prepare_tree", [4, 1, 1], [1, 1, 1], 1),
        ("advance_tree", [4, 1, 1], [1, 1, 1], LEVELS),
        ("finish_tree", [1, 1, 1], [1, 1, 1], 1),
        ("prepare_challenges", [16, 1, 1], [8, 1, 1], 1),
        ("advance_challenges", [16, 1, 1], [8, 1, 1], CHALLENGE_STEPS),
        ("finish_challenges", [1, 1, 1], [1, 1, 1], 1),
    ];
    for ((pass, shader), (name, workgroup, dispatch, repeat)) in bundle.manifest.passes
        [..COMPUTE_PASSES]
        .iter()
        .zip(&bundle.pass_wgsl[..COMPUTE_PASSES])
        .zip(expected_passes)
    {
        assert_eq!(pass.source_entry, name);
        assert_eq!(pass.layout.workgroup_size, workgroup);
        assert_eq!(pass.dispatch, Some(dispatch));
        assert_eq!(pass.repeat, repeat);
        let module = naga::front::wgsl::parse_str(&shader.source)
            .unwrap_or_else(|error| panic!("{name} WGSL parse failed: {error:?}"));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap_or_else(|error| panic!("{name} WGSL browser validation failed: {error:?}"));
    }

    let Some((adapter, device, queue)) = request_browser_profile_device() else {
        return;
    };
    eprintln!(
        "  Poseidon Merkle WebGPU adapter (no required features): {}",
        adapter.get_info().name,
    );
    eprintln!(
        "  Poseidon Merkle WGSL bytes: leaves={}, parents={}, challenges={}",
        bundle.pass_wgsl[1].source.len(),
        bundle.pass_wgsl[3].source.len(),
        bundle.pass_wgsl[6].source.len(),
    );

    let shapes = bundle
        .manifest
        .resources
        .iter()
        .map(|resource| (resource.name.as_str(), (resource.length, resource.stride)))
        .collect::<HashMap<_, _>>();
    assert_eq!(shapes["values"], (VALUE_WORDS as u32, 4));
    assert_eq!(
        shapes["commitment_workspace"],
        (COMMITMENT_WORKSPACE_WORDS as u32, 4),
    );
    assert_eq!(shapes["tree"], (TREE_WORDS as u32, 4));
    assert_eq!(shapes["node_valid"], (TREE_NODES as u32, 4));
    assert_eq!(shapes["progress"], (PARENT_TASKS as u32, 4));
    assert_eq!(
        shapes["challenge_workspace"],
        (CHALLENGE_WORKSPACE_WORDS as u32, 4),
    );
    assert_eq!(
        shapes["challenge_output"],
        (CHALLENGE_OUTPUT_WORDS as u32, 4),
    );
    assert_eq!(shapes.len(), 7);

    let resources = bundle
        .manifest
        .resources
        .iter()
        .map(|resource| {
            (
                resource.name.clone(),
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(resource.name.as_str()),
                    size: u64::from(resource.length) * u64::from(resource.stride),
                    usage: wgpu::BufferUsages::STORAGE
                        | wgpu::BufferUsages::COPY_SRC
                        | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut extras = HashMap::<(usize, u32), wgpu::Buffer>::new();
    for (pass_index, pass) in bundle.manifest.passes[..COMPUTE_PASSES].iter().enumerate() {
        for binding in &pass.layout.bindings {
            if binding.role != WebBindingRole::Resource {
                extras.insert(
                    (pass_index, binding.binding),
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(&format!("{} {}", pass.source_entry, binding.name)),
                        size: u64::from(binding.span),
                        usage: wgpu::BufferUsages::STORAGE
                            | wgpu::BufferUsages::COPY_SRC
                            | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                );
            }
        }
    }

    let mut executable = Vec::new();
    for (pass_index, (pass, shader)) in bundle.manifest.passes[..COMPUTE_PASSES]
        .iter()
        .zip(&bundle.pass_wgsl[..COMPUTE_PASSES])
        .enumerate()
    {
        let layout_entries = pass
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
            label: Some(&format!("{} layout", pass.source_entry)),
            entries: &layout_entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&format!("{} pipeline layout", pass.source_entry)),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&format!("{} Fe WGSL", pass.source_entry)),
            source: wgpu::ShaderSource::Wgsl(shader.source.as_str().into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(&format!("{} pipeline", pass.source_entry)),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let entries = pass
            .layout
            .bindings
            .iter()
            .map(|binding| {
                let buffer = if binding.role == WebBindingRole::Resource {
                    resources
                        .get(&binding.name)
                        .unwrap_or_else(|| panic!("missing resource {}", binding.name))
                } else {
                    extras
                        .get(&(pass_index, binding.binding))
                        .unwrap_or_else(|| panic!("missing extra binding {}", binding.name))
                };
                wgpu::BindGroupEntry {
                    binding: binding.binding,
                    resource: buffer.as_entire_binding(),
                }
            })
            .collect::<Vec<_>>();
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("{} bindings", pass.source_entry)),
            layout: &bind_group_layout,
            entries: &entries,
        });
        executable.push(ExecutablePass {
            pipeline,
            bind_group,
            dispatch: pass.dispatch.expect("fixed compute dispatch"),
            repeat: pass.repeat,
        });
    }

    let values = input_values();
    let expected_tree = reference_tree(&values);
    let expected_tree_words = expected_tree
        .iter()
        .flat_map(|digest| digest.iter().copied())
        .collect::<Vec<_>>();
    let expected_progress = vec![3, 2, 1, 1];
    let expected_challenges = reference_challenges(*expected_tree.last().expect("root"));
    let expected_challenge_output = expected_challenges
        .iter()
        .copied()
        .chain(std::iter::repeat_n(1, CHALLENGE_COUNT))
        .collect::<Vec<_>>();
    let zero_tree = vec![0u32; TREE_WORDS];
    let zero_commitment_workspace = vec![0u32; COMMITMENT_WORKSPACE_WORDS];
    let zero_valid = vec![0u32; TREE_NODES];
    let zero_progress = vec![0u32; PARENT_TASKS];
    let zero_challenge_workspace = vec![0u32; CHALLENGE_WORKSPACE_WORDS];
    let zero_challenge_output = vec![0u32; CHALLENGE_OUTPUT_WORDS];

    queue.write_buffer(&resources["values"], 0, &bytes(&values));
    queue.write_buffer(
        &resources["commitment_workspace"],
        0,
        &bytes(&zero_commitment_workspace),
    );
    queue.write_buffer(&resources["tree"], 0, &bytes(&zero_tree));
    queue.write_buffer(&resources["node_valid"], 0, &bytes(&zero_valid));
    queue.write_buffer(&resources["progress"], 0, &bytes(&zero_progress));
    queue.write_buffer(
        &resources["challenge_workspace"],
        0,
        &bytes(&zero_challenge_workspace),
    );
    queue.write_buffer(
        &resources["challenge_output"],
        0,
        &bytes(&zero_challenge_output),
    );
    submit_passes(
        &device,
        &queue,
        &executable,
        "canonical rows and ordered Poseidon Merkle tree",
    );
    assert_eq!(
        read_words(&device, &queue, &resources["tree"], TREE_WORDS),
        expected_tree_words,
        "every GPU leaf and ordered parent must match Plonky3",
    );
    assert_eq!(
        read_words(&device, &queue, &resources["node_valid"], TREE_NODES),
        vec![1; TREE_NODES],
    );
    assert_eq!(
        read_words(&device, &queue, &resources["progress"], PARENT_TASKS),
        expected_progress,
    );
    assert_eq!(
        read_words(
            &device,
            &queue,
            &resources["challenge_output"],
            CHALLENGE_OUTPUT_WORDS,
        ),
        expected_challenge_output,
        "all eight GPU interaction challenges must match Plonky3",
    );

    let incomplete_challenge = CHALLENGE_STEPS - 1;
    queue.write_buffer(
        &resources["challenge_workspace"],
        (CHALLENGE_PROGRESS_START * 4) as u64,
        &bytes(&[incomplete_challenge]),
    );
    submit_passes(
        &device,
        &queue,
        &executable[7..8],
        "reject incomplete interaction challenge",
    );
    assert_eq!(
        read_words(&device, &queue, &resources["node_valid"], TREE_NODES)[TREE_NODES - 1..],
        vec![0],
        "one incomplete private challenge cursor must invalidate the batch",
    );

    queue.write_buffer(
        &resources["node_valid"],
        ((TREE_NODES - 1) * 4) as u64,
        &bytes(&[1]),
    );
    queue.write_buffer(
        &resources["challenge_workspace"],
        (CHALLENGE_PROGRESS_START * 4) as u64,
        &bytes(&[CHALLENGE_STEPS]),
    );
    queue.write_buffer(&resources["challenge_output"], 0, &bytes(&[MODULUS]));
    submit_passes(
        &device,
        &queue,
        &executable[7..8],
        "reject noncanonical interaction challenge",
    );
    assert_eq!(
        read_words(&device, &queue, &resources["node_valid"], TREE_NODES)[TREE_NODES - 1..],
        vec![0],
        "one noncanonical challenge coefficient must invalidate the batch",
    );

    let mut incomplete = expected_progress.clone();
    incomplete[0] -= 1;
    queue.write_buffer(&resources["progress"], 0, &bytes(&incomplete));
    queue.write_buffer(&resources["node_valid"], 0, &bytes(&vec![1; TREE_NODES]));
    queue.write_buffer(
        &resources["challenge_workspace"],
        0,
        &bytes(&zero_challenge_workspace),
    );
    queue.write_buffer(
        &resources["challenge_output"],
        0,
        &bytes(&zero_challenge_output),
    );
    submit_passes(
        &device,
        &queue,
        &executable[4..],
        "reject incomplete ordered tree",
    );
    assert_eq!(
        read_words(&device, &queue, &resources["node_valid"], TREE_NODES)[TREE_NODES - 1..],
        vec![0],
        "one incomplete private cursor must invalidate the root",
    );

    let mut noncanonical = values.clone();
    let invalid_row = 3;
    noncanonical[2 * LEAVES + invalid_row] = MODULUS;
    queue.write_buffer(&resources["values"], 0, &bytes(&noncanonical));
    queue.write_buffer(
        &resources["commitment_workspace"],
        0,
        &bytes(&zero_commitment_workspace),
    );
    queue.write_buffer(&resources["tree"], 0, &bytes(&zero_tree));
    queue.write_buffer(&resources["node_valid"], 0, &bytes(&zero_valid));
    queue.write_buffer(&resources["progress"], 0, &bytes(&zero_progress));
    queue.write_buffer(
        &resources["challenge_workspace"],
        0,
        &bytes(&zero_challenge_workspace),
    );
    queue.write_buffer(
        &resources["challenge_output"],
        0,
        &bytes(&zero_challenge_output),
    );
    submit_passes(
        &device,
        &queue,
        &executable,
        "reject noncanonical row before the Merkle root",
    );
    let invalid_words = read_words(&device, &queue, &resources["node_valid"], TREE_NODES);
    assert_eq!(invalid_words[invalid_row], 0);
    assert_eq!(invalid_words[TREE_NODES - 1], 0);

    for ((pass_index, binding), buffer) in &extras {
        let pass = &bundle.manifest.passes[*pass_index];
        let layout = pass
            .layout
            .bindings
            .iter()
            .find(|candidate| candidate.binding == *binding)
            .expect("extra binding layout");
        assert_eq!(
            read_words(&device, &queue, buffer, (layout.span / 4) as usize),
            vec![0; (layout.span / 4) as usize],
            "{} {} must retain a clean compiler receipt",
            pass.source_entry,
            layout.name,
        );
    }
}
