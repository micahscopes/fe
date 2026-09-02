use std::collections::BTreeSet;
use std::path::Path;
use std::sync::OnceLock;

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{layout_for, BackendKind, OptLevel};
use hir::hir_def::HirIngot;
use quilting_core::patch::QBTriPatch;
use quilting_core::permutation::{perm_sign, S3_PERMUTATIONS};
use quilting_core::quaternion::Quat;
use url::Url;
use wasmtime::{Instance, Store, TypedFunc};

static ORACLE_WASM: OnceLock<Vec<u8>> = OnceLock::new();

fn compile_oracle_gate() -> &'static [u8] {
    ORACLE_WASM.get_or_init(|| {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ingots/classic_quilting_oracle");
        let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
        let mut db = DriverDataBase::default();
        assert!(
            !driver::init_ingot(&mut db, &url),
            "classic Quilting oracle ingot initialization diagnostics"
        );
        let ingot = db
            .workspace()
            .containing_ingot(&db, url)
            .expect("classic Quilting oracle ingot");
        let top_mod = ingot.root_mod(&db);
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
        assert!(
            diagnostics.is_empty(),
            "unexpected classic Quilting diagnostics:\n{diagnostics}"
        );
        let wasm = BackendKind::Wasm
            .create()
            .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
            .expect("classic Quilting oracle should compile to Wasm")
            .into_bytecode()
            .expect("Wasm output should be bytecode");
        wasmparser::validate(&wasm).expect("classic Quilting oracle Wasm should validate");
        wasm
    })
}

fn instantiate() -> (Store<()>, Instance) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, compile_oracle_gate())
        .expect("load classic Quilting oracle Wasm");
    assert!(
        module.imports().next().is_none(),
        "pure M1 Fe oracle must be self-contained"
    );
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).expect("instantiate M1 oracle");
    (store, instance)
}

fn function<P, R>(store: &mut Store<()>, instance: &Instance, name: &str) -> TypedFunc<P, R>
where
    P: wasmtime::WasmParams,
    R: wasmtime::WasmResults,
{
    instance
        .get_typed_func::<P, R>(store, name)
        .unwrap_or_else(|error| panic!("missing {name}: {error}"))
}

fn call2(store: &mut Store<()>, instance: &Instance, name: &str, a: f32, b: f32) -> f32 {
    function::<(f32, f32), f32>(store, instance, name)
        .call(store, (a, b))
        .unwrap()
}

fn call_u32(store: &mut Store<()>, instance: &Instance, name: &str, value: u32) -> u32 {
    function::<u32, u32>(store, instance, name)
        .call(store, value)
        .unwrap()
}

fn call2_u32(store: &mut Store<()>, instance: &Instance, name: &str, values: [u32; 2]) -> u32 {
    let [a, b] = values;
    function::<(u32, u32), u32>(store, instance, name)
        .call(store, (a, b))
        .unwrap()
}

fn call4_u32(store: &mut Store<()>, instance: &Instance, name: &str, values: [u32; 4]) -> u32 {
    let [a, b, c, d] = values;
    function::<(u32, u32, u32, u32), u32>(store, instance, name)
        .call(store, (a, b, c, d))
        .unwrap()
}

#[allow(clippy::many_single_char_names)]
fn call5_u32(store: &mut Store<()>, instance: &Instance, name: &str, values: [u32; 5]) -> u32 {
    let [a, b, c, d, e] = values;
    function::<(u32, u32, u32, u32, u32), u32>(store, instance, name)
        .call(store, (a, b, c, d, e))
        .unwrap()
}

#[allow(clippy::many_single_char_names)]
fn call6_u32(store: &mut Store<()>, instance: &Instance, name: &str, values: [u32; 6]) -> u32 {
    let [a, b, c, d, e, f] = values;
    function::<(u32, u32, u32, u32, u32, u32), u32>(store, instance, name)
        .call(store, (a, b, c, d, e, f))
        .unwrap()
}

#[allow(clippy::many_single_char_names)]
fn call8_u32(store: &mut Store<()>, instance: &Instance, name: &str, values: [u32; 8]) -> u32 {
    let [a, b, c, d, e, f, g, h] = values;
    function::<(u32, u32, u32, u32, u32, u32, u32, u32), u32>(store, instance, name)
        .call(store, (a, b, c, d, e, f, g, h))
        .unwrap()
}

fn canonical_lod_keys() -> Vec<[u32; 3]> {
    let mut keys = Vec::new();
    for a in 0..=8 {
        for b in a..=8 {
            for c in b..=8 {
                keys.push([a, b, c]);
            }
        }
    }
    keys
}

fn mix32_oracle(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^ (value >> 16)
}

