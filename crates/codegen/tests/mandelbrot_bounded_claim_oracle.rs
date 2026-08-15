//! Independent semantic gate for the first bounded Mandelbrot proof slice.
//!
//! The Fe kernel owns the claim and witness transition. This test executes its
//! Wasm exports and compares every observed value with an independent i64
//! model. It is an oracle for witness generation, not a succinct proof.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProofShape {
    valid: bool,
    terminal_step: u32,
    trace_length: u32,
    padded_length: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaddedAirRow {
    valid: bool,
    active: bool,
    terminal: bool,
    air: AirRow,
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

fn reference_proof_shape(c_re: i32, c_im: i32, bound: u32) -> ProofShape {
    let witness = reference_witness(c_re, c_im, bound);
    if !witness.valid || !witness.escaped {
        return ProofShape {
            valid: false,
            terminal_step: NO_ESCAPE_STEP,
            trace_length: 0,
            padded_length: 0,
        };
    }
    let trace_length = witness.row.step + 1;
    ProofShape {
        valid: true,
        terminal_step: witness.row.step,
        trace_length,
        padded_length: trace_length.next_power_of_two(),
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

fn compile_field_air() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../demos/capstones/mandelbrot-proof/field-air");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "Mandelbrot BN254 AIR ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("Mandelbrot BN254 AIR ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "Mandelbrot BN254 AIR diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("Mandelbrot BN254 AIR should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("Mandelbrot BN254 AIR Wasm should validate");
    bytes
}

fn function_imports(bytes: &[u8]) -> Vec<(String, String)> {
    use wasmparser::{Payload, TypeRef};
    let mut imports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let Payload::ImportSection(reader) = payload.expect("valid Wasm payload") {
            for import in reader.into_imports() {
                let import = import.expect("valid import entry");
                if let TypeRef::Func(_) = import.ty {
                    imports.push((import.module.to_string(), import.name.to_string()));
                }
            }
        }
    }
    imports
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

fn call_mask(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    args: &[i32],
) -> u32 {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("{name} export should exist"));
    let params = args.iter().copied().map(Val::I32).collect::<Vec<_>>();
    let mut results = [Val::I32(0)];
    function
        .call(&mut *store, &params, &mut results)
        .unwrap_or_else(|error| panic!("{name} should execute: {error:?}"));
    match results[0] {
        Val::I32(word) => word as u32,
        ref other => panic!("{name} result must be i32, got {other:?}"),
    }
}

fn call_mask_pair(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    args: &[i32],
) -> (u32, u32) {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("{name} export should exist"));
    let params = args.iter().copied().map(Val::I32).collect::<Vec<_>>();
    let mut results = [Val::I32(0), Val::I32(0)];
    function
        .call(&mut *store, &params, &mut results)
        .unwrap_or_else(|error| panic!("{name} should execute: {error:?}"));
    match (&results[0], &results[1]) {
        (Val::I32(left), Val::I32(right)) => (*left as u32, *right as u32),
        other => panic!("{name} results must be i32, got {other:?}"),
    }
}

fn sign_magnitude(value: i32) -> [i32; 2] {
    [i32::from(value < 0), value.unsigned_abs() as i32]
}

fn field_local_args(row: AirRow) -> [i32; 13] {
    let zr = sign_magnitude(row.row.zr);
    let zi = sign_magnitude(row.row.zi);
    let q_re = sign_magnitude(row.q_re);
    let q_im = sign_magnitude(row.q_im);
    [
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
    ]
}

fn field_transition_args(c_re: i32, c_im: i32, row: AirRow, next: AirRow) -> [i32; 14] {
    let c_re = sign_magnitude(c_re);
    let c_im = sign_magnitude(c_im);
    let q_re = sign_magnitude(row.q_re);
    let q_im = sign_magnitude(row.q_im);
    let next_zr = sign_magnitude(next.row.zr);
    let next_zi = sign_magnitude(next.row.zi);
    [
        c_re[0],
        c_re[1],
        c_im[0],
        c_im[1],
        row.row.step as i32,
        q_re[0],
        q_re[1],
        q_im[0],
        q_im[1],
        next.row.step as i32,
        next_zr[0],
        next_zr[1],
        next_zi[0],
        next_zi[1],
    ]
}

fn field_air_encoding(row: AirRow) -> [i32; 15] {
    let zr = sign_magnitude(row.row.zr);
    let zi = sign_magnitude(row.row.zi);
    let q_re = sign_magnitude(row.q_re);
    let q_im = sign_magnitude(row.q_im);
    [
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

fn field_proof_encoding(active: bool, terminal: bool, row: AirRow) -> [i32; 17] {
    let mut words = [0; 17];
    words[0] = i32::from(active);
    words[1] = i32::from(terminal);
    words[2..].copy_from_slice(&field_air_encoding(row));
    words
}

fn field_pair_args(c_re: i32, c_im: i32, row: [i32; 17], next: [i32; 17]) -> Vec<i32> {
    let c_re = sign_magnitude(c_re);
    let c_im = sign_magnitude(c_im);
    let mut args = Vec::with_capacity(38);
    args.extend(c_re);
    args.extend(c_im);
    args.extend(row);
    args.extend(next);
    args
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

fn wasm_proof_shape(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    c_re: i32,
    c_im: i32,
    bound: u32,
) -> ProofShape {
    let words = call_words(
        store,
        instance,
        "escape_proof_shape_q12",
        &[c_re, c_im, bound as i32],
        4,
    );
    ProofShape {
        valid: words[0] == 1,
        terminal_step: words[1] as u32,
        trace_length: words[2] as u32,
        padded_length: words[3] as u32,
    }
}

fn wasm_proof_row(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    c_re: i32,
    c_im: i32,
    bound: u32,
    target: u32,
) -> PaddedAirRow {
    let words = call_words(
        store,
        instance,
        "escape_proof_row_q12",
        &[c_re, c_im, bound as i32, target as i32],
        14,
    );
    PaddedAirRow {
        valid: words[0] == 1,
        active: words[1] == 1,
        terminal: words[2] == 1,
        air: AirRow {
            row: Row {
                step: words[3] as u32,
                zr: words[4],
                zi: words[5],
                magnitude: words[8],
                terminal: words[13] == 1,
            },
            rr: words[6],
            ii: words[7],
            q_re: words[9],
            r_re: words[10],
            q_im: words[11],
            r_im: words[12],
        },
    }
}

fn wasm_proof_encoding(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    c_re: i32,
    c_im: i32,
    bound: u32,
    target: u32,
) -> Vec<i32> {
    call_words(
        store,
        instance,
        "escape_proof_row_encoding_q12",
        &[c_re, c_im, bound as i32, target as i32],
        18,
    )
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

fn wasm_padded_pair_holds_raw(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    c_re: i32,
    c_im: i32,
    bound: u32,
    row_active: i32,
    row_terminal: i32,
    row: AirRow,
    next_active: i32,
    next_terminal: i32,
    next: AirRow,
) -> bool {
    let mut args = vec![c_re, c_im, bound as i32, row_active, row_terminal];
    args.extend(air_row_words(row));
    args.push(next_active);
    args.push(next_terminal);
    args.extend(air_row_words(next));
    call_words(
        store,
        instance,
        "escape_padded_air_pair_holds_q12",
        &args,
        1,
    ) == [1]
}

fn wasm_padded_pair_holds(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    c_re: i32,
    c_im: i32,
    bound: u32,
    row: PaddedAirRow,
    next: PaddedAirRow,
) -> bool {
    wasm_padded_pair_holds_raw(
        store,
        instance,
        c_re,
        c_im,
        bound,
        i32::from(row.active),
        i32::from(row.terminal),
        row.air,
        i32::from(next.active),
        i32::from(next.terminal),
        next.air,
    )
}

fn wasm_padded_first_holds(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    c_re: i32,
    c_im: i32,
    bound: u32,
    row: PaddedAirRow,
) -> bool {
    let mut args = vec![
        c_re,
        c_im,
        bound as i32,
        i32::from(row.active),
        i32::from(row.terminal),
    ];
    args.extend(air_row_words(row.air));
    call_words(
        store,
        instance,
        "escape_padded_air_first_holds_q12",
        &args,
        1,
    ) == [1]
}

fn wasm_padded_last_holds(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    row: PaddedAirRow,
) -> bool {
    let mut args = vec![i32::from(row.active), i32::from(row.terminal)];
    args.extend(air_row_words(row.air));
    call_words(
        store,
        instance,
        "escape_padded_air_last_holds_q12",
        &args,
        1,
    ) == [1]
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

        let expected_shape = reference_proof_shape(c_re, c_im, bound);
        let observed_shape = wasm_proof_shape(&mut store, &instance, c_re, c_im, bound);
        assert_eq!(
            observed_shape, expected_shape,
            "proof trace shape ({c_re}, {c_im}, {bound})"
        );

        if expected_shape.valid {
            assert!(expected_shape.padded_length.is_power_of_two());
            assert!(expected_shape.padded_length >= expected_shape.trace_length);
            assert!(expected_shape.padded_length < expected_shape.trace_length * 2);
            let semantic_rows = reference_rows(c_re, c_im, bound);
            let terminal_air = reference_air_row(
                expected_shape.terminal_step,
                expected.row.zr,
                expected.row.zi,
            );
            let mut active_count = 0u32;
            let mut terminal_count = 0u32;
            let mut padded_rows = Vec::new();
            for target in 0..expected_shape.padded_length {
                let active = target < expected_shape.trace_length;
                let terminal = target == expected_shape.terminal_step;
                let expected_air = if active {
                    let row = semantic_rows[target as usize];
                    reference_air_row(row.step, row.zr, row.zi)
                } else {
                    terminal_air
                };
                let expected_proof_row = PaddedAirRow {
                    valid: true,
                    active,
                    terminal,
                    air: expected_air,
                };
                padded_rows.push(expected_proof_row);
                assert_eq!(
                    wasm_proof_row(&mut store, &instance, c_re, c_im, bound, target,),
                    expected_proof_row,
                    "padded proof row ({c_re}, {c_im}) at {target}"
                );

                let mut expected_encoding = vec![1];
                expected_encoding.extend(reference_encoding(expected_air));
                expected_encoding.push(i32::from(active));
                expected_encoding.push(i32::from(terminal));
                assert_eq!(
                    wasm_proof_encoding(&mut store, &instance, c_re, c_im, bound, target,),
                    expected_encoding,
                    "padded proof encoding ({c_re}, {c_im}) at {target}"
                );
                active_count += u32::from(active);
                terminal_count += u32::from(terminal);
            }
            assert_eq!(active_count, expected_shape.trace_length);
            assert_eq!(
                terminal_count, 1,
                "the padded trace has one terminal marker"
            );
            assert!(
                wasm_padded_first_holds(&mut store, &instance, c_re, c_im, bound, padded_rows[0],),
                "Fe first-row constraint ({c_re}, {c_im}, {bound})"
            );
            for (index, pair) in padded_rows.windows(2).enumerate() {
                assert!(
                    wasm_padded_pair_holds(
                        &mut store, &instance, c_re, c_im, bound, pair[0], pair[1],
                    ),
                    "Fe padded transition ({c_re}, {c_im}, {bound}) at {index}"
                );
            }
            assert!(
                wasm_padded_last_holds(
                    &mut store,
                    &instance,
                    *padded_rows.last().expect("proof trace has a final row"),
                ),
                "Fe last-row constraint ({c_re}, {c_im}, {bound})"
            );
            assert!(
                !wasm_proof_row(
                    &mut store,
                    &instance,
                    c_re,
                    c_im,
                    bound,
                    expected_shape.padded_length,
                )
                .valid,
                "row requests outside the padded domain fail closed"
            );
        } else {
            assert!(
                !wasm_proof_row(&mut store, &instance, c_re, c_im, bound, 0).valid,
                "a non-escaping claim cannot materialize proof rows"
            );
        }

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
        assert_eq!(
            wasm_proof_shape(&mut store, &instance, c_re, c_im, bound),
            reference_proof_shape(c_re, c_im, bound)
        );
        assert!(
            !wasm_proof_row(&mut store, &instance, c_re, c_im, bound, 0).valid,
            "an invalid claim cannot materialize proof rows"
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

    let active_base = PaddedAirRow {
        valid: true,
        active: true,
        terminal: false,
        air: base,
    };
    let active_next = PaddedAirRow {
        valid: true,
        active: true,
        terminal: false,
        air: next,
    };
    assert!(wasm_padded_pair_holds(
        &mut store,
        &instance,
        -3048,
        2216,
        255,
        active_base,
        active_next,
    ));
    for (column, mutation) in one_unit_mutations(base).into_iter().enumerate() {
        assert!(
            !wasm_padded_pair_holds(
                &mut store,
                &instance,
                -3048,
                2216,
                255,
                PaddedAirRow {
                    air: mutation,
                    ..active_base
                },
                active_next,
            ),
            "padded constraint must reject current AIR mutation {column}"
        );
    }
    assert!(!wasm_padded_pair_holds_raw(
        &mut store, &instance, -3048, 2216, 255, 2, 0, base, 1, 0, next,
    ));
    assert!(!wasm_padded_pair_holds_raw(
        &mut store, &instance, -3048, 2216, 255, 1, 0, base, 2, 0, next,
    ));
    assert!(
        !wasm_padded_first_holds(
            &mut store,
            &instance,
            -3048,
            2216,
            255,
            PaddedAirRow {
                active: false,
                ..active_base
            },
        ),
        "the first row must be active"
    );
    assert!(
        !wasm_padded_last_holds(&mut store, &instance, active_next),
        "an active nonterminal row cannot close the padded domain"
    );

    let padded_case_rows = reference_rows(4095, 4095, 2);
    let terminal_air = {
        let row = *padded_case_rows.last().expect("directed point escapes");
        reference_air_row(row.step, row.zr, row.zi)
    };
    let terminal_proof_row = PaddedAirRow {
        valid: true,
        active: true,
        terminal: true,
        air: terminal_air,
    };
    let padding_row = PaddedAirRow {
        valid: true,
        active: false,
        terminal: false,
        air: terminal_air,
    };
    assert_eq!(reference_proof_shape(4095, 4095, 2).padded_length, 4);
    assert!(wasm_padded_pair_holds(
        &mut store,
        &instance,
        4095,
        4095,
        2,
        terminal_proof_row,
        padding_row,
    ));
    assert!(wasm_padded_pair_holds(
        &mut store,
        &instance,
        4095,
        4095,
        2,
        padding_row,
        padding_row,
    ));
    assert!(wasm_padded_last_holds(&mut store, &instance, padding_row,));
    for (column, mutation) in one_unit_mutations(terminal_air).into_iter().enumerate() {
        assert!(
            !wasm_padded_pair_holds(
                &mut store,
                &instance,
                4095,
                4095,
                2,
                padding_row,
                PaddedAirRow {
                    air: mutation,
                    ..padding_row
                },
            ),
            "padding fixed point must reject AIR mutation {column}"
        );
    }
}

#[test]
fn mandelbrot_residual_polynomials_execute_in_bn254_without_host_shims() {
    let bytes = compile_field_air();
    assert_eq!(
        function_imports(&bytes),
        Vec::<(String, String)>::new(),
        "BN254 AIR evaluation must be self-contained Fe/Wasm"
    );
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("load BN254 AIR Wasm");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("instantiate BN254 AIR Wasm");

    let local_export = "mandelbrot_bn254_local_residual_mask";
    let transition_export = "mandelbrot_bn254_transition_residual_mask";
    let padded_pair_export = "mandelbrot_bn254_padded_pair_residual_masks";
    let padded_first_export = "mandelbrot_bn254_padded_first_residual_mask";
    let padded_last_export = "mandelbrot_bn254_padded_last_residual_mask";
    for (c_re, c_im, bound) in [(-8192, -6144, 100), (4095, 4095, 2), (-3048, 2216, 255)] {
        let rows = reference_rows(c_re, c_im, bound);
        let mut selected = vec![0usize, rows.len() - 1];
        if rows.len() > 2 {
            selected.push(1);
            selected.push(rows.len() / 2);
        }
        selected.sort_unstable();
        selected.dedup();
        for index in selected {
            let row = rows[index];
            let air = reference_air_row(row.step, row.zr, row.zi);
            assert_eq!(
                call_mask(&mut store, &instance, local_export, &field_local_args(air),),
                0,
                "BN254 local residuals ({c_re}, {c_im}) Z_{}",
                row.step
            );
            if index + 1 < rows.len() {
                let next = rows[index + 1];
                let next_air = reference_air_row(next.step, next.zr, next.zi);
                assert_eq!(
                    call_mask(
                        &mut store,
                        &instance,
                        transition_export,
                        &field_transition_args(c_re, c_im, air, next_air),
                    ),
                    0,
                    "BN254 transition residuals ({c_re}, {c_im}) Z_{}",
                    row.step
                );
            }
        }
    }

    let rows = reference_rows(-3048, 2216, 255);
    let base = reference_air_row(rows[0].step, rows[0].zr, rows[0].zi);
    let next = reference_air_row(rows[1].step, rows[1].zr, rows[1].zi);
    for (column, mutation) in one_unit_mutations(base).into_iter().enumerate() {
        if (1..=9).contains(&column) {
            assert_ne!(
                call_mask(
                    &mut store,
                    &instance,
                    local_export,
                    &field_local_args(mutation),
                ),
                0,
                "BN254 local residuals must reject AIR mutation {column}"
            );
        }
    }

    for (label, mutated_c_re, mutated_c_im, mutated_row, mutated_next) in [
        ("c_re", -3047, 2216, base, next),
        ("c_im", -3048, 2217, base, next),
        (
            "q_re",
            -3048,
            2216,
            AirRow {
                q_re: base.q_re + 1,
                ..base
            },
            next,
        ),
        (
            "q_im",
            -3048,
            2216,
            AirRow {
                q_im: base.q_im + 1,
                ..base
            },
            next,
        ),
        (
            "next_step",
            -3048,
            2216,
            base,
            AirRow {
                row: Row {
                    step: next.row.step + 1,
                    ..next.row
                },
                ..next
            },
        ),
        (
            "next_zr",
            -3048,
            2216,
            base,
            AirRow {
                row: Row {
                    zr: next.row.zr + 1,
                    ..next.row
                },
                ..next
            },
        ),
        (
            "next_zi",
            -3048,
            2216,
            base,
            AirRow {
                row: Row {
                    zi: next.row.zi + 1,
                    ..next.row
                },
                ..next
            },
        ),
    ] {
        assert_ne!(
            call_mask(
                &mut store,
                &instance,
                transition_export,
                &field_transition_args(mutated_c_re, mutated_c_im, mutated_row, mutated_next),
            ),
            0,
            "BN254 transition residuals must reject {label} mutation"
        );
    }

    let mut non_bit_sign = field_local_args(base);
    non_bit_sign[0] = 2;
    assert_ne!(
        call_mask(&mut store, &instance, local_export, &non_bit_sign,) & 1,
        0,
        "the x sign polynomial must reject a non-bit"
    );

    let mut noncanonical_shift = base;
    noncanonical_shift.q_re -= 1;
    noncanonical_shift.r_re += 4096;
    assert_eq!(
        call_mask(
            &mut store,
            &instance,
            local_export,
            &field_local_args(noncanonical_shift),
        ),
        0,
        "equalities alone cannot reject a residual-zero out-of-range remainder"
    );

    let mut negative_zero = field_local_args(base);
    negative_zero[0] = 1;
    assert_eq!(
        call_mask(&mut store, &instance, local_export, &negative_zero,),
        0,
        "canonical-zero enforcement still requires the pending range/encoding columns"
    );

    let base_proof = field_proof_encoding(true, false, base);
    let next_proof = field_proof_encoding(true, false, next);
    assert_eq!(
        call_mask_pair(
            &mut store,
            &instance,
            padded_pair_export,
            &field_pair_args(-3048, 2216, base_proof, next_proof),
        ),
        (0, 0),
        "an active nonterminal pair must satisfy both state and selected math"
    );
    assert_eq!(
        call_mask(&mut store, &instance, padded_first_export, &base_proof),
        0,
        "the canonical initial row must satisfy the BN254 boundary"
    );
    assert_ne!(
        call_mask(&mut store, &instance, padded_last_export, &base_proof),
        0,
        "an active nonterminal row cannot close the BN254 domain"
    );

    let padded_rows = reference_rows(4095, 4095, 2);
    let terminal_air = reference_air_row(
        padded_rows.last().unwrap().step,
        padded_rows.last().unwrap().zr,
        padded_rows.last().unwrap().zi,
    );
    let terminal_proof = field_proof_encoding(true, true, terminal_air);
    let padding_proof = field_proof_encoding(false, false, terminal_air);
    assert_eq!(
        call_mask_pair(
            &mut store,
            &instance,
            padded_pair_export,
            &field_pair_args(4095, 4095, terminal_proof, padding_proof),
        ),
        (0, 0),
        "the terminal-to-padding fixed point must satisfy BN254 constraints"
    );
    assert_eq!(
        call_mask_pair(
            &mut store,
            &instance,
            padded_pair_export,
            &field_pair_args(4095, 4095, padding_proof, padding_proof),
        ),
        (0, 0),
        "padding must remain a fixed point"
    );
    assert_eq!(
        call_mask(&mut store, &instance, padded_last_export, &terminal_proof),
        0,
        "an active terminal row may close an unpadded domain"
    );
    assert_eq!(
        call_mask(&mut store, &instance, padded_last_export, &padding_proof),
        0,
        "inactive padding may close a padded domain"
    );

    let mut inactive_successor = next_proof;
    inactive_successor[0] = 0;
    assert_ne!(
        call_mask_pair(
            &mut store,
            &instance,
            padded_pair_export,
            &field_pair_args(-3048, 2216, base_proof, inactive_successor),
        )
        .0,
        0,
        "activity cannot stop before a terminal row"
    );
    let mut non_bit_active = base_proof;
    non_bit_active[0] = 2;
    assert_ne!(
        call_mask(&mut store, &instance, padded_first_export, &non_bit_active),
        0,
        "the active column must be boolean"
    );
    for column in 2..padding_proof.len() {
        let mut changed_padding = padding_proof;
        changed_padding[column] += 1;
        let changed_padding_masks = call_mask_pair(
            &mut store,
            &instance,
            padded_pair_export,
            &field_pair_args(4095, 4095, padding_proof, changed_padding),
        );
        assert_ne!(
            changed_padding_masks.0, 0,
            "terminal-state padding must reject encoded AIR mutation {column}"
        );
        assert_eq!(
            changed_padding_masks.1, 0,
            "the Mandelbrot transition must be disabled on padding"
        );
    }
    let wrong_claim_masks = call_mask_pair(
        &mut store,
        &instance,
        padded_pair_export,
        &field_pair_args(-3047, 2216, base_proof, next_proof),
    );
    assert_eq!(wrong_claim_masks.0, 0);
    assert_ne!(
        wrong_claim_masks.1, 0,
        "the selected transition must bind the public point"
    );
    assert_eq!(
        wrong_claim_masks.1,
        call_mask(
            &mut store,
            &instance,
            transition_export,
            &field_transition_args(-3047, 2216, base, next),
        ),
        "an active nonterminal selector must preserve every transition residual"
    );

    let mut premature_air = next;
    premature_air.row.terminal = true;
    let premature_terminal = field_proof_encoding(true, true, premature_air);
    let premature_padding = field_proof_encoding(false, false, premature_air);
    assert_eq!(
        call_mask_pair(
            &mut store,
            &instance,
            padded_pair_export,
            &field_pair_args(-3048, 2216, base_proof, premature_terminal),
        ),
        (0, 0),
        "state and transition equalities do not yet constrain the escape threshold"
    );
    assert_eq!(
        call_mask_pair(
            &mut store,
            &instance,
            padded_pair_export,
            &field_pair_args(-3048, 2216, premature_terminal, premature_padding),
        ),
        (0, 0),
        "the pending threshold comparison is required to reject premature closure"
    );
}
