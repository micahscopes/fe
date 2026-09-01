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