fn radius_squared_oracle(density_exponent_q8: u32) -> u32 {
    let doubled_exponent = density_exponent_q8 * 2;
    let integer_shift = doubled_exponent / 256;
    let fraction = doubled_exponent % 256;
    let mut value = 65_536_u32;
    for (bit, factor) in [
        (1, 65_359),
        (2, 65_182),
        (4, 64_830),
        (8, 64_132),
        (16, 62_757),
        (32, 60_097),
        (64, 55_109),
        (128, 46_341),
    ] {
        if fraction & bit != 0 {
            value = (value * factor + 32_768) >> 16;
        }
    }
    (value << 12) >> integer_shift
}

fn density_exponent_q8_oracle(key: [u32; 3], point: [u32; 3]) -> u32 {
    let [a, b, c] = point.map(u64::from);
    let weights = [b * c, a * c, a * b];
    let sum = weights.iter().sum::<u64>();
    if sum == 0 {
        return key[2] * 256;
    }
    let weighted = weights
        .into_iter()
        .zip(key.map(u64::from))
        .map(|(weight, lod)| weight * lod)
        .sum::<u64>();
    u32::try_from(weighted * 256 / sum).unwrap()
}

fn continuous_density_exponent(key: [u32; 3], point: [u32; 3]) -> f64 {
    let [a, b, c] = point.map(f64::from);
    let weights = [b * c, a * c, a * b];
    let sum = weights.iter().sum::<f64>();
    if sum == 0.0 {
        return f64::from(key[2]);
    }
    weights
        .into_iter()
        .zip(key.map(f64::from))
        .map(|(weight, lod)| weight * lod)
        .sum::<f64>()
        / sum
}

