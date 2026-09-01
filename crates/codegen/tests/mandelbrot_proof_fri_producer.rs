//! Structural WebGPU gate for the production BabyBear FRI actor.
//!
//! Fe owns the complete thirteen-round protocol schedule. This test validates
//! the browser contract and every bounded phase shader. The separate browser
//! execution gate compares device-produced challenges, folded evaluations,
//! Merkle nodes, roots, and transcripts with an independent Plonky3 recurrence.

use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WebBuildOptions, WebBundle, WebBundleMode, WebDispatchTaper, resolve_web_entry};
use hir::hir_def::HirIngot;
use p3_baby_bear::{BabyBear, default_babybear_poseidon2_16};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_symmetric::Permutation;
use url::Url;

const THREADS: u32 = 64;
const FRI_ROUNDS: usize = 13;
const ROUND_PLACEMENT_WORDS: u32 = 169;
const ARENA_WORDS: u32 = 421_759;
const CONTROL_WORDS: u32 = 311;
const INPUT_GROUPS: u32 = 128;
const PROVER_GROUPS: u32 = 1_024;
const PAIR_GROUPS: u32 = 64;
const TREE_GROUPS: u32 = 512;
const CHALLENGE_STEPS: u32 = 89;
const LEAF_HASH_STEPS: u32 = 44;
const TREE_STEPS: u32 = 540;
const TREE_STEP_DECREMENT: u32 = 45;
const BINDING_STEPS: u32 = 133;
const TAPERED_WORKGROUPS: u64 = 605_295;
const PADDED_WORKGROUPS_PER_ROUND: u32 = 830_465;
const BABY_BEAR_MODULUS: u32 = 2_013_265_921;
const BABY_BEAR_TWO_ADICITY: u32 = 27;
const POSEIDON_WIDTH: usize = 16;
const EXTENSION_WORDS: usize = 4;
const DIGEST_WORDS: usize = 8;
const COMPOSITION_VALUES: usize = 8_192;
const COMPOSITION_WORDS: usize = COMPOSITION_VALUES * EXTENSION_WORDS;
const COMPOSITION_VALID: usize = COMPOSITION_WORDS;
const EVALUATIONS_START: usize = COMPOSITION_VALID + 1;
const FOLDED_EVALUATIONS: usize = COMPOSITION_VALUES - 1;
const FOLDED_EVALUATION_WORDS: usize = FOLDED_EVALUATIONS * EXTENSION_WORDS;
const EVALUATION_VALID_START: usize = EVALUATIONS_START + FOLDED_EVALUATION_WORDS;
const TREE_START: usize = EVALUATION_VALID_START + FOLDED_EVALUATIONS;
const LAYER_TREE_NODES: usize = 16_369;
const TREE_WORDS: usize = LAYER_TREE_NODES * DIGEST_WORDS;
const NODE_VALID_START: usize = TREE_START + TREE_WORDS;
const TRANSCRIPTS_START: usize = 0;
const TRANSCRIPT_WORDS: usize = (FRI_ROUNDS + 1) * DIGEST_WORDS;
const TRANSCRIPT_VALID_START: usize = TRANSCRIPTS_START + TRANSCRIPT_WORDS;
const CHALLENGES_START: usize = TRANSCRIPT_VALID_START + FRI_ROUNDS + 1;
const CHALLENGE_WORDS: usize = FRI_ROUNDS * EXTENSION_WORDS;
const CHALLENGE_VALID_START: usize = CHALLENGES_START + CHALLENGE_WORDS;
const ROOTS_START: usize = CHALLENGE_VALID_START + FRI_ROUNDS;
const ROOT_WORDS: usize = FRI_ROUNDS * DIGEST_WORDS;
const ROOT_VALID_START: usize = ROOTS_START + ROOT_WORDS;
const ROUND_CURSOR: usize = ROOT_VALID_START + FRI_ROUNDS;
const ROUND_VALID: usize = ROUND_CURSOR + 1;
const PROVER_COMPLETE: usize = ROUND_VALID + 1;
const BROWSER_RECEIPT_DIR: &str = "MB2_FRI_BROWSER_RECEIPT_DIR";
const EXTENSION_NONRESIDUE: u32 = 11;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repository root")
        .to_path_buf()
}

