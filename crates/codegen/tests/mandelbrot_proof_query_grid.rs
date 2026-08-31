//! Executed WebGPU gate for the production Mandelbrot FRI query grid.
//!
//! Fe derives the 114-query Cartesian schedule from the security policy and
//! the thirteen-round FRI tree. This test checks the emitted browser contract,
//! executes every staged work item, and compares the materialized round plan,
//! sampled indices, evaluation fields, and Merkle siblings with independent
//! Rust and Plonky3 recurrences.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    resolve_web_entry, WebBindingAccess, WebBindingRole, WebBuildOptions, WebBundle, WebBundleMode,
};
use hir::hir_def::HirIngot;
use p3_baby_bear::{default_babybear_poseidon2_16, BabyBear};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_symmetric::Permutation;
use url::Url;

const BABY_BEAR_MODULUS: u32 = 2_013_265_921;
const POSEIDON_WIDTH: usize = 16;
const QUERY_COUNT: usize = 114;
const EVALUATIONS_PER_QUERY: usize = 25;
const SIBLINGS_PER_QUERY: usize = 132;
const EVALUATION_ITEMS: usize = QUERY_COUNT * EVALUATIONS_PER_QUERY;
const SIBLING_ITEMS: usize = QUERY_COUNT * SIBLINGS_PER_QUERY;
const EVALUATION_WORDS: usize = 4;
const DIGEST_WORDS: usize = 8;
const FRI_ROUNDS: usize = 13;
const ROUND_PLACEMENT_FIELDS: usize = 13;
const QUERY_CURSOR_FIELDS: usize = 8;
const ROUND_PLACEMENT_WORDS: usize = FRI_ROUNDS * ROUND_PLACEMENT_FIELDS;
const FOLDED_EVALUATIONS: usize = 8191;
const LAYER_TREE_NODES: usize = 16369;
const FOLDED_EVALUATION_WORDS: usize = FOLDED_EVALUATIONS * EVALUATION_WORDS;
const LAYER_TREE_WORDS: usize = LAYER_TREE_NODES * DIGEST_WORDS;
const PROOF_DATA_WORDS: usize = FOLDED_EVALUATION_WORDS + LAYER_TREE_WORDS;
const EVALUATION_CURSOR_WORDS: usize = EVALUATION_ITEMS * QUERY_CURSOR_FIELDS;
const SIBLING_CURSOR_WORDS: usize = SIBLING_ITEMS * QUERY_CURSOR_FIELDS;
const OPENING_ACTIVITY_START: usize = PROOF_DATA_WORDS
    + EVALUATION_CURSOR_WORDS
    + EVALUATION_ITEMS
    + SIBLING_CURSOR_WORDS
    + SIBLING_ITEMS;
const PROOF_ARENA_WORDS: usize = OPENING_ACTIVITY_START + LAYER_TREE_NODES;
const COMPACT_METADATA_WORDS: usize = 3 * FRI_ROUNDS + EVALUATION_ITEMS;
const OPENING_PADDING_START: usize = COMPACT_METADATA_WORDS;
const OPENING_METADATA_WORDS: usize = COMPACT_METADATA_WORDS + EVALUATION_PADDING + SIBLING_PADDING;
const COMPACT_VALID_START: usize = 0;
const COMPACT_LEAF_COUNT_START: usize = FRI_ROUNDS;
const COMPACT_SIBLING_COUNT_START: usize = 2 * FRI_ROUNDS;
const COMPACT_LEAF_INDEX_START: usize = 3 * FRI_ROUNDS;
const THREADS: u32 = 64;
const EVALUATION_GROUPS: u32 = 45;
const SIBLING_GROUPS: u32 = 236;
const EVALUATION_PADDING: usize = EVALUATION_GROUPS as usize * THREADS as usize - EVALUATION_ITEMS;
const SIBLING_PADDING: usize = SIBLING_GROUPS as usize * THREADS as usize - SIBLING_ITEMS;
const QUERY_SAMPLE_GROUPS: u32 = 29;
const QUERY_SAMPLE_STEPS: u32 = 89;
const QUERY_SAMPLE_WORK_ITEMS: usize = QUERY_COUNT * POSEIDON_WIDTH;
const QUERY_SAMPLE_PADDING: usize =
    QUERY_SAMPLE_GROUPS as usize * THREADS as usize - QUERY_SAMPLE_WORK_ITEMS;
