//! Independent semantic gate for the first Fe-authored Mandelbrot trace root.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use num_bigint::BigUint;
use std::path::Path;
use url::Url;

const WIDTH: usize = 3;
const ROUNDS: usize = 65;
const ROUND_CONSTANT_COUNT: usize = WIDTH * ROUNDS;
const LIMB_BITS: usize = 13;
const LIMBS: usize = 20;
const ROW_WIDTHS: [usize; 17] = [21, 1, 15, 1, 15, 30, 30, 31, 1, 18, 12, 1, 19, 12, 1, 1, 1];
const PUBLIC_WIDTHS: [usize; 8] = [1, 14, 1, 13, 21, 21, 21, 22];
const SCALE: i64 = 4096;
const ESCAPE_MAGNITUDE: i64 = 1 << 26;
const CANONICAL_POSEIDON: &str = include_str!("../../fe/tests/fixtures/fe_test/const_poseidon.fe");

#[derive(Clone, Debug)]
struct SignedWord {
    sign: u32,
    magnitude: u32,
}

#[derive(Clone, Debug)]
struct AirRow {
    step: u32,
    zr: SignedWord,
    zi: SignedWord,
    rr: u32,
    ii: u32,
    magnitude: u32,
    q_re: SignedWord,
    r_re: u32,
    q_im: SignedWord,
    r_im: u32,
    terminal: u32,
}

#[derive(Clone, Debug)]
struct ProofRow {
    active: u32,
    terminal: u32,
    air: AirRow,
}

#[derive(Clone, Debug)]
struct PublicStatement {
    c_re: SignedWord,
    c_im: SignedWord,
    bound: u32,
    terminal_step: u32,
    trace_length: u32,
    padded_length: u32,
}

fn signed(value: i64) -> SignedWord {
    SignedWord {
        sign: u32::from(value < 0),
        magnitude: value.unsigned_abs() as u32,
    }
}

fn air_row(step: u32, zr: i64, zi: i64) -> AirRow {
    let rr = zr * zr;
    let ii = zi * zi;
    let magnitude = rr + ii;
    let real_numerator = rr - ii;
    let imaginary_numerator = 2 * zr * zi;
    AirRow {
        step,
        zr: signed(zr),
        zi: signed(zi),
        rr: rr as u32,
        ii: ii as u32,
        magnitude: magnitude as u32,
        q_re: signed(real_numerator.div_euclid(SCALE)),
        r_re: real_numerator.rem_euclid(SCALE) as u32,
        q_im: signed(imaginary_numerator.div_euclid(SCALE)),
        r_im: imaginary_numerator.rem_euclid(SCALE) as u32,
        terminal: u32::from(magnitude >= ESCAPE_MAGNITUDE),
    }
}

fn four_row_escape_trace() -> Vec<ProofRow> {
    let c_re = 3072i64;
    let c_im = 0i64;
    let mut zr = 0i64;
    let mut zi = 0i64;
    let mut rows = Vec::new();
    for step in 0..16 {
        let air = air_row(step, zr, zi);
        let terminal = air.terminal;
        rows.push(ProofRow {
            active: 1,
            terminal,
            air: air.clone(),
        });
        if terminal == 1 {
            break;
        }
        let real_numerator = zr * zr - zi * zi;
        let imaginary_numerator = 2 * zr * zi;
        zr = real_numerator.div_euclid(SCALE) + c_re;
        zi = imaginary_numerator.div_euclid(SCALE) + c_im;
    }
    assert_eq!(rows.len(), 4);
    assert_eq!(rows.iter().map(|row| row.terminal).sum::<u32>(), 1);
    rows
}

fn commitment_words(row: &ProofRow) -> [u32; 17] {
    [
        row.air.step,
        row.air.zr.sign,
        row.air.zr.magnitude,
        row.air.zi.sign,
        row.air.zi.magnitude,
        row.air.rr,
        row.air.ii,
        row.air.magnitude,
        row.air.q_re.sign,
        row.air.q_re.magnitude,
        row.air.r_re,
        row.air.q_im.sign,
        row.air.q_im.magnitude,
        row.air.r_im,
        row.air.terminal,
        row.active,
        row.terminal,
    ]
}

