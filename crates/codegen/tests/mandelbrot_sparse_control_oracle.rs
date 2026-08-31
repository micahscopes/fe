//! Independent schedule oracle for the high-precision sparse control AIR.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const LIMBS: u32 = 4;
const CONTROL_LANES: usize = 38;

fn control_row(
    kind: usize,
    major: u32,
    minor: u32,
    step: u32,
    flag: u32,
    width: u32,
    weight: u32,
) -> [u32; CONTROL_LANES] {
    let mut row = [0u32; CONTROL_LANES];
    row[kind] = 1;
    if kind == 9 {
        row[15 + minor as usize] = 1;
    }
    if kind <= 2 {
        if major < 6 {
            row[19] = 1;
            row[20 + (major / 2) as usize] = 1;
        } else if major < 15 {
            row[23 + ((major - 6) % 3) as usize] = 1;
        } else {
            row[26 + ((major - 15) % 4) as usize] = 1;
        }
    }
    if kind == 0 || kind == 1 {
        row[30] = u32::from(minor + 2 == LIMBS);
        row[31] = u32::from(minor + 1 == LIMBS);
    }
    row[32] = major;
    row[33] = minor;
    row[34] = step;
    row[35] = if kind <= 2 {
        u32::from(row[19] == 1 || row[25] == 1 || row[29] == 1)
    } else if kind == 9 || kind == 10 {
        u32::from(major == 0)
    } else {
        flag
    };
    row[36] = width;
    row[37] = weight;
    row
}

fn convolution_width(coefficient: u32) -> u32 {
    if coefficient < LIMBS {
        coefficient + 1
    } else if coefficient < 2 * LIMBS - 1 {
        2 * LIMBS - 1 - coefficient
    } else {
        0
    }
}

fn expected_control_rows() -> Vec<[u32; CONTROL_LANES]> {
    let mut rows = Vec::new();
    for range in 0..31u32 {
        for limb in 0..LIMBS {
            for bit in 0..13u32 {
                rows.push(control_row(0, range, limb, bit, 0, 0, 1 << bit));
            }
            rows.push(control_row(1, range, limb, 0, 0, 0, 0));
        }
        rows.push(control_row(2, range, 0, 0, 0, 0, 0));
    }
    for product in 0..3u32 {
        for coefficient in 0..2 * LIMBS {
            for slack in 0..2u32 {
                for bit in 0..18u32 {
                    rows.push(control_row(
                        3,
                        product,
                        coefficient,
                        bit,
                        slack,
                        0,
                        1 << bit,
                    ));
                }
            }
            rows.push(control_row(4, product, coefficient, 0, 0, 0, 0));
        }
    }
    for product in 0..3u32 {
        for coefficient in 0..2 * LIMBS {
            let width = convolution_width(coefficient);
            let descending = u32::from(coefficient >= LIMBS);
            for term in 0..width {
                rows.push(control_row(
                    5,
                    product,
                    coefficient,
                    term,
                    descending,
                    width,
                    0,
                ));
            }
            rows.push(control_row(
                6,
                product,
                coefficient,
                0,
                descending,
                width,
                0,
            ));
        }
    }
    for product in 0..3u32 {
        for limb in 0..LIMBS {
            rows.push(control_row(7, product, limb, 0, 0, 0, 0));
        }
        rows.push(control_row(8, product, 0, 0, 0, 0, 0));
    }
    for linear in 0..4u32 {
        for role in 0..4u32 {
            for limb in 0..LIMBS {
                rows.push(control_row(9, linear, role, limb, 0, 0, 0));
            }
        }
        rows.push(control_row(10, linear, 0, 0, 0, 0, 0));
    }
    for coordinate in 0..2u32 {
        rows.push(control_row(11, coordinate, 0, 0, 0, 0, 0));
        for limb in 0..LIMBS {
            rows.push(control_row(12, coordinate, limb, 0, 0, 0, 0));
        }
    }
    rows.push(control_row(13, 0, 0, 0, 0, 0, 0));

    let trace_length = rows.len().next_power_of_two();
    rows.resize(trace_length, control_row(14, 0, 0, 0, 0, 0, 0));
    rows
}

fn baby_bear_sub(left: u32, right: u32) -> u32 {
    const MODULUS: u64 = 2_013_265_921;
    ((left as u64 + MODULUS - right as u64) % MODULUS) as u32
}

fn baby_bear_add(left: u32, right: u32) -> u32 {
    const MODULUS: u64 = 2_013_265_921;
    ((left as u64 + right as u64) % MODULUS) as u32
}

fn baby_bear_mul(left: u32, right: u32) -> u32 {
    const MODULUS: u64 = 2_013_265_921;
    (left as u64 * right as u64 % MODULUS) as u32
}

fn expected_control_plan(row: [u32; CONTROL_LANES]) -> [u32; 3] {
    let major = row[32];
    [
        baby_bear_mul(major, baby_bear_sub(major, 1)),
        baby_bear_mul(baby_bear_sub(major, 2), baby_bear_sub(major, 3)),
        baby_bear_mul(baby_bear_sub(major, 4), baby_bear_sub(major, 5)),
    ]
}