const QUERY_STATE_WORDS: usize = QUERY_COUNT * 2 * POSEIDON_WIDTH;
const QUERY_TAIL_WORDS: usize = QUERY_COUNT * 3;
const QUERY_PROGRESS_WORDS: usize = QUERY_COUNT * POSEIDON_WIDTH;
const QUERY_HALF_MASK: u32 = 4095;
const QUERY_TRANSCRIPT_START: usize = 0;
const QUERY_TRANSCRIPT_VALID_START: usize = QUERY_TRANSCRIPT_START + 8;
const QUERY_INDICES_START: usize = QUERY_TRANSCRIPT_VALID_START + 1;
const QUERY_PADDING_START: usize = QUERY_INDICES_START + QUERY_COUNT;
const QUERY_CONTROL_WORDS: usize = QUERY_PADDING_START + QUERY_SAMPLE_PADDING;
const QUERY_STATES_START: usize = 0;
const QUERY_TAILS_START: usize = QUERY_STATES_START + QUERY_STATE_WORDS;
const QUERY_VALIDITY_START: usize = QUERY_TAILS_START + QUERY_TAIL_WORDS;
const QUERY_PROGRESS_START: usize = QUERY_VALIDITY_START + QUERY_COUNT;
const QUERY_WORKSPACE_WORDS: usize = QUERY_PROGRESS_START + QUERY_PROGRESS_WORDS;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repository root")
        .to_path_buf()
}

fn compile_query_grid() -> WebBundle {
    let dir = repo_root().join("crates/codegen/tests/fixtures/mandelbrot_proof_query_grid_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "query-grid ingot initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("query-grid fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "query-grid source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("mandelbrot_proof_query_grid".into())),
    )
    .expect("query-grid fixture should compile into a WebBundle")
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
            eprintln!("  production query grid SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
            return None;
        }
        Err(error) => panic!(
            "production query grid has no WebGPU adapter ({error:?}). Set up Vulkan/lavapipe, or \
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
                eprintln!("  production query grid SKIPPED (MB2_ALLOW_GPU_SKIP): {error:?}");
                return None;
            }
            Err(error) => panic!("production query-grid device request failed: {error:?}"),
        };
    Some((adapter, device, queue))
}