fn runtime_words(row: &ProofRow) -> [u32; 17] {
    [
        row.active,
        row.terminal,
        row.air.step,
        row.air.zr.sign,
        row.air.zr.magnitude,
        row.air.zi.sign,
        row.air.zi.magnitude,
        row.air.rr,
        row.air.ii,
        row.air.magnitude,
        row.air.q_re.sign,
        row.air.q_re.magnitude,
        row.air.r_re,
        row.air.q_im.sign,
        row.air.q_im.magnitude,
        row.air.r_im,
        row.air.terminal,
    ]
}

fn public_words(statement: &PublicStatement) -> [u32; 8] {
    [
        statement.c_re.sign,
        statement.c_re.magnitude,
        statement.c_im.sign,
        statement.c_im.magnitude,
        statement.bound,
        statement.terminal_step,
        statement.trace_length,
        statement.padded_length,
    ]
}

fn bn254_fr_prime() -> BigUint {
    BigUint::parse_bytes(
        b"21888242871839275222246405745257275088548364400416034343698204186575808495617",
        10,
    )
    .unwrap()
}

fn parse_const_block(source: &str, name: &str) -> Vec<BigUint> {
    let bytes = source.as_bytes();
    let start = source.find(name).expect("named const block");
    let equals = start + source[start..].find('=').expect("const equals");
    let open = equals + source[equals..].find('[').expect("array open");
    let mut depth = 0usize;
    let mut close = open;
    for (index, &byte) in bytes.iter().enumerate().skip(open) {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    close = index;
                    break;
                }
            }
            _ => {}
        }
    }
    let block = &source[open..=close];
    let mut values = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative) = block[cursor..].find("0x") {
        let value_start = cursor + relative + 2;
        let mut value_end = value_start;
        while value_end < block.len() && block.as_bytes()[value_end].is_ascii_hexdigit() {
            value_end += 1;
        }
        values.push(BigUint::parse_bytes(block[value_start..value_end].as_bytes(), 16).unwrap());
        cursor = value_end;
    }
    values
}

fn parameters() -> Vec<BigUint> {
    let mut values = parse_const_block(CANONICAL_POSEIDON, "POSEIDON_T3_ROUND_CONSTANTS");
    values.extend(parse_const_block(CANONICAL_POSEIDON, "POSEIDON_T3_MDS"));
    assert_eq!(values.len(), ROUND_CONSTANT_COUNT + WIDTH * WIDTH);
    values
}

fn permute(mut state: [BigUint; WIDTH], parameters: &[BigUint]) -> [BigUint; WIDTH] {
    let prime = bn254_fr_prime();
    for round in 0..ROUNDS {
        for lane in 0..WIDTH {
            state[lane] = (&state[lane] + &parameters[round * WIDTH + lane]) % &prime;
        }
        state[0] = state[0].modpow(&BigUint::from(5u32), &prime);
        if round < 4 || round >= 61 {
            state[1] = state[1].modpow(&BigUint::from(5u32), &prime);
            state[2] = state[2].modpow(&BigUint::from(5u32), &prime);
        }
        let before = state.clone();
        for row in 0..WIDTH {
            state[row] = BigUint::from(0u32);
            for column in 0..WIDTH {
                state[row] = (&state[row]
                    + &before[column] * &parameters[ROUND_CONSTANT_COUNT + row * WIDTH + column])
                    % &prime;
            }
        }
    }
    state
}

fn protocol_tag(label: &[u8; 4]) -> BigUint {
    BigUint::from(u32::from_be_bytes(*label))
}

fn pack_words<const N: usize>(words: [u32; N], widths: [usize; N]) -> BigUint {
    let mut packed = BigUint::from(0u32);
    let mut offset = 0usize;
    for (value, width) in words.into_iter().zip(widths) {
        assert!(BigUint::from(value) < (BigUint::from(1u32) << width));
        packed += BigUint::from(value) << offset;
        offset += width;
    }
    assert!(offset < 254);
    packed
}