#[test]
fn quilting_atlas_ctfe_plan_matches_an_independent_rust_schedule() {
    let (mut store, instance) = instantiate();
    let keys = canonical_lod_keys();
    assert_eq!(keys.len(), 165);

    let mut estimated = Vec::with_capacity(keys.len());
    for (ordinal, [a, b, c]) in keys.iter().copied().enumerate() {
        let ordinal = u32::try_from(ordinal).unwrap();
        assert_eq!(call_u32(&mut store, &instance, "atlas_key_a", ordinal), a);
        assert_eq!(call_u32(&mut store, &instance, "atlas_key_b", ordinal), b);
        assert_eq!(call_u32(&mut store, &instance, "atlas_key_c", ordinal), c);

        let resolutions = [1_u32 << a, 1_u32 << b, 1_u32 << c];
        let boundary = resolutions.iter().sum::<u32>();
        assert_eq!(
            call_u32(&mut store, &instance, "atlas_boundary_vertices", ordinal),
            boundary
        );

        let vertex_capacity =
            (1654 * resolutions[2] * resolutions[2] + 3308 * resolutions[2] + 999) / 1000 + 1;
        let triangle_capacity = (2 * vertex_capacity).saturating_sub(boundary + 2);
        assert_eq!(
            call_u32(&mut store, &instance, "atlas_vertex_capacity", ordinal),
            vertex_capacity
        );
        assert_eq!(
            call_u32(&mut store, &instance, "atlas_triangle_capacity", ordinal,),
            triangle_capacity
        );

        let work = resolutions[0] * resolutions[1]
            + resolutions[1] * resolutions[2]
            + resolutions[2] * resolutions[0];
        assert_eq!(
            call_u32(&mut store, &instance, "atlas_estimated_work", ordinal),
            work
        );
        estimated.push(work);

        let root = [0, b - a, c - a];
        let root_ordinal =
            u32::try_from(keys.iter().position(|key| *key == root).unwrap()).unwrap();
        assert_eq!(
            call_u32(&mut store, &instance, "atlas_root_ordinal", ordinal),
            root_ordinal
        );
        let expected_parent = if a == 0 {
            u32::MAX
        } else {
            let parent = [a - 1, b - 1, c - 1];
            u32::try_from(keys.iter().position(|key| *key == parent).unwrap()).unwrap()
        };
        assert_eq!(
            call_u32(&mut store, &instance, "atlas_parent_ordinal", ordinal),
            expected_parent
        );
    }

    let expected_order: Vec<usize> = (0..keys.len()).rev().collect();
    let mut expected_lane_for_job = vec![0_u32; keys.len()];
    let mut expected_loads = [0_u32; 16];
    for (rank, &ordinal) in expected_order.iter().enumerate() {
        assert_eq!(
            call_u32(
                &mut store,
                &instance,
                "atlas_schedule_order",
                u32::try_from(rank).unwrap(),
            ),
            u32::try_from(ordinal).unwrap()
        );
        let lane = expected_loads
            .iter()
            .enumerate()
            .min_by_key(|&(lane, load)| (*load, lane))
            .map(|(lane, _)| lane)
            .unwrap();
        expected_lane_for_job[ordinal] = u32::try_from(lane).unwrap();
        expected_loads[lane] += estimated[ordinal];
    }
    for (ordinal, expected_lane) in expected_lane_for_job.into_iter().enumerate() {
        assert_eq!(
            call_u32(
                &mut store,
                &instance,
                "atlas_schedule_lane",
                u32::try_from(ordinal).unwrap(),
            ),
            expected_lane
        );
    }
    for (lane, expected_load) in expected_loads.into_iter().enumerate() {
        assert_eq!(
            call_u32(
                &mut store,
                &instance,
                "atlas_schedule_lane_load",
                u32::try_from(lane).unwrap(),
            ),
            expected_load
        );
    }
    assert_eq!(
        function::<(), u32>(&mut store, &instance, "atlas_schedule_maximum_lane_load")
            .call(&mut store, ())
            .unwrap(),
        expected_loads.into_iter().max().unwrap()
    );

    let resolution_keys: Vec<crate::AtlasKey> = keys
        .iter()
        .map(|[a, b, c]| crate::AtlasKey::new(1_u32 << a, 1_u32 << b, 1_u32 << c))
        .collect();
    let direct = crate::quilting_export::build_direct_fixture_artifact(&resolution_keys, 16)
        .expect("exhaustive direct Rust atlas");
    let actual: Vec<u64> = direct
        .patches
        .iter()
        .map(|patch| u64::from(patch.vertex_count) + u64::from(patch.triangle_count))
        .collect();
    let mut scheduled_actual_loads = [0_u64; 16];
    let mut round_robin_loads = [0_u64; 16];
    for (ordinal, work) in actual.iter().copied().enumerate() {
        let lane = call_u32(
            &mut store,
            &instance,
            "atlas_schedule_lane",
            u32::try_from(ordinal).unwrap(),
        );
        scheduled_actual_loads[usize::try_from(lane).unwrap()] += work;
        round_robin_loads[ordinal % 16] += work;
    }
    let mut actual_order: Vec<usize> = (0..actual.len()).collect();
    actual_order.sort_by_key(|&ordinal| (std::cmp::Reverse(actual[ordinal]), ordinal));
    let mut oracle_actual_loads = [0_u64; 16];
    for ordinal in actual_order {
        let lane = oracle_actual_loads
            .iter()
            .enumerate()
            .min_by_key(|&(lane, load)| (*load, lane))
            .map(|(lane, _)| lane)
            .unwrap();
        oracle_actual_loads[lane] += actual[ordinal];
    }
    let scheduled_maximum = scheduled_actual_loads.into_iter().max().unwrap();
    let round_robin_maximum = round_robin_loads.into_iter().max().unwrap();
    let oracle_maximum = oracle_actual_loads.into_iter().max().unwrap();
    assert_eq!(scheduled_maximum, oracle_maximum);
    assert!(scheduled_maximum < round_robin_maximum);

    for ([a, b, c], expected_permutation) in [
        ([0_u32, 1, 2], 0_u32),
        ([0, 2, 1], 1),
        ([1, 0, 2], 2),
        ([2, 0, 1], 3),
        ([1, 2, 0], 4),
        ([2, 1, 0], 5),
    ] {
        let mut expected = [a, b, c];
        expected.sort_unstable();
        assert_eq!(
            function::<(u32, u32, u32), u32>(&mut store, &instance, "atlas_canonical_a")
                .call(&mut store, (a, b, c))
                .unwrap(),
            expected[0]
        );
        assert_eq!(
            function::<(u32, u32, u32), u32>(&mut store, &instance, "atlas_canonical_b")
                .call(&mut store, (a, b, c))
                .unwrap(),
            expected[1]
        );
        assert_eq!(
            function::<(u32, u32, u32), u32>(&mut store, &instance, "atlas_canonical_c")
                .call(&mut store, (a, b, c))
                .unwrap(),
            expected[2]
        );
        assert_eq!(
            function::<(u32, u32, u32), u32>(&mut store, &instance, "atlas_permutation")
                .call(&mut store, (a, b, c))
                .unwrap(),
            expected_permutation
        );
    }
}