fn compile_fri_producer() -> WebBundle {
    let dir = repo_root().join("crates/codegen/tests/fixtures/mandelbrot_proof_fri_producer_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "FRI producer ingot initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("FRI producer fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "FRI producer source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("mandelbrot_proof_fri_producer".into())),
    )
    .expect("FRI producer fixture should compile into a WebBundle")
}

#[test]
fn production_fri_actor_schedule_lowers_to_browser_webgpu() {
    let bundle = compile_fri_producer();
    assert_eq!(bundle.manifest.passes.len(), 14);
    assert_eq!(bundle.manifest.resources.len(), 3);

    let prepare = &bundle.manifest.passes[0];
    assert_eq!(prepare.source_entry, "prepare_rounds");
    assert_eq!(prepare.layout.workgroup_size, [THREADS, 1, 1]);
    assert_eq!(prepare.dispatch, Some([1, 1, 1]));
    assert_eq!(prepare.repeat, 1);

    let seed = &bundle.manifest.passes[1];
    assert_eq!(seed.source_entry, "seed_prover_input");
    assert_eq!(seed.layout.workgroup_size, [THREADS, 1, 1]);
    assert_eq!(seed.dispatch, Some([INPUT_GROUPS, 1, 1]));
    assert_eq!(seed.repeat, 1);

    let initialize = &bundle.manifest.passes[2];
    assert_eq!(initialize.source_entry, "initialize_prover");
    assert_eq!(initialize.layout.workgroup_size, [THREADS, 1, 1]);
    assert_eq!(initialize.dispatch, Some([PROVER_GROUPS, 1, 1]));
    assert_eq!(initialize.repeat, 1);

    let phases = [
        (
            "begin_round",
            1,
            [PROVER_GROUPS, 1, 1],
            Some(WebDispatchTaper {
                shifts: [1, 0, 0],
                repeat_decrement: 0,
            }),
        ),
        ("derive_challenge", CHALLENGE_STEPS, [1, 1, 1], None),
        (
            "fold_round",
            1,
            [PAIR_GROUPS, 1, 1],
            Some(WebDispatchTaper {
                shifts: [1, 0, 0],
                repeat_decrement: 0,
            }),
        ),
        (
            "initialize_leaves",
            1,
            [PROVER_GROUPS, 1, 1],
            Some(WebDispatchTaper {
                shifts: [1, 0, 0],
                repeat_decrement: 0,
            }),
        ),
        (
            "hash_leaves",
            LEAF_HASH_STEPS,
            [PROVER_GROUPS, 1, 1],
            Some(WebDispatchTaper {
                shifts: [1, 0, 0],
                repeat_decrement: 0,
            }),
        ),
        (
            "begin_tree",
            1,
            [TREE_GROUPS, 1, 1],
            Some(WebDispatchTaper {
                shifts: [1, 0, 0],
                repeat_decrement: 0,
            }),
        ),
        (
            "reduce_tree",
            TREE_STEPS,
            [TREE_GROUPS, 1, 1],
            Some(WebDispatchTaper {
                shifts: [1, 0, 0],
                repeat_decrement: TREE_STEP_DECREMENT,
            }),
        ),
        ("begin_binding", 1, [1, 1, 1], None),
        ("bind_transcript", BINDING_STEPS, [1, 1, 1], None),
        ("finish_round", 1, [1, 1, 1], None),
    ];
    for (index, (entry, repeat, dispatch, taper)) in phases.iter().copied().enumerate() {
        let pass = &bundle.manifest.passes[index + 3];
        assert_eq!(pass.source_entry, entry);
        assert_eq!(pass.layout.workgroup_size, [THREADS, 1, 1]);
        assert_eq!(pass.dispatch, Some(dispatch));
        assert_eq!(pass.repeat, repeat);
        assert_eq!(pass.taper, taper);
        let cooperation = matches!(entry, "hash_leaves" | "reduce_tree")
            .then_some(fe_codegen::WebDispatchCooperation { repeat_batch: 8 });
        assert_eq!(pass.cooperation, cooperation);
        let cycle = pass
            .cycle
            .expect("every FRI phase belongs to one round cycle");
        assert_eq!(cycle.group, 0);
        assert_eq!(cycle.repeat, FRI_ROUNDS as u32);
    }
    assert_eq!(bundle.manifest.passes[0].cycle, None);
    assert_eq!(bundle.manifest.passes[1].cycle, None);
    assert_eq!(bundle.manifest.passes[2].cycle, None);
    assert_eq!(bundle.manifest.passes[13].source_entry, "paint");
    assert_eq!(bundle.manifest.passes[13].cycle, None);

    let mut derived_workgroups = 0_u64;
    for cycle_iteration in 0..FRI_ROUNDS as u32 {
        for (_, base_repeat, base_dispatch, taper) in phases {
            let (dispatch, repeat) = match taper {
                Some(taper) => {
                    let dispatch = std::array::from_fn(|axis| {
                        let exponent = cycle_iteration * taper.shifts[axis];
                        if exponent >= 31 {
                            1
                        } else {
                            base_dispatch[axis].div_ceil(1_u32 << exponent)
                        }
                    });
                    (
                        dispatch,
                        base_repeat - cycle_iteration * taper.repeat_decrement,
                    )
                }
                None => (base_dispatch, base_repeat),
            };
            derived_workgroups += u64::from(dispatch[0])
                * u64::from(dispatch[1])
                * u64::from(dispatch[2])
                * u64::from(repeat);
        }
    }
    assert_eq!(derived_workgroups, TAPERED_WORKGROUPS);
    assert!(derived_workgroups < u64::from(PADDED_WORKGROUPS_PER_ROUND) * FRI_ROUNDS as u64 / 17);

    for (pass, shader) in bundle.manifest.passes.iter().zip(&bundle.pass_wgsl) {
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

    let resource_length = |name: &str| {
        bundle
            .manifest
            .resources
            .iter()
            .find(|resource| resource.name == name)
            .map(|resource| resource.length)
    };
    assert_eq!(
        resource_length("round_placements"),
        Some(ROUND_PLACEMENT_WORDS)
    );
    assert_eq!(resource_length("arena"), Some(ARENA_WORDS));
    assert_eq!(resource_length("control"), Some(CONTROL_WORDS));
    assert_eq!(FRI_ROUNDS, 13);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Extension4([u32; EXTENSION_WORDS]);

impl Extension4 {
    fn add(self, other: Self) -> Self {
        Self(std::array::from_fn(|lane| {
            add_mod(self.0[lane], other.0[lane])
        }))
    }

    fn sub(self, other: Self) -> Self {
        Self(std::array::from_fn(|lane| {
            sub_mod(self.0[lane], other.0[lane])
        }))
    }

    fn scale(self, scalar: u32) -> Self {
        Self(self.0.map(|coefficient| mul_mod(coefficient, scalar)))
    }

    fn mul(self, other: Self) -> Self {
        let mut coefficients = [0_u32; 7];
        for left in 0..EXTENSION_WORDS {
            for right in 0..EXTENSION_WORDS {
                coefficients[left + right] = add_mod(
                    coefficients[left + right],
                    mul_mod(self.0[left], other.0[right]),
                );
            }
        }
        for degree in (EXTENSION_WORDS..coefficients.len()).rev() {
            coefficients[degree - EXTENSION_WORDS] = add_mod(
                coefficients[degree - EXTENSION_WORDS],
                mul_mod(coefficients[degree], EXTENSION_NONRESIDUE),
            );
        }
        Self(
            coefficients[..EXTENSION_WORDS]
                .try_into()
                .expect("four extension coefficients"),
        )
    }
}

#[derive(Debug)]
struct IndependentFriReceipt {
    round_placements: Vec<u32>,
    composition: Vec<u32>,
    evaluations: Vec<u32>,
    evaluation_validity: Vec<u32>,
    tree: Vec<u32>,
    node_validity: Vec<u32>,
    control: Vec<u32>,
}

fn add_mod(left: u32, right: u32) -> u32 {
    ((u64::from(left) + u64::from(right)) % u64::from(BABY_BEAR_MODULUS)) as u32
}

fn sub_mod(left: u32, right: u32) -> u32 {
    ((u64::from(left) + u64::from(BABY_BEAR_MODULUS) - u64::from(right))
        % u64::from(BABY_BEAR_MODULUS)) as u32
}

fn mul_mod(left: u32, right: u32) -> u32 {
    (u64::from(left) * u64::from(right) % u64::from(BABY_BEAR_MODULUS)) as u32
}

fn pow_mod(mut base: u64, mut exponent: u32) -> u32 {
    let modulus = u64::from(BABY_BEAR_MODULUS);
    base %= modulus;
    let mut result = 1_u64;
    while exponent != 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus;
        }
        base = base * base % modulus;
        exponent >>= 1;
    }
    result as u32
}