fn read_words(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &wgpu::Buffer,
    words: usize,
) -> Vec<u32> {
    let size = (words * 4) as u64;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("production query-grid readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("production query-grid readback encoder"),
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
        .expect("production query-grid submission should complete");
    rx.recv()
        .expect("map callback should fire")
        .expect("query-grid staging buffer should map");
    let mapped = slice.get_mapped_range();
    let result = mapped
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one u32")))
        .collect();
    drop(mapped);
    staging.unmap();
    result
}

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RoundPlacement {
    index: u32,
    round: u32,
    input_log: u32,
    input_width: u32,
    output_width: u32,
    output_offset: u32,
    tree_offset: u32,
    tree_nodes: u32,
    query_value_offset: u32,
    query_sibling_offset: u32,
    pair_width: u32,
    pair_depth: u32,
}

fn reference_round_placements() -> Vec<RoundPlacement> {
    let mut output_offset = 0;
    let mut tree_offset = 0;
    let mut query_value_offset = 0;
    let mut query_sibling_offset = 0;
    (0..FRI_ROUNDS)
        .map(|index| {
            let input_log = (FRI_ROUNDS - index) as u32;
            let input_width = 1u32 << input_log;
            let output_width = input_width / 2;
            let pair_width = if input_log > 1 {
                1u32 << (input_log - 2)
            } else {
                0
            };
            let pair_depth = input_log.saturating_sub(2);
            let placement = RoundPlacement {
                index: index as u32,
                round: index as u32 + 1,
                input_log,
                input_width,
                output_width,
                output_offset,
                tree_offset,
                tree_nodes: 2 * output_width - 1,
                query_value_offset,
                query_sibling_offset,
                pair_width,
                pair_depth,
            };
            output_offset += output_width;
            tree_offset += placement.tree_nodes;
            query_value_offset += if input_log > 1 { 2 } else { 1 };
            query_sibling_offset += 2 * pair_depth;
            placement
        })
        .collect()
}

fn flattened_round_placements(rounds: &[RoundPlacement]) -> Vec<u32> {
    rounds
        .iter()
        .flat_map(|round| {
            [
                1,
                round.index,
                round.round,
                round.input_log,
                round.input_width,
                round.output_width,
                round.output_offset,
                round.tree_offset,
                round.tree_nodes,
                round.query_value_offset,
                round.query_sibling_offset,
                round.pair_width,
                round.pair_depth,
            ]
        })
        .collect()
}

fn reference_evaluation_index(rounds: &[RoundPlacement], opening: u32, query: u32) -> usize {
    let mut remaining = opening;
    let mut current_query = query;
    for placement in rounds {
        assert!(current_query < placement.output_width);
        let opened = if placement.output_width > 1 { 2 } else { 1 };
        if remaining < opened {
            let evaluation = if placement.output_width > 1 {
                let pair = current_query & (placement.pair_width - 1);
                pair + remaining * placement.pair_width
            } else {
                0
            };
            return (placement.output_offset + evaluation) as usize;
        }
        remaining -= opened;
        current_query = if placement.output_width > 1 {
            current_query & (placement.pair_width - 1)
        } else {
            0
        };
    }
    panic!("evaluation opening {opening} is outside the derived FRI schedule")
}

fn tree_level_offset(width: u32, level: u32) -> u32 {
    let mut offset = 0;
    let mut level_width = width;
    for _ in 0..level {
        offset += level_width;
        level_width /= 2;
    }
    offset
}

fn reference_tree_node(rounds: &[RoundPlacement], sibling: u32, query: u32) -> usize {
    let mut remaining = sibling;
    let mut current_query = query;
    for placement in rounds {
        assert!(current_query < placement.output_width);
        let siblings = 2 * placement.pair_depth;
        if placement.pair_depth > 0 && remaining < siblings {
            let side = remaining / placement.pair_depth;
            let level = remaining % placement.pair_depth;
            let leaf = (current_query & (placement.pair_width - 1)) + side * placement.pair_width;
            return (placement.tree_offset
                + tree_level_offset(placement.output_width, level)
                + ((leaf >> level) ^ 1)) as usize;
        }
        remaining -= siblings;
        current_query = if placement.output_width > 1 {
            current_query & (placement.pair_width - 1)
        } else {
            0
        };
    }
    panic!("sibling opening {sibling} is outside the derived FRI schedule")
}

fn proof_word(domain: u32, index: usize, lane: usize) -> u32 {
    let mixed = u64::from(domain) * 1_000_003 + index as u64 * 65_537 + lane as u64 * 257 + 17;
    (mixed % u64::from(BABY_BEAR_MODULUS - 1)) as u32 + 1
}

fn reference_proof_arena() -> Vec<u32> {
    let evaluations = (0..FOLDED_EVALUATIONS)
        .flat_map(|index| (0..EVALUATION_WORDS).map(move |lane| proof_word(1, index, lane)));
    let tree = (0..LAYER_TREE_NODES)
        .flat_map(|index| (0..DIGEST_WORDS).map(move |lane| proof_word(2, index, lane)));
    let mut arena = evaluations.chain(tree).collect::<Vec<_>>();
    assert_eq!(arena.len(), PROOF_DATA_WORDS);
    arena.resize(PROOF_ARENA_WORDS, 0);
    arena
}

fn expected_evaluation_openings(queries: &[u32], rounds: &[RoundPlacement]) -> Vec<u32> {
    queries
        .iter()
        .flat_map(|query| {
            (0..EVALUATIONS_PER_QUERY).flat_map(move |opening| {
                let index = reference_evaluation_index(rounds, opening as u32, *query);
                (0..EVALUATION_WORDS).map(move |lane| proof_word(1, index, lane))
            })
        })
        .collect()
}

fn expected_sibling_openings(queries: &[u32], rounds: &[RoundPlacement]) -> Vec<u32> {
    queries
        .iter()
        .flat_map(|query| {
            (0..SIBLINGS_PER_QUERY).flat_map(move |sibling| {
                let node = reference_tree_node(rounds, sibling as u32, *query);
                (0..DIGEST_WORDS).map(move |lane| proof_word(2, node, lane))
            })
        })
        .collect()
}

fn expected_compact_openings(
    queries: &[u32],
    rounds: &[RoundPlacement],
) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
    let mut metadata = vec![0; COMPACT_METADATA_WORDS];
    let mut values = vec![0; EVALUATION_ITEMS * EVALUATION_WORDS];
    let mut siblings = vec![0; SIBLING_ITEMS * DIGEST_WORDS];

    for placement in rounds {
        let round = placement.index as usize;
        let value_slot = placement.query_value_offset as usize * QUERY_COUNT;
        let sibling_slot = placement.query_sibling_offset as usize * QUERY_COUNT;
        let leaf_index_start = COMPACT_LEAF_INDEX_START + value_slot;
        let value_word_start = value_slot * EVALUATION_WORDS;
        let sibling_word_start = sibling_slot * DIGEST_WORDS;
        metadata[COMPACT_VALID_START + round] = 1;

        if placement.output_width == 1 {
            metadata[COMPACT_LEAF_COUNT_START + round] = 1;
            for lane in 0..EVALUATION_WORDS {
                values[value_word_start + lane] =
                    proof_word(1, placement.output_offset as usize, lane);
            }
            continue;
        }

        let mut active = vec![false; placement.tree_nodes as usize];
        for query in queries {
            let local = query & (placement.pair_width - 1);
            active[local as usize] = true;
            active[(local + placement.pair_width) as usize] = true;
        }

        let mut leaf_count = 0;
        for leaf in 0..placement.output_width as usize {
            if active[leaf] {
                metadata[leaf_index_start + leaf_count] = leaf as u32;
                for lane in 0..EVALUATION_WORDS {
                    values[value_word_start + leaf_count * EVALUATION_WORDS + lane] =
                        proof_word(1, placement.output_offset as usize + leaf, lane);
                }
                leaf_count += 1;
            }
        }

        let mut sibling_count = 0;
        let mut width = placement.output_width as usize;
        let mut level_start = 0;
        let mut next_start = width;
        while width > 1 {
            for node in 0..width {
                if active[level_start + node] && !active[level_start + (node ^ 1)] {
                    for lane in 0..DIGEST_WORDS {
                        siblings[sibling_word_start + sibling_count * DIGEST_WORDS + lane] =
                            proof_word(
                                2,
                                placement.tree_offset as usize + level_start + (node ^ 1),
                                lane,
                            );
                    }
                    sibling_count += 1;
                }
            }
            let parents = width / 2;
            for parent in 0..parents {
                active[next_start + parent] =
                    active[level_start + 2 * parent] || active[level_start + 2 * parent + 1];
            }
            level_start = next_start;
            next_start += parents;
            width = parents;
        }
        assert!(active[level_start]);
        metadata[COMPACT_LEAF_COUNT_START + round] = leaf_count as u32;
        metadata[COMPACT_SIBLING_COUNT_START + round] = sibling_count as u32;
    }
    (metadata, values, siblings)
}