fn expected_control_link_plan(
    current: [u32; CONTROL_LANES],
    next: [u32; CONTROL_LANES],
) -> [u32; 68] {
    let one = 1;
    let mut nodes = [0u32; 68];
    let phase_pairs = [
        (0, 0),
        (0, 1),
        (1, 0),
        (1, 2),
        (2, 0),
        (2, 3),
        (3, 3),
        (3, 4),
        (4, 3),
        (4, 5),
        (5, 5),
        (5, 6),
        (6, 5),
        (6, 6),
        (6, 7),
        (7, 7),
        (7, 8),
        (8, 7),
        (8, 9),
        (9, 9),
        (9, 10),
        (10, 9),
        (10, 11),
    ];
    for (index, (left, right)) in phase_pairs.into_iter().enumerate() {
        nodes[index] = baby_bear_mul(current[left], next[right]);
    }
    let boundary_pairs = [(11, 12), (12, 12), (12, 11), (12, 13), (13, 14), (14, 14)];
    for (index, (left, right)) in boundary_pairs.into_iter().enumerate() {
        nodes[23 + index] = baby_bear_mul(current[left], next[right]);
    }

    nodes[29] = baby_bear_mul(current[19], next[19]);
    nodes[30] = baby_bear_mul(current[19], next[23]);
    nodes[31] = baby_bear_mul(current[23], next[24]);
    nodes[32] = baby_bear_mul(current[24], next[25]);
    nodes[33] = baby_bear_mul(current[25], next[23]);
    nodes[34] = baby_bear_mul(current[25], next[26]);
    nodes[35] = baby_bear_mul(current[26], next[27]);
    nodes[36] = baby_bear_mul(current[27], next[28]);
    nodes[37] = baby_bear_mul(current[28], next[29]);
    nodes[38] = baby_bear_mul(current[29], next[26]);
    nodes[39] = baby_bear_mul(nodes[4], nodes[30]);
    nodes[40] = baby_bear_mul(nodes[4], nodes[34]);

    let carry_reset = baby_bear_sub(next[35], current[35]);
    let carry_delta = baby_bear_sub(next[32], current[32]);
    nodes[41] = baby_bear_mul(carry_reset, baby_bear_sub(carry_reset, one));
    nodes[42] = baby_bear_mul(current[35], baby_bear_sub(one, next[35]));
    nodes[43] = baby_bear_mul(nodes[6], carry_reset);
    nodes[44] = baby_bear_mul(
        baby_bear_sub(one, carry_reset),
        baby_bear_add(current[34], one),
    );
    nodes[45] = baby_bear_mul(
        baby_bear_sub(one, carry_reset),
        baby_bear_mul(2, current[37]),
    );
    nodes[46] = baby_bear_mul(carry_delta, baby_bear_sub(carry_delta, one));
    nodes[47] = baby_bear_mul(
        baby_bear_sub(one, carry_delta),
        baby_bear_add(current[33], one),
    );
    nodes[48] = baby_bear_mul(nodes[8], carry_delta);

    let product_delta = baby_bear_sub(next[32], current[32]);
    let same_product = baby_bear_sub(one, product_delta);
    let product_rise = baby_bear_sub(next[35], current[35]);
    nodes[49] = baby_bear_mul(product_delta, baby_bear_sub(product_delta, one));
    nodes[50] = baby_bear_mul(same_product, current[35]);
    nodes[51] = baby_bear_mul(nodes[50], baby_bear_sub(one, next[35]));
    nodes[52] = baby_bear_mul(same_product, product_rise);
    nodes[53] = baby_bear_mul(nodes[12], nodes[52]);
    nodes[54] = baby_bear_mul(same_product, baby_bear_add(current[33], one));
    nodes[55] = baby_bear_mul(same_product, next[35]);
    nodes[56] = baby_bear_mul(
        same_product,
        baby_bear_sub(baby_bear_add(current[36], one), baby_bear_mul(2, next[35])),
    );
    nodes[57] = baby_bear_mul(nodes[12], product_delta);

    let linear_delta = baby_bear_sub(next[33], current[33]);
    let retained_role = baby_bear_sub(one, linear_delta);
    nodes[58] = baby_bear_mul(linear_delta, baby_bear_sub(linear_delta, one));
    nodes[59] = baby_bear_mul(retained_role, baby_bear_add(current[34], one));
    nodes[60] = baby_bear_mul(nodes[19], linear_delta);
    nodes[61] = baby_bear_mul(retained_role, current[15]);
    nodes[62] = baby_bear_mul(retained_role, current[16]);
    nodes[63] = baby_bear_mul(linear_delta, current[15]);
    nodes[64] = baby_bear_mul(retained_role, current[17]);
    nodes[65] = baby_bear_mul(linear_delta, current[16]);
    nodes[66] = baby_bear_mul(retained_role, current[18]);
    nodes[67] = baby_bear_mul(linear_delta, current[17]);
    nodes
}

