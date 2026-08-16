//! Independent bigint oracle for the first Fe-authored composition polynomial.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use num_bigint::BigUint;
use std::path::Path;
use std::sync::OnceLock;
use url::Url;
use wasmtime::Val;

const SCALE: i64 = 4096;
const ESCAPE_MAGNITUDE: i64 = 1 << 26;
const AUXILIARY_WIDTH: usize = 411;
const LIMB_BITS: usize = 13;
const WIDTH: usize = 3;
const ROUNDS: usize = 65;
const ROUND_CONSTANT_COUNT: usize = WIDTH * ROUNDS;
const ROW_WIDTHS: [usize; 17] = [21, 1, 15, 1, 15, 30, 30, 31, 1, 18, 12, 1, 19, 12, 1, 1, 1];
const PUBLIC_WIDTHS: [usize; 8] = [1, 14, 1, 13, 21, 21, 21, 22];
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct F(BigUint);

impl F {
    fn new(value: BigUint) -> Self {
        Self(value % modulus())
    }

    fn zero() -> Self {
        Self(BigUint::from(0u32))
    }

    fn one() -> Self {
        Self(BigUint::from(1u32))
    }

    fn from_u32(value: u32) -> Self {
        Self(BigUint::from(value))
    }

    fn add(&self, other: &Self) -> Self {
        Self::new(&self.0 + &other.0)
    }

    fn sub(&self, other: &Self) -> Self {
        if self.0 >= other.0 {
            Self(&self.0 - &other.0)
        } else {
            Self(modulus() - (&other.0 - &self.0))
        }
    }

    fn mul(&self, other: &Self) -> Self {
        Self::new(&self.0 * &other.0)
    }

    fn square(&self) -> Self {
        self.mul(self)
    }

    fn pow_u32(&self, exponent: u32) -> Self {
        Self(self.0.modpow(&BigUint::from(exponent), modulus()))
    }

    fn inverse(&self) -> Self {
        assert_ne!(self, &Self::zero(), "oracle division by zero");
        Self(self.0.modpow(&(modulus() - BigUint::from(2u32)), modulus()))
    }
}

#[derive(Clone)]
struct SignedField {
    sign: F,
    magnitude: F,
    value: F,
}

#[derive(Clone)]
struct AirFieldRow {
    step: F,
    zr: SignedField,
    zi: SignedField,
    rr: F,
    ii: F,
    magnitude: F,
    q_re: SignedField,
    r_re: F,
    q_im: SignedField,
    r_im: F,
    terminal: F,
}

#[derive(Clone)]
struct ProofFieldRow {
    active: F,
    terminal: F,
    air: AirFieldRow,
}

struct Fold {
    challenge: F,
    power: F,
    value: F,
}

impl Fold {
    fn new(challenge: F) -> Self {
        Self {
            challenge,
            power: F::one(),
            value: F::zero(),
        }
    }

    fn absorb(&mut self, residual: F) {
        self.value = self.value.add(&self.power.mul(&residual));
        self.power = self.power.mul(&self.challenge);
    }

    fn next_family(&mut self) {
        self.value = F::zero();
    }
}

fn modulus() -> &'static BigUint {
    static MODULUS: OnceLock<BigUint> = OnceLock::new();
    MODULUS.get_or_init(|| {
        BigUint::parse_bytes(
            b"21888242871839275222246405745257275088548364400416034343698204186575808495617",
            10,
        )
        .expect("BN254 Fr modulus")
    })
}

