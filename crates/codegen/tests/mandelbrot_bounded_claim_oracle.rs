//! Independent semantic gate for the first bounded Mandelbrot proof slice.
//!
//! The Fe kernel owns the claim and witness transition. This test executes its
//! Wasm exports and compares every observed value with an independent i64
//! model. It is an oracle for witness generation, not a succinct proof.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;
use wasmtime::Val;

const SOURCE: &str = include_str!("../../../demos/capstones/mandelbrot-proof/kernel.fe");
const RADIUS_SQUARED_Q24: i64 = 67_108_864;
const NO_ESCAPE_STEP: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Row {
    step: u32,
    zr: i32,
    zi: i32,
    magnitude: i32,
    terminal: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Witness {
    valid: bool,
    escaped: bool,
    row: Row,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AirRow {
    row: Row,
    rr: i32,
    ii: i32,
    q_re: i32,
    r_re: i32,
    q_im: i32,
    r_im: i32,
}

fn valid_claim(c_re: i32, c_im: i32, bound: u32) -> bool {
    (-8192..4096).contains(&c_re) && (-6144..6144).contains(&c_im) && bound <= 1_048_576
}

fn reference_row(step: u32, zr: i32, zi: i32) -> Row {
    let rr = i64::from(zr) * i64::from(zr);
    let ii = i64::from(zi) * i64::from(zi);
    let magnitude = rr + ii;
    assert!(rr <= i64::from(i32::MAX), "real square must fit i32");
    assert!(ii <= i64::from(i32::MAX), "imaginary square must fit i32");
    assert!(
        magnitude <= i64::from(i32::MAX),
        "squared magnitude must fit i32"
    );
    Row {
        step,
        zr,
        zi,
        magnitude: magnitude as i32,
        terminal: magnitude >= RADIUS_SQUARED_Q24,
    }
}

fn reference_air_row(step: u32, zr: i32, zi: i32) -> AirRow {
    let row = reference_row(step, zr, zi);
    let rr = i64::from(zr) * i64::from(zr);
    let ii = i64::from(zi) * i64::from(zi);
    let real_numerator = rr - ii;
    let imaginary_numerator = 2 * i64::from(zr) * i64::from(zi);
    let q_re = real_numerator >> 12;
    let q_im = imaginary_numerator >> 12;
    let r_re = real_numerator - q_re * 4096;
    let r_im = imaginary_numerator - q_im * 4096;
    assert!((0..4096).contains(&r_re));
    assert!((0..4096).contains(&r_im));
    AirRow {
        row,
        rr: rr as i32,
        ii: ii as i32,
        q_re: q_re as i32,
        r_re: r_re as i32,
        q_im: q_im as i32,
        r_im: r_im as i32,
    }
}

fn air_row_holds(row: AirRow) -> bool {
    row == reference_air_row(row.row.step, row.row.zr, row.row.zi)
}

fn transition_holds(c_re: i32, c_im: i32, row: AirRow, next: AirRow) -> bool {
    air_row_holds(row)
        && air_row_holds(next)
        && !row.row.terminal
        && next.row.step == row.row.step + 1
        && next.row.zr == row.q_re + c_re
        && next.row.zi == row.q_im + c_im
}

fn reference_rows(c_re: i32, c_im: i32, bound: u32) -> Vec<Row> {
    assert!(valid_claim(c_re, c_im, bound));
    let mut rows = Vec::new();
    let mut step = 0u32;
    let mut zr = 0i32;
    let mut zi = 0i32;
    loop {
        let row = reference_row(step, zr, zi);
        rows.push(row);
        if row.terminal || step == bound {
            return rows;
        }

        let rr = i64::from(zr) * i64::from(zr);
        let ii = i64::from(zi) * i64::from(zi);
        let cross = 2 * i64::from(zr) * i64::from(zi);
        assert!(
            (-RADIUS_SQUARED_Q24..RADIUS_SQUARED_Q24).contains(&(rr - ii)),
            "continued real numerator must remain bounded"
        );
        assert!(
            (-RADIUS_SQUARED_Q24..RADIUS_SQUARED_Q24).contains(&cross),
            "continued imaginary numerator must remain bounded"
        );
        zr = ((rr - ii) >> 12) as i32 + c_re;
        zi = (cross >> 12) as i32 + c_im;
        step += 1;
    }
}

fn reference_witness(c_re: i32, c_im: i32, bound: u32) -> Witness {
    if !valid_claim(c_re, c_im, bound) {
        return Witness {
            valid: false,
            escaped: false,
            row: reference_row(0, 0, 0),
        };
    }
    let row = *reference_rows(c_re, c_im, bound)
        .last()
        .expect("a trace always contains z_0");
    Witness {
        valid: true,
        escaped: row.terminal,
        row,
    }
}

fn compile() -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///mandelbrot_bounded_proof.fe").expect("fixture URL");
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_owned()));
    let file = db.workspace().get(&db, &url).expect("fixture file");
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "bounded proof Fe diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("bounded proof witness should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("bounded proof witness Wasm should validate");
    bytes
}