fn reference_permutation(input: [u32; POSEIDON_WIDTH]) -> [u32; POSEIDON_WIDTH] {
    let mut state = input.map(BabyBear::from_u32);
    default_babybear_poseidon2_16().permute_mut(&mut state);
    state.map(|value| value.as_canonical_u32())
}

fn reference_field_commitment(tag: [u8; 4], fields: &[u32]) -> [u32; DIGEST_WORDS] {
    let mut message = Vec::with_capacity(fields.len() + 2);
    message.push(u32::from_be_bytes(tag));
    message.push(fields.len() as u32);
    message.extend_from_slice(fields);
    let mut state = [0_u32; POSEIDON_WIDTH];
    for block in message.chunks(DIGEST_WORDS) {
        state[..block.len()].copy_from_slice(block);
        state = reference_permutation(state);
    }
    state[..DIGEST_WORDS]
        .try_into()
        .expect("eight digest fields")
}

fn reference_compress(
    left: [u32; DIGEST_WORDS],
    right: [u32; DIGEST_WORDS],
) -> [u32; DIGEST_WORDS] {
    let mut state = [0_u32; POSEIDON_WIDTH];
    state[..DIGEST_WORDS].copy_from_slice(&left);
    state[DIGEST_WORDS..].copy_from_slice(&right);
    reference_permutation(state)[..DIGEST_WORDS]
        .try_into()
        .expect("eight digest fields")
}