fn reference_permutation(input: [u32; POSEIDON_WIDTH]) -> [u32; POSEIDON_WIDTH] {
    let mut state = input.map(BabyBear::from_u32);
    default_babybear_poseidon2_16().permute_mut(&mut state);
    state.map(|value| value.as_canonical_u32())
}

fn reference_sponge(message: &[u32]) -> [u32; 8] {
    let mut state = [0u32; POSEIDON_WIDTH];
    for block in message.chunks(8) {
        state[..block.len()].copy_from_slice(block);
        state = reference_permutation(state);
    }
    state[..8].try_into().expect("eight digest fields")
}

fn reference_indexed_squeeze(tag: &[u8; 4], digest: &[u32; 8], index: u32) -> [u32; 4] {
    let mut message = vec![u32::from_be_bytes(*tag), 9];
    message.extend_from_slice(digest);
    message.push(index);
    reference_sponge(&message)[..4]
        .try_into()
        .expect("four extension coefficients")
}

struct ExecutablePass {
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    dispatch: [u32; 3],
    repeat: u32,
    auxiliary: Vec<(u32, String, WebBindingRole, wgpu::Buffer)>,
}

#[test]
fn production_policy_query_grid_is_derived_and_exact_on_webgpu() {
    let compile_started = Instant::now();
    let bundle = compile_query_grid();
    let compile_elapsed = compile_started.elapsed();
    assert_eq!(bundle.manifest.passes.len(), 6);
    let prepare_pass = &bundle.manifest.passes[0];
    let sample_pass = &bundle.manifest.passes[1];
    let evaluation_pass = &bundle.manifest.passes[2];
    let sibling_pass = &bundle.manifest.passes[3];
    let compact_pass = &bundle.manifest.passes[4];
    assert_eq!(prepare_pass.source_entry, "prepare_rounds");
    assert_eq!(prepare_pass.layout.workgroup_size, [THREADS, 1, 1]);
    assert_eq!(prepare_pass.dispatch, Some([1, 1, 1]));
    assert_eq!(prepare_pass.repeat, 1);
    assert_eq!(sample_pass.source_entry, "sample_queries");
    assert_eq!(sample_pass.layout.workgroup_size, [THREADS, 1, 1]);
    assert_eq!(sample_pass.dispatch, Some([QUERY_SAMPLE_GROUPS, 1, 1]));
    assert_eq!(sample_pass.repeat, QUERY_SAMPLE_STEPS);
    assert_eq!(evaluation_pass.source_entry, "open_evaluations");
    assert_eq!(evaluation_pass.layout.workgroup_size, [THREADS, 1, 1]);
    assert_eq!(evaluation_pass.dispatch, Some([EVALUATION_GROUPS, 1, 1]));
    assert_eq!(evaluation_pass.repeat, FRI_ROUNDS as u32);
    assert_eq!(sibling_pass.source_entry, "open_siblings");
    assert_eq!(sibling_pass.layout.workgroup_size, [THREADS, 1, 1]);
    assert_eq!(sibling_pass.dispatch, Some([SIBLING_GROUPS, 1, 1]));
    assert_eq!(sibling_pass.repeat, FRI_ROUNDS as u32);
    assert_eq!(compact_pass.source_entry, "compact_openings");
    assert_eq!(compact_pass.layout.workgroup_size, [THREADS, 1, 1]);
    assert_eq!(compact_pass.dispatch, Some([1, 1, 1]));
    assert_eq!(compact_pass.repeat, 1);

    for (pass, shader) in bundle.manifest.passes[..5]
        .iter()
        .zip(&bundle.pass_wgsl[..5])
    {
        let module = naga::front::wgsl::parse_str(&shader.source)
            .unwrap_or_else(|error| panic!("{} WGSL parse failed: {error:?}", pass.source_entry));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap_or_else(|error| {
            panic!(
                "{} WGSL browser validation failed: {error:?}",
                pass.source_entry
            )
        });
    }

    let Some((adapter, device, queue)) = request_browser_profile_device() else {
        return;
    };
    let adapter_name = adapter.get_info().name;
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
    assert_eq!(
        bundle.manifest.resources.len(),
        7,
        "the portable actor must stay within seven storage resources so a fail-closed trap fits \
         the browser-profile limit",
    );
    assert_eq!(
        bundle
            .manifest
            .resources
            .iter()
            .find(|resource| resource.name == "evaluation_openings")
            .map(|resource| resource.length),
        Some((EVALUATION_ITEMS * EVALUATION_WORDS) as u32),
    );
    assert_eq!(
        bundle
            .manifest
            .resources
            .iter()
            .find(|resource| resource.name == "sibling_openings")
            .map(|resource| resource.length),
        Some((SIBLING_ITEMS * DIGEST_WORDS) as u32),
    );
    let resource_length = |name: &str| {
        bundle
            .manifest
            .resources
            .iter()
            .find(|resource| resource.name == name)
            .map(|resource| resource.length)
    };
    assert_eq!(
        resource_length("query_workspace"),
        Some(QUERY_WORKSPACE_WORDS as u32),
    );
    assert_eq!(
        resource_length("query_control"),
        Some(QUERY_CONTROL_WORDS as u32),
    );
    assert_eq!(
        resource_length("round_placements"),
        Some(ROUND_PLACEMENT_WORDS as u32),
    );
    assert_eq!(
        resource_length("proof_arena"),
        Some(PROOF_ARENA_WORDS as u32),
    );
    assert_eq!(
        resource_length("opening_metadata"),
        Some(OPENING_METADATA_WORDS as u32),
    );

    let mut executable = Vec::new();
    for (pass, shader) in bundle.manifest.passes[..5]
        .iter()
        .zip(&bundle.pass_wgsl[..5])
    {
        assert!(
            pass.layout.bindings.iter().all(|binding| {
                binding.role == WebBindingRole::Resource
                    || (binding.role == WebBindingRole::Output && binding.name == "trap")
            }),
            "query-grid compute passes should need only actor resources and fail-closed traps",
        );
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
            label: Some(pass.source_entry.as_str()),
            entries: &layout_entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(pass.source_entry.as_str()),
            bind_group_layouts: &[Some(&bind_group_layout)],
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
        let auxiliary = pass
            .layout
            .bindings
            .iter()
            .filter(|binding| binding.role != WebBindingRole::Resource)
            .map(|binding| {
                (
                    binding.binding,
                    binding.name.clone(),
                    binding.role,
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(binding.name.as_str()),
                        size: u64::from(binding.span),
                        usage: wgpu::BufferUsages::STORAGE
                            | wgpu::BufferUsages::COPY_SRC
                            | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                )
            })
            .collect::<Vec<_>>();
        let entries = pass
            .layout
            .bindings
            .iter()
            .map(|binding| wgpu::BindGroupEntry {
                binding: binding.binding,
                resource: if binding.role == WebBindingRole::Resource {
                    resources
                        .get(&binding.name)
                        .unwrap_or_else(|| panic!("missing resource {}", binding.name))
                        .as_entire_binding()
                } else {
                    auxiliary
                        .iter()
                        .find(|(candidate, _, _, _)| *candidate == binding.binding)
                        .unwrap_or_else(|| panic!("missing auxiliary binding {}", binding.name))
                        .3
                        .as_entire_binding()
                },
            })
            .collect::<Vec<_>>();
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(pass.source_entry.as_str()),
            layout: &bind_group_layout,
            entries: &entries,
        });
        executable.push(ExecutablePass {
            pipeline,
            bind_group,
            dispatch: pass.dispatch.expect("fixed compute dispatch"),
            repeat: pass.repeat,
            auxiliary,
        });
    }

    let transcript = [3, 5, 8, 13, 21, 34, 55, 89];
    assert!(transcript.iter().all(|word| *word < BABY_BEAR_MODULUS));
    queue.write_buffer(
        &resources["query_control"],
        (QUERY_TRANSCRIPT_START * 4) as u64,
        &bytes(&transcript),
    );
    queue.write_buffer(
        &resources["query_control"],
        (QUERY_TRANSCRIPT_VALID_START * 4) as u64,
        &bytes(&[1]),
    );
    let proof_arena = reference_proof_arena();
    assert_eq!(proof_arena.len(), PROOF_ARENA_WORDS);
    queue.write_buffer(&resources["proof_arena"], 0, &bytes(&proof_arena));

    let execution_started = Instant::now();
    let mut raw_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("production query-grid raw opening execution"),
    });
    for pass in &executable[..4] {
        let mut compute = raw_encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Fe-derived production raw query grid"),
            timestamp_writes: None,
        });
        compute.set_pipeline(&pass.pipeline);
        compute.set_bind_group(0, &pass.bind_group, &[]);
        for _ in 0..pass.repeat {
            compute.dispatch_workgroups(pass.dispatch[0], pass.dispatch[1], pass.dispatch[2]);
        }
    }
    queue.submit(Some(raw_encoder.finish()));

    let query_workspace = read_words(
        &device,
        &queue,
        &resources["query_workspace"],
        QUERY_WORKSPACE_WORDS,
    );
    let query_control = read_words(
        &device,
        &queue,
        &resources["query_control"],
        QUERY_CONTROL_WORDS,
    );
    let query_indices = &query_control[QUERY_INDICES_START..QUERY_PADDING_START];
    let query_validity = &query_workspace[QUERY_VALIDITY_START..QUERY_PROGRESS_START];
    let query_progress = &query_workspace[QUERY_PROGRESS_START..QUERY_WORKSPACE_WORDS];
    let query_sample_padding = &query_control[QUERY_PADDING_START..QUERY_CONTROL_WORDS];
    let round_placements = read_words(
        &device,
        &queue,
        &resources["round_placements"],
        ROUND_PLACEMENT_WORDS,
    );
    let evaluation_openings = read_words(
        &device,
        &queue,
        &resources["evaluation_openings"],
        EVALUATION_ITEMS * EVALUATION_WORDS,
    );
    let sibling_openings = read_words(
        &device,
        &queue,
        &resources["sibling_openings"],
        SIBLING_ITEMS * DIGEST_WORDS,
    );

    let compact_pass = &executable[4];
    let mut compact_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("production query-grid compact opening execution"),
    });
    {
        let mut compute = compact_encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Fe-derived production compact query grid"),
            timestamp_writes: None,
        });
        compute.set_pipeline(&compact_pass.pipeline);
        compute.set_bind_group(0, &compact_pass.bind_group, &[]);
        for _ in 0..compact_pass.repeat {
            compute.dispatch_workgroups(
                compact_pass.dispatch[0],
                compact_pass.dispatch[1],
                compact_pass.dispatch[2],
            );
        }
    }
    queue.submit(Some(compact_encoder.finish()));

    let opening_metadata = read_words(
        &device,
        &queue,
        &resources["opening_metadata"],
        OPENING_METADATA_WORDS,
    );
    let compact_metadata = &opening_metadata[..COMPACT_METADATA_WORDS];
    let padding_receipts = &opening_metadata[OPENING_PADDING_START..];
    let compact_evaluations = read_words(
        &device,
        &queue,
        &resources["evaluation_openings"],
        EVALUATION_ITEMS * EVALUATION_WORDS,
    );
    let compact_siblings = read_words(
        &device,
        &queue,
        &resources["sibling_openings"],
        SIBLING_ITEMS * DIGEST_WORDS,
    );
    let trap_receipts = executable
        .iter()
        .map(|pass| {
            pass.auxiliary
                .iter()
                .find(|(_, name, role, _)| name == "trap" && *role == WebBindingRole::Output)
                .map(|(_, _, _, buffer)| {
                    read_words(
                        &device,
                        &queue,
                        buffer,
                        pass.dispatch[0] as usize * THREADS as usize,
                    )
                })
        })
        .collect::<Vec<_>>();
    let execution_elapsed = execution_started.elapsed();

    let expected_query_indices = (1..=QUERY_COUNT as u32)
        .map(|identity| {
            reference_indexed_squeeze(b"FQ02", &transcript, identity)[0] & QUERY_HALF_MASK
        })
        .collect::<Vec<_>>();
    assert_eq!(
        query_progress,
        vec![QUERY_SAMPLE_STEPS; QUERY_PROGRESS_WORDS],
    );
    assert_eq!(
        query_sample_padding,
        (QUERY_SAMPLE_WORK_ITEMS..QUERY_SAMPLE_GROUPS as usize * THREADS as usize)
            .map(|lane| lane as u32 + 1)
            .collect::<Vec<_>>(),
        "all padded sampler lanes must remain outside the cryptographic schedule",
    );
    for (pass, trap) in executable.iter().zip(&trap_receipts) {
        if let Some(trap) = trap {
            assert_eq!(
                trap,
                &vec![0; pass.dispatch[0] as usize * THREADS as usize],
                "all dynamic Fe accesses must remain in bounds for dispatch {:?}",
                pass.dispatch,
            );
        }
    }
    assert_eq!(query_validity, vec![1; QUERY_COUNT]);
    assert_eq!(
        query_indices, expected_query_indices,
        "all production Fiat-Shamir indices must match independent Plonky3 squeezes",
    );

    let expected_rounds = reference_round_placements();
    assert_eq!(
        round_placements,
        flattened_round_placements(&expected_rounds),
        "the device-resident FRI schedule must match the independent recurrence",
    );
    assert_eq!(
        evaluation_openings,
        expected_evaluation_openings(&expected_query_indices, &expected_rounds),
        "every production evaluation lane must extract its independently derived field value",
    );
    assert_eq!(
        sibling_openings,
        expected_sibling_openings(&expected_query_indices, &expected_rounds),
        "every production sibling lane must extract its independently derived digest",
    );
    let (expected_metadata, expected_compact_evaluations, expected_compact_siblings) =
        expected_compact_openings(&expected_query_indices, &expected_rounds);
    assert_eq!(
        compact_metadata,
        expected_metadata.as_slice(),
        "canonical compact counts and sorted leaf indices must match the independent traversal",
    );
    assert_eq!(
        compact_evaluations, expected_compact_evaluations,
        "canonical compact evaluation prefixes must match the independent traversal",
    );
    assert_eq!(
        compact_siblings, expected_compact_siblings,
        "canonical compact sibling frontiers must match the independent traversal",
    );
    let expected_padding = (EVALUATION_ITEMS..EVALUATION_GROUPS as usize * THREADS as usize)
        .chain(SIBLING_ITEMS..SIBLING_GROUPS as usize * THREADS as usize)
        .map(|lane| lane as u32 + 1)
        .collect::<Vec<_>>();
    assert_eq!(
        padding_receipts,
        expected_padding.as_slice(),
        "all padded GPU lanes must resolve to invalid work items",
    );

    eprintln!(
        "  production query grid: adapter={adapter_name:?}, compile={compile_elapsed:?}, \
         execute_and_read={execution_elapsed:?}, query_samples={QUERY_COUNT}, \
         evaluation_items={EVALUATION_ITEMS}, sibling_items={SIBLING_ITEMS}, wgsl_bytes={}",
        bundle.pass_wgsl[..5]
            .iter()
            .map(|shader| shader.source.len())
            .sum::<usize>(),
    );
}