fn call_words(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    args: [i32; 3],
    result_count: usize,
) -> Vec<i32> {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("{name} export should exist"));
    let params = args.into_iter().map(Val::I32).collect::<Vec<_>>();
    let mut results = vec![Val::I32(0); result_count];
    function
        .call(&mut *store, &params, &mut results)
        .unwrap_or_else(|error| panic!("{name} should execute: {error:?}"));
    results
        .into_iter()
        .map(|value| match value {
            Val::I32(word) => word,
            other => panic!("{name} result must be i32, got {other:?}"),
        })
        .collect()
}

fn wasm_witness(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    c_re: i32,
    c_im: i32,
    bound: u32,
) -> Witness {
    let words = call_words(
        store,
        instance,
        "escape_witness_q12",
        [c_re, c_im, bound as i32],
        6,
    );
    Witness {
        valid: words[0] == 1,
        escaped: words[1] == 1,
        row: Row {
            step: words[2] as u32,
            zr: words[3],
            zi: words[4],
            magnitude: words[5],
            terminal: words[1] == 1,
        },
    }
}

fn wasm_row(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    c_re: i32,
    c_im: i32,
    target: u32,
) -> Row {
    let words = call_words(
        store,
        instance,
        "escape_trace_row_q12",
        [c_re, c_im, target as i32],
        5,
    );
    Row {
        step: words[0] as u32,
        zr: words[1],
        zi: words[2],
        magnitude: words[3],
        terminal: words[4] == 1,
    }
}

fn wasm_air_row(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    c_re: i32,
    c_im: i32,
    target: u32,
) -> AirRow {
    let words = call_words(
        store,
        instance,
        "escape_air_row_q12",
        [c_re, c_im, target as i32],
        11,
    );
    AirRow {
        row: Row {
            step: words[0] as u32,
            zr: words[1],
            zi: words[2],
            magnitude: words[5],
            terminal: words[10] == 1,
        },
        rr: words[3],
        ii: words[4],
        q_re: words[6],
        r_re: words[7],
        q_im: words[8],
        r_im: words[9],
    }
}