fn protocol_round_tag(prefix: [u8; 2], round: usize) -> [u8; 4] {
    assert!((1..100).contains(&round));
    [
        prefix[0],
        prefix[1],
        b'0' + (round / 10) as u8,
        b'0' + (round % 10) as u8,
    ]
}

fn reference_fold_pair(
    positive: Extension4,
    negative: Extension4,
    challenge: Extension4,
    point: u32,
) -> Extension4 {
    let inverse_two = pow_mod(2, BABY_BEAR_MODULUS - 2);
    let inverse_point = pow_mod(u64::from(point), BABY_BEAR_MODULUS - 2);
    let even = positive.add(negative).scale(inverse_two);
    let odd = positive
        .sub(negative)
        .scale(inverse_two)
        .scale(inverse_point);
    challenge.mul(odd).add(even)
}

fn reference_round_placements() -> Vec<u32> {
    let mut output = Vec::with_capacity(ROUND_PLACEMENT_WORDS as usize);
    let mut output_offset = 0_u32;
    let mut tree_offset = 0_u32;
    let mut query_value_offset = 0_u32;
    let mut query_sibling_offset = 0_u32;
    for index in 0..FRI_ROUNDS as u32 {
        let input_log = FRI_ROUNDS as u32 - index;
        let input_width = 1_u32 << input_log;
        let output_width = input_width / 2;
        let tree_nodes = 2 * output_width - 1;
        let pair_width = if input_log > 1 {
            1_u32 << (input_log - 2)
        } else {
            0
        };
        let pair_depth = input_log.saturating_sub(2);
        output.extend([
            1,
            index,
            index + 1,
            input_log,
            input_width,
            output_width,
            output_offset,
            tree_offset,
            tree_nodes,
            query_value_offset,
            query_sibling_offset,
            pair_width,
            pair_depth,
        ]);
        output_offset += output_width;
        tree_offset += tree_nodes;
        query_value_offset += if input_log > 1 { 2 } else { 1 };
        query_sibling_offset += 2 * pair_depth;
    }
    assert_eq!(output.len(), ROUND_PLACEMENT_WORDS as usize);
    assert_eq!(output_offset as usize, FOLDED_EVALUATIONS);
    assert_eq!(tree_offset as usize, LAYER_TREE_NODES);
    output
}

