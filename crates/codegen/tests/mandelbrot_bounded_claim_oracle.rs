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

fn one_unit_mutations(base: AirRow) -> [AirRow; 11] {
    [
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
    ]
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
    args: &[i32],
    result_count: usize,
) -> Vec<i32> {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("{name} export should exist"));
    let params = args.iter().copied().map(Val::I32).collect::<Vec<_>>();
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
        &[c_re, c_im, bound as i32],
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
        &[c_re, c_im, target as i32],
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
        &[c_re, c_im, target as i32],
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

fn air_row_words(row: AirRow) -> [i32; 11] {
    [
        row.row.step as i32,
        row.row.zr,
        row.row.zi,
        row.rr,
        row.ii,
        row.row.magnitude,
        row.q_re,
        row.r_re,
        row.q_im,
        row.r_im,
        i32::from(row.row.terminal),
    ]
}

fn reference_encoding(row: AirRow) -> Vec<i32> {
    fn signed(value: i32) -> [i32; 2] {
        [i32::from(value < 0), value.unsigned_abs() as i32]
    }
    let zr = signed(row.row.zr);
    let zi = signed(row.row.zi);
    let q_re = signed(row.q_re);
    let q_im = signed(row.q_im);
    vec![
        row.row.step as i32,
        zr[0],
        zr[1],
        zi[0],
        zi[1],
        row.rr,
        row.ii,
        row.row.magnitude,
        q_re[0],
        q_re[1],
        row.r_re,
        q_im[0],
        q_im[1],
        row.r_im,
        i32::from(row.row.terminal),
    ]
}

fn wasm_encoding(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    c_re: i32,
    c_im: i32,
    target: u32,
) -> Vec<i32> {
    call_words(
        store,
        instance,
        "escape_air_row_encoding_q12",
        &[c_re, c_im, target as i32],
        15,
    )
}

fn wasm_residuals(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    row: AirRow,
    terminal_word: i32,
) -> ([i64; 5], [i32; 4]) {
    let mut args = air_row_words(row);
    args[10] = terminal_word;
    let function = instance
        .get_func(&mut *store, "escape_air_residuals_q12")
        .expect("escape_air_residuals_q12 export should exist");
    let params = args.into_iter().map(Val::I32).collect::<Vec<_>>();
    let mut results = vec![
        Val::I64(0),
        Val::I64(0),
        Val::I64(0),
        Val::I64(0),
        Val::I64(0),
        Val::I32(0),
        Val::I32(0),
        Val::I32(0),
        Val::I32(0),
    ];
    function
        .call(&mut *store, &params, &mut results)
        .expect("escape_air_residuals_q12 should execute");
    let residuals = std::array::from_fn(|index| match results[index] {
        Val::I64(value) => value,
        ref other => panic!("residual {index} must be i64, got {other:?}"),
    });
    let relations = std::array::from_fn(|index| match results[index + 5] {
        Val::I32(value) => value,
        ref other => panic!("relation {index} must be i32, got {other:?}"),
    });
    (residuals, relations)
}