#[test]
fn fe_bounded_escape_witness_matches_independent_i64_model() {
    let bytes = compile();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("load bounded proof Wasm");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("instantiate bounded proof Wasm");

    let directed = [
        (0, 0, 0),
        (0, 0, 100),
        (-8192, -6144, 100),
        (4072, 6120, 100),
        (4095, 4095, 2),
        (-4096, 0, 100),
        (-3048, 2216, 255),
    ];
    for (c_re, c_im, bound) in directed {
        let expected = reference_witness(c_re, c_im, bound);
        let observed = wasm_witness(&mut store, &instance, c_re, c_im, bound);
        assert_eq!(
            observed, expected,
            "directed claim ({c_re}, {c_im}, {bound})"
        );

        if expected.valid {
            let rows = reference_rows(c_re, c_im, bound);
            for expected_row in rows.iter().copied() {
                assert_eq!(
                    wasm_row(&mut store, &instance, c_re, c_im, expected_row.step),
                    expected_row,
                    "directed row ({c_re}, {c_im}) Z_{}",
                    expected_row.step
                );
                let expected_air =
                    reference_air_row(expected_row.step, expected_row.zr, expected_row.zi);
                let observed_air =
                    wasm_air_row(&mut store, &instance, c_re, c_im, expected_row.step);
                assert_eq!(
                    observed_air, expected_air,
                    "directed AIR row ({c_re}, {c_im}) Z_{}",
                    expected_row.step
                );
                assert!(air_row_holds(observed_air));
            }
            for pair in rows.windows(2) {
                let row = reference_air_row(pair[0].step, pair[0].zr, pair[0].zi);
                let next = reference_air_row(pair[1].step, pair[1].zr, pair[1].zi);
                assert!(
                    transition_holds(c_re, c_im, row, next),
                    "directed AIR transition ({c_re}, {c_im}) Z_{}",
                    row.row.step
                );
            }
            if expected.escaped {
                assert_eq!(
                    wasm_row(&mut store, &instance, c_re, c_im, expected.row.step + 7),
                    expected.row,
                    "post-terminal row requests must clamp"
                );
                assert_eq!(
                    wasm_air_row(&mut store, &instance, c_re, c_im, expected.row.step + 7),
                    reference_air_row(expected.row.step, expected.row.zr, expected.row.zi,),
                    "post-terminal AIR row requests must clamp"
                );
            }
        }
    }

    let invalid = [
        (-8193, 0, 100),
        (4096, 0, 100),
        (0, -6145, 100),
        (0, 6144, 100),
        (0, 0, 1_048_577),
    ];
    for (c_re, c_im, bound) in invalid {
        let observed = wasm_witness(&mut store, &instance, c_re, c_im, bound);
        assert!(!observed.valid, "invalid claim must fail closed");
        assert!(
            !observed.escaped,
            "invalid claim cannot produce escape evidence"
        );
        assert_eq!(
            if observed.escaped {
                observed.row.step
            } else {
                NO_ESCAPE_STEP
            },
            NO_ESCAPE_STEP
        );
    }

    let mut state = 0x243f_6a88u32;
    for case in 0..512u32 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let px = state & 511;
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let py = state & 511;
        let c_re = -8192 + (px as i32) * 24;
        let c_im = -6144 + (py as i32) * 24;
        let bound = [0, 1, 2, 7, 31, 100, 255][case as usize % 7];
        assert_eq!(
            wasm_witness(&mut store, &instance, c_re, c_im, bound),
            reference_witness(c_re, c_im, bound),
            "deterministic case {case}, pixel ({px}, {py}), bound {bound}"
        );
    }

    // Every expanded column is load-bearing. This is not yet a polynomial AIR
    // verifier, but it ensures the independent integer relation rejects a
    // one-unit mutation in each witness column before arithmetization.
    let rows = reference_rows(-3048, 2216, 255);
    let base = reference_air_row(rows[0].step, rows[0].zr, rows[0].zi);
    let next = reference_air_row(rows[1].step, rows[1].zr, rows[1].zi);
    let mutations = [
        AirRow {
            row: Row {
                step: base.row.step + 1,
                ..base.row
            },
            ..base
        },
        AirRow {
            row: Row {
                zr: base.row.zr + 1,
                ..base.row
            },
            ..base
        },
        AirRow {
            row: Row {
                zi: base.row.zi + 1,
                ..base.row
            },
            ..base
        },
        AirRow {
            rr: base.rr + 1,
            ..base
        },
        AirRow {
            ii: base.ii + 1,
            ..base
        },
        AirRow {
            row: Row {
                magnitude: base.row.magnitude + 1,
                ..base.row
            },
            ..base
        },
        AirRow {
            q_re: base.q_re + 1,
            ..base
        },
        AirRow {
            r_re: base.r_re + 1,
            ..base
        },
        AirRow {
            q_im: base.q_im + 1,
            ..base
        },
        AirRow {
            r_im: base.r_im + 1,
            ..base
        },
        AirRow {
            row: Row {
                terminal: !base.row.terminal,
                ..base.row
            },
            ..base
        },
    ];
    for (column, mutation) in mutations.into_iter().enumerate() {
        if column != 0 {
            assert!(
                !air_row_holds(mutation),
                "mutated row-local AIR column {column} must reject"
            );
        }
        assert!(
            !transition_holds(-3048, 2216, mutation, next),
            "transition with mutated AIR column {column} must reject"
        );
    }
}