fn independent_fri_receipt() -> IndependentFriReceipt {
    let composition_values = (0..COMPOSITION_VALUES as u32)
        .map(|lane| Extension4([lane + 1, lane * 3 + 5, lane * 7 + 11, lane * 13 + 17]))
        .collect::<Vec<_>>();
    let composition = composition_values
        .iter()
        .flat_map(|value| value.0)
        .collect::<Vec<_>>();
    let mut current = composition_values;
    let mut transcript = std::array::from_fn(|lane| lane as u32 * 17 + 5);
    let mut transcripts = Vec::with_capacity(TRANSCRIPT_WORDS);
    let mut challenges = Vec::with_capacity(CHALLENGE_WORDS);
    let mut roots = Vec::with_capacity(ROOT_WORDS);
    let mut evaluations = Vec::with_capacity(FOLDED_EVALUATION_WORDS);
    let mut tree = Vec::with_capacity(TREE_WORDS);
    let maximal_root = pow_mod(31, 15);
    let mut shift = 7_u32;
    transcripts.extend(transcript);

    for round_index in 0..FRI_ROUNDS {
        let round = round_index + 1;
        let challenge_digest =
            reference_field_commitment(protocol_round_tag(*b"FC", round), &transcript);
        let challenge = Extension4(
            challenge_digest[..EXTENSION_WORDS]
                .try_into()
                .expect("quartic challenge"),
        );
        challenges.extend(challenge.0);

        let output_width = current.len() / 2;
        let root = pow_mod(
            u64::from(maximal_root),
            1 << (BABY_BEAR_TWO_ADICITY - current.len().ilog2()),
        );
        let folded = (0..output_width)
            .map(|pair| {
                let point = mul_mod(shift, pow_mod(u64::from(root), pair as u32));
                reference_fold_pair(
                    current[pair],
                    current[pair + output_width],
                    challenge,
                    point,
                )
            })
            .collect::<Vec<_>>();
        evaluations.extend(folded.iter().flat_map(|value| value.0));

        let row_tag = protocol_round_tag(*b"FR", round);
        let mut level = folded
            .iter()
            .map(|value| reference_field_commitment(row_tag, &value.0))
            .collect::<Vec<_>>();
        tree.extend(level.iter().flatten().copied());
        while level.len() > 1 {
            level = level
                .chunks_exact(2)
                .map(|children| reference_compress(children[0], children[1]))
                .collect();
            tree.extend(level.iter().flatten().copied());
        }
        let layer_root = level[0];
        roots.extend(layer_root);
        let mut binding = Vec::with_capacity(DIGEST_WORDS * 2);
        binding.extend(transcript);
        binding.extend(layer_root);
        transcript = reference_field_commitment(protocol_round_tag(*b"FT", round), &binding);
        transcripts.extend(transcript);
        current = folded;
        shift = mul_mod(shift, shift);
    }

    assert_eq!(current.len(), 1);
    assert_eq!(composition.len(), COMPOSITION_WORDS);
    assert_eq!(evaluations.len(), FOLDED_EVALUATION_WORDS);
    assert_eq!(tree.len(), TREE_WORDS);
    assert_eq!(transcripts.len(), TRANSCRIPT_WORDS);
    assert_eq!(challenges.len(), CHALLENGE_WORDS);
    assert_eq!(roots.len(), ROOT_WORDS);

    let mut control = vec![0_u32; CONTROL_WORDS as usize];
    control[TRANSCRIPTS_START..TRANSCRIPT_VALID_START].copy_from_slice(&transcripts);
    control[TRANSCRIPT_VALID_START..CHALLENGES_START].fill(1);
    control[CHALLENGES_START..CHALLENGE_VALID_START].copy_from_slice(&challenges);
    control[CHALLENGE_VALID_START..ROOTS_START].fill(1);
    control[ROOTS_START..ROOT_VALID_START].copy_from_slice(&roots);
    control[ROOT_VALID_START..ROUND_CURSOR].fill(1);
    control[ROUND_CURSOR] = FRI_ROUNDS as u32;
    control[ROUND_VALID] = 0;
    control[PROVER_COMPLETE] = 1;

    IndependentFriReceipt {
        round_placements: reference_round_placements(),
        composition,
        evaluations,
        evaluation_validity: vec![1; FOLDED_EVALUATIONS],
        tree,
        node_validity: vec![1; LAYER_TREE_NODES],
        control,
    }
}