fn row_commitment(row: &ProofRow, parameters: &[BigUint]) -> BigUint {
    assert_eq!(ROW_WIDTHS.iter().sum::<usize>(), 210);
    let packed = pack_words(commitment_words(row), ROW_WIDTHS);
    permute(
        [protocol_tag(b"MR01"), packed, BigUint::from(0u32)],
        parameters,
    )[0]
    .clone()
}

fn node_commitment(left: BigUint, right: BigUint, parameters: &[BigUint]) -> BigUint {
    permute([protocol_tag(b"MN01"), left, right], parameters)[0].clone()
}

fn trace_root4(rows: &[ProofRow], parameters: &[BigUint]) -> BigUint {
    let leaves: Vec<_> = rows
        .iter()
        .map(|row| row_commitment(row, parameters))
        .collect();
    let left = node_commitment(leaves[0].clone(), leaves[1].clone(), parameters);
    let right = node_commitment(leaves[2].clone(), leaves[3].clone(), parameters);
    node_commitment(left, right, parameters)
}

fn statement_commitment(
    statement: &PublicStatement,
    rows: &[ProofRow],
    parameters: &[BigUint],
) -> BigUint {
    assert_eq!(PUBLIC_WIDTHS.iter().sum::<usize>(), 114);
    permute(
        [
            protocol_tag(b"MT01"),
            pack_words(public_words(statement), PUBLIC_WIDTHS),
            trace_root4(rows, parameters),
        ],
        parameters,
    )[0]
    .clone()
}

fn compile_gate() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../demos/capstones/mandelbrot-proof/commitment");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url), "commitment diagnostics");
    let ingot = db.workspace().containing_ingot(&db, url).unwrap();
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let wasm = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("Mandelbrot commitment should compile")
        .into_bytecode()
        .expect("Wasm bytecode");
    wasmparser::validate(&wasm).unwrap();
    wasm
}

fn call_root(
    store: &mut wasmtime::Store<()>,
    root: &wasmtime::Func,
    reset: &wasmtime::TypedFunc<(), ()>,
    rows: &[ProofRow],
) -> BigUint {
    reset.call(&mut *store, ()).unwrap();
    let arguments: Vec<_> = rows
        .iter()
        .flat_map(runtime_words)
        .map(|value| wasmtime::Val::I32(value as i32))
        .collect();
    assert_eq!(arguments.len(), 68);
    let mut output = vec![wasmtime::Val::I32(0); LIMBS];
    root.call(&mut *store, &arguments, &mut output).unwrap();
    output
        .into_iter()
        .enumerate()
        .fold(BigUint::from(0u32), |value, (word, limb)| {
            let wasmtime::Val::I32(limb) = limb else {
                panic!("root word {word} was not i32")
            };
            value + (BigUint::from(limb as u32) << (word * LIMB_BITS))
        })
}

fn call_statement(
    store: &mut wasmtime::Store<()>,
    commitment: &wasmtime::Func,
    reset: &wasmtime::TypedFunc<(), ()>,
    statement: &PublicStatement,
    rows: &[ProofRow],
) -> BigUint {
    reset.call(&mut *store, ()).unwrap();
    let arguments: Vec<_> = public_words(statement)
        .into_iter()
        .chain(rows.iter().flat_map(runtime_words))
        .map(|value| wasmtime::Val::I32(value as i32))
        .collect();
    assert_eq!(arguments.len(), 76);
    let mut output = vec![wasmtime::Val::I32(0); LIMBS];
    commitment
        .call(&mut *store, &arguments, &mut output)
        .unwrap();
    output
        .into_iter()
        .enumerate()
        .fold(BigUint::from(0u32), |value, (word, limb)| {
            let wasmtime::Val::I32(limb) = limb else {
                panic!("statement word {word} was not i32")
            };
            value + (BigUint::from(limb as u32) << (word * LIMB_BITS))
        })
}