#[test]
fn quilting_atlas_fe_sampler_matches_independent_integer_oracles() {
    const SCALE: u32 = 16_384;
    const SEED: u32 = 0x51c3_2a97;

    let (mut store, instance) = instantiate();
    let keys = canonical_lod_keys();

    for value in [0, 1, 42, u32::MAX, 0x8000_0000, 0xdead_beef] {
        assert_eq!(
            call_u32(&mut store, &instance, "atlas_mix32", value),
            mix32_oracle(value)
        );
    }

    for (patch, key) in keys.iter().copied().enumerate() {
        let patch = u32::try_from(patch).unwrap();
        let resolutions = key.map(|lod| 1_u32 << lod);
        let boundary_count = resolutions.iter().sum::<u32>();
        let mut unique_boundary = BTreeSet::new();

        for boundary in 0..boundary_count {
            let values = [patch, boundary];
            let actual = [
                call2_u32(&mut store, &instance, "atlas_boundary_a", values),
                call2_u32(&mut store, &instance, "atlas_boundary_b", values),
                call2_u32(&mut store, &instance, "atlas_boundary_c", values),
            ];
            let (expected, edge, step) = if boundary <= resolutions[2] {
                let step = boundary;
                let distance = step * (SCALE / resolutions[2]);
                ([SCALE - distance, distance, 0], 2, step)
            } else {
                let after_ab = boundary - resolutions[2] - 1;
                if after_ab < resolutions[1] {
                    let step = after_ab + 1;
                    let distance = step * (SCALE / resolutions[1]);
                    ([SCALE - distance, 0, distance], 1, step)
                } else {
                    let step = after_ab - resolutions[1] + 1;
                    let distance = step * (SCALE / resolutions[0]);
                    ([0, SCALE - distance, distance], 0, step)
                }
            };
            assert_eq!(actual, expected, "patch={patch} boundary={boundary}");
            assert_eq!(
                call2_u32(&mut store, &instance, "atlas_boundary_edge", values),
                edge
            );
            assert_eq!(
                call2_u32(&mut store, &instance, "atlas_boundary_step", values),
                step
            );
            assert_eq!(
                call2_u32(&mut store, &instance, "atlas_boundary_valid", values),
                1
            );
            assert!(
                unique_boundary.insert(actual),
                "duplicate patch={patch} boundary={boundary} point={actual:?}"
            );
        }
        assert_eq!(
            unique_boundary.len(),
            usize::try_from(boundary_count).unwrap()
        );
        assert_eq!(
            call2_u32(
                &mut store,
                &instance,
                "atlas_boundary_valid",
                [patch, boundary_count],
            ),
            0
        );

        let density_points = [
            [SCALE, 0, 0],
            [0, SCALE, 0],
            [0, 0, SCALE],
            [SCALE / 2, SCALE / 4, SCALE / 4],
            [5_462, 5_461, 5_461],
            [1, SCALE / 2 - 1, SCALE / 2],
        ];
        for point in density_points {
            let arguments = [patch, point[0], point[1], point[2]];
            let expected_exponent = density_exponent_q8_oracle(key, point);
            let actual_exponent = call4_u32(
                &mut store,
                &instance,
                "atlas_density_exponent_q8",
                arguments,
            );
            assert_eq!(actual_exponent, expected_exponent);

            let actual_radius = call4_u32(
                &mut store,
                &instance,
                "atlas_poisson_radius_squared",
                arguments,
            );
            assert_eq!(actual_radius, radius_squared_oracle(expected_exponent));

            let continuous_exponent = continuous_density_exponent(key, point);
            let continuous_radius =
                f64::from(SCALE).powi(2) * 2.0_f64.powf(-2.0 * continuous_exponent);
            let relative_error =
                (f64::from(actual_radius) - continuous_radius).abs() / continuous_radius;
            assert!(
                relative_error < 0.006,
                "patch={patch} point={point:?} radius={actual_radius} expected={continuous_radius} relative_error={relative_error}"
            );
        }

        let side = resolutions[2] * 2;
        let cell_count = side * (side + 1) / 2;
        let slot_count = cell_count * 2;
        assert_eq!(
            call_u32(&mut store, &instance, "atlas_candidate_grid_side", patch),
            side
        );
        assert_eq!(
            call_u32(&mut store, &instance, "atlas_candidate_cell_count", patch),
            cell_count
        );
        assert_eq!(
            call_u32(&mut store, &instance, "atlas_candidate_slot_count", patch),
            slot_count
        );

        for cell_b in [0, side / 2, side - 1] {
            let row_cells = side - cell_b;
            for cell_c in [0, row_cells / 2, row_cells - 1] {
                for trial in 0..2 {
                    let arguments = [patch, SEED, cell_b, cell_c, trial];
                    let cell = cell_b * (side + side - cell_b + 1) / 2 + cell_c;
                    let slot = cell * 2 + trial;
                    let priority = mix32_oracle(
                        SEED ^ patch.wrapping_mul(0x9e37_79b9) ^ slot.wrapping_mul(0x85eb_ca6b),
                    );
                    let second_hash = mix32_oracle(priority ^ 0xa511_e9b3);
                    let cell_width = SCALE / side;
                    let mut jitter_b = priority % cell_width;
                    let mut jitter_c = second_hash % cell_width;
                    if jitter_b + jitter_c >= cell_width {
                        jitter_b = cell_width - 1 - jitter_b;
                        jitter_c = cell_width - 1 - jitter_c;
                    }
                    let b = cell_b * cell_width + jitter_b;
                    let c = cell_c * cell_width + jitter_c;
                    let expected_point = [SCALE - b - c, b, c];
                    let actual_point = [
                        call5_u32(&mut store, &instance, "atlas_candidate_a", arguments),
                        call5_u32(&mut store, &instance, "atlas_candidate_b", arguments),
                        call5_u32(&mut store, &instance, "atlas_candidate_c", arguments),
                    ];
                    assert_eq!(actual_point, expected_point);
                    assert_eq!(
                        call5_u32(&mut store, &instance, "atlas_candidate_slot", arguments),
                        slot
                    );
                    assert!(slot < slot_count);
                    assert_eq!(
                        call5_u32(&mut store, &instance, "atlas_candidate_priority", arguments),
                        priority
                    );
                    let exponent = density_exponent_q8_oracle(key, expected_point);
                    assert_eq!(
                        call5_u32(
                            &mut store,
                            &instance,
                            "atlas_candidate_radius_squared",
                            arguments,
                        ),
                        radius_squared_oracle(exponent)
                    );
                    let expected_valid = u32::from(expected_point.into_iter().all(|lane| lane > 0));
                    assert_eq!(
                        call5_u32(&mut store, &instance, "atlas_candidate_valid", arguments),
                        expected_valid
                    );
                }
            }
        }
        assert_eq!(
            call5_u32(
                &mut store,
                &instance,
                "atlas_candidate_slot",
                [patch, SEED, side - 1, 0, 1],
            ),
            slot_count - 1
        );
        assert_eq!(
            call5_u32(
                &mut store,
                &instance,
                "atlas_candidate_valid",
                [patch, SEED, side, 0, 0],
            ),
            0
        );
    }

    let points = [
        ([SCALE, 0, 0], [0, SCALE, 0]),
        ([0, SCALE, 0], [0, 0, SCALE]),
        (
            [SCALE / 2, SCALE / 4, SCALE / 4],
            [SCALE / 4, SCALE / 2, SCALE / 4],
        ),
    ];
    for (left, right) in points {
        let db = i64::from(left[1]) - i64::from(right[1]);
        let dc = i64::from(left[2]) - i64::from(right[2]);
        let distance = u32::try_from(3 * (db * db + dc * dc + db * dc)).unwrap();
        assert_eq!(
            call6_u32(
                &mut store,
                &instance,
                "atlas_equilateral_distance_squared",
                [left[0], left[1], left[2], right[0], right[1], right[2]],
            ),
            distance
        );
        assert_eq!(
            call8_u32(
                &mut store,
                &instance,
                "atlas_poisson_conflict",
                [left[0], left[1], left[2], distance, right[0], right[1], right[2], distance,],
            ),
            0,
            "contact at exactly the larger disk radius is admitted"
        );
        assert_eq!(
            call8_u32(
                &mut store,
                &instance,
                "atlas_poisson_conflict",
                [
                    right[0],
                    right[1],
                    right[2],
                    1,
                    left[0],
                    left[1],
                    left[2],
                    distance + 1,
                ],
            ),
            1,
            "the symmetric larger-disk rule rejects an overlap"
        );
    }
}