fn read_u32le(path: &Path) -> Vec<u32> {
    let bytes = std::fs::read(path)
        .unwrap_or_else(|error| panic!("read browser receipt {}: {error}", path.display()));
    assert_eq!(
        bytes.len() % 4,
        0,
        "browser receipt {} is not a u32 tape",
        path.display(),
    );
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one little-endian u32")))
        .collect()
}

fn assert_words_eq(label: &str, actual: &[u32], expected: &[u32]) {
    assert_eq!(actual.len(), expected.len(), "{label} word count differs");
    if let Some((index, (actual, expected))) = actual
        .iter()
        .zip(expected)
        .enumerate()
        .find(|(_, (actual, expected))| actual != expected)
    {
        panic!("{label} differs at word {index}: browser={actual}, independent={expected}");
    }
}

#[test]
fn independent_production_fri_reference_has_derived_geometry() {
    let receipt = independent_fri_receipt();
    assert_eq!(
        receipt.round_placements.len(),
        ROUND_PLACEMENT_WORDS as usize
    );
    assert_eq!(receipt.composition.len(), COMPOSITION_WORDS);
    assert_eq!(receipt.evaluations.len(), FOLDED_EVALUATION_WORDS);
    assert_eq!(receipt.evaluation_validity, vec![1; FOLDED_EVALUATIONS]);
    assert_eq!(receipt.tree.len(), TREE_WORDS);
    assert_eq!(receipt.node_validity, vec![1; LAYER_TREE_NODES]);
    assert_eq!(receipt.control.len(), CONTROL_WORDS as usize);
    assert_eq!(receipt.control[ROUND_CURSOR], FRI_ROUNDS as u32);
    assert_eq!(receipt.control[ROUND_VALID], 0);
    assert_eq!(receipt.control[PROVER_COMPLETE], 1);
    assert!(
        receipt
            .composition
            .iter()
            .chain(&receipt.evaluations)
            .chain(&receipt.tree)
            .chain(&receipt.control[..ROUND_CURSOR])
            .all(|word| *word < BABY_BEAR_MODULUS),
        "the independent receipt must remain canonically field encoded",
    );
}

#[test]
#[ignore = "requires an explicit real-Chrome resource receipt"]
fn production_fri_browser_buffers_match_independent_plonky3_recurrence() {
    assert!(
        !cfg!(debug_assertions),
        "run the production browser receipt gate with --release"
    );
    let receipt_dir = std::env::var_os(BROWSER_RECEIPT_DIR)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!(
                "set {BROWSER_RECEIPT_DIR} to the directory emitted by the generic browser resource snapshot"
            )
        });
    let placements = read_u32le(&receipt_dir.join("round_placements.u32le"));
    let arena = read_u32le(&receipt_dir.join("arena.u32le"));
    let control = read_u32le(&receipt_dir.join("control.u32le"));
    assert_eq!(placements.len(), ROUND_PLACEMENT_WORDS as usize);
    assert_eq!(arena.len(), ARENA_WORDS as usize);
    assert_eq!(control.len(), CONTROL_WORDS as usize);

    let expected = independent_fri_receipt();
    assert_words_eq(
        "schedule-derived round placements",
        &placements,
        &expected.round_placements,
    );
    assert_words_eq(
        "Fe-seeded composition codeword",
        &arena[..COMPOSITION_VALID],
        &expected.composition,
    );
    assert_eq!(arena[COMPOSITION_VALID], 1);
    assert_words_eq(
        "all thirteen quartic fold layers",
        &arena[EVALUATIONS_START..EVALUATION_VALID_START],
        &expected.evaluations,
    );
    assert_words_eq(
        "folded-evaluation validity",
        &arena[EVALUATION_VALID_START..TREE_START],
        &expected.evaluation_validity,
    );
    assert_words_eq(
        "all ordered Poseidon2 Merkle nodes",
        &arena[TREE_START..NODE_VALID_START],
        &expected.tree,
    );
    assert_words_eq(
        "Merkle-node validity",
        &arena[NODE_VALID_START..NODE_VALID_START + LAYER_TREE_NODES],
        &expected.node_validity,
    );
    assert_words_eq(
        "all challenges, roots, transcripts, and actor completion state",
        &control,
        &expected.control,
    );
}