fn compile_fixture() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_sparse_control_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "sparse control oracle fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("sparse control oracle fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected sparse control oracle diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("sparse control oracle should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit sparse control oracle bytes");
    wasmparser::validate(&bytes).expect("sparse control oracle Wasm should validate");
    bytes
}

fn call(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    arguments: &[u32],
    result_count: usize,
) -> Vec<u32> {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("missing `{name}` export"));
    let params: Vec<Val> = arguments
        .iter()
        .map(|value| Val::I32(*value as i32))
        .collect();
    let mut results = vec![Val::I32(0); result_count];
    function
        .call(&mut *store, &params, &mut results)
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
fn sparse_control_air_is_index_free_and_rejects_schedule_mutations() {
    let bytes = compile_fixture();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes).expect("Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "sparse control oracle must stay zero-import",
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("zero-import sparse control oracle should instantiate");

    let rows = expected_control_rows();
    assert_eq!(rows.len(), 4096);
    for (index, expected) in rows.iter().enumerate() {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "sparse_control4_row",
                &[index as u32],
                CONTROL_LANES,
            ),
            expected,
            "independently reconstructed control row {index}",
        );
        assert_eq!(
            call(
                &mut store,
                &instance,
                "sparse_control4_plan",
                &[index as u32],
                3,
            ),
            expected_control_plan(*expected),
            "independently reconstructed local control plan {index}",
        );
        if index + 1 < rows.len() {
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "sparse_control4_link_plan",
                    &[index as u32],
                    68,
                ),
                expected_control_link_plan(*expected, rows[index + 1]),
                "independently reconstructed control link plan {index}",
            );
        }
    }

    for challenge in [7u32, 17, 31] {
        for node in 0u32..3 {
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "sparse_control4_plan_mutation_rejected",
                    &[challenge, 0, node],
                    1,
                ),
                [1],
                "local control plan node {node}, challenge {challenge} must be constrained",
            );
        }
        for node in 0u32..68 {
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "sparse_control4_link_plan_mutation_rejected",
                    &[challenge, 0, node],
                    1,
                ),
                [1],
                "control link plan node {node}, challenge {challenge} must be constrained",
            );
        }
    }

    let first_by_kind: Vec<usize> = (0..15usize)
        .map(|kind| {
            rows.iter()
                .position(|row| row[kind] == 1)
                .expect("every control selector must occur")
        })
        .collect();
    let mutations = [
        (0usize, 0u32),
        (first_by_kind[1], 1),
        (first_by_kind[2], 32),
        (first_by_kind[3], 35),
        (first_by_kind[3], 37),
        (first_by_kind[4], 33),
        (first_by_kind[5], 34),
        (first_by_kind[5], 36),
        (first_by_kind[6], 35),
        (first_by_kind[7], 33),
        (first_by_kind[8], 32),
        (first_by_kind[9], 15),
        (first_by_kind[9], 16),
        (first_by_kind[9], 34),
        (first_by_kind[9], 35),
        (first_by_kind[10], 32),
        (first_by_kind[11], 32),
        (first_by_kind[12], 33),
        (first_by_kind[13], 13),
        (first_by_kind[14], 14),
        (rows.iter().position(|row| row[19] == 1).unwrap(), 19),
        (rows.iter().position(|row| row[20] == 1).unwrap(), 20),
        (rows.iter().position(|row| row[21] == 1).unwrap(), 21),
        (rows.iter().position(|row| row[22] == 1).unwrap(), 22),
        (rows.iter().position(|row| row[23] == 1).unwrap(), 23),
        (rows.iter().position(|row| row[24] == 1).unwrap(), 24),
        (rows.iter().position(|row| row[25] == 1).unwrap(), 25),
        (rows.iter().position(|row| row[26] == 1).unwrap(), 26),
        (rows.iter().position(|row| row[27] == 1).unwrap(), 27),
        (rows.iter().position(|row| row[28] == 1).unwrap(), 28),
        (rows.iter().position(|row| row[29] == 1).unwrap(), 29),
        (rows.iter().position(|row| row[30] == 1).unwrap(), 30),
        (rows.iter().position(|row| row[31] == 1).unwrap(), 31),
        (rows.len() - 1, 37),
    ];
    for challenge in [7u32, 17, 31] {
        let baseline = call(
            &mut store,
            &instance,
            "sparse_control4_audit",
            &[challenge, u32::MAX, u32::MAX],
            3,
        );
        assert_eq!(
            baseline[0], 0,
            "canonical control schedule must satisfy its AIR",
        );
        assert_eq!(baseline[2], rows.len() as u32);
        assert_eq!(
            baseline[1], 1_486_555,
            "every local, adjacency, and boundary constraint must be evaluated",
        );
        for (index, lane) in mutations {
            let audit = call(
                &mut store,
                &instance,
                "sparse_control4_audit",
                &[challenge, index as u32, lane],
                3,
            );
            assert!(
                audit[0] > 0,
                "control mutation at row {index}, lane {lane}, challenge {challenge} must fail",
            );
            assert_eq!(audit[1], baseline[1]);
            assert_eq!(audit[2], baseline[2]);
        }
    }
}