fn call3(store: &mut Store<()>, instance: &Instance, name: &str, values: [f32; 3]) -> f32 {
    let [a, b, c] = values;
    function::<(f32, f32, f32), f32>(store, instance, name)
        .call(store, (a, b, c))
        .unwrap()
}

fn call4_f32(store: &mut Store<()>, instance: &Instance, name: &str, values: [f32; 4]) -> f32 {
    let [a, b, c, d] = values;
    function::<(f32, f32, f32, f32), f32>(store, instance, name)
        .call(store, (a, b, c, d))
        .unwrap()
}

fn call4_i32(store: &mut Store<()>, instance: &Instance, name: &str, values: [f32; 4]) -> i32 {
    let [a, b, c, d] = values;
    function::<(f32, f32, f32, f32), i32>(store, instance, name)
        .call(store, (a, b, c, d))
        .unwrap()
}

#[allow(clippy::many_single_char_names)]
fn call5_f32(store: &mut Store<()>, instance: &Instance, name: &str, values: [f32; 5]) -> f32 {
    let [a, b, c, d, e] = values;
    function::<(f32, f32, f32, f32, f32), f32>(store, instance, name)
        .call(store, (a, b, c, d, e))
        .unwrap()
}

#[allow(clippy::many_single_char_names)]
fn call5_i32(store: &mut Store<()>, instance: &Instance, name: &str, values: [f32; 5]) -> i32 {
    let [a, b, c, d, e] = values;
    function::<(f32, f32, f32, f32, f32), i32>(store, instance, name)
        .call(store, (a, b, c, d, e))
        .unwrap()
}