#[test]
fn fe_trace_and_statement_match_independent_orbit_encoding_poseidon_and_merkle_model() {
    let parameters = parameters();
    assert_eq!(
        permute([0u32.into(), 0u32.into(), 0u32.into()], &parameters)[0],
        BigUint::parse_bytes(
            b"2098f5fb9e239eab3ceac3f27b81e481dc3124d55ffed523a839ee8446b64864",
            16,
        )
        .unwrap()
    );
    let rows = four_row_escape_trace();
    let expected = trace_root4(&rows, &parameters);
    let statement = PublicStatement {
        c_re: signed(3072),
        c_im: signed(0),
        bound: 16,
        terminal_step: 3,
        trace_length: 4,
        padded_length: 4,
    };
    let expected_statement = statement_commitment(&statement, &rows, &parameters);

    let wasm = compile_gate();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).unwrap();
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let root = instance
        .get_func(&mut store, "trace_root4_plain_words")
        .unwrap();
    let bind = instance
        .get_func(&mut store, "statement_commitment4_plain_words")
        .unwrap();
    let reset = instance
        .get_typed_func::<(), ()>(&mut store, "fe_cabi_reset")
        .unwrap();
    let actual = call_root(&mut store, &root, &reset, &rows);
    assert_eq!(actual, expected);
    assert_eq!(
        call_statement(&mut store, &bind, &reset, &statement, &rows),
        expected_statement
    );

    for word in 0..17 {
        let mut mutated = rows.clone();
        match word {
            0 => mutated[1].air.step += 1,
            1 => mutated[1].air.zr.sign ^= 1,
            2 => mutated[1].air.zr.magnitude += 1,
            3 => mutated[1].air.zi.sign ^= 1,
            4 => mutated[1].air.zi.magnitude += 1,
            5 => mutated[1].air.rr += 1,
            6 => mutated[1].air.ii += 1,
            7 => mutated[1].air.magnitude += 1,
            8 => mutated[1].air.q_re.sign ^= 1,
            9 => mutated[1].air.q_re.magnitude += 1,
            10 => mutated[1].air.r_re += 1,
            11 => mutated[1].air.q_im.sign ^= 1,
            12 => mutated[1].air.q_im.magnitude += 1,
            13 => mutated[1].air.r_im += 1,
            14 => mutated[1].air.terminal ^= 1,
            15 => mutated[1].active ^= 1,
            16 => mutated[1].terminal ^= 1,
            _ => unreachable!(),
        }
        let expected_mutation = trace_root4(&mutated, &parameters);
        assert_ne!(expected_mutation, expected, "word {word} did not bind");
        assert_eq!(
            call_root(&mut store, &root, &reset, &mutated),
            expected_mutation,
            "word {word} mutation"
        );
    }

    let mut reordered = rows.clone();
    reordered.swap(0, 1);
    let reordered_expected = trace_root4(&reordered, &parameters);
    assert_ne!(reordered_expected, expected);
    assert_eq!(
        call_root(&mut store, &root, &reset, &reordered),
        reordered_expected
    );

    for word in 0..8 {
        let mut mutated = statement.clone();
        match word {
            0 => mutated.c_re.sign ^= 1,
            1 => mutated.c_re.magnitude += 1,
            2 => mutated.c_im.sign ^= 1,
            3 => mutated.c_im.magnitude += 1,
            4 => mutated.bound += 1,
            5 => mutated.terminal_step += 1,
            6 => mutated.trace_length += 1,
            7 => mutated.padded_length += 1,
            _ => unreachable!(),
        }
        let expected_mutation = statement_commitment(&mutated, &rows, &parameters);
        assert_ne!(expected_mutation, expected_statement, "public word {word}");
        assert_eq!(
            call_statement(&mut store, &bind, &reset, &mutated, &rows),
            expected_mutation,
            "public word {word} mutation"
        );
    }

    let reordered_statement = statement_commitment(&statement, &reordered, &parameters);
    assert_ne!(reordered_statement, expected_statement);
    assert_eq!(
        call_statement(&mut store, &bind, &reset, &statement, &reordered),
        reordered_statement
    );
}
