//! Independent schedule oracle for the high-precision sparse control AIR.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{layout_for, BackendKind, OptLevel};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const LIMBS: u32 = 4;
const CONTROL_LANES: usize = 33;

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
        } else if major < 15 {
            row[20 + ((major - 6) % 3) as usize] = 1;
        } else {
            row[23 + ((major - 15) % 4) as usize] = 1;
        }
    }
    row[27] = major;
    row[28] = minor;
    row[29] = step;
    row[30] = if kind <= 2 {
        u32::from(row[19] == 1 || row[22] == 1 || row[26] == 1)
    } else if kind == 9 || kind == 10 {
        u32::from(major == 0)
    } else {
        flag
    };
    row[31] = width;
    row[32] = weight;
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
        (first_by_kind[2], 27),
        (first_by_kind[3], 30),
        (first_by_kind[3], 32),
        (first_by_kind[4], 28),
        (first_by_kind[5], 29),
        (first_by_kind[5], 31),
        (first_by_kind[6], 30),
        (first_by_kind[7], 28),
        (first_by_kind[8], 27),
        (first_by_kind[9], 15),
        (first_by_kind[9], 16),
        (first_by_kind[9], 29),
        (first_by_kind[9], 30),
        (first_by_kind[10], 27),
        (first_by_kind[11], 27),
        (first_by_kind[12], 28),
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
        (rows.len() - 1, 32),
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
            baseline[1], 1_109_798,
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