#[allow(clippy::many_single_char_names)]
fn call8(store: &mut Store<()>, instance: &Instance, name: &str, values: [f32; 8]) -> f32 {
    let [a, b, c, d, e, f, g, h] = values;
    function::<(f32, f32, f32, f32, f32, f32, f32, f32), f32>(store, instance, name)
        .call(store, (a, b, c, d, e, f, g, h))
        .unwrap()
}

fn assert_close(actual: f32, expected: f32, tolerance: f32, context: &str) {
    assert!(
        actual.is_finite(),
        "{context}: nonfinite Fe output {actual}"
    );
    assert!(
        (actual - expected).abs() <= tolerance,
        "{context}: Fe={actual:?}, oracle={expected:?}, tolerance={tolerance:?}"
    );
}

#[test]
fn quilting_domain_wasm_matches_the_frozen_m0_barycentrics() {
    const MATRIX: &[u8] =
        include_bytes!("../../../fixtures/classic-quilting/v1/direct-seed42-matrix.cqa");
    let artifact = crate::decode(MATRIX).expect("frozen M0 matrix");
    let (mut store, instance) = instantiate();

    for (index, vertex) in artifact.vertices.iter().enumerate() {
        let [a, b, c] = vertex.barycentric;
        let expected_x = 0.866_025_4_f32 * (c - b);
        let expected_y = (3.0 * a - 1.0) * 0.5;
        let actual_x = call3(&mut store, &instance, "domain_cartesian_x", [a, b, c]);
        let actual_y = call3(&mut store, &instance, "domain_cartesian_y", [a, b, c]);
        assert_close(
            actual_x,
            expected_x,
            f32::EPSILON,
            &format!("vertex {index} x"),
        );
        assert_close(
            actual_y,
            expected_y,
            f32::EPSILON,
            &format!("vertex {index} y"),
        );

        let round_trip = [
            call2(&mut store, &instance, "domain_bary_a", actual_x, actual_y),
            call2(&mut store, &instance, "domain_bary_b", actual_x, actual_y),
            call2(&mut store, &instance, "domain_bary_c", actual_x, actual_y),
        ];
        for (lane, (&actual, &expected)) in
            round_trip.iter().zip(vertex.barycentric.iter()).enumerate()
        {
            assert_close(
                actual,
                expected,
                3.0e-7,
                &format!("vertex {index} bary lane {lane}"),
            );
        }
        assert_eq!(
            call4_i32(&mut store, &instance, "domain_contains", [a, b, c, 2.0e-6],),
            1,
            "frozen vertex {index} must remain admitted"
        );
        for edge in 0..3 {
            if vertex.barycentric[edge].to_bits() == 0.0_f32.to_bits() {
                let expected_parameter = match edge {
                    0 => c,
                    1 => a,
                    2 => b,
                    _ => unreachable!(),
                };
                let edge_u32 = u32::try_from(edge).unwrap();
                let actual_parameter = function::<(u32, f32, f32, f32), f32>(
                    &mut store,
                    &instance,
                    "domain_edge_parameter",
                )
                .call(&mut store, (edge_u32, a, b, c))
                .unwrap();
                assert_eq!(actual_parameter.to_bits(), expected_parameter.to_bits());
            }
        }
    }

    let near_boundary = [1.0e-8_f32, 0.25, 0.75 - 1.0e-8, 1.0e-6];
    let admitted = [
        call4_f32(&mut store, &instance, "domain_admit_a", near_boundary),
        call4_f32(&mut store, &instance, "domain_admit_b", near_boundary),
        call4_f32(&mut store, &instance, "domain_admit_c", near_boundary),
    ];
    assert_eq!(admitted[0].to_bits(), 0.0_f32.to_bits());
    assert_close(admitted.iter().sum(), 1.0, f32::EPSILON, "admitted sum");
    assert_eq!(
        call4_i32(&mut store, &instance, "domain_admit_valid", near_boundary),
        1
    );
    assert_eq!(
        call4_i32(
            &mut store,
            &instance,
            "domain_admit_valid",
            [-1.0, -2.0, -3.0, 1.0e-6],
        ),
        0
    );
}