fn subgroup_root(log_n: u32) -> F {
    let maximal =
        BigUint::from(5u32).modpow(&((modulus() - BigUint::from(1u32)) >> 28usize), modulus());
    F(maximal.modpow(&(BigUint::from(1u32) << (28u32 - log_n)), modulus()))
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

fn trace4(c_re: i64, c_im: i64, bound: u32) -> Option<[ProofRow; 4]> {
    if !(-8192..4096).contains(&c_re) || !(-6144..6144).contains(&c_im) || bound > 1_048_576 {
        return None;
    }
    let mut zr = 0i64;
    let mut zi = 0i64;
    let mut rows = Vec::new();
    for step in 0..=bound {
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
        let next_zr = (zr * zr - zi * zi).div_euclid(SCALE) + c_re;
        let next_zi = (2 * zr * zi).div_euclid(SCALE) + c_im;
        zr = next_zr;
        zi = next_zi;
    }
    if rows.last().is_none_or(|row| row.terminal == 0) || rows.len().next_power_of_two() != 4 {
        return None;
    }
    let terminal = rows.last().unwrap().air.clone();
    while rows.len() < 4 {
        rows.push(ProofRow {
            active: 0,
            terminal: 0,
            air: terminal.clone(),
        });
    }
    Some(rows.try_into().expect("four-row trace"))
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

fn append_range_witness(bits: &mut Vec<u32>, value: u32, width: usize) {
    let decomposition = (0..width).map(|bit| (value >> bit) & 1).collect::<Vec<_>>();
    bits.extend(decomposition.iter().copied());
    let mut seen = 0u32;
    bits.extend(decomposition.into_iter().map(|bit| {
        seen |= bit;
        seen
    }));
}

fn auxiliary_bits(row: &ProofRow) -> Vec<u32> {
    let mut bits = Vec::with_capacity(AUXILIARY_WIDTH);
    append_range_witness(&mut bits, row.air.step, 21);
    append_range_witness(&mut bits, row.air.zr.magnitude, 15);
    append_range_witness(&mut bits, row.air.zi.magnitude, 15);
    append_range_witness(&mut bits, row.air.rr, 30);
    append_range_witness(&mut bits, row.air.ii, 30);
    append_range_witness(&mut bits, row.air.magnitude, 31);
    append_range_witness(&mut bits, row.air.q_re.magnitude, 18);
    append_range_witness(&mut bits, row.air.r_re, 12);
    append_range_witness(&mut bits, row.air.q_im.magnitude, 19);
    append_range_witness(&mut bits, row.air.r_im, 12);
    let mut seen = 0u32;
    bits.extend((26..31).map(|bit| {
        seen |= (row.air.magnitude >> bit) & 1;
        seen
    }));
    assert_eq!(bits.len(), AUXILIARY_WIDTH);
    bits
}

/// Direct inverse DFT plus polynomial evaluation. This intentionally does not
/// replay Fe's radix-2 butterfly schedule.
fn evaluate_trace_column(values: [u32; 4], point: &F) -> F {
    let root = subgroup_root(2);
    let inverse_root = root.inverse();
    let inverse_size = F::from_u32(4).inverse();
    let coefficients: [F; 4] = std::array::from_fn(|coefficient| {
        let mut sum = F::zero();
        for (sample, value) in values.iter().enumerate() {
            let twiddle = inverse_root.pow_u32((sample * coefficient) as u32);
            sum = sum.add(&F::from_u32(*value).mul(&twiddle));
        }
        sum.mul(&inverse_size)
    });
    coefficients
        .iter()
        .rev()
        .fold(F::zero(), |value, coefficient| {
            value.mul(point).add(coefficient)
        })
}

fn main_at(rows: &[ProofRow; 4], point: &F) -> [F; 17] {
    std::array::from_fn(|column| {
        evaluate_trace_column(
            std::array::from_fn(|row| commitment_words(&rows[row])[column]),
            point,
        )
    })
}

fn auxiliary_at(rows: &[ProofRow; 4], point: &F) -> Vec<F> {
    let encoded: [Vec<u32>; 4] = std::array::from_fn(|row| auxiliary_bits(&rows[row]));
    (0..AUXILIARY_WIDTH)
        .map(|column| evaluate_trace_column(std::array::from_fn(|row| encoded[row][column]), point))
        .collect()
}

fn signed_field(sign: F, magnitude: F) -> SignedField {
    let value = magnitude.sub(&sign.mul(&magnitude).mul(&F::from_u32(2)));
    SignedField {
        sign,
        magnitude,
        value,
    }
}

fn proof_field_row(words: &[F; 17]) -> ProofFieldRow {
    ProofFieldRow {
        active: words[15].clone(),
        terminal: words[16].clone(),
        air: AirFieldRow {
            step: words[0].clone(),
            zr: signed_field(words[1].clone(), words[2].clone()),
            zi: signed_field(words[3].clone(), words[4].clone()),
            rr: words[5].clone(),
            ii: words[6].clone(),
            magnitude: words[7].clone(),
            q_re: signed_field(words[8].clone(), words[9].clone()),
            r_re: words[10].clone(),
            q_im: signed_field(words[11].clone(), words[12].clone()),
            r_im: words[13].clone(),
            terminal: words[14].clone(),
        },
    }
}

fn bit_residual(value: &F) -> F {
    value.mul(&value.sub(&F::one()))
}

fn local_residuals(row: &AirFieldRow) -> Vec<F> {
    vec![
        bit_residual(&row.zr.sign),
        bit_residual(&row.zi.sign),
        bit_residual(&row.q_re.sign),
        bit_residual(&row.q_im.sign),
        row.rr.sub(&row.zr.value.square()),
        row.ii.sub(&row.zi.value.square()),
        row.magnitude.sub(&row.rr).sub(&row.ii),
        row.rr
            .sub(&row.ii)
            .sub(&F::from_u32(4096).mul(&row.q_re.value))
            .sub(&row.r_re),
        F::from_u32(2)
            .mul(&row.zr.value)
            .mul(&row.zi.value)
            .sub(&F::from_u32(4096).mul(&row.q_im.value))
            .sub(&row.r_im),
    ]
}

fn row_state_residuals(row: &ProofFieldRow) -> Vec<F> {
    vec![
        bit_residual(&row.active),
        bit_residual(&row.terminal),
        bit_residual(&row.air.terminal),
        row.terminal.sub(&row.active.mul(&row.air.terminal)),
        F::one()
            .sub(&row.active)
            .mul(&row.air.terminal.sub(&F::one())),
    ]
}

fn air_words(row: &AirFieldRow) -> Vec<F> {
    vec![
        row.step.clone(),
        row.zr.sign.clone(),
        row.zr.magnitude.clone(),
        row.zi.sign.clone(),
        row.zi.magnitude.clone(),
        row.rr.clone(),
        row.ii.clone(),
        row.magnitude.clone(),
        row.q_re.sign.clone(),
        row.q_re.magnitude.clone(),
        row.r_re.clone(),
        row.q_im.sign.clone(),
        row.q_im.magnitude.clone(),
        row.r_im.clone(),
        row.terminal.clone(),
    ]
}

fn transition_residuals(
    c_re: &SignedField,
    c_im: &SignedField,
    row: &AirFieldRow,
    next: &AirFieldRow,
) -> Vec<F> {
    vec![
        bit_residual(&c_re.sign),
        bit_residual(&c_im.sign),
        bit_residual(&row.q_re.sign),
        bit_residual(&row.q_im.sign),
        bit_residual(&next.zr.sign),
        bit_residual(&next.zi.sign),
        next.step.sub(&row.step).sub(&F::one()),
        next.zr.value.sub(&row.q_re.value).sub(&c_re.value),
        next.zi.value.sub(&row.q_im.value).sub(&c_im.value),
    ]
}

fn absorb_residuals(fold: &mut Fold, residuals: impl IntoIterator<Item = F>) {
    for residual in residuals {
        fold.absorb(residual);
    }
}

fn absorb_range(fold: &mut Fold, value: &F, auxiliary: &[F], start: usize, width: usize) {
    let mut reconstructed = F::zero();
    let mut weight = F::one();
    let mut previous_any = F::zero();
    for index in 0..width {
        let bit = &auxiliary[start + index];
        let any = &auxiliary[start + width + index];
        fold.absorb(bit_residual(bit));
        fold.absorb(bit_residual(any));
        fold.absorb(any.sub(&previous_any).sub(bit).add(&previous_any.mul(bit)));
        reconstructed = reconstructed.add(&bit.mul(&weight));
        weight = weight.add(&weight);
        previous_any = any.clone();
    }
    fold.absorb(value.sub(&reconstructed));
}

fn absorb_signed_range(
    fold: &mut Fold,
    value: &SignedField,
    auxiliary: &[F],
    start: usize,
    width: usize,
) {
    fold.absorb(bit_residual(&value.sign));
    absorb_range(fold, &value.magnitude, auxiliary, start, width);
    let nonzero = &auxiliary[start + 2 * width - 1];
    fold.absorb(value.sign.mul(&F::one().sub(nonzero)));
}

fn absorb_terminal_range(
    fold: &mut Fold,
    row: &AirFieldRow,
    auxiliary: &[F],
    magnitude_start: usize,
    terminal_start: usize,
) {
    absorb_range(fold, &row.magnitude, auxiliary, magnitude_start, 31);
    fold.absorb(bit_residual(&row.terminal));
    let mut previous = F::zero();
    for index in 0..5 {
        let bit = &auxiliary[magnitude_start + 26 + index];
        let any = &auxiliary[terminal_start + index];
        fold.absorb(bit_residual(any));
        fold.absorb(any.sub(&previous).sub(bit).add(&previous.mul(bit)));
        previous = any.clone();
    }
    fold.absorb(row.terminal.sub(&previous));
}

fn constraint_numerators(
    challenge: F,
    c_re: SignedField,
    c_im: SignedField,
    row: &ProofFieldRow,
    next: &ProofFieldRow,
    auxiliary: &[F],
) -> [F; 4] {
    let one = F::one();
    let continue_selector = row.active.mul(&one.sub(&row.terminal));
    let freeze_selector = row.terminal.add(&one.sub(&row.active));
    let mut fold = Fold::new(challenge);
    absorb_residuals(&mut fold, local_residuals(&row.air));
    absorb_residuals(&mut fold, row_state_residuals(row));

    let mut cursor = 0usize;
    absorb_range(&mut fold, &row.air.step, auxiliary, cursor, 21);
    cursor += 2 * 21;
    absorb_signed_range(&mut fold, &row.air.zr, auxiliary, cursor, 15);
    cursor += 2 * 15;
    absorb_signed_range(&mut fold, &row.air.zi, auxiliary, cursor, 15);
    cursor += 2 * 15;
    absorb_range(&mut fold, &row.air.rr, auxiliary, cursor, 30);
    cursor += 2 * 30;
    absorb_range(&mut fold, &row.air.ii, auxiliary, cursor, 30);
    cursor += 2 * 30;
    let magnitude_start = cursor;
    cursor += 2 * 31;
    absorb_signed_range(&mut fold, &row.air.q_re, auxiliary, cursor, 18);
    cursor += 2 * 18;
    absorb_range(&mut fold, &row.air.r_re, auxiliary, cursor, 12);
    cursor += 2 * 12;
    absorb_signed_range(&mut fold, &row.air.q_im, auxiliary, cursor, 19);
    cursor += 2 * 19;
    absorb_range(&mut fold, &row.air.r_im, auxiliary, cursor, 12);
    cursor += 2 * 12;
    assert_eq!(cursor + 5, AUXILIARY_WIDTH);
    absorb_terminal_range(&mut fold, &row.air, auxiliary, magnitude_start, cursor);
    let all_rows = fold.value.clone();

    fold.next_family();
    absorb_residuals(&mut fold, row_state_residuals(row));
    absorb_residuals(&mut fold, row_state_residuals(next));
    fold.absorb(continue_selector.mul(&next.active.sub(&one)));
    fold.absorb(freeze_selector.mul(&next.active));
    fold.absorb(freeze_selector.mul(&next.terminal));
    for (current, following) in air_words(&row.air).iter().zip(air_words(&next.air)) {
        fold.absorb(freeze_selector.mul(&following.sub(current)));
    }
    for residual in transition_residuals(&c_re, &c_im, &row.air, &next.air) {
        fold.absorb(continue_selector.mul(&residual));
    }
    let pair_rows = fold.value.clone();

    fold.next_family();
    absorb_residuals(&mut fold, row_state_residuals(row));
    fold.absorb(row.active.sub(&one));
    fold.absorb(row.terminal.clone());
    fold.absorb(row.air.step.clone());
    fold.absorb(row.air.zr.sign.clone());
    fold.absorb(row.air.zr.magnitude.clone());
    fold.absorb(row.air.zi.sign.clone());
    fold.absorb(row.air.zi.magnitude.clone());
    let first_row = fold.value.clone();

    fold.next_family();
    absorb_residuals(&mut fold, row_state_residuals(row));
    fold.absorb(row.active.mul(&one.sub(&row.terminal)));
    [all_rows, pair_rows, first_row, fold.value]
}

fn composition_at(rows: &[ProofRow; 4], c_re: i64, c_im: i64, challenge: F, index: u32) -> F {
    let output_root = subgroup_root(4);
    let trace_root = subgroup_root(2);
    let point = F::from_u32(5).mul(&output_root.pow_u32(index));
    let next_point = point.mul(&trace_root);
    let row = proof_field_row(&main_at(rows, &point));
    let next = proof_field_row(&main_at(rows, &next_point));
    let auxiliary = auxiliary_at(rows, &point);
    let numerators = constraint_numerators(
        challenge,
        signed_field(
            F::from_u32(u32::from(c_re < 0)),
            F::from_u32(c_re.unsigned_abs() as u32),
        ),
        signed_field(
            F::from_u32(u32::from(c_im < 0)),
            F::from_u32(c_im.unsigned_abs() as u32),
        ),
        &row,
        &next,
        &auxiliary,
    );
    let trace_zerofier = point.pow_u32(4).sub(&F::one());
    let last_trace_point = trace_root.inverse();
    numerators[0]
        .mul(&trace_zerofier.inverse())
        .add(
            &numerators[1]
                .mul(&point.sub(&last_trace_point))
                .mul(&trace_zerofier.inverse()),
        )
        .add(&numerators[2].mul(&point.sub(&F::one()).inverse()))
        .add(&numerators[3].mul(&point.sub(&last_trace_point).inverse()))
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
        values.push(
            BigUint::parse_bytes(block[value_start..value_end].as_bytes(), 16)
                .expect("Poseidon constant"),
        );
        cursor = value_end;
    }
    values
}

fn poseidon_parameters() -> Vec<BigUint> {
    let mut values = parse_const_block(CANONICAL_POSEIDON, "POSEIDON_T3_ROUND_CONSTANTS");
    values.extend(parse_const_block(CANONICAL_POSEIDON, "POSEIDON_T3_MDS"));
    assert_eq!(values.len(), ROUND_CONSTANT_COUNT + WIDTH * WIDTH);
    values
}

fn permute(mut state: [BigUint; WIDTH], parameters: &[BigUint]) -> [BigUint; WIDTH] {
    for round in 0..ROUNDS {
        for lane in 0..WIDTH {
            state[lane] = (&state[lane] + &parameters[round * WIDTH + lane]) % modulus();
        }
        state[0] = state[0].modpow(&BigUint::from(5u32), modulus());
        if round < 4 || round >= 61 {
            state[1] = state[1].modpow(&BigUint::from(5u32), modulus());
            state[2] = state[2].modpow(&BigUint::from(5u32), modulus());
        }
        let before = state.clone();
        for row in 0..WIDTH {
            state[row] = BigUint::from(0u32);
            for column in 0..WIDTH {
                state[row] = (&state[row]
                    + &before[column] * &parameters[ROUND_CONSTANT_COUNT + row * WIDTH + column])
                    % modulus();
            }
        }
    }
    state
}

fn protocol_tag(label: &[u8; 4]) -> BigUint {
    BigUint::from(u32::from_be_bytes(*label))
}

fn hash(label: &[u8; 4], left: BigUint, right: BigUint, parameters: &[BigUint]) -> BigUint {
    permute([protocol_tag(label), left, right], parameters)[0].clone()
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

fn packed_auxiliary(row: &ProofRow) -> [BigUint; 2] {
    let mut packed = [BigUint::from(0u32), BigUint::from(0u32)];
    for (offset, bit) in auxiliary_bits(row).into_iter().enumerate() {
        packed[offset / 253] += BigUint::from(bit) << (offset % 253);
    }
    packed
}

fn merkle_root(mut level: Vec<BigUint>, node_label: &[u8; 4], parameters: &[BigUint]) -> BigUint {
    assert!(!level.is_empty() && level.len().is_power_of_two());
    while level.len() > 1 {
        level = level
            .chunks_exact(2)
            .map(|pair| hash(node_label, pair[0].clone(), pair[1].clone(), parameters))
            .collect();
    }
    level.pop().unwrap()
}

struct ProductionOracle {
    evaluations: Vec<BigUint>,
    root: BigUint,
    transcript: BigUint,
    fri: BigUint,
}

struct FriLayerOracle {
    evaluations: Vec<BigUint>,
    root: BigUint,
    transcript: BigUint,
    next_challenge: BigUint,
}

fn production_oracle(rows: &[ProofRow; 4], c_re: i64, c_im: i64, bound: u32) -> ProductionOracle {
    let parameters = poseidon_parameters();
    let trace_root = merkle_root(
        rows.iter()
            .map(|row| {
                hash(
                    b"MR01",
                    pack_words(commitment_words(row), ROW_WIDTHS),
                    BigUint::from(0u32),
                    &parameters,
                )
            })
            .collect(),
        b"MN01",
        &parameters,
    );
    let auxiliary_root = merkle_root(
        rows.iter()
            .map(|row| {
                let packed = packed_auxiliary(row);
                hash(b"AR01", packed[0].clone(), packed[1].clone(), &parameters)
            })
            .collect(),
        b"AN01",
        &parameters,
    );
    let terminal_step = rows
        .iter()
        .find(|row| row.terminal == 1)
        .expect("terminal row")
        .air
        .step;
    let public = pack_words(
        [
            u32::from(c_re < 0),
            c_re.unsigned_abs() as u32,
            u32::from(c_im < 0),
            c_im.unsigned_abs() as u32,
            bound,
            terminal_step,
            terminal_step + 1,
            4,
        ],
        PUBLIC_WIDTHS,
    );
    let statement = hash(b"MT01", public, trace_root, &parameters);
    let proof_transcript = hash(b"AT01", statement, auxiliary_root, &parameters);
    let challenge = hash(
        b"MC01",
        proof_transcript.clone(),
        BigUint::from(0u32),
        &parameters,
    );
    let evaluations = (0..16)
        .map(|index| composition_at(rows, c_re, c_im, F(challenge.clone()), index).0)
        .collect::<Vec<_>>();
    let root = merkle_root(
        evaluations
            .iter()
            .cloned()
            .map(|value| hash(b"CR01", value, BigUint::from(0u32), &parameters))
            .collect(),
        b"CN01",
        &parameters,
    );
    let transcript = hash(b"CT01", proof_transcript, root.clone(), &parameters);
    let fri = hash(
        b"FC01",
        transcript.clone(),
        BigUint::from(0u32),
        &parameters,
    );
    ProductionOracle {
        evaluations,
        root,
        transcript,
        fri,
    }
}

fn interpolate_composition_coset(evaluations: &[BigUint]) -> Vec<F> {
    assert_eq!(evaluations.len(), 16);
    let root = subgroup_root(4);
    let inverse_root = root.inverse();
    let inverse_size = F::from_u32(16).inverse();
    let shift = F::from_u32(5);
    (0..16)
        .map(|coefficient| {
            let mut scaled = F::zero();
            for (sample, value) in evaluations.iter().enumerate() {
                scaled = scaled.add(
                    &F(value.clone()).mul(&inverse_root.pow_u32((sample * coefficient) as u32)),
                );
            }
            scaled
                .mul(&inverse_size)
                .mul(&shift.pow_u32(coefficient as u32).inverse())
        })
        .collect()
}

fn fri_layer_oracle(production: &ProductionOracle) -> FriLayerOracle {
    let parameters = poseidon_parameters();
    let challenge = F(production.fri.clone());
    let inverse_two = F::from_u32(2).inverse();
    let root16 = subgroup_root(4);
    let mut point = F::from_u32(5);
    let mut evaluations = Vec::with_capacity(8);
    for index in 0..8 {
        let positive = F(production.evaluations[index].clone());
        let negative = F(production.evaluations[index + 8].clone());
        let even = positive.add(&negative).mul(&inverse_two);
        let odd = positive
            .sub(&negative)
            .mul(&inverse_two)
            .mul(&point.inverse());
        evaluations.push(even.add(&challenge.mul(&odd)).0);
        point = point.mul(&root16);
    }

    let coefficients = interpolate_composition_coset(&production.evaluations);
    let folded_coefficients = (0..8)
        .map(|coefficient| {
            coefficients[2 * coefficient].add(&challenge.mul(&coefficients[2 * coefficient + 1]))
        })
        .collect::<Vec<_>>();
    point = F::from_u32(5);
    for (index, folded) in evaluations.iter().enumerate() {
        let squared_point = point.square();
        let expected = folded_coefficients
            .iter()
            .rev()
            .fold(F::zero(), |value, coefficient| {
                value.mul(&squared_point).add(coefficient)
            });
        assert_eq!(
            &expected.0, folded,
            "pair fold and coefficient fold differ at FRI point {index}",
        );
        point = point.mul(&root16);
    }

    let root = merkle_root(
        evaluations
            .iter()
            .cloned()
            .map(|value| hash(b"FR01", value, BigUint::from(0u32), &parameters))
            .collect(),
        b"FN01",
        &parameters,
    );
    let transcript = hash(
        b"FT01",
        production.transcript.clone(),
        root.clone(),
        &parameters,
    );
    let next_challenge = hash(
        b"FC02",
        transcript.clone(),
        BigUint::from(0u32),
        &parameters,
    );
    FriLayerOracle {
        evaluations,
        root,
        transcript,
        next_challenge,
    }
}

fn compile_gate() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_composition_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("Mandelbrot composition ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected composition diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("composition gate should compile")
        .into_bytecode()
        .expect("Wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("composition Wasm should validate");
    bytes
}

fn call_words(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    args: &[i32],
    result_count: usize,
) -> Vec<u32> {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("`{name}` export should exist"));
    let params = args.iter().copied().map(Val::I32).collect::<Vec<_>>();
    let mut results = vec![Val::I32(0); result_count];
    function
        .call(&mut *store, &params, &mut results)
        .unwrap_or_else(|error| panic!("{name} should run: {error:?}"));
    results
        .into_iter()
        .map(|result| match result {
            Val::I32(word) => word as u32,
            other => panic!("{name} result must be i32, got {other:?}"),
        })
        .collect()
}

fn plain_limbs(words: &[u32]) -> BigUint {
    words
        .iter()
        .enumerate()
        .fold(BigUint::from(0u32), |value, (word, limb)| {
            assert!(*limb < (1 << LIMB_BITS));
            value + (BigUint::from(*limb) << (word * LIMB_BITS))
        })
}

#[test]
fn composition_and_first_fri_layer_match_independent_bigint_oracle() {
    let wasm = compile_gate();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).expect("Wasm module should load");
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("zero-import composition gate should instantiate");

    let rows = trace4(3072, 0, 16).expect("canonical four-row escape trace");
    for evaluation in 0..16u32 {
        let output = call_words(
            &mut store,
            &instance,
            "composition4x16_challenge_evaluation_words",
            &[3072, 0, 16, 7, evaluation as i32],
            21,
        );
        assert_eq!(output[0], 1, "evaluation {evaluation} must be valid");
        assert_eq!(
            plain_limbs(&output[1..]),
            composition_at(&rows, 3072, 0, F::from_u32(7), evaluation).0,
            "independent composition mismatch at coset point {evaluation}",
        );
    }

    let changed_challenge = call_words(
        &mut store,
        &instance,
        "composition4x16_challenge_evaluation_words",
        &[3072, 0, 16, 11, 5],
        21,
    );
    assert_eq!(changed_challenge[0], 1);
    assert_eq!(
        plain_limbs(&changed_challenge[1..]),
        composition_at(&rows, 3072, 0, F::from_u32(11), 5).0,
    );

    let changed_claim_rows = trace4(3200, 0, 16).expect("second four-row escape trace");
    let changed_claim = call_words(
        &mut store,
        &instance,
        "composition4x16_challenge_evaluation_words",
        &[3200, 0, 16, 7, 9],
        21,
    );
    assert_eq!(changed_claim[0], 1);
    assert_eq!(
        plain_limbs(&changed_claim[1..]),
        composition_at(&changed_claim_rows, 3200, 0, F::from_u32(7), 9).0,
    );

    for invalid in [[0, 0, 16, 7, 0], [4096, 0, 16, 7, 0], [3072, 0, 16, 7, 16]] {
        let output = call_words(
            &mut store,
            &instance,
            "composition4x16_challenge_evaluation_words",
            &invalid,
            21,
        );
        assert_eq!(
            output,
            vec![0; 21],
            "invalid input must fail closed: {invalid:?}"
        );
    }

    let production = call_words(
        &mut store,
        &instance,
        "composition4x16_production_words",
        &[3072, 0, 16],
        61,
    );
    assert_eq!(production[0], 1);
    let oracle = production_oracle(&rows, 3072, 0, 16);
    let fri_oracle = fri_layer_oracle(&oracle);
    assert_eq!(plain_limbs(&production[1..21]), oracle.root);
    assert_eq!(plain_limbs(&production[21..41]), oracle.transcript);
    assert_eq!(plain_limbs(&production[41..61]), oracle.fri);

    let fri = call_words(
        &mut store,
        &instance,
        "fri_layer4x16_production_words",
        &[3072, 0, 16],
        221,
    );
    assert_eq!(fri[0], 1);
    for evaluation in 0..8 {
        let start = 1 + evaluation * 20;
        assert_eq!(
            plain_limbs(&fri[start..start + 20]),
            fri_oracle.evaluations[evaluation],
            "independent first FRI fold mismatch at point {evaluation}",
        );
    }
    assert_eq!(plain_limbs(&fri[161..181]), fri_oracle.root);
    assert_eq!(plain_limbs(&fri[181..201]), fri_oracle.transcript);
    assert_eq!(plain_limbs(&fri[201..221]), fri_oracle.next_challenge);

    let invalid_fri = call_words(
        &mut store,
        &instance,
        "fri_layer4x16_production_words",
        &[0, 0, 16],
        221,
    );
    assert_eq!(invalid_fri, vec![0; 221]);
}