fn wasm_pair_holds(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    c_re: i32,
    c_im: i32,
    bound: u32,
    row: AirRow,
    next: AirRow,
) -> bool {
    let mut args = vec![c_re, c_im, bound as i32];
    args.extend(air_row_words(row));
    args.extend(air_row_words(next));
    call_words(store, instance, "escape_air_pair_holds_q12", &args, 1) == [1]
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
                assert_eq!(
                    wasm_encoding(&mut store, &instance, c_re, c_im, expected_row.step),
                    reference_encoding(expected_air),
                    "canonical AIR encoding ({c_re}, {c_im}) Z_{}",
                    expected_row.step
                );
                assert_eq!(
                    wasm_residuals(
                        &mut store,
                        &instance,
                        expected_air,
                        i32::from(expected_air.row.terminal),
                    ),
                    ([0; 5], [1; 4]),
                    "Fe residuals and non-polynomial relations ({c_re}, {c_im}) Z_{}",
                    expected_row.step
                );
            }
            for pair in rows.windows(2) {
                let row = reference_air_row(pair[0].step, pair[0].zr, pair[0].zi);
                let next = reference_air_row(pair[1].step, pair[1].zr, pair[1].zi);
                assert!(
                    transition_holds(c_re, c_im, row, next),
                    "directed AIR transition ({c_re}, {c_im}) Z_{}",
                    row.row.step
                );
                assert!(
                    wasm_pair_holds(&mut store, &instance, c_re, c_im, bound, row, next),
                    "Fe AIR pair verifier ({c_re}, {c_im}) Z_{}",
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
    for (column, mutation) in one_unit_mutations(base).into_iter().enumerate() {
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
        assert!(
            !wasm_pair_holds(&mut store, &instance, -3048, 2216, 255, mutation, next),
            "Fe verifier must reject current-row AIR mutation {column}"
        );
    }
    for (column, mutation) in one_unit_mutations(next).into_iter().enumerate() {
        assert!(
            !wasm_pair_holds(&mut store, &instance, -3048, 2216, 255, base, mutation),
            "Fe verifier must reject next-row AIR mutation {column}"
        );
    }

    assert!(
        !wasm_pair_holds(&mut store, &instance, -3047, 2216, 255, base, next),
        "Fe verifier must reject an altered public real coordinate"
    );
    assert!(
        !wasm_pair_holds(&mut store, &instance, -3048, 2217, 255, base, next),
        "Fe verifier must reject an altered public imaginary coordinate"
    );
    assert!(
        !wasm_pair_holds(&mut store, &instance, -3048, 2216, 0, base, next),
        "Fe verifier must reject a bound smaller than the directed pair"
    );

    let mut noncanonical_real = base;
    noncanonical_real.q_re -= 1;
    noncanonical_real.r_re += 4096;
    let (real_residuals, real_relations) =
        wasm_residuals(&mut store, &instance, noncanonical_real, 0);
    assert_eq!(real_residuals, [0; 5]);
    assert_eq!(real_relations, [0, 1, 1, 1]);
    assert!(
        !wasm_pair_holds(
            &mut store,
            &instance,
            -3048,
            2216,
            255,
            noncanonical_real,
            next,
        ),
        "a residual-zero but noncanonical real quotient/remainder must reject"
    );

    let mut noncanonical_imaginary = base;
    noncanonical_imaginary.q_im -= 1;
    noncanonical_imaginary.r_im += 4096;
    let (imaginary_residuals, imaginary_relations) =
        wasm_residuals(&mut store, &instance, noncanonical_imaginary, 0);
    assert_eq!(imaginary_residuals, [0; 5]);
    assert_eq!(imaginary_relations, [0, 1, 1, 1]);
    assert!(
        !wasm_pair_holds(
            &mut store,
            &instance,
            -3048,
            2216,
            255,
            noncanonical_imaginary,
            next,
        ),
        "a residual-zero but noncanonical imaginary quotient/remainder must reject"
    );

    let zero_encoding = wasm_encoding(&mut store, &instance, -3048, 2216, 0);
    assert_eq!(
        zero_encoding[1], 0,
        "zero real coordinate has positive sign"
    );
    assert_eq!(
        zero_encoding[3], 0,
        "zero imaginary coordinate has positive sign"
    );
    assert_eq!(zero_encoding[8], 0, "zero real quotient has positive sign");
    assert_eq!(
        zero_encoding[11], 0,
        "zero imaginary quotient has positive sign"
    );

    let (terminal_residuals, terminal_relations) = wasm_residuals(&mut store, &instance, base, 2);
    assert_eq!(terminal_residuals, [0; 5]);
    assert_eq!(terminal_relations, [1, 1, 0, 1]);

    let unsafe_row = AirRow {
        row: Row {
            zr: i32::MIN,
            zi: i32::MIN,
            ..base.row
        },
        ..base
    };
    let (unsafe_residuals, unsafe_relations) = wasm_residuals(&mut store, &instance, unsafe_row, 0);
    assert_eq!(unsafe_residuals, [1; 5]);
    assert_eq!(unsafe_relations[3], 0);
    assert!(
        !wasm_pair_holds(&mut store, &instance, -3048, 2216, 255, unsafe_row, next),
        "out-of-domain alleged coordinates must reject before widened arithmetic"
    );
}