fn multiply_f32(left: [f32; 4], right: [f32; 4]) -> [f32; 4] {
    let [aw, ax, ay, az] = left;
    let [bw, bx, by, bz] = right;
    [
        aw * bw - ax * bx - ay * by - az * bz,
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
    ]
}

#[test]
fn quilting_quaternion_wasm_matches_independent_f32_vectors_and_fails_closed() {
    let (mut store, instance) = instantiate();
    let cases = [
        ([0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]),
        ([1.0, 2.0, -3.0, 0.5], [-0.25, 4.0, 0.75, -2.0]),
        ([0.9, -0.15, 0.25, 0.1], [1.1, 0.1, 0.05, -0.2]),
    ];
    let exports = [
        "quaternion_multiply_w",
        "quaternion_multiply_x",
        "quaternion_multiply_y",
        "quaternion_multiply_z",
    ];
    for (case_index, (left, right)) in cases.into_iter().enumerate() {
        let expected = multiply_f32(left, right);
        let arguments = [
            left[0], left[1], left[2], left[3], right[0], right[1], right[2], right[3],
        ];
        for lane in 0..4 {
            let actual = call8(&mut store, &instance, exports[lane], arguments);
            assert_close(
                actual,
                expected[lane],
                2.0 * f32::EPSILON,
                &format!("quaternion case {case_index} lane {lane}"),
            );
        }
    }

    let value = [1.0_f32, -2.0, 0.5, 3.0];
    let minimum = 1.0e-20;
    let norm_squared = value.iter().map(|lane| lane * lane).sum::<f32>();
    let expected = [
        value[0] / norm_squared,
        -value[1] / norm_squared,
        -value[2] / norm_squared,
        -value[3] / norm_squared,
    ];
    let inverse_exports = [
        "quaternion_inverse_w",
        "quaternion_inverse_x",
        "quaternion_inverse_y",
        "quaternion_inverse_z",
    ];
    for lane in 0..4 {
        assert_close(
            call5_f32(
                &mut store,
                &instance,
                inverse_exports[lane],
                [value[0], value[1], value[2], value[3], minimum],
            ),
            expected[lane],
            2.0 * f32::EPSILON,
            &format!("inverse lane {lane}"),
        );
    }
    assert_eq!(
        call5_i32(
            &mut store,
            &instance,
            "quaternion_inverse_valid",
            [value[0], value[1], value[2], value[3], minimum],
        ),
        1
    );

    let pole = [1.0e-12_f32, 0.0, 0.0, 0.0, minimum];
    assert_eq!(
        call5_i32(&mut store, &instance, "quaternion_inverse_valid", pole,),
        0
    );
    for export in inverse_exports {
        let lane = call5_f32(&mut store, &instance, export, pole);
        assert_eq!(lane.to_bits(), 0.0_f32.to_bits());
    }
}

fn curved_patch() -> QBTriPatch {
    QBTriPatch::new(
        [
            Quat::from_point(-0.75, -0.25, 0.1),
            Quat::from_point(0.8, -0.15, -0.2),
            Quat::from_point(0.05, 0.9, 0.35),
        ],
        [
            Quat::new(1.0, 0.2, -0.1, 0.05),
            Quat::new(0.9, -0.15, 0.25, 0.1),
            Quat::new(1.1, 0.1, 0.05, -0.2),
        ],
    )
}

fn normal_from_tangents(tangent_u: [f64; 3], tangent_v: [f64; 3]) -> [f64; 3] {
    let cross = [
        tangent_u[1] * tangent_v[2] - tangent_u[2] * tangent_v[1],
        tangent_u[2] * tangent_v[0] - tangent_u[0] * tangent_v[2],
        tangent_u[0] * tangent_v[1] - tangent_u[1] * tangent_v[0],
    ];
    let length = cross.iter().map(|value| value * value).sum::<f64>().sqrt();
    cross.map(|value| value / length)
}

fn oracle_f32(value: f64) -> f32 {
    assert!(value.is_finite());
    assert!(value >= f64::from(f32::MIN) && value <= f64::from(f32::MAX));
    #[allow(clippy::cast_possible_truncation)]
    {
        value as f32
    }
}

fn assert_curved_patch(store: &mut Store<()>, instance: &Instance) {
    let patch = curved_patch();
    let position_exports = ["qb_position_x", "qb_position_y", "qb_position_z"];
    let first_tangent_exports = ["qb_tangent_u_x", "qb_tangent_u_y", "qb_tangent_u_z"];
    let second_tangent_exports = ["qb_tangent_v_x", "qb_tangent_v_y", "qb_tangent_v_z"];
    let normal_exports = ["qb_normal_x", "qb_normal_y", "qb_normal_z"];

    for denominator in 1_u16..=4 {
        for u_step in 0..=denominator {
            for v_step in 0..=denominator - u_step {
                let u = f32::from(u_step) / f32::from(denominator);
                let v = f32::from(v_step) / f32::from(denominator);
                let expected = patch.eval_differential(f64::from(u), f64::from(v));
                let expected_normal = normal_from_tangents(expected.tangent_u, expected.tangent_v);
                for lane in 0..3 {
                    assert_close(
                        call2(store, instance, position_exports[lane], u, v),
                        oracle_f32(expected.position[lane]),
                        2.0e-6,
                        &format!("QB position ({u},{v}) lane {lane}"),
                    );
                    assert_close(
                        call2(store, instance, first_tangent_exports[lane], u, v),
                        oracle_f32(expected.tangent_u[lane]),
                        4.0e-6,
                        &format!("QB tangent u ({u},{v}) lane {lane}"),
                    );
                    assert_close(
                        call2(store, instance, second_tangent_exports[lane], u, v),
                        oracle_f32(expected.tangent_v[lane]),
                        4.0e-6,
                        &format!("QB tangent v ({u},{v}) lane {lane}"),
                    );
                    assert_close(
                        call2(store, instance, normal_exports[lane], u, v),
                        oracle_f32(expected_normal[lane]),
                        4.0e-6,
                        &format!("QB normal ({u},{v}) lane {lane}"),
                    );
                }
            }
        }
    }
}

fn assert_flat_patch(store: &mut Store<()>, instance: &Instance) {
    for (u, v) in [(0.0_f32, 0.0_f32), (1.0, 0.0), (0.0, 1.0), (0.25, 0.5)] {
        assert_close(
            call2(store, instance, "qb_flat_position_x", u, v),
            u,
            f32::EPSILON,
            "flat x",
        );
        assert_close(
            call2(store, instance, "qb_flat_position_y", u, v),
            v,
            f32::EPSILON,
            "flat y",
        );
        assert_eq!(
            call2(store, instance, "qb_flat_position_z", u, v).to_bits(),
            0.0_f32.to_bits()
        );
        assert_close(
            call2(store, instance, "qb_flat_normal_z", u, v),
            1.0,
            f32::EPSILON,
            "flat normal",
        );
    }
}

fn assert_pole_fails_closed(store: &mut Store<()>, instance: &Instance) {
    assert_eq!(
        function::<(f32, f32), i32>(store, instance, "qb_zero_weight_conditioned")
            .call(&mut *store, (1.0 / 3.0, 1.0 / 3.0))
            .unwrap(),
        0
    );
    assert_eq!(
        call2(
            store,
            instance,
            "qb_zero_weight_position_x",
            1.0 / 3.0,
            1.0 / 3.0,
        )
        .to_bits(),
        0.0_f32.to_bits()
    );
}

fn assert_s3_remaps(store: &mut Store<()>, instance: &Instance) {
    let bary = [0.2_f32, 0.3, 0.5];
    let remap_exports = ["qb_remap_a", "qb_remap_b", "qb_remap_c"];
    for (permutation, indices) in S3_PERMUTATIONS.into_iter().enumerate() {
        let permutation_u32 = u32::try_from(permutation).unwrap();
        for lane in 0..3 {
            let actual =
                function::<(u32, f32, f32, f32), f32>(store, instance, remap_exports[lane])
                    .call(&mut *store, (permutation_u32, bary[0], bary[1], bary[2]))
                    .unwrap();
            assert_eq!(actual.to_bits(), bary[indices[lane]].to_bits());
        }
        let parity = function::<u32, f32>(store, instance, "qb_permutation_parity")
            .call(&mut *store, permutation_u32)
            .unwrap();
        let expected_parity = if perm_sign(permutation) == 1 {
            1.0
        } else {
            -1.0
        };
        assert_eq!(parity, expected_parity);
        let normal_z = call2(store, instance, "qb_normal_z", 0.25, 0.25);
        let permuted_z = function::<(u32, f32, f32), f32>(store, instance, "qb_permuted_normal_z")
            .call(&mut *store, (permutation_u32, 0.25, 0.25))
            .unwrap();
        assert_close(
            permuted_z,
            normal_z * parity,
            f32::EPSILON,
            "permuted normal parity",
        );
    }
}

#[test]
fn quilting_qb_wasm_matches_rust_differentials_flat_patch_and_s3() {
    let (mut store, instance) = instantiate();
    assert_curved_patch(&mut store, &instance);
    assert_flat_patch(&mut store, &instance);
    assert_pole_fails_closed(&mut store, &instance);
    assert_s3_remaps(&mut store, &instance);
}
