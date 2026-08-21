//! Independent bigint gate for the field-neutral recursive Mandelbrot carrier.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{layout_for, BackendKind, OptLevel};
use hir::hir_def::HirIngot;
use num_bigint::BigUint;
use p3_baby_bear::{default_babybear_poseidon2_16, BabyBear as P3BabyBear};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_symmetric::Permutation;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const LIMBS: usize = 4;
const LIMB_BITS: usize = 13;
const LIMB_BASE: u32 = 8192;
const ACCUMULATOR_WORDS: usize = 37;
const COMMITTED_ACCUMULATOR_WORDS: usize = 31;
const PRODUCT_WITNESS_WORDS: usize = 28;
const LINEAR_WITNESS_WORDS: usize = 33;
const RANGE_WITNESS_WORDS: usize = LIMBS * LIMB_BITS * 2 + 1;
const TRANSITION_WITNESS_WORDS: usize = 1 + 3 * PRODUCT_WITNESS_WORDS + 4 * LINEAR_WITNESS_WORDS;
const PRODUCT_CARRY_RANGE_WORDS: usize = LIMBS * 2 * 18 * 2;
const POSEIDON_WIDTH: usize = 16;
const BABY_BEAR_MODULUS: u32 = 2_013_265_921;

fn expected_fixed_air_constraint_count(limbs: u32) -> u32 {
    let radix_range = 40 * limbs + 2;
    let ranged_fixed = radix_range + 2;
    let bounded_carry = 38;
    let product = 1
        + 2 * radix_range
        + ranged_fixed
        + 2 * limbs * (bounded_carry + 1)
        + 2
        + 2 * limbs
        + 2
        + 1;
    let linear = 4 + 3 * radix_range + ranged_fixed + 7 * limbs + 5;
    1 + 6 * ranged_fixed + 3 * product + 4 * linear + 2 * (limbs + 1)
}

fn expected_sparse_transition_tasks(limbs: u32) -> Vec<[u32; 5]> {
    let mut tasks = Vec::new();
    for range in 0..31u32 {
        for limb in 0..limbs {
            for bit in 0..13u32 {
                tasks.push([0, range, limb, bit, 0]);
            }
            tasks.push([1, range, limb, 0, 0]);
        }
        tasks.push([2, range, 0, 0, 0]);
    }
    for product in 0..3u32 {
        for coefficient in 0..2 * limbs {
            for slack in 0..2u32 {
                for bit in 0..18u32 {
                    tasks.push([3, product, coefficient, slack, bit]);
                }
            }
            tasks.push([4, product, coefficient, 0, 0]);
        }
    }
    for product in 0..3u32 {
        for coefficient in 0..2 * limbs {
            let count = if coefficient < limbs {
                coefficient + 1
            } else if coefficient < 2 * limbs - 1 {
                2 * limbs - 1 - coefficient
            } else {
                0
            };
            for term in 0..count {
                tasks.push([5, product, coefficient, term, 0]);
            }
            tasks.push([6, product, coefficient, 0, 0]);
        }
    }
    for product in 0..3u32 {
        for limb in 0..limbs {
            tasks.push([7, product, limb, 0, 0]);
        }
        tasks.push([8, product, 0, 0, 0]);
    }
    for linear in 0..4u32 {
        for role in 0..4u32 {
            for limb in 0..limbs {
                tasks.push([9, linear, role, limb, 0]);
            }
        }
        tasks.push([10, linear, 0, 0, 0]);
    }
    for coordinate in 0..2u32 {
        tasks.push([11, coordinate, 0, 0, 0]);
        for limb in 0..limbs {
            tasks.push([12, coordinate, limb, 0, 0]);
        }
    }
    tasks.push([13, 0, 0, 0, 0]);
    assert_eq!(tasks.len() as u32, 3 * limbs * limbs + 683 * limbs + 41,);
    tasks
}

fn expected_sparse_control_fields(task: [u32; 5]) -> [u32; 35] {
    let [kind, first, second, third, fourth] = task;
    let mut fields = [0u32; 35];
    fields[kind as usize] = 1;
    match kind {
        0 => {
            fields[29] = first;
            fields[30] = second;
            fields[31] = third;
            fields[34] = 1 << third;
        }
        1 => {
            fields[29] = first;
            fields[30] = second;
        }
        2 => fields[29] = first,
        3 => {
            fields[29] = first;
            fields[30] = second;
            fields[31] = fourth;
            fields[32] = third;
            fields[34] = 1 << fourth;
        }
        4 => {
            fields[29] = first;
            fields[30] = second;
        }
        5 => {
            fields[29] = first;
            fields[30] = second;
            fields[31] = third;
            fields[32] = u32::from(second >= LIMBS as u32);
            fields[33] = if second < LIMBS as u32 {
                second + 1
            } else if second < 2 * LIMBS as u32 - 1 {
                2 * LIMBS as u32 - 1 - second
            } else {
                0
            };
        }
        6 => {
            fields[29] = first;
            fields[30] = second;
            fields[32] = u32::from(second >= LIMBS as u32);
            fields[33] = if second < LIMBS as u32 {
                second + 1
            } else if second < 2 * LIMBS as u32 - 1 {
                2 * LIMBS as u32 - 1 - second
            } else {
                0
            };
        }
        7 => {
            fields[29] = first;
            fields[30] = second;
        }
        8 => fields[29] = first,
        9 => {
            fields[15 + second as usize] = 1;
            fields[29] = first;
            fields[30] = second;
            fields[31] = third;
            fields[32] = u32::from(first == 0);
        }
        10 => {
            fields[29] = first;
            fields[32] = u32::from(first == 0);
        }
        11 => fields[29] = first,
        12 => {
            fields[29] = first;
            fields[30] = second;
        }
        13 | 14 => {}
        _ => panic!("unknown sparse control task kind {kind}"),
    }
    if kind <= 2 {
        if first < 6 {
            fields[19] = 1;
        } else if first < 15 {
            fields[20 + ((first - 6) % 3) as usize] = 1;
        } else {
            fields[23 + ((first - 15) % 4) as usize] = 1;
        }
        fields[32] = u32::from(fields[19] == 1 || fields[22] == 1 || fields[26] == 1);
    }
    if kind == 0 || kind == 1 {
        fields[27] = u32::from(second + 2 == LIMBS as u32);
        fields[28] = u32::from(second + 1 == LIMBS as u32);
    }
    fields
}

fn expected_sparse_control_rows() -> Vec<[u32; 35]> {
    let mut rows: Vec<[u32; 35]> = expected_sparse_transition_tasks(LIMBS as u32)
        .into_iter()
        .map(expected_sparse_control_fields)
        .collect();
    rows.resize(4_096, expected_sparse_control_fields([14, 0, 0, 0, 0]));
    rows
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fx {
    negative: bool,
    magnitude: BigUint,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ComplexFx {
    real: Fx,
    imaginary: Fx,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Boundary {
    iteration: u32,
    z: ComplexFx,
    escaped: bool,
}

#[derive(Clone)]
struct Claim {
    point: ComplexFx,
    bound: u32,
}

fn radix_modulus() -> BigUint {
    BigUint::from(LIMB_BASE).pow(LIMBS as u32)
}

fn scale() -> BigUint {
    BigUint::from(LIMB_BASE).pow((LIMBS - 1) as u32)
}

fn zero() -> Fx {
    Fx {
        negative: false,
        magnitude: BigUint::from(0u32),
    }
}

fn fixed(negative: bool, numerator: u32, denominator: u32) -> Fx {
    let magnitude = scale() * BigUint::from(numerator) / BigUint::from(denominator);
    Fx {
        negative: negative && magnitude != BigUint::from(0u32),
        magnitude,
    }
}

fn seeded_fixed(seed: &mut u32) -> Fx {
    let mut magnitude = BigUint::from(0u32);
    for limb_index in 0..LIMBS {
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        magnitude += BigUint::from(*seed & (LIMB_BASE - 1)) << (LIMB_BITS * limb_index);
    }
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    Fx {
        negative: (*seed & 1) == 1 && magnitude != BigUint::from(0u32),
        magnitude,
    }
}

fn add(left: &Fx, right: &Fx) -> Fx {
    let (negative, magnitude) = if left.negative == right.negative {
        (
            left.negative,
            (&left.magnitude + &right.magnitude) % radix_modulus(),
        )
    } else if left.magnitude >= right.magnitude {
        (left.negative, &left.magnitude - &right.magnitude)
    } else {
        (right.negative, &right.magnitude - &left.magnitude)
    };
    Fx {
        negative: negative && magnitude != BigUint::from(0u32),
        magnitude,
    }
}

fn subtract(left: &Fx, right: &Fx) -> Fx {
    let mut negative_right = right.clone();
    if negative_right.magnitude != BigUint::from(0u32) {
        negative_right.negative = !negative_right.negative;
    }
    add(left, &negative_right)
}

fn multiply(left: &Fx, right: &Fx) -> Fx {
    let half = scale() >> 1u32;
    let magnitude = ((&left.magnitude * &right.magnitude + half) / scale()) % radix_modulus();
    Fx {
        negative: (left.negative ^ right.negative) && magnitude != BigUint::from(0u32),
        magnitude,
    }
}

fn escaped(z: &ComplexFx) -> bool {
    let magnitude = add(
        &multiply(&z.real, &z.real),
        &multiply(&z.imaginary, &z.imaginary),
    );
    magnitude.magnitude > BigUint::from(4u32) * scale()
}

fn advance(claim: &Claim, boundary: &Boundary) -> Boundary {
    let xx = multiply(&boundary.z.real, &boundary.z.real);
    let yy = multiply(&boundary.z.imaginary, &boundary.z.imaginary);
    let xy = multiply(&boundary.z.real, &boundary.z.imaginary);
    let z = ComplexFx {
        real: add(&subtract(&xx, &yy), &claim.point.real),
        imaginary: add(&add(&xy, &xy), &claim.point.imaginary),
    };
    Boundary {
        iteration: boundary.iteration + 1,
        escaped: escaped(&z),
        z,
    }
}

fn evaluate(claim: &Claim, requested_steps: u32) -> Boundary {
    let mut end = Boundary {
        iteration: 0,
        z: ComplexFx {
            real: zero(),
            imaginary: zero(),
        },
        escaped: false,
    };
    let mut performed = 0;
    while performed < requested_steps && !end.escaped {
        end = advance(claim, &end);
        performed += 1;
    }
    end
}

fn limbs(value: &Fx) -> [u32; LIMBS] {
    let mask = BigUint::from(LIMB_BASE - 1);
    std::array::from_fn(|index| {
        ((&value.magnitude >> (LIMB_BITS * index)) & &mask)
            .to_u32_digits()
            .first()
            .copied()
            .unwrap_or(0)
    })
}

fn fixed_words(value: &Fx) -> Vec<u32> {
    let mut words = vec![value.negative as u32];
    words.extend(limbs(value));
    words
}

fn fixed_arguments(left: &Fx, right: &Fx) -> Vec<u32> {
    let mut arguments = fixed_words(left);
    arguments.extend(fixed_words(right));
    arguments
}

fn expected_product_witness_words(left: &Fx, right: &Fx) -> Vec<u32> {
    let left_limbs = limbs(left);
    let right_limbs = limbs(right);
    let mut digits = [0u32; LIMBS * 2];
    let mut carries = [0u32; LIMBS * 2];
    let mut carry = 0u64;
    for coefficient in 0..LIMBS * 2 {
        let mut total = carry;
        for left_index in 0..LIMBS {
            if coefficient >= left_index {
                let right_index = coefficient - left_index;
                if right_index < LIMBS {
                    total += left_limbs[left_index] as u64 * right_limbs[right_index] as u64;
                }
            }
        }
        digits[coefficient] = (total % LIMB_BASE as u64) as u32;
        carry = total / LIMB_BASE as u64;
        carries[coefficient] = carry as u32;
    }
    assert_eq!(carry, 0);

    let round_up = digits[LIMBS - 2] >= LIMB_BASE / 2;
    let mut round_carry = round_up as u32;
    let mut round_carries = [0u32; LIMBS];
    let mut output_limbs = [0u32; LIMBS];
    for index in 0..LIMBS {
        let retained = digits[LIMBS - 1 + index];
        let total = retained + round_carry;
        output_limbs[index] = total % LIMB_BASE;
        round_carry = total / LIMB_BASE;
        round_carries[index] = round_carry;
    }
    let expected_output = multiply(left, right);
    assert_eq!(output_limbs, limbs(&expected_output));

    let mut words = vec![1];
    words.extend(&digits[..LIMBS]);
    words.extend(&carries[..LIMBS]);
    words.extend(&digits[LIMBS..]);
    words.extend(&carries[LIMBS..]);
    words.push(round_up as u32);
    words.extend(round_carries);
    words.push((round_carry == 1) as u32);
    words.extend(fixed_words(&expected_output));
    assert_eq!(words.len(), PRODUCT_WITNESS_WORDS);
    words
}

fn expected_bounded_u18_words(value: u32, bound: u32) -> Vec<u32> {
    assert!(value <= bound);
    assert!(bound < (1 << 18));
    let mut words = Vec::with_capacity(36);
    words.extend((0..18).map(|bit| (value >> bit) & 1));
    let slack = bound - value;
    words.extend((0..18).map(|bit| (slack >> bit) & 1));
    words
}

fn expected_product_carry_range_words(left: &Fx, right: &Fx) -> Vec<u32> {
    let product = expected_product_witness_words(left, right);
    let bound = LIMBS as u32 * (LIMB_BASE - 1);
    let low_carries = &product[1 + LIMBS..1 + 2 * LIMBS];
    let high_carries = &product[1 + 3 * LIMBS..1 + 4 * LIMBS];
    let mut words = Vec::with_capacity(PRODUCT_CARRY_RANGE_WORDS);
    for carry in low_carries {
        words.extend(expected_bounded_u18_words(*carry, bound));
    }
    for carry in high_carries {
        words.extend(expected_bounded_u18_words(*carry, bound));
    }
    assert_eq!(words.len(), PRODUCT_CARRY_RANGE_WORDS);
    words
}

fn expected_linear_witness_words(left: &Fx, right: &Fx, subtract_right: bool) -> Vec<u32> {
    let left_limbs = limbs(left);
    let right_limbs = limbs(right);
    let effective_right_negative = right.negative ^ subtract_right;
    let same_sign = left.negative == effective_right_negative;
    let mut sum_digits = [0u32; LIMBS];
    let mut sum_carries = [0u32; LIMBS];
    let mut left_difference_digits = [0u32; LIMBS];
    let mut left_borrows = [0u32; LIMBS];
    let mut right_difference_digits = [0u32; LIMBS];
    let mut right_borrows = [0u32; LIMBS];
    let mut sum_carry = 0u32;
    let mut left_borrow = 0u32;
    let mut right_borrow = 0u32;
    for index in 0..LIMBS {
        let sum = left_limbs[index] + right_limbs[index] + sum_carry;
        sum_digits[index] = sum % LIMB_BASE;
        sum_carry = sum / LIMB_BASE;
        sum_carries[index] = sum_carry;

        let right_subtrahend = right_limbs[index] + left_borrow;
        if left_limbs[index] >= right_subtrahend {
            left_difference_digits[index] = left_limbs[index] - right_subtrahend;
            left_borrow = 0;
        } else {
            left_difference_digits[index] = left_limbs[index] + LIMB_BASE - right_subtrahend;
            left_borrow = 1;
        }
        left_borrows[index] = left_borrow;

        let left_subtrahend = left_limbs[index] + right_borrow;
        if right_limbs[index] >= left_subtrahend {
            right_difference_digits[index] = right_limbs[index] - left_subtrahend;
            right_borrow = 0;
        } else {
            right_difference_digits[index] = right_limbs[index] + LIMB_BASE - left_subtrahend;
            right_borrow = 1;
        }
        right_borrows[index] = right_borrow;
    }
    let select_right = left_borrow == 1;
    let output_limbs = if same_sign {
        sum_digits
    } else if select_right {
        right_difference_digits
    } else {
        left_difference_digits
    };
    let output_nonzero = output_limbs.iter().any(|limb| *limb != 0);
    let selected_negative = if same_sign {
        left.negative
    } else if select_right {
        effective_right_negative
    } else {
        left.negative
    };
    let mut output_magnitude = BigUint::from(0u32);
    for (index, limb) in output_limbs.iter().enumerate() {
        output_magnitude += BigUint::from(*limb) << (LIMB_BITS * index);
    }
    let output = Fx {
        negative: selected_negative && output_nonzero,
        magnitude: output_magnitude,
    };
    let expected_output = if subtract_right {
        subtract(left, right)
    } else {
        add(left, right)
    };
    assert_eq!(output, expected_output);

    let mut words = vec![
        1,
        same_sign as u32,
        select_right as u32,
        output_nonzero as u32,
    ];
    words.extend(sum_digits);
    words.extend(sum_carries);
    words.extend(left_difference_digits);
    words.extend(left_borrows);
    words.extend(right_difference_digits);
    words.extend(right_borrows);
    words.extend(fixed_words(&output));
    assert_eq!(words.len(), LINEAR_WITNESS_WORDS);
    words
}

fn expected_range_witness_words(value: &Fx) -> Vec<u32> {
    let value_limbs = limbs(value);
    let mut seen = false;
    let mut words = Vec::with_capacity(RANGE_WITNESS_WORDS);
    for limb in value_limbs {
        let bits: [u32; LIMB_BITS] = std::array::from_fn(|bit_index| (limb >> bit_index) & 1);
        let mut prefixes = [0u32; LIMB_BITS];
        for (bit_index, bit) in bits.iter().enumerate() {
            seen |= *bit == 1;
            prefixes[bit_index] = seen as u32;
        }
        words.extend(bits);
        words.extend(prefixes);
    }
    words.push(seen as u32);
    assert_eq!(words.len(), RANGE_WITNESS_WORDS);
    words
}

fn expected_transition_witness_words(point: &ComplexFx, current: &ComplexFx) -> Vec<u32> {
    let xx = multiply(&current.real, &current.real);
    let yy = multiply(&current.imaginary, &current.imaginary);
    let xy = multiply(&current.real, &current.imaginary);
    let real_difference = subtract(&xx, &yy);
    let next_real = add(&real_difference, &point.real);
    let double_xy = add(&xy, &xy);
    let next_imaginary = add(&double_xy, &point.imaginary);

    let mut words = vec![1];
    words.extend(expected_product_witness_words(&current.real, &current.real));
    words.extend(expected_product_witness_words(
        &current.imaginary,
        &current.imaginary,
    ));
    words.extend(expected_product_witness_words(
        &current.real,
        &current.imaginary,
    ));
    words.extend(expected_linear_witness_words(&xx, &yy, true));
    words.extend(expected_linear_witness_words(
        &real_difference,
        &point.real,
        false,
    ));
    words.extend(expected_linear_witness_words(&xy, &xy, false));
    words.extend(expected_linear_witness_words(
        &double_xy,
        &point.imaginary,
        false,
    ));
    assert_eq!(words.len(), TRANSITION_WITNESS_WORDS);

    let expected_next = advance(
        &Claim {
            point: point.clone(),
            bound: 1,
        },
        &Boundary {
            iteration: 0,
            z: current.clone(),
            escaped: escaped(current),
        },
    );
    assert_eq!(expected_next.z.real, next_real);
    assert_eq!(expected_next.z.imaginary, next_imaginary);
    words
}

#[derive(Clone, Debug)]
struct ExpectedSparseRange {
    negative: Option<bool>,
    digits: [u32; LIMBS],
}

fn words4(words: &[u32]) -> [u32; LIMBS] {
    words
        .try_into()
        .expect("a sparse radix range must contain four limbs")
}

fn signed_range(value: &Fx) -> ExpectedSparseRange {
    ExpectedSparseRange {
        negative: Some(value.negative),
        digits: limbs(value),
    }
}

fn product_ranges(left: &Fx, right: &Fx) -> [ExpectedSparseRange; 3] {
    let words = expected_product_witness_words(left, right);
    let output = multiply(left, right);
    [
        ExpectedSparseRange {
            negative: None,
            digits: words4(&words[1..1 + LIMBS]),
        },
        ExpectedSparseRange {
            negative: None,
            digits: words4(&words[1 + 2 * LIMBS..1 + 3 * LIMBS]),
        },
        signed_range(&output),
    ]
}

fn linear_ranges(left: &Fx, right: &Fx, subtract_right: bool) -> [ExpectedSparseRange; 4] {
    let words = expected_linear_witness_words(left, right, subtract_right);
    let output = if subtract_right {
        subtract(left, right)
    } else {
        add(left, right)
    };
    [
        ExpectedSparseRange {
            negative: None,
            digits: words4(&words[4..4 + LIMBS]),
        },
        ExpectedSparseRange {
            negative: None,
            digits: words4(&words[4 + 2 * LIMBS..4 + 3 * LIMBS]),
        },
        ExpectedSparseRange {
            negative: None,
            digits: words4(&words[4 + 4 * LIMBS..4 + 5 * LIMBS]),
        },
        signed_range(&output),
    ]
}

fn expected_sparse_ranges(point: &ComplexFx, current: &ComplexFx) -> Vec<ExpectedSparseRange> {
    let xx = multiply(&current.real, &current.real);
    let yy = multiply(&current.imaginary, &current.imaginary);
    let xy = multiply(&current.real, &current.imaginary);
    let real_difference = subtract(&xx, &yy);
    let next_real = add(&real_difference, &point.real);
    let double_xy = add(&xy, &xy);
    let next_imaginary = add(&double_xy, &point.imaginary);

    let mut ranges = vec![
        signed_range(&point.real),
        signed_range(&point.imaginary),
        signed_range(&current.real),
        signed_range(&current.imaginary),
        signed_range(&next_real),
        signed_range(&next_imaginary),
    ];
    ranges.extend(product_ranges(&current.real, &current.real));
    ranges.extend(product_ranges(&current.imaginary, &current.imaginary));
    ranges.extend(product_ranges(&current.real, &current.imaginary));
    ranges.extend(linear_ranges(&xx, &yy, true));
    ranges.extend(linear_ranges(&real_difference, &point.real, false));
    ranges.extend(linear_ranges(&xy, &xy, false));
    ranges.extend(linear_ranges(&double_xy, &point.imaginary, false));
    assert_eq!(ranges.len(), 31);
    ranges
}

fn expected_sparse_radix_rows(point: &ComplexFx, current: &ComplexFx) -> Vec<[u32; 6]> {
    let mut rows = Vec::new();
    for range in expected_sparse_ranges(point, current) {
        let mut seen = 0u32;
        for digit in range.digits {
            let mut reconstructed = 0u32;
            for bit_index in 0..LIMB_BITS {
                let bit = (digit >> bit_index) & 1;
                let before = reconstructed;
                let state_before = seen;
                reconstructed += bit << bit_index;
                seen |= bit;
                rows.push([bit, 0, before, reconstructed, state_before, seen]);
            }
            rows.push([digit, 0, reconstructed, 0, seen, seen]);
        }
        rows.push([seen, range.negative.unwrap_or(false) as u32, 0, 0, seen, 0]);
    }
    assert_eq!(rows.len(), 31 * (LIMBS * 14 + 1));
    rows
}

fn expected_sparse_carry_rows(current: &ComplexFx) -> Vec<[u32; 6]> {
    let products = [
        expected_product_witness_words(&current.real, &current.real),
        expected_product_witness_words(&current.imaginary, &current.imaginary),
        expected_product_witness_words(&current.real, &current.imaginary),
    ];
    let bound = LIMBS as u32 * (LIMB_BASE - 1);
    let mut rows = Vec::new();
    for product in products {
        let mut carries: Vec<u32> = Vec::with_capacity(LIMBS * 2);
        carries.extend(product[1 + LIMBS..1 + 2 * LIMBS].iter().copied());
        carries.extend(product[1 + 3 * LIMBS..1 + 4 * LIMBS].iter().copied());
        for carry in carries {
            let mut reconstructed = 0u32;
            for bit_index in 0..18u32 {
                let bit = (carry >> bit_index) & 1u32;
                let before = reconstructed;
                reconstructed += bit << bit_index;
                rows.push([bit, 0, before, reconstructed, 0, 0]);
            }
            assert_eq!(reconstructed, carry);
            let slack = bound - carry;
            reconstructed = 0;
            for bit_index in 0..18u32 {
                let bit = (slack >> bit_index) & 1u32;
                let before = reconstructed;
                reconstructed += bit << bit_index;
                rows.push([bit, 0, before, reconstructed, carry, carry]);
            }
            assert_eq!(reconstructed, slack);
            rows.push([carry, 0, slack, 0, carry, 0]);
        }
    }
    assert_eq!(rows.len(), 3 * LIMBS * 2 * 37);
    rows
}

fn expected_sparse_product_rows(current: &ComplexFx) -> Vec<[u32; 6]> {
    let product_inputs = [
        (&current.real, &current.real),
        (&current.imaginary, &current.imaginary),
        (&current.real, &current.imaginary),
    ];
    let mut rows = Vec::new();
    for (left, right) in product_inputs {
        let left_digits = limbs(left);
        let right_digits = limbs(right);
        let witness = expected_product_witness_words(left, right);
        let mut digits = Vec::with_capacity(LIMBS * 2);
        digits.extend(witness[1..1 + LIMBS].iter().copied());
        digits.extend(witness[1 + 2 * LIMBS..1 + 3 * LIMBS].iter().copied());
        let mut carries = Vec::with_capacity(LIMBS * 2);
        carries.extend(witness[1 + LIMBS..1 + 2 * LIMBS].iter().copied());
        carries.extend(witness[1 + 3 * LIMBS..1 + 4 * LIMBS].iter().copied());
        for coefficient in 0..LIMBS * 2 {
            let carry_in = if coefficient == 0 {
                0
            } else {
                carries[coefficient - 1]
            };
            let first_left = coefficient.saturating_sub(LIMBS - 1);
            let terms = if coefficient < LIMBS {
                coefficient + 1
            } else if coefficient < 2 * LIMBS - 1 {
                2 * LIMBS - 1 - coefficient
            } else {
                0
            };
            let mut accumulator = 0u32;
            for term in 0..terms {
                let left_index = first_left + term;
                let right_index = coefficient - left_index;
                let left_digit = left_digits[left_index];
                let right_digit = right_digits[right_index];
                let before = accumulator;
                accumulator += left_digit * right_digit;
                rows.push([
                    left_digit,
                    right_digit,
                    before,
                    accumulator,
                    carry_in,
                    carry_in,
                ]);
            }
            rows.push([
                digits[coefficient],
                carries[coefficient],
                accumulator,
                0,
                carry_in,
                carries[coefficient],
            ]);
        }
    }
    assert_eq!(rows.len(), 3 * (LIMBS * LIMBS + 2 * LIMBS));
    rows
}

fn expected_sparse_round_rows(current: &ComplexFx) -> Vec<[u32; 6]> {
    let product_inputs = [
        (&current.real, &current.real),
        (&current.imaginary, &current.imaginary),
        (&current.real, &current.imaginary),
    ];
    let mut rows = Vec::new();
    for (left, right) in product_inputs {
        let witness = expected_product_witness_words(left, right);
        let low = &witness[1..1 + LIMBS];
        let high = &witness[1 + 2 * LIMBS..1 + 3 * LIMBS];
        let round_up = witness[1 + 4 * LIMBS];
        let round_carries = &witness[2 + 4 * LIMBS..2 + 5 * LIMBS];
        let output_negative = witness[3 + 5 * LIMBS];
        let output = &witness[4 + 5 * LIMBS..4 + 6 * LIMBS];
        for output_index in 0..LIMBS {
            let retained = if output_index == 0 {
                low[LIMBS - 1]
            } else {
                high[output_index - 1]
            };
            let carry_in = if output_index == 0 {
                round_up
            } else {
                round_carries[output_index - 1]
            };
            rows.push([
                retained,
                output[output_index],
                carry_in,
                round_carries[output_index],
                round_up,
                round_up,
            ]);
        }
        rows.push([
            output_negative,
            output.iter().any(|digit| *digit != 0) as u32,
            round_carries[LIMBS - 1],
            left.negative as u32,
            round_up,
            right.negative as u32,
        ]);
    }
    assert_eq!(rows.len(), 3 * (LIMBS + 1));
    rows
}

fn expected_sparse_linear_rows(point: &ComplexFx, current: &ComplexFx) -> Vec<[u32; 6]> {
    let xx = multiply(&current.real, &current.real);
    let yy = multiply(&current.imaginary, &current.imaginary);
    let xy = multiply(&current.real, &current.imaginary);
    let real_difference = subtract(&xx, &yy);
    let double_xy = add(&xy, &xy);
    let inputs = [
        (&xx, &yy, true),
        (&real_difference, &point.real, false),
        (&xy, &xy, false),
        (&double_xy, &point.imaginary, false),
    ];
    let mut rows = Vec::new();
    for (left, right, subtract_right) in inputs {
        let left_digits = limbs(left);
        let right_digits = limbs(right);
        let witness = expected_linear_witness_words(left, right, subtract_right);
        let same_sign = witness[1];
        let select_right = witness[2];
        let output_nonzero = witness[3];
        let sum = &witness[4..4 + LIMBS];
        let sum_carries = &witness[4 + LIMBS..4 + 2 * LIMBS];
        let left_difference = &witness[4 + 2 * LIMBS..4 + 3 * LIMBS];
        let left_borrows = &witness[4 + 3 * LIMBS..4 + 4 * LIMBS];
        let right_difference = &witness[4 + 4 * LIMBS..4 + 5 * LIMBS];
        let right_borrows = &witness[4 + 5 * LIMBS..4 + 6 * LIMBS];
        let output_negative = witness[4 + 6 * LIMBS];
        let output = &witness[5 + 6 * LIMBS..5 + 7 * LIMBS];
        for limb in 0..LIMBS {
            rows.push([
                left_digits[limb],
                right_digits[limb],
                if limb == 0 { 0 } else { sum_carries[limb - 1] },
                sum_carries[limb],
                sum[limb],
                0,
            ]);
        }
        for limb in 0..LIMBS {
            rows.push([
                left_digits[limb],
                right_digits[limb],
                if limb == 0 { 0 } else { left_borrows[limb - 1] },
                left_borrows[limb],
                left_difference[limb],
                0,
            ]);
        }
        for limb in 0..LIMBS {
            rows.push([
                left_digits[limb],
                right_digits[limb],
                if limb == 0 {
                    0
                } else {
                    right_borrows[limb - 1]
                },
                right_borrows[limb],
                right_difference[limb],
                0,
            ]);
        }
        for limb in 0..LIMBS {
            rows.push([
                sum[limb],
                left_difference[limb],
                right_difference[limb],
                output[limb],
                same_sign,
                select_right,
            ]);
        }
        rows.push([
            left.negative as u32,
            right.negative as u32,
            left_borrows[LIMBS - 1],
            right_borrows[LIMBS - 1],
            output_nonzero,
            output_negative,
        ]);
    }
    assert_eq!(rows.len(), 4 * (4 * LIMBS + 1));
    rows
}

fn expected_sparse_boundary_rows(point: &ComplexFx, current: &ComplexFx) -> Vec<[u32; 6]> {
    let next = advance(
        &Claim {
            point: point.clone(),
            bound: 1,
        },
        &Boundary {
            iteration: 0,
            z: current.clone(),
            escaped: escaped(current),
        },
    )
    .z;
    let mut rows = Vec::new();
    for coordinate in [&next.real, &next.imaginary] {
        rows.push([
            coordinate.negative as u32,
            coordinate.negative as u32,
            0,
            0,
            0,
            0,
        ]);
        for digit in limbs(coordinate) {
            rows.push([digit, digit, 0, 0, 0, 0]);
        }
    }
    rows.push([1, 0, 0, 0, 0, 0]);
    assert_eq!(rows.len(), 2 * (LIMBS + 1) + 1);
    rows
}

fn complex_words(value: &ComplexFx) -> Vec<u32> {
    let mut words = fixed_words(&value.real);
    words.extend(fixed_words(&value.imaginary));
    words
}

fn transition_arguments(point: &ComplexFx, current: &ComplexFx) -> Vec<u32> {
    let mut words = complex_words(point);
    words.extend(complex_words(current));
    words
}

fn boundary_words(value: &Boundary) -> Vec<u32> {
    let mut words = vec![value.iteration];
    words.extend(complex_words(&value.z));
    words.push(value.escaped as u32);
    words
}

fn claim_args(claim: &Claim) -> Vec<u32> {
    let mut words = complex_words(&claim.point);
    words.push(claim.bound);
    words
}

fn expected_accumulator_words(
    claim: &Claim,
    start: &Boundary,
    end: &Boundary,
    leaves: u32,
) -> Vec<u32> {
    let mut words = vec![1, leaves];
    words.extend(complex_words(&claim.point));
    words.push(claim.bound);
    words.extend(boundary_words(start));
    words.extend(boundary_words(end));
    assert_eq!(words.len(), ACCUMULATOR_WORDS);
    words
}

fn expected_leaf_words(claim: &Claim, end: &Boundary) -> Vec<u32> {
    let start = Boundary {
        iteration: 0,
        z: ComplexFx {
            real: zero(),
            imaginary: zero(),
        },
        escaped: false,
    };
    expected_accumulator_words(claim, &start, end, 1)
}

fn reference_poseidon_permutation(input: [u32; POSEIDON_WIDTH]) -> [u32; POSEIDON_WIDTH] {
    let mut state = input.map(P3BabyBear::from_u32);
    default_babybear_poseidon2_16().permute_mut(&mut state);
    state.map(|value| value.as_canonical_u32())
}

fn reference_poseidon_digest(tag: &[u8; 4], fields: &[u32]) -> [u32; 8] {
    let mut message = vec![u32::from_be_bytes(*tag), fields.len() as u32];
    message.extend_from_slice(fields);
    let mut state = [0u32; POSEIDON_WIDTH];
    for block in message.chunks(8) {
        state[..block.len()].copy_from_slice(block);
        state = reference_poseidon_permutation(state);
    }
    state[..8].try_into().unwrap()
}

fn reference_digest_compress(left: [u32; 8], right: [u32; 8]) -> [u32; 8] {
    let mut state = [0u32; POSEIDON_WIDTH];
    state[..8].copy_from_slice(&left);
    state[8..].copy_from_slice(&right);
    reference_poseidon_permutation(state)[..8]
        .try_into()
        .unwrap()
}

fn reference_merkle_root(mut leaves: Vec<[u32; 8]>) -> [u32; 8] {
    assert!(!leaves.is_empty());
    assert!(leaves.len().is_power_of_two());
    while leaves.len() > 1 {
        leaves = leaves
            .chunks_exact(2)
            .map(|pair| reference_digest_compress(pair[0], pair[1]))
            .collect();
    }
    leaves[0]
}

fn expected_sparse_rows(point: &ComplexFx, current: &ComplexFx) -> Vec<[u32; 6]> {
    let mut rows = expected_sparse_radix_rows(point, current);
    rows.extend(expected_sparse_carry_rows(current));
    rows.extend(expected_sparse_product_rows(current));
    rows.extend(expected_sparse_round_rows(current));
    rows.extend(expected_sparse_linear_rows(point, current));
    rows.extend(expected_sparse_boundary_rows(point, current));
    let task_count = rows.len() as u32;
    assert_eq!(task_count, 2_821);
    rows.resize(4_096, [0; 6]);
    rows
}

fn bb_add(left: u32, right: u32) -> u32 {
    ((left as u64 + right as u64) % BABY_BEAR_MODULUS as u64) as u32
}

fn bb_sub(left: u32, right: u32) -> u32 {
    ((left as u64 + BABY_BEAR_MODULUS as u64 - right as u64) % BABY_BEAR_MODULUS as u64) as u32
}

fn bb_mul(left: u32, right: u32) -> u32 {
    (left as u64 * right as u64 % BABY_BEAR_MODULUS as u64) as u32
}

fn bb_pow(mut base: u32, mut exponent: u32) -> u32 {
    let mut result = 1;
    while exponent != 0 {
        if exponent & 1 == 1 {
            result = bb_mul(result, base);
        }
        base = bb_mul(base, base);
        exponent >>= 1;
    }
    result
}

fn bb_inverse(value: u32) -> u32 {
    assert_ne!(value, 0, "copy compression challenge hit an inverse pole");
    bb_pow(value, BABY_BEAR_MODULUS - 2)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ext4([u32; 4]);

impl Ext4 {
    const ZERO: Self = Self([0; 4]);

    fn from_words(words: &[u32]) -> Self {
        Self(words.try_into().expect("quartic value must have four coefficients"))
    }

    fn from_base(value: u32) -> Self {
        Self([value, 0, 0, 0])
    }

    fn add(self, other: Self) -> Self {
        Self(std::array::from_fn(|index| bb_add(self.0[index], other.0[index])))
    }

    fn scale(self, scalar: u32) -> Self {
        Self(self.0.map(|coefficient| bb_mul(coefficient, scalar)))
    }

    fn mul(self, other: Self) -> Self {
        let a = self.0;
        let b = other.0;
        let nonresidue = 11;
        Self([
            bb_add(
                bb_mul(a[0], b[0]),
                bb_mul(
                    nonresidue,
                    bb_add(bb_add(bb_mul(a[1], b[3]), bb_mul(a[2], b[2])), bb_mul(a[3], b[1])),
                ),
            ),
            bb_add(
                bb_add(bb_mul(a[0], b[1]), bb_mul(a[1], b[0])),
                bb_mul(nonresidue, bb_add(bb_mul(a[2], b[3]), bb_mul(a[3], b[2]))),
            ),
            bb_add(
                bb_add(bb_add(bb_mul(a[0], b[2]), bb_mul(a[1], b[1])), bb_mul(a[2], b[0])),
                bb_mul(nonresidue, bb_mul(a[3], b[3])),
            ),
            bb_add(
                bb_add(bb_mul(a[0], b[3]), bb_mul(a[1], b[2])),
                bb_add(bb_mul(a[2], b[1]), bb_mul(a[3], b[0])),
            ),
        ])
    }

    fn inverse(self) -> Self {
        let [a0, a1, a2, a3] = self.0;
        let w = 11;
        let two = 2;
        let norm0 = bb_sub(
            bb_add(bb_mul(a0, a0), bb_mul(w, bb_mul(a2, a2))),
            bb_mul(bb_mul(two, w), bb_mul(a1, a3)),
        );
        let norm1 = bb_sub(
            bb_sub(bb_mul(two, bb_mul(a0, a2)), bb_mul(a1, a1)),
            bb_mul(w, bb_mul(a3, a3)),
        );
        let norm_inverse = bb_inverse(bb_sub(
            bb_mul(norm0, norm0),
            bb_mul(w, bb_mul(norm1, norm1)),
        ));
        let inverse0 = bb_mul(norm0, norm_inverse);
        let inverse1 = bb_sub(0, bb_mul(norm1, norm_inverse));
        let even0 = bb_add(bb_mul(a0, inverse0), bb_mul(w, bb_mul(a2, inverse1)));
        let even1 = bb_add(bb_mul(a0, inverse1), bb_mul(a2, inverse0));
        let odd0 = bb_add(bb_mul(a1, inverse0), bb_mul(w, bb_mul(a3, inverse1)));
        let odd1 = bb_add(bb_mul(a1, inverse1), bb_mul(a3, inverse0));
        Self([even0, bb_sub(0, odd0), even1, bb_sub(0, odd1)])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExpectedCopyPort {
    address: u32,
    value: u32,
    coefficient: i32,
}

fn expected_base_interaction_row<const PORTS: usize>(
    ports: [Option<ExpectedCopyPort>; PORTS],
    beta: u32,
    gamma: u32,
) -> ([u32; PORTS], u32) {
    let mut inverses = [0; PORTS];
    let mut delta = 0;
    for (index, port) in ports.into_iter().enumerate() {
        if let Some(port) = port {
            let inverse = product_copy_compressed_inverse(port.address, port.value, beta, gamma);
            inverses[index] = inverse;
            let coefficient = if port.coefficient < 0 {
                BABY_BEAR_MODULUS - port.coefficient.unsigned_abs()
            } else {
                port.coefficient as u32
            };
            delta = bb_add(delta, bb_mul(coefficient, inverse));
        }
    }
    (inverses, delta)
}

fn expected_ext4_interaction_row<const PORTS: usize>(
    ports: [Option<ExpectedCopyPort>; PORTS],
    beta: Ext4,
    gamma: Ext4,
) -> ([Ext4; PORTS], Ext4) {
    let mut inverses = [Ext4::ZERO; PORTS];
    let mut delta = Ext4::ZERO;
    for (index, port) in ports.into_iter().enumerate() {
        if let Some(port) = port {
            let compressed = gamma
                .add(Ext4::from_base(port.address))
                .add(beta.scale(port.value));
            let inverse = compressed.inverse();
            assert_eq!(
                compressed.mul(inverse),
                Ext4::from_base(1),
                "quartic copy inverse must be exact",
            );
            inverses[index] = inverse;
            let coefficient = if port.coefficient < 0 {
                BABY_BEAR_MODULUS - port.coefficient.unsigned_abs()
            } else {
                port.coefficient as u32
            };
            delta = delta.add(inverse.scale(coefficient));
        }
    }
    (inverses, delta)
}

fn product_range_rank(range: u32) -> Option<u32> {
    match range {
        2 => Some(0),
        3 => Some(1),
        6 | 9 | 12 => Some(2 + 2 * ((range - 6) / 3)),
        7 | 10 | 13 => Some(3 + 2 * ((range - 7) / 3)),
        _ => None,
    }
}

fn product_copy_compressed_inverse(address: u32, value: u32, beta: u32, gamma: u32) -> u32 {
    bb_inverse(bb_add(gamma, bb_add(address, bb_mul(beta, value))))
}

fn expected_product_copy_ports(
    task: [u32; 5],
    row: [u32; 6],
) -> [Option<ExpectedCopyPort>; 2] {
    let [kind, first, second, third, _fourth] = task;
    let limbs = LIMBS as u32;
    let mut ports = [None; 2];
    match kind {
        1 => {
            if let Some(rank) = product_range_rank(first) {
                let multiplicity = if first == 2 || first == 3 {
                    3 * limbs
                } else {
                    1
                };
                ports[0] = Some(ExpectedCopyPort {
                    address: rank * limbs + second,
                    value: row[0],
                    coefficient: multiplicity as i32,
                });
            }
        }
        4 => {
            ports[0] = Some(ExpectedCopyPort {
                address: 8 * limbs + first * 2 * limbs + second,
                value: row[0],
                coefficient: 1,
            });
        }
        5 => {
            let left_rank = [0, 1, 0][first as usize];
            let right_rank = [0, 1, 1][first as usize];
            let left_index = if second < limbs {
                third
            } else {
                second - (limbs - 1) + third
            };
            let right_index = second - left_index;
            ports[0] = Some(ExpectedCopyPort {
                address: left_rank * limbs + left_index,
                value: row[0],
                coefficient: -1,
            });
            ports[1] = Some(ExpectedCopyPort {
                address: right_rank * limbs + right_index,
                value: row[1],
                coefficient: -1,
            });
        }
        6 => {
            let high = u32::from(second >= limbs);
            let digit_rank = 2 + 2 * first + high;
            let digit_limb = second - high * limbs;
            ports[0] = Some(ExpectedCopyPort {
                address: digit_rank * limbs + digit_limb,
                value: row[0],
                coefficient: -1,
            });
            ports[1] = Some(ExpectedCopyPort {
                address: 8 * limbs + first * 2 * limbs + second,
                value: row[1],
                coefficient: -1,
            });
        }
        _ => {}
    }
    ports
}

fn expected_product_interaction_ports(
    task: [u32; 5],
    row: [u32; 6],
    beta: u32,
    gamma: u32,
) -> (u32, u32, u32) {
    let (inverses, delta) =
        expected_base_interaction_row(expected_product_copy_ports(task, row), beta, gamma);
    (inverses[0], inverses[1], delta)
}

fn expected_product_interaction_receipt(
    point: &ComplexFx,
    current: &ComplexFx,
    beta: u32,
    gamma: u32,
    receipt_challenge: u32,
) -> u32 {
    let mut tasks = expected_sparse_transition_tasks(LIMBS as u32);
    tasks.resize(4_096, [14, 0, 0, 0, 0]);
    let rows = expected_sparse_rows(point, current);
    let mut accumulator = 0;
    let mut receipt = 0;
    let mut power = 1;
    for (task, row) in tasks.into_iter().zip(rows) {
        let (inverse_first, inverse_second, delta) =
            expected_product_interaction_ports(task, row, beta, gamma);
        receipt = bb_add(receipt, bb_mul(power, accumulator));
        power = bb_mul(power, receipt_challenge);
        receipt = bb_add(receipt, bb_mul(power, inverse_first));
        power = bb_mul(power, receipt_challenge);
        receipt = bb_add(receipt, bb_mul(power, inverse_second));
        power = bb_mul(power, receipt_challenge);
        accumulator = bb_add(accumulator, delta);
    }
    assert_eq!(accumulator, 0, "independent product interaction must close");
    receipt
}

fn round_range_rank(range: u32) -> Option<u32> {
    match range {
        2 => Some(0),
        3 => Some(1),
        6..=14 => Some(range - 4),
        _ => None,
    }
}

fn round_digit_address(rank: u32, limb: u32) -> u32 {
    rank * LIMBS as u32 + limb
}

fn round_bit_address(rank: u32, limb: u32, bit: u32) -> u32 {
    let limbs = LIMBS as u32;
    11 * limbs + rank * limbs * 13 + limb * 13 + bit
}

fn round_sign_address(rank: u32) -> u32 {
    let limbs = LIMBS as u32;
    11 * limbs + 11 * limbs * 13 + rank
}

fn round_nonzero_address(rank: u32) -> u32 {
    round_sign_address(rank) + 11
}

fn expected_round_copy_ports(
    task: [u32; 5],
    row: [u32; 6],
) -> [Option<ExpectedCopyPort>; 5] {
    let [kind, first, second, third, _fourth] = task;
    let limbs = LIMBS as u32;
    let mut ports: [Option<ExpectedCopyPort>; 5] = [None; 5];
    match kind {
        0 => {
            if matches!(first, 6 | 9 | 12) && second + 2 == limbs && third == 12 {
                let rank = round_range_rank(first).unwrap();
                ports[0] = Some(ExpectedCopyPort {
                    address: round_bit_address(rank, second, third),
                    value: row[0],
                    coefficient: 1,
                });
            }
        }
        1 => {
            let retained_low = matches!(first, 6 | 9 | 12) && second + 1 == limbs;
            let retained_high = matches!(first, 7 | 10 | 13) && second + 1 < limbs;
            if retained_low || retained_high {
                let rank = round_range_rank(first).unwrap();
                ports[0] = Some(ExpectedCopyPort {
                    address: round_digit_address(rank, second),
                    value: row[0],
                    coefficient: 1,
                });
            }
            if matches!(first, 8 | 11 | 14) {
                let rank = round_range_rank(first).unwrap();
                ports[1] = Some(ExpectedCopyPort {
                    address: round_digit_address(rank, second),
                    value: row[0],
                    coefficient: 1,
                });
            }
        }
        2 => {
            if matches!(first, 2 | 3) {
                let rank = round_range_rank(first).unwrap();
                ports[3] = Some(ExpectedCopyPort {
                    address: round_sign_address(rank),
                    value: row[1],
                    coefficient: 3,
                });
            }
            if matches!(first, 8 | 11 | 14) {
                let rank = round_range_rank(first).unwrap();
                ports[1] = Some(ExpectedCopyPort {
                    address: round_sign_address(rank),
                    value: row[1],
                    coefficient: 1,
                });
                ports[2] = Some(ExpectedCopyPort {
                    address: round_nonzero_address(rank),
                    value: row[0],
                    coefficient: 1,
                });
            }
        }
        7 => {
            let low_rank = 2 + 3 * first;
            let output_rank = low_rank + 2;
            ports[0] = Some(ExpectedCopyPort {
                address: round_digit_address(low_rank, limbs - 1 + second),
                value: row[0],
                coefficient: -1,
            });
            ports[1] = Some(ExpectedCopyPort {
                address: round_digit_address(output_rank, second),
                value: row[1],
                coefficient: -1,
            });
        }
        8 => {
            let low_rank = 2 + 3 * first;
            let output_rank = low_rank + 2;
            let left_rank = [0, 1, 0][first as usize];
            let right_rank = [0, 1, 1][first as usize];
            ports[0] = Some(ExpectedCopyPort {
                address: round_bit_address(low_rank, limbs - 2, 12),
                value: row[4], coefficient: -1,
            });
            ports[1] = Some(ExpectedCopyPort {
                address: round_sign_address(output_rank), value: row[0], coefficient: -1,
            });
            ports[2] = Some(ExpectedCopyPort {
                address: round_nonzero_address(output_rank), value: row[1], coefficient: -1,
            });
            ports[3] = Some(ExpectedCopyPort {
                address: round_sign_address(left_rank), value: row[3], coefficient: -1,
            });
            ports[4] = Some(ExpectedCopyPort {
                address: round_sign_address(right_rank), value: row[5], coefficient: -1,
            });
        }
        _ => {}
    }

    ports
}

fn expected_round_interaction_ports(
    task: [u32; 5],
    row: [u32; 6],
    beta: u32,
    gamma: u32,
) -> ([u32; 5], u32) {
    expected_base_interaction_row(expected_round_copy_ports(task, row), beta, gamma)
}

fn expected_round_interaction_receipt(
    point: &ComplexFx,
    current: &ComplexFx,
    beta: u32,
    gamma: u32,
    receipt_challenge: u32,
) -> u32 {
    let mut tasks = expected_sparse_transition_tasks(LIMBS as u32);
    tasks.resize(4_096, [14, 0, 0, 0, 0]);
    let rows = expected_sparse_rows(point, current);
    let mut accumulator = 0;
    let mut receipt = 0;
    let mut power = 1;
    for (task, row) in tasks.into_iter().zip(rows) {
        let (inverses, delta) = expected_round_interaction_ports(task, row, beta, gamma);
        receipt = bb_add(receipt, bb_mul(power, accumulator));
        power = bb_mul(power, receipt_challenge);
        for inverse in inverses {
            receipt = bb_add(receipt, bb_mul(power, inverse));
            power = bb_mul(power, receipt_challenge);
        }
        accumulator = bb_add(accumulator, delta);
    }
    assert_eq!(
        accumulator, 0,
        "independent rounding interaction must close"
    );
    receipt
}

fn linear_digit_address(rank: u32, limb: u32) -> u32 {
    rank * LIMBS as u32 + limb
}

fn linear_sign_address(rank: u32) -> u32 {
    31 * LIMBS as u32 + rank
}

fn linear_nonzero_address(rank: u32) -> u32 {
    31 * LIMBS as u32 + 31 + rank
}

fn linear_borrow_address(node: u32, right: bool) -> u32 {
    31 * LIMBS as u32 + 62 + 2 * node + u32::from(right)
}

fn linear_same_address(node: u32) -> u32 {
    31 * LIMBS as u32 + 70 + node
}

fn linear_select_address(node: u32) -> u32 {
    31 * LIMBS as u32 + 74 + node
}

fn linear_left_range_rank(node: u32) -> u32 {
    [8, 18, 14, 26][node as usize]
}

fn linear_right_range_rank(node: u32) -> u32 {
    [11, 0, 14, 1][node as usize]
}

fn expected_linear_copy_ports(
    task: [u32; 5],
    row: [u32; 6],
) -> [Option<ExpectedCopyPort>; 8] {
    let [kind, first, second, third, _fourth] = task;
    let limbs = LIMBS as u32;
    let mut ports: [Option<(u32, u32, i32)>; 8] = [None; 8];
    match kind {
        1 => {
            let multiplicity = match first {
                0 | 1 => 3,
                8 | 11 => 3,
                14 => 6,
                15 | 16 | 17 | 19 | 20 | 21 | 23 | 24 | 25 | 27 | 28 | 29 => 2,
                18 | 26 => 4,
                22 | 30 => 1,
                _ => 0,
            };
            if multiplicity != 0 {
                ports[0] = Some((linear_digit_address(first, second), row[0], multiplicity));
            }
        }
        2 => {
            let sign_multiplicity = match first {
                0 | 1 | 8 | 11 | 22 | 30 => 1,
                14 | 18 | 26 => 2,
                _ => 0,
            };
            if sign_multiplicity != 0 {
                ports[1] = Some((linear_sign_address(first), row[1], sign_multiplicity));
            }
            if matches!(first, 18 | 22 | 26 | 30) {
                ports[2] = Some((linear_nonzero_address(first), row[0], 1));
            }
        }
        9 => {
            let node = first;
            let left = linear_left_range_rank(node);
            let right = linear_right_range_rank(node);
            let sum = 15 + 4 * node;
            match second {
                0 | 1 | 2 => {
                    ports[0] = Some((linear_digit_address(left, third), row[0], -1));
                    ports[1] = Some((linear_digit_address(right, third), row[1], -1));
                    ports[2] = Some((linear_digit_address(sum + second, third), row[4], -1));
                    if second == 1 && third + 1 == limbs {
                        ports[3] = Some((linear_borrow_address(node, false), row[3], 1));
                    }
                    if second == 2 && third + 1 == limbs {
                        ports[4] = Some((linear_borrow_address(node, true), row[3], 1));
                    }
                }
                3 => {
                    ports[0] = Some((linear_digit_address(sum, third), row[0], -1));
                    ports[1] = Some((linear_digit_address(sum + 1, third), row[1], -1));
                    ports[2] = Some((linear_digit_address(sum + 2, third), row[2], -1));
                    ports[3] = Some((linear_digit_address(sum + 3, third), row[3], -1));
                    ports[4] = Some((linear_same_address(node), row[4], -1));
                    ports[5] = Some((linear_select_address(node), row[5], -1));
                }
                _ => unreachable!("invalid linear role"),
            }
        }
        10 => {
            let node = first;
            let left = linear_left_range_rank(node);
            let right = linear_right_range_rank(node);
            let output = 18 + 4 * node;
            ports[0] = Some((linear_sign_address(left), row[0], -1));
            ports[1] = Some((linear_sign_address(right), row[1], -1));
            ports[2] = Some((linear_borrow_address(node, false), row[2], -1));
            ports[3] = Some((linear_borrow_address(node, true), row[3], -1));
            ports[4] = Some((linear_nonzero_address(output), row[4], -1));
            ports[5] = Some((linear_sign_address(output), row[5], -1));
            let effective_right = row[1] ^ u32::from(node == 0);
            let same = u32::from(row[0] == effective_right);
            ports[6] = Some((linear_same_address(node), same, limbs as i32));
            ports[7] = Some((linear_select_address(node), row[2], limbs as i32));
        }
        _ => {}
    }

    ports.map(|port| {
        port.map(|(address, value, coefficient)| ExpectedCopyPort {
            address,
            value,
            coefficient,
        })
    })
}

fn expected_linear_interaction_ports(
    task: [u32; 5],
    row: [u32; 6],
    beta: u32,
    gamma: u32,
) -> ([u32; 8], u32) {
    expected_base_interaction_row(expected_linear_copy_ports(task, row), beta, gamma)
}

fn expected_linear_interaction_receipt(
    point: &ComplexFx,
    current: &ComplexFx,
    beta: u32,
    gamma: u32,
    receipt_challenge: u32,
) -> u32 {
    let mut tasks = expected_sparse_transition_tasks(LIMBS as u32);
    tasks.resize(4_096, [14, 0, 0, 0, 0]);
    let rows = expected_sparse_rows(point, current);
    let mut accumulator = 0;
    let mut receipt = 0;
    let mut power = 1;
    for (task, row) in tasks.into_iter().zip(rows) {
        let (inverses, delta) = expected_linear_interaction_ports(task, row, beta, gamma);
        receipt = bb_add(receipt, bb_mul(power, accumulator));
        power = bb_mul(power, receipt_challenge);
        for inverse in inverses {
            receipt = bb_add(receipt, bb_mul(power, inverse));
            power = bb_mul(power, receipt_challenge);
        }
        accumulator = bb_add(accumulator, delta);
    }
    assert_eq!(accumulator, 0, "independent linear interaction must close");
    receipt
}

fn expected_boundary_copy_ports(
    task: [u32; 5],
    row: [u32; 6],
) -> [Option<ExpectedCopyPort>; 2] {
    let [kind, first, second, _third, _fourth] = task;
    let mut ports: [Option<(u32, u32, i32)>; 2] = [None; 2];
    match kind {
        1 if matches!(first, 4 | 5 | 22 | 30) => {
            ports[0] = Some((linear_digit_address(first, second), row[0], 1));
        }
        2 if matches!(first, 4 | 5 | 22 | 30) => {
            ports[0] = Some((linear_sign_address(first), row[1], 1));
        }
        11 => {
            let computed = 22 + 8 * first;
            let claimed = 4 + first;
            ports[0] = Some((linear_sign_address(computed), row[0], -1));
            ports[1] = Some((linear_sign_address(claimed), row[1], -1));
        }
        12 => {
            let computed = 22 + 8 * first;
            let claimed = 4 + first;
            ports[0] = Some((linear_digit_address(computed, second), row[0], -1));
            ports[1] = Some((linear_digit_address(claimed, second), row[1], -1));
        }
        _ => {}
    }

    ports.map(|port| {
        port.map(|(address, value, coefficient)| ExpectedCopyPort {
            address,
            value,
            coefficient,
        })
    })
}

fn expected_boundary_interaction_ports(
    task: [u32; 5],
    row: [u32; 6],
    beta: u32,
    gamma: u32,
) -> ([u32; 2], u32) {
    expected_base_interaction_row(expected_boundary_copy_ports(task, row), beta, gamma)
}

fn expected_boundary_interaction_receipt(
    point: &ComplexFx,
    current: &ComplexFx,
    beta: u32,
    gamma: u32,
    receipt_challenge: u32,
) -> u32 {
    let mut tasks = expected_sparse_transition_tasks(LIMBS as u32);
    tasks.resize(4_096, [14, 0, 0, 0, 0]);
    let rows = expected_sparse_rows(point, current);
    let mut accumulator = 0;
    let mut receipt = 0;
    let mut power = 1;
    for (task, row) in tasks.into_iter().zip(rows) {
        let (inverses, delta) = expected_boundary_interaction_ports(task, row, beta, gamma);
        receipt = bb_add(receipt, bb_mul(power, accumulator));
        power = bb_mul(power, receipt_challenge);
        for inverse in inverses {
            receipt = bb_add(receipt, bb_mul(power, inverse));
            power = bb_mul(power, receipt_challenge);
        }
        accumulator = bb_add(accumulator, delta);
    }
    assert_eq!(
        accumulator, 0,
        "independent boundary interaction must close"
    );
    receipt
}

fn expected_sparse_trace_root(point: &ComplexFx, current: &ComplexFx) -> [u32; 8] {
    let rows = expected_sparse_rows(point, current);
    let task_count = 2_821;
    let controls = expected_sparse_control_rows();
    let leaves = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let mut fields = vec![
                LIMBS as u32,
                task_count,
                4_096,
                index as u32,
                u32::from(index < task_count as usize),
            ];
            fields.extend(controls[index]);
            fields.extend(row);
            reference_poseidon_digest(b"SL03", &fields)
        })
        .collect();
    reference_merkle_root(leaves)
}

fn expected_sparse_interaction_challenges(root: [u32; 8]) -> Vec<u32> {
    let mut words = vec![1];
    for tag in [
        b"PB01", b"PG01", b"RB01", b"RG01", b"LB01", b"LG01", b"BB01", b"BG01",
    ] {
        words.extend(&reference_poseidon_digest(tag, &root)[..4]);
    }
    words
}

fn extend_ext4_words(destination: &mut Vec<u32>, value: Ext4) {
    destination.extend(value.0);
}

fn expected_sparse_interaction_root(
    point: &ComplexFx,
    current: &ComplexFx,
    base_root: [u32; 8],
) -> [u32; 8] {
    let challenge_words = expected_sparse_interaction_challenges(base_root);
    let product_beta = Ext4::from_words(&challenge_words[1..5]);
    let product_gamma = Ext4::from_words(&challenge_words[5..9]);
    let round_beta = Ext4::from_words(&challenge_words[9..13]);
    let round_gamma = Ext4::from_words(&challenge_words[13..17]);
    let linear_beta = Ext4::from_words(&challenge_words[17..21]);
    let linear_gamma = Ext4::from_words(&challenge_words[21..25]);
    let boundary_beta = Ext4::from_words(&challenge_words[25..29]);
    let boundary_gamma = Ext4::from_words(&challenge_words[29..33]);

    let mut tasks = expected_sparse_transition_tasks(LIMBS as u32);
    tasks.resize(4_096, [14, 0, 0, 0, 0]);
    let rows = expected_sparse_rows(point, current);
    let task_count = 2_821;
    let mut product_accumulator = Ext4::ZERO;
    let mut round_accumulator = Ext4::ZERO;
    let mut linear_accumulator = Ext4::ZERO;
    let mut boundary_accumulator = Ext4::ZERO;
    let mut leaves = Vec::with_capacity(4_096);

    for (index, (task, row)) in tasks.into_iter().zip(rows).enumerate() {
        let (product_inverses, product_delta) = expected_ext4_interaction_row(
            expected_product_copy_ports(task, row),
            product_beta,
            product_gamma,
        );
        let (round_inverses, round_delta) = expected_ext4_interaction_row(
            expected_round_copy_ports(task, row),
            round_beta,
            round_gamma,
        );
        let (linear_inverses, linear_delta) = expected_ext4_interaction_row(
            expected_linear_copy_ports(task, row),
            linear_beta,
            linear_gamma,
        );
        let (boundary_inverses, boundary_delta) = expected_ext4_interaction_row(
            expected_boundary_copy_ports(task, row),
            boundary_beta,
            boundary_gamma,
        );

        let mut fields = vec![
            LIMBS as u32,
            task_count,
            4_096,
            index as u32,
            u32::from(index < task_count as usize),
        ];
        fields.extend(base_root);
        extend_ext4_words(&mut fields, product_accumulator);
        for inverse in product_inverses {
            extend_ext4_words(&mut fields, inverse);
        }
        extend_ext4_words(&mut fields, round_accumulator);
        for inverse in round_inverses {
            extend_ext4_words(&mut fields, inverse);
        }
        extend_ext4_words(&mut fields, linear_accumulator);
        for inverse in linear_inverses {
            extend_ext4_words(&mut fields, inverse);
        }
        extend_ext4_words(&mut fields, boundary_accumulator);
        for inverse in boundary_inverses {
            extend_ext4_words(&mut fields, inverse);
        }
        assert_eq!(fields.len(), 97, "interaction leaf schema must stay nominal");
        leaves.push(reference_poseidon_digest(b"SI01", &fields));

        product_accumulator = product_accumulator.add(product_delta);
        round_accumulator = round_accumulator.add(round_delta);
        linear_accumulator = linear_accumulator.add(linear_delta);
        boundary_accumulator = boundary_accumulator.add(boundary_delta);
    }
    assert_eq!(product_accumulator, Ext4::ZERO, "product bus must close");
    assert_eq!(round_accumulator, Ext4::ZERO, "round bus must close");
    assert_eq!(linear_accumulator, Ext4::ZERO, "linear bus must close");
    assert_eq!(boundary_accumulator, Ext4::ZERO, "boundary bus must close");
    reference_merkle_root(leaves)
}

fn expected_committed_words(
    claim: &Claim,
    start: &Boundary,
    end: &Boundary,
    leaves: u32,
) -> Vec<u32> {
    let mut statement_fields = complex_words(&claim.point);
    statement_fields.push(claim.bound);
    let statement = reference_poseidon_digest(b"RS01", &statement_fields);
    let start_digest = reference_poseidon_digest(b"RB01", &boundary_words(start));
    let end_digest = reference_poseidon_digest(b"RB01", &boundary_words(end));
    let mut words = vec![1, leaves, 1];
    words.extend(statement);
    words.extend([start.iteration, end.iteration, 1]);
    words.extend(start_digest);
    words.push(1);
    words.extend(end_digest);
    assert_eq!(words.len(), COMMITTED_ACCUMULATOR_WORDS);
    words
}

fn compile_fixture() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_recursive_fixed_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "recursive fixed oracle fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("recursive fixed oracle fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected recursive fixed oracle diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("recursive fixed oracle should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("recursive fixed oracle Wasm should validate");
    bytes
}

fn compile_sparse_auth_fixture() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_sparse_trace_auth_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "sparse trace authentication fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("sparse trace authentication fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected sparse trace authentication diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O2)
        .expect("sparse trace authentication fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit sparse trace authentication bytes");
    wasmparser::validate(&bytes).expect("sparse trace authentication Wasm should validate");
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

fn read_words(
    store: &wasmtime::Store<()>,
    memory: wasmtime::Memory,
    pointer: u32,
    length: u32,
) -> Vec<u32> {
    assert_eq!(length & 3, 0, "encoded carrier must be word-aligned");
    let mut bytes = vec![0u8; length as usize];
    memory
        .read(store, pointer as usize, &mut bytes)
        .expect("encoded accumulator must be readable");
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect()
}

fn write_word(
    store: &mut wasmtime::Store<()>,
    memory: wasmtime::Memory,
    pointer: u32,
    index: usize,
    value: u32,
) {
    memory
        .write(store, pointer as usize + index * 4, &value.to_le_bytes())
        .expect("mutated accumulator word must be writable");
}

fn encoded(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    memory: wasmtime::Memory,
    function: &str,
    arguments: &[u32],
) -> (u32, u32, Vec<u32>) {
    let result = call(store, instance, function, arguments, 2);
    let words = read_words(store, memory, result[0], result[1]);
    (result[0], result[1], words)
}

#[test]
fn recursive_fixed_chunks_match_bigint_and_reject_mutated_boundaries() {
    let bytes = compile_fixture();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes).expect("Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "oracle fixture must stay zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("zero-import oracle fixture should instantiate");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("oracle fixture should export linear memory");

    for (function, limbs) in [
        ("sparse_transition4_metadata", 4u32),
        ("sparse_transition8_metadata", 8u32),
        ("sparse_transition20_metadata", 20u32),
    ] {
        let tasks = 3 * limbs * limbs + 683 * limbs + 41;
        let trace_length = tasks.next_power_of_two();
        assert_eq!(
            call(&mut store, &instance, function, &[], 3),
            [tasks, trace_length, trace_length.ilog2()],
            "sparse transition metadata L={limbs}",
        );
    }
    let sparse_tasks = expected_sparse_transition_tasks(4);
    for (index, expected) in sparse_tasks.iter().enumerate() {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "sparse_transition4_task_signature",
                &[index as u32],
                5,
            ),
            *expected,
            "sparse transition task {index}",
        );
    }
    assert_eq!(
        call(
            &mut store,
            &instance,
            "sparse_transition4_task_signature",
            &[sparse_tasks.len() as u32],
            5,
        ),
        [14, 0, 0, 0, 0],
        "the first padding row must be explicitly invalid",
    );

    assert_eq!(
        call(
            &mut store,
            &instance,
            "baby_bear_safe_convolution_limbs",
            &[],
            1,
        ),
        [30],
        "BabyBear-safe convolution width must be derived from the modulus",
    );
    for limb_count in [4usize, 8] {
        let function = if limb_count == 4 {
            "convolution_schedule4"
        } else {
            "convolution_schedule8"
        };
        for coefficient in 0..limb_count * 2 {
            let count = if coefficient >= limb_count * 2 - 1 {
                0
            } else if coefficient < limb_count {
                coefficient + 1
            } else {
                limb_count * 2 - 1 - coefficient
            };
            for term in 0..=count {
                let first = if coefficient < limb_count {
                    0
                } else {
                    coefficient - (limb_count - 1)
                };
                let expected = if term < count {
                    [
                        count as u32,
                        (first + term) as u32,
                        (coefficient - first - term) as u32,
                    ]
                } else {
                    [count as u32, 0, 0]
                };
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        function,
                        &[coefficient as u32, term as u32],
                        3,
                    ),
                    expected,
                    "sparse convolution schedule L={limb_count}, k={coefficient}, t={term}",
                );
            }
        }
    }

    let mut product_cases = vec![
        (zero(), zero()),
        (fixed(false, 1, 1), fixed(false, 1, 1)),
        (fixed(true, 3, 4), fixed(false, 7, 8)),
        (
            Fx {
                negative: false,
                magnitude: radix_modulus() - BigUint::from(1u32),
            },
            Fx {
                negative: true,
                magnitude: radix_modulus() - BigUint::from(1u32),
            },
        ),
    ];
    let mut product_seed = 0xa5c3_1f27u32;
    for _ in 0..24 {
        let mut next_magnitude = || {
            let mut value = BigUint::from(0u32);
            for limb_index in 0..LIMBS {
                product_seed = product_seed
                    .wrapping_mul(1_664_525)
                    .wrapping_add(1_013_904_223);
                value += BigUint::from(product_seed & (LIMB_BASE - 1)) << (LIMB_BITS * limb_index);
            }
            value
        };
        let left_magnitude = next_magnitude();
        let right_magnitude = next_magnitude();
        product_cases.push((
            Fx {
                negative: (product_seed & 1) == 1 && left_magnitude != BigUint::from(0u32),
                magnitude: left_magnitude,
            },
            Fx {
                negative: (product_seed & 2) == 2 && right_magnitude != BigUint::from(0u32),
                magnitude: right_magnitude,
            },
        ));
    }

    for (case, (left, right)) in product_cases.iter().enumerate() {
        let arguments = fixed_arguments(left, right);
        let (_, _, actual) = encoded(
            &mut store,
            &instance,
            memory,
            "fixed_product4_encoded",
            &arguments,
        );
        let expected_witness = expected_product_witness_words(left, right);
        assert_eq!(
            actual, expected_witness,
            "fixed product witness case {case}",
        );
        let (_, _, actual_carry_range) = encoded(
            &mut store,
            &instance,
            memory,
            "fixed_product_carry_range4_encoded",
            &arguments,
        );
        assert_eq!(
            actual_carry_range,
            expected_product_carry_range_words(left, right),
            "fixed product carry range case {case}",
        );
        for coefficient in 0..(LIMBS * 2) as u32 {
            for relation in [0u32, 1] {
                for bit_index in 0..18u32 {
                    let mut residual_arguments = arguments.clone();
                    residual_arguments.extend([relation, coefficient, bit_index]);
                    assert_eq!(
                        call(
                            &mut store,
                            &instance,
                            "fixed_product_carry_range4_residual",
                            &residual_arguments,
                            1,
                        ),
                        [0],
                        "carry bit relation {relation}, coefficient {coefficient}, bit {bit_index}, case {case}",
                    );
                }
            }
            for relation in [2u32, 3] {
                let mut residual_arguments = arguments.clone();
                residual_arguments.extend([relation, coefficient, 0]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_product_carry_range4_residual",
                        &residual_arguments,
                        1,
                    ),
                    [0],
                    "carry scalar relation {relation}, coefficient {coefficient}, case {case}",
                );
            }
        }
        for mutation in 0..=3u32 {
            let mut mutation_arguments = arguments.clone();
            mutation_arguments.push(mutation);
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "fixed_product_carry_range4_mutation_holds",
                    &mutation_arguments,
                    1,
                ),
                [(mutation == 0) as u32],
                "carry range mutation {mutation}, case {case}",
            );
        }
        for mutation in 1..=3u32 {
            let mut mutation_arguments = arguments.clone();
            mutation_arguments.push(mutation);
            assert_ne!(
                call(
                    &mut store,
                    &instance,
                    "fixed_product_carry_range4_mutated_residual",
                    &mutation_arguments,
                    1,
                )[0],
                0,
                "mutated carry range residual {mutation}, case {case}",
            );
        }
        assert_eq!(
            call(
                &mut store,
                &instance,
                "fixed_product4_residuals",
                &arguments,
                18,
            ),
            [0; 18],
            "BabyBear AIR residual case {case}",
        );
        for mutation in 0..=8 {
            let mut mutated_arguments = arguments.clone();
            mutated_arguments.push(mutation);
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "fixed_product4_mutation_holds",
                    &mutated_arguments,
                    1,
                ),
                [(mutation == 0) as u32],
                "fixed product mutation {mutation}, case {case}",
            );
        }
        for mutation in [1u32, 2, 3, 4, 7, 8] {
            let mut mutated_arguments = arguments.clone();
            mutated_arguments.push(mutation);
            let expected = match mutation {
                2 => BABY_BEAR_MODULUS - LIMB_BASE,
                3 if expected_witness[17] == 0 => 1,
                8 if expected_witness[18] == 0 => BABY_BEAR_MODULUS - LIMB_BASE,
                8 => LIMB_BASE,
                _ => BABY_BEAR_MODULUS - 1,
            };
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "fixed_product4_mutated_residual",
                    &mutated_arguments,
                    1,
                ),
                [expected],
                "mutated BabyBear residual {mutation}, case {case}",
            );
        }
    }

    let mut linear_cases = product_cases.clone();
    linear_cases.extend([
        (fixed(false, 3, 2), fixed(false, 3, 2)),
        (fixed(true, 3, 2), fixed(true, 3, 2)),
        (fixed(false, 1, 8), fixed(true, 7, 8)),
        (zero(), fixed(true, 1, 1)),
        (fixed(false, 1, 1), zero()),
    ]);
    for (case, (left, right)) in linear_cases.iter().enumerate() {
        for subtract_right in [false, true] {
            let mut arguments = fixed_arguments(left, right);
            arguments.push(subtract_right as u32);
            let (_, _, actual) = encoded(
                &mut store,
                &instance,
                memory,
                "fixed_linear4_encoded",
                &arguments,
            );
            assert_eq!(
                actual,
                expected_linear_witness_words(left, right, subtract_right),
                "fixed linear witness case {case}, subtract={subtract_right}",
            );

            for relation in 0..=3u32 {
                for index in 0..LIMBS as u32 {
                    let mut residual_arguments = arguments.clone();
                    residual_arguments.extend([relation, index]);
                    assert_eq!(
                        call(
                            &mut store,
                            &instance,
                            "fixed_linear4_residual",
                            &residual_arguments,
                            1,
                        ),
                        [0],
                        "linear relation {relation}, limb {index}, case {case}, subtract={subtract_right}",
                    );
                }
            }
            for relation in 4..=9u32 {
                let mut residual_arguments = arguments.clone();
                residual_arguments.extend([relation, 0]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_linear4_residual",
                        &residual_arguments,
                        1,
                    ),
                    [0],
                    "linear scalar relation {relation}, case {case}, subtract={subtract_right}",
                );
            }
            let mut nonzero_arguments = arguments.clone();
            nonzero_arguments.extend([13, 0]);
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "fixed_linear4_residual",
                    &nonzero_arguments,
                    1,
                ),
                [0],
                "linear output nonzero relation, case {case}, subtract={subtract_right}",
            );
            for relation in 10..=12u32 {
                for index in 0..LIMBS as u32 {
                    let mut residual_arguments = arguments.clone();
                    residual_arguments.extend([relation, index]);
                    assert_eq!(
                        call(
                            &mut store,
                            &instance,
                            "fixed_linear4_residual",
                            &residual_arguments,
                            1,
                        ),
                        [0],
                        "linear bit relation {relation}, limb {index}, case {case}, subtract={subtract_right}",
                    );
                }
            }
            for mutation in 0..=12u32 {
                let mut mutation_arguments = arguments.clone();
                mutation_arguments.push(mutation);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_linear4_mutation_holds",
                        &mutation_arguments,
                        1,
                    ),
                    [(mutation == 0) as u32],
                    "linear mutation {mutation}, case {case}, subtract={subtract_right}",
                );
            }
            for mutation in [1u32, 2, 3, 4, 5, 6, 7, 8, 10, 11] {
                let mut mutation_arguments = arguments.clone();
                mutation_arguments.push(mutation);
                let residual = call(
                    &mut store,
                    &instance,
                    "fixed_linear4_mutated_residual",
                    &mutation_arguments,
                    1,
                )[0];
                assert_ne!(
                    residual, 0,
                    "mutated linear residual {mutation}, case {case}, subtract={subtract_right}",
                );
            }
        }
    }

    let range_cases = [
        zero(),
        fixed(false, 1, 1),
        fixed(true, 1, 1),
        fixed(false, 1, 8),
        fixed(true, 7, 8),
        Fx {
            negative: false,
            magnitude: radix_modulus() - BigUint::from(1u32),
        },
        Fx {
            negative: true,
            magnitude: radix_modulus() - BigUint::from(1u32),
        },
    ];
    for (case, value) in range_cases.iter().enumerate() {
        let arguments = fixed_words(value);
        let (_, _, actual) = encoded(
            &mut store,
            &instance,
            memory,
            "fixed_range4_encoded",
            &arguments,
        );
        assert_eq!(
            actual,
            expected_range_witness_words(value),
            "radix range witness case {case}",
        );
        for relation in [0u32, 1, 3] {
            for limb_index in 0..LIMBS as u32 {
                for bit_index in 0..LIMB_BITS as u32 {
                    let mut residual_arguments = arguments.clone();
                    residual_arguments.extend([relation, limb_index, bit_index]);
                    assert_eq!(
                        call(
                            &mut store,
                            &instance,
                            "fixed_range4_residual",
                            &residual_arguments,
                            1,
                        ),
                        [0],
                        "range relation {relation}, limb {limb_index}, bit {bit_index}, case {case}",
                    );
                }
            }
        }
        for limb_index in 0..LIMBS as u32 {
            let mut residual_arguments = arguments.clone();
            residual_arguments.extend([2, limb_index, 0]);
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "fixed_range4_residual",
                    &residual_arguments,
                    1,
                ),
                [0],
                "range reconstruction limb {limb_index}, case {case}",
            );
        }
        for relation in 4..=6u32 {
            let mut residual_arguments = arguments.clone();
            residual_arguments.extend([relation, 0, 0]);
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "fixed_range4_residual",
                    &residual_arguments,
                    1,
                ),
                [0],
                "range scalar relation {relation}, case {case}",
            );
        }
        for mutation in 0..=5u32 {
            let mut mutation_arguments = arguments.clone();
            mutation_arguments.push(mutation);
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "fixed_range4_mutation_holds",
                    &mutation_arguments,
                    1,
                ),
                [(mutation == 0) as u32],
                "range mutation {mutation}, case {case}",
            );
        }
        for mutation in 1..=5u32 {
            let mut mutation_arguments = arguments.clone();
            mutation_arguments.push(mutation);
            assert_ne!(
                call(
                    &mut store,
                    &instance,
                    "fixed_range4_mutated_residual",
                    &mutation_arguments,
                    1,
                )[0],
                0,
                "mutated range residual {mutation}, case {case}",
            );
        }
    }

    let mut transition_cases = vec![
        (
            ComplexFx {
                real: zero(),
                imaginary: zero(),
            },
            ComplexFx {
                real: zero(),
                imaginary: zero(),
            },
        ),
        (
            ComplexFx {
                real: fixed(true, 3, 4),
                imaginary: fixed(false, 1, 8),
            },
            ComplexFx {
                real: fixed(false, 5, 4),
                imaginary: fixed(true, 3, 8),
            },
        ),
        (
            ComplexFx {
                real: fixed(false, 1, 1),
                imaginary: fixed(false, 1, 1),
            },
            ComplexFx {
                real: fixed(true, 3, 2),
                imaginary: fixed(true, 3, 2),
            },
        ),
    ];
    let mut transition_seed = 0x9e37_79b9u32;
    for _ in 0..16 {
        transition_cases.push((
            ComplexFx {
                real: seeded_fixed(&mut transition_seed),
                imaginary: seeded_fixed(&mut transition_seed),
            },
            ComplexFx {
                real: seeded_fixed(&mut transition_seed),
                imaginary: seeded_fixed(&mut transition_seed),
            },
        ));
    }
    for (case, (point, current)) in transition_cases.iter().enumerate() {
        let arguments = transition_arguments(point, current);
        let (_, _, actual) = encoded(
            &mut store,
            &instance,
            memory,
            "fixed_transition4_encoded",
            &arguments,
        );
        assert_eq!(
            actual,
            expected_transition_witness_words(point, current),
            "fixed Mandelbrot transition witness case {case}",
        );
        if case == 0 {
            let expected_rows = expected_sparse_radix_rows(point, current);
            for (index, expected) in expected_rows.iter().enumerate() {
                let mut row_arguments = arguments.clone();
                row_arguments.push(index as u32);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_radix_row",
                        &row_arguments,
                        6,
                    ),
                    *expected,
                    "independently reconstructed sparse radix row {index}",
                );
            }
            let expected_constraints = 2_573 * LIMBS as u32 + 261;
            for challenge in [3u32, 7, 31] {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([challenge, u32::MAX, 0]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_radix_audit",
                        &audit_arguments,
                        3,
                    ),
                    [0, expected_constraints, expected_rows.len() as u32],
                    "clean sparse radix audit, challenge {challenge}",
                );
            }
            let first_unsigned_finish = 6 * (LIMBS as u32 * 14 + 1) + LIMBS as u32 * 14;
            for (row, lane) in [
                (0u32, 1u32),
                (0, 3),
                (0, 4),
                (0, 5),
                (0, 6),
                (first_unsigned_finish, 2),
            ] {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([7, row, lane]);
                let audited = call(
                    &mut store,
                    &instance,
                    "fixed_transition4_sparse_radix_audit",
                    &audit_arguments,
                    3,
                );
                assert_ne!(
                    audited[0], 0,
                    "sparse radix row {row} lane {lane} mutation must fail",
                );
                assert_eq!(audited[1], expected_constraints);
                assert_eq!(audited[2], expected_rows.len() as u32);
            }

            let expected_carry_rows = expected_sparse_carry_rows(current);
            for (index, expected) in expected_carry_rows.iter().enumerate() {
                let mut row_arguments = arguments.clone();
                row_arguments.push(index as u32);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_carry_row",
                        &row_arguments,
                        6,
                    ),
                    *expected,
                    "independently reconstructed sparse carry row {index}",
                );
            }
            let expected_carry_constraints = 1_122 * LIMBS as u32;
            for challenge in [3u32, 7, 31] {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([challenge, u32::MAX, 0]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_carry_audit",
                        &audit_arguments,
                        3,
                    ),
                    [
                        0,
                        expected_carry_constraints,
                        expected_carry_rows.len() as u32,
                    ],
                    "clean sparse carry audit, challenge {challenge}",
                );
            }
            for (row, lane) in [(0u32, 1u32), (0, 3), (0, 4), (0, 5), (0, 6), (36, 2)] {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([7, row, lane]);
                let audited = call(
                    &mut store,
                    &instance,
                    "fixed_transition4_sparse_carry_audit",
                    &audit_arguments,
                    3,
                );
                assert_ne!(
                    audited[0], 0,
                    "sparse carry row {row} lane {lane} mutation must fail",
                );
                assert_eq!(audited[1], expected_carry_constraints);
                assert_eq!(audited[2], expected_carry_rows.len() as u32);
            }

            let expected_product_rows = expected_sparse_product_rows(current);
            for (index, expected) in expected_product_rows.iter().enumerate() {
                let mut row_arguments = arguments.clone();
                row_arguments.push(index as u32);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_product_row",
                        &row_arguments,
                        6,
                    ),
                    *expected,
                    "independently reconstructed sparse product row {index}",
                );
            }
            let product_limb_count = LIMBS as u32;
            let expected_product_constraints =
                12 * product_limb_count * product_limb_count + 30 * product_limb_count + 3;
            for challenge in [3u32, 7, 31] {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([challenge, u32::MAX, 0]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_product_audit",
                        &audit_arguments,
                        3,
                    ),
                    [
                        0,
                        expected_product_constraints,
                        expected_product_rows.len() as u32,
                    ],
                    "clean sparse product and copy-bus audit, challenge {challenge}",
                );
            }
            for lane in 1u32..=6 {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([7, 0, lane]);
                let audited = call(
                    &mut store,
                    &instance,
                    "fixed_transition4_sparse_product_audit",
                    &audit_arguments,
                    3,
                );
                assert_ne!(
                    audited[0], 0,
                    "sparse product first-row lane {lane} mutation must fail",
                );
                assert_eq!(audited[1], expected_product_constraints);
                assert_eq!(audited[2], expected_product_rows.len() as u32);
            }
            for challenge in [3u32, 7, 31] {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([challenge, 0, 7]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_product_audit",
                        &audit_arguments,
                        3,
                    ),
                    [
                        1,
                        expected_product_constraints,
                        expected_product_rows.len() as u32,
                    ],
                    "the copy bus alone must reject a locally valid coordinated mutation",
                );
            }

            let expected_round_rows = expected_sparse_round_rows(current);
            for (index, expected) in expected_round_rows.iter().enumerate() {
                let mut row_arguments = arguments.clone();
                row_arguments.push(index as u32);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_round_row",
                        &row_arguments,
                        6,
                    ),
                    *expected,
                    "independently reconstructed sparse rounding row {index}",
                );
            }
            let expected_round_constraints = 15 * LIMBS as u32 + 25;
            for challenge in [3u32, 7, 31] {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([challenge, u32::MAX, 0]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_round_audit",
                        &audit_arguments,
                        3,
                    ),
                    [
                        0,
                        expected_round_constraints,
                        expected_round_rows.len() as u32,
                    ],
                    "clean sparse rounding and copy-bus audit, challenge {challenge}",
                );
            }
            for lane in 1u32..=6 {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([7, 0, lane]);
                let audited = call(
                    &mut store,
                    &instance,
                    "fixed_transition4_sparse_round_audit",
                    &audit_arguments,
                    3,
                );
                assert_ne!(
                    audited[0], 0,
                    "sparse rounding first-row lane {lane} mutation must fail",
                );
                assert_eq!(audited[1], expected_round_constraints);
                assert_eq!(audited[2], expected_round_rows.len() as u32);
            }
            for challenge in [3u32, 7, 31] {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([challenge, 0, 7]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_round_audit",
                        &audit_arguments,
                        3,
                    ),
                    [
                        1,
                        expected_round_constraints,
                        expected_round_rows.len() as u32,
                    ],
                    "the rounding copy bus alone must reject a locally valid mutation",
                );
            }

            let expected_linear_rows = expected_sparse_linear_rows(point, current);
            for (index, expected) in expected_linear_rows.iter().enumerate() {
                let mut row_arguments = arguments.clone();
                row_arguments.push(index as u32);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_linear_row",
                        &row_arguments,
                        6,
                    ),
                    *expected,
                    "independently reconstructed sparse linear row {index}",
                );
            }
            let expected_linear_constraints = 60 * LIMBS as u32 + 36;
            for challenge in [3u32, 7, 31] {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([challenge, u32::MAX, 0]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_linear_audit",
                        &audit_arguments,
                        3,
                    ),
                    [
                        0,
                        expected_linear_constraints,
                        expected_linear_rows.len() as u32,
                    ],
                    "clean sparse linear and copy-bus audit, challenge {challenge}",
                );
            }
            for lane in 1u32..=6 {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([7, 0, lane]);
                let audited = call(
                    &mut store,
                    &instance,
                    "fixed_transition4_sparse_linear_audit",
                    &audit_arguments,
                    3,
                );
                assert_ne!(
                    audited[0], 0,
                    "sparse linear first-row lane {lane} mutation must fail",
                );
                assert_eq!(audited[1], expected_linear_constraints);
                assert_eq!(audited[2], expected_linear_rows.len() as u32);
            }
            for challenge in [3u32, 7, 31] {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([challenge, 0, 7]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_linear_audit",
                        &audit_arguments,
                        3,
                    ),
                    [
                        1,
                        expected_linear_constraints,
                        expected_linear_rows.len() as u32,
                    ],
                    "the linear copy bus alone must reject a locally valid mutation",
                );
            }

            let expected_boundary_rows = expected_sparse_boundary_rows(point, current);
            for (index, expected) in expected_boundary_rows.iter().enumerate() {
                let mut row_arguments = arguments.clone();
                row_arguments.push(index as u32);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_boundary_row",
                        &row_arguments,
                        6,
                    ),
                    *expected,
                    "independently reconstructed sparse boundary row {index}",
                );
            }
            let expected_boundary_constraints = 10 * LIMBS as u32 + 21;
            for challenge in [3u32, 7, 31] {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([challenge, u32::MAX, 0]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_boundary_audit",
                        &audit_arguments,
                        3,
                    ),
                    [
                        0,
                        expected_boundary_constraints,
                        expected_boundary_rows.len() as u32,
                    ],
                    "clean sparse boundary and copy-bus audit, challenge {challenge}",
                );
            }
            for lane in 1u32..=6 {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([7, 0, lane]);
                let audited = call(
                    &mut store,
                    &instance,
                    "fixed_transition4_sparse_boundary_audit",
                    &audit_arguments,
                    3,
                );
                assert_ne!(
                    audited[0], 0,
                    "sparse boundary first-row lane {lane} mutation must fail",
                );
                assert_eq!(audited[1], expected_boundary_constraints);
                assert_eq!(audited[2], expected_boundary_rows.len() as u32);
            }
            for challenge in [3u32, 7, 31] {
                let mut audit_arguments = arguments.clone();
                audit_arguments.extend([challenge, 1, 7]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_boundary_audit",
                        &audit_arguments,
                        3,
                    ),
                    [
                        1,
                        expected_boundary_constraints,
                        expected_boundary_rows.len() as u32,
                    ],
                    "the boundary copy bus alone must reject a locally valid mutation",
                );
            }

            let first_sparse_kind = |kind: u32| {
                sparse_tasks
                    .iter()
                    .position(|task| task[0] == kind)
                    .expect("every active sparse arithmetic kind must occur")
            };
            let first_linear_role = |role: u32| {
                sparse_tasks
                    .iter()
                    .position(|task| task[0] == 9 && task[2] == role)
                    .expect("every sparse linear role must occur")
            };
            let arithmetic_mutations = [
                (first_sparse_kind(0), 1u32),
                (first_sparse_kind(1), 1),
                (first_sparse_kind(2), 2),
                (first_sparse_kind(3), 1),
                (first_sparse_kind(4), 1),
                (first_sparse_kind(5), 4),
                (first_sparse_kind(6), 2),
                (first_sparse_kind(7), 4),
                (first_sparse_kind(8), 1),
                (first_linear_role(0), 4),
                (first_linear_role(1), 4),
                (first_linear_role(2), 4),
                (first_linear_role(3), 4),
                (first_sparse_kind(10), 6),
                (first_sparse_kind(11), 1),
                (first_sparse_kind(12), 1),
                (first_sparse_kind(13), 1),
                (sparse_tasks.len(), 1),
            ];
            for challenge in [3u32, 7, 31] {
                let mut baseline_arguments = arguments.clone();
                baseline_arguments.extend([challenge, u32::MAX, 0]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_arithmetic_audit",
                        &baseline_arguments,
                        3,
                    ),
                    [0, 413_678, 4096],
                    "index-free sparse arithmetic baseline, challenge {challenge}",
                );
                for (row, lane) in arithmetic_mutations {
                    let mut audit_arguments = arguments.clone();
                    audit_arguments.extend([challenge, row as u32, lane]);
                    let audit = call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_arithmetic_audit",
                        &audit_arguments,
                        3,
                    );
                    assert!(
                        audit[0] > 0,
                        "index-free sparse arithmetic mutation at row {row}, lane {lane}, challenge {challenge}",
                    );
                    assert_eq!(audit[1], 413_678);
                    assert_eq!(audit[2], 4096);
                }
            }

            for (beta, gamma, fold_challenge, receipt_challenge) in [
                (17u32, 29u32, 7u32, 31u32),
                (41, 73, 19, 37),
                (101, 211, 43, 61),
            ] {
                let mut interaction_arguments = arguments.clone();
                interaction_arguments.extend([beta, gamma, fold_challenge, receipt_challenge, 0]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_product_interaction_audit",
                        &interaction_arguments,
                        4,
                    ),
                    [
                        0,
                        20_481,
                        4_096,
                        expected_product_interaction_receipt(
                            point,
                            current,
                            beta,
                            gamma,
                            receipt_challenge,
                        ),
                    ],
                    "selector-only product interaction challenges ({beta}, {gamma})",
                );
                let mut round_arguments = arguments.clone();
                round_arguments.extend([beta, gamma, fold_challenge, receipt_challenge, 0]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_round_interaction_audit",
                        &round_arguments,
                        4,
                    ),
                    [
                        0,
                        45_057,
                        4_096,
                        expected_round_interaction_receipt(
                            point,
                            current,
                            beta,
                            gamma,
                            receipt_challenge,
                        ),
                    ],
                    "selector-only rounding interaction challenges ({beta}, {gamma})",
                );
                let mut linear_arguments = arguments.clone();
                linear_arguments.extend([beta, gamma, fold_challenge, receipt_challenge, 0]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_linear_interaction_audit",
                        &linear_arguments,
                        4,
                    ),
                    [
                        0,
                        69_633,
                        4_096,
                        expected_linear_interaction_receipt(
                            point,
                            current,
                            beta,
                            gamma,
                            receipt_challenge,
                        ),
                    ],
                    "selector-only linear interaction challenges ({beta}, {gamma})",
                );
                let mut boundary_arguments = arguments.clone();
                boundary_arguments.extend([beta, gamma, fold_challenge, receipt_challenge, 0]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_boundary_interaction_audit",
                        &boundary_arguments,
                        4,
                    ),
                    [
                        0,
                        20_481,
                        4_096,
                        expected_boundary_interaction_receipt(
                            point,
                            current,
                            beta,
                            gamma,
                            receipt_challenge,
                        ),
                    ],
                    "selector-only boundary interaction challenges ({beta}, {gamma})",
                );
            }
            for mutation in 1u32..=6 {
                let mut interaction_arguments = arguments.clone();
                interaction_arguments.extend([17, 29, 7, 31, mutation]);
                let audit = call(
                    &mut store,
                    &instance,
                    "fixed_transition4_sparse_product_interaction_audit",
                    &interaction_arguments,
                    4,
                );
                if mutation == 6 {
                    assert_eq!(
                        audit[0], 1,
                        "the locally valid coordinated mutation must fail only at the interaction terminal",
                    );
                } else {
                    assert!(
                        audit[0] > 0,
                        "product interaction mutation {mutation} must fail",
                    );
                }
                assert_eq!(audit[1], 20_481);
                assert_eq!(audit[2], 4_096);
            }
            for mutation in 1u32..=6 {
                let mut interaction_arguments = arguments.clone();
                interaction_arguments.extend([17, 29, 7, 31, mutation]);
                let audit = call(
                    &mut store,
                    &instance,
                    "fixed_transition4_sparse_round_interaction_audit",
                    &interaction_arguments,
                    4,
                );
                if mutation == 6 {
                    assert_eq!(
                        audit[0], 1,
                        "the locally valid rounding mutation must fail only at the interaction terminal",
                    );
                } else {
                    assert!(
                        audit[0] > 0,
                        "rounding interaction mutation {mutation} must fail",
                    );
                }
                assert_eq!(audit[1], 45_057);
                assert_eq!(audit[2], 4_096);
            }
            for mutation in 1u32..=6 {
                let mut interaction_arguments = arguments.clone();
                interaction_arguments.extend([17, 29, 7, 31, mutation]);
                let audit = call(
                    &mut store,
                    &instance,
                    "fixed_transition4_sparse_linear_interaction_audit",
                    &interaction_arguments,
                    4,
                );
                if mutation == 6 {
                    assert_eq!(
                        audit[0], 1,
                        "the locally valid linear mutation must fail only at the interaction terminal",
                    );
                } else {
                    assert!(
                        audit[0] > 0,
                        "linear interaction mutation {mutation} must fail",
                    );
                }
                assert_eq!(audit[1], 69_633);
                assert_eq!(audit[2], 4_096);
            }
            for mutation in 1u32..=6 {
                let mut interaction_arguments = arguments.clone();
                interaction_arguments.extend([17, 29, 7, 31, mutation]);
                let audit = call(
                    &mut store,
                    &instance,
                    "fixed_transition4_sparse_boundary_interaction_audit",
                    &interaction_arguments,
                    4,
                );
                if mutation == 6 {
                    assert_eq!(
                        audit[0], 1,
                        "the locally valid boundary mutation must fail only at the interaction terminal",
                    );
                } else {
                    assert!(
                        audit[0] > 0,
                        "boundary interaction mutation {mutation} must fail",
                    );
                }
                assert_eq!(audit[1], 20_481);
                assert_eq!(audit[2], 4_096);
            }

            let shared_mutations = [
                (1u32, "fixed_transition4_sparse_product_audit", 1u32, 1u32),
                (2, "fixed_transition4_sparse_product_audit", 1, 2),
                (3, "fixed_transition4_sparse_round_audit", 0, 4),
                (4, "fixed_transition4_sparse_linear_audit", LIMBS as u32, 4),
                (5, "fixed_transition4_sparse_boundary_audit", 1, 2),
                (6, "fixed_transition4_sparse_radix_audit", 13, 8),
            ];
            for challenge in [3u32, 7, 31] {
                for (mutation, sparse_function, sparse_row, sparse_lane) in
                    shared_mutations.iter().copied()
                {
                    let mut wide_arguments = arguments.clone();
                    wide_arguments.extend([challenge, mutation]);
                    let wide = call(
                        &mut store,
                        &instance,
                        "fixed_transition4_constraint_fold",
                        &wide_arguments,
                        3,
                    );
                    assert_eq!(wide[0], 1);
                    assert_ne!(
                        wide[2], 0,
                        "wide mutation {mutation}, challenge {challenge}",
                    );

                    let mut sparse_arguments = arguments.clone();
                    sparse_arguments.extend([challenge, sparse_row, sparse_lane]);
                    let sparse = call(&mut store, &instance, sparse_function, &sparse_arguments, 3);
                    assert_ne!(
                        sparse[0], 0,
                        "sparse mutation {mutation}, challenge {challenge}",
                    );
                }
            }
        }
        if case == 1 {
            let auth_bytes = compile_sparse_auth_fixture();
            let auth_engine = wasmtime::Engine::default();
            let auth_module = wasmtime::Module::new(&auth_engine, &auth_bytes)
                .expect("sparse trace authentication Wasm module should load");
            assert_eq!(
                auth_module.imports().count(),
                0,
                "sparse trace authentication fixture must stay zero-import",
            );
            let mut auth_store = wasmtime::Store::new(&auth_engine, ());
            let auth_instance = wasmtime::Instance::new(&mut auth_store, &auth_module, &[])
                .expect("sparse trace authentication fixture should instantiate");
            let auth_memory = auth_instance
                .get_memory(&mut auth_store, "memory")
                .expect("sparse trace authentication fixture should export linear memory");

            let requests = [0u32, 2_820, 2_821, 4_095];
            let mut opening_arguments = arguments.clone();
            opening_arguments.extend(requests);
            let (pointer, length, opening_words) = encoded(
                &mut auth_store,
                &auth_instance,
                auth_memory,
                "fixed_transition4_sparse_trace_authentication_encoded",
                &opening_arguments,
            );
            assert!(length > 0, "sparse opening must encode");
            assert_eq!(opening_words[0], 1, "authentication must be valid");
            assert_eq!(opening_words[1], 1, "sparse trace root must be valid");
            let expected_root = expected_sparse_trace_root(point, current);
            assert_eq!(
                opening_words[2..10],
                expected_root,
                "Fe sparse trace root must match independent Plonky3 reconstruction",
            );
            assert_eq!(opening_words[10], 1, "opening must be valid");
            assert_eq!(opening_words[11], 1, "multipath must be valid");
            assert_eq!(opening_words[12], requests.len() as u32);

            let mut challenge_arguments = vec![1];
            challenge_arguments.extend(expected_root);
            let challenge_words = call(
                &mut auth_store,
                &auth_instance,
                "sparse_interaction_challenges4",
                &challenge_arguments,
                33,
            );
            assert_eq!(
                challenge_words,
                expected_sparse_interaction_challenges(expected_root),
                "all interaction challenges must be transcript-derived quartic values",
            );
            let mut invalid_challenge_arguments = vec![0];
            invalid_challenge_arguments.extend(expected_root);
            assert_eq!(
                call(
                    &mut auth_store,
                    &auth_instance,
                    "sparse_interaction_challenges4",
                    &invalid_challenge_arguments,
                    33,
                ),
                vec![0; 33],
                "invalid trace roots must fail closed before challenge derivation",
            );

            let verify_arguments = vec![pointer, length];
            assert_eq!(
                call(
                    &mut auth_store,
                    &auth_instance,
                    "fixed_transition4_sparse_trace_authentication_verify_at",
                    &verify_arguments,
                    1,
                ),
                [1],
                "canonical sparse opening must authenticate",
            );

            let leaf_count = opening_words[12] as usize;
            let sibling_count_index = 13 + leaf_count;
            let sibling_count = opening_words[sibling_count_index] as usize;
            let first_sibling = sibling_count_index + 1;
            let first_leaf = first_sibling + sibling_count * 8;
            for mutation_index in [
                0usize,
                2,
                10,
                13,
                first_sibling,
                first_leaf,
                first_leaf + 4,
                first_leaf + 5,
                first_leaf + 20,
                first_leaf + 24,
                first_leaf + 32,
                first_leaf + 33,
                first_leaf + 34,
                first_leaf + 37,
                first_leaf + 40,
            ] {
                let original = opening_words[mutation_index];
                let mutated = if mutation_index == 0 || mutation_index == 10 {
                    2
                } else {
                    (original + 1) % BABY_BEAR_MODULUS
                };
                write_word(
                    &mut auth_store,
                    auth_memory,
                    pointer,
                    mutation_index,
                    mutated,
                );
                assert_eq!(
                    call(
                        &mut auth_store,
                        &auth_instance,
                        "fixed_transition4_sparse_trace_authentication_verify_at",
                        &verify_arguments,
                        1,
                    ),
                    [0],
                    "opening mutation at word {mutation_index} must fail",
                );
                write_word(
                    &mut auth_store,
                    auth_memory,
                    pointer,
                    mutation_index,
                    original,
                );
            }

            let mut truncated_arguments = verify_arguments.clone();
            truncated_arguments[1] = length - 4;
            assert_eq!(
                call(
                    &mut auth_store,
                    &auth_instance,
                    "fixed_transition4_sparse_trace_authentication_verify_at",
                    &truncated_arguments,
                    1,
                ),
                [0],
                "truncated sparse opening must fail",
            );

            let mut invalid_request_arguments = arguments.clone();
            invalid_request_arguments.extend([0, 1, 2, 4_096]);
            let invalid = call(
                &mut auth_store,
                &auth_instance,
                "fixed_transition4_sparse_trace_authentication_encoded",
                &invalid_request_arguments,
                2,
            );
            assert_eq!(invalid[1], 0, "out-of-range opening request must fail");
        }
        for challenge in [3u32, 7, 31] {
            let mut clean_arguments = arguments.clone();
            clean_arguments.extend([challenge, 0]);
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "fixed_transition4_constraint_fold",
                    &clean_arguments,
                    3,
                ),
                [1, expected_fixed_air_constraint_count(LIMBS as u32), 0],
                "pure field transition constraints, case {case}, challenge {challenge}",
            );
            for mutation in 1..=6u32 {
                let mut mutation_arguments = arguments.clone();
                mutation_arguments.extend([challenge, mutation]);
                let evaluated = call(
                    &mut store,
                    &instance,
                    "fixed_transition4_constraint_fold",
                    &mutation_arguments,
                    3,
                );
                assert_eq!(evaluated[0], 1);
                assert_eq!(
                    evaluated[1],
                    expected_fixed_air_constraint_count(LIMBS as u32),
                );
                assert_ne!(
                    evaluated[2], 0,
                    "field transition mutation {mutation}, case {case}, challenge {challenge}",
                );
            }
        }
        let mut unsafe_width_arguments = arguments.clone();
        unsafe_width_arguments.extend([7, 7]);
        assert_eq!(
            call(
                &mut store,
                &instance,
                "fixed_transition4_constraint_fold",
                &unsafe_width_arguments,
                3,
            ),
            [0, 0, 0],
            "unsafe field width must fail closed, case {case}",
        );
        for mutation in 0..=10u32 {
            let mut mutation_arguments = arguments.clone();
            mutation_arguments.push(mutation);
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "fixed_transition4_mutation_holds",
                    &mutation_arguments,
                    1,
                ),
                [(mutation == 0) as u32],
                "transition mutation {mutation}, case {case}",
            );
        }
        for mutation in [1u32, 2] {
            let mut mutation_arguments = arguments.clone();
            mutation_arguments.push(mutation);
            assert_ne!(
                call(
                    &mut store,
                    &instance,
                    "fixed_transition4_mutated_output_residual",
                    &mutation_arguments,
                    1,
                )[0],
                0,
                "transition output residual mutation {mutation}, case {case}",
            );
        }
    }

    let directed = [
        (
            Claim {
                point: ComplexFx {
                    real: zero(),
                    imaginary: zero(),
                },
                bound: 32,
            },
            9,
        ),
        (
            Claim {
                point: ComplexFx {
                    real: fixed(true, 3, 4),
                    imaginary: fixed(false, 1, 8),
                },
                bound: 64,
            },
            17,
        ),
        (
            Claim {
                point: ComplexFx {
                    real: fixed(false, 1, 1),
                    imaginary: fixed(false, 1, 1),
                },
                bound: 32,
            },
            12,
        ),
        (
            Claim {
                point: ComplexFx {
                    real: fixed(true, 2, 1),
                    imaginary: zero(),
                },
                bound: 32,
            },
            16,
        ),
    ];

    for (claim, steps) in &directed {
        let end = evaluate(claim, *steps);
        let mut arguments = claim_args(claim);
        arguments.push(*steps);
        let (_, _, actual) = encoded(
            &mut store,
            &instance,
            memory,
            "recursive_chunk4_encoded",
            &arguments,
        );
        assert_eq!(actual, expected_leaf_words(claim, &end));

        let (_, _, committed) = encoded(
            &mut store,
            &instance,
            memory,
            "recursive_committed_leaf4_encoded",
            &arguments,
        );
        let start = Boundary {
            iteration: 0,
            z: ComplexFx {
                real: zero(),
                imaginary: zero(),
            },
            escaped: false,
        };
        assert_eq!(committed, expected_committed_words(claim, &start, &end, 1),);
    }

    let mut seed = 0x4d42_3255u32;
    for case in 0..24u32 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let real_low = seed;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let real_mag = ((BigUint::from(seed & 0xff) << 32) + BigUint::from(real_low))
            % (BigUint::from(2u32) * scale());
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let imaginary_low = seed;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let imaginary_mag = ((BigUint::from(seed & 0xff) << 32) + BigUint::from(imaginary_low))
            % (BigUint::from(2u32) * scale());
        let claim = Claim {
            point: ComplexFx {
                real: Fx {
                    negative: (seed & 1) == 1 && real_mag != BigUint::from(0u32),
                    magnitude: real_mag,
                },
                imaginary: Fx {
                    negative: (seed & 2) == 2 && imaginary_mag != BigUint::from(0u32),
                    magnitude: imaginary_mag,
                },
            },
            bound: 48,
        };
        let steps = 1 + case % 11;
        let end = evaluate(&claim, steps);
        let mut arguments = claim_args(&claim);
        arguments.push(steps);
        let (_, _, actual) = encoded(
            &mut store,
            &instance,
            memory,
            "recursive_chunk4_encoded",
            &arguments,
        );
        assert_eq!(actual, expected_leaf_words(&claim, &end), "case {case}");
    }

    let merge_claim = Claim {
        point: ComplexFx {
            real: fixed(true, 3, 4),
            imaginary: zero(),
        },
        bound: 64,
    };
    for mutation in 0..=4 {
        let mut arguments = claim_args(&merge_claim);
        arguments.extend([4, 5, mutation]);
        let (_, _, words) = encoded(
            &mut store,
            &instance,
            memory,
            "recursive_merge4_encoded",
            &arguments,
        );
        assert_eq!(words[0], (mutation == 0) as u32, "mutation {mutation}");
        assert_eq!(words[1], if mutation == 0 { 2 } else { 0 });
        if mutation == 0 {
            let start = Boundary {
                iteration: 0,
                z: ComplexFx {
                    real: zero(),
                    imaginary: zero(),
                },
                escaped: false,
            };
            let end = evaluate(&merge_claim, 9);
            assert_eq!(
                words,
                expected_accumulator_words(&merge_claim, &start, &end, 2),
            );
        }
    }

    for mutation in 0..=5 {
        let mut arguments = claim_args(&merge_claim);
        arguments.extend([4, 5, mutation]);
        let (_, _, words) = encoded(
            &mut store,
            &instance,
            memory,
            "recursive_committed_merge4_encoded",
            &arguments,
        );
        assert_eq!(words.len(), COMMITTED_ACCUMULATOR_WORDS);
        assert_eq!(words[0], (mutation == 0) as u32, "mutation {mutation}");
        assert_eq!(words[1], if mutation == 0 { 2 } else { 0 });
        if mutation == 0 {
            let start = Boundary {
                iteration: 0,
                z: ComplexFx {
                    real: zero(),
                    imaginary: zero(),
                },
                escaped: false,
            };
            let end = evaluate(&merge_claim, 9);
            assert_eq!(
                words,
                expected_committed_words(&merge_claim, &start, &end, 2),
            );
        }
    }

    let zero_claim = Claim {
        point: ComplexFx {
            real: zero(),
            imaginary: zero(),
        },
        bound: 16,
    };
    let mut base_arguments = claim_args(&zero_claim);
    base_arguments.push(4);
    let (pointer, length, _) = encoded(
        &mut store,
        &instance,
        memory,
        "recursive_chunk4_encoded",
        &base_arguments,
    );
    assert_eq!(
        call(
            &mut store,
            &instance,
            "recursive_accumulator4_decode_at",
            &[pointer, length],
            2,
        ),
        [1, 1],
    );

    for (index, value, expected) in [
        (0usize, 2u32, [0u32, 0u32]),
        (2, 1, [0, 0]),
        (3, LIMB_BASE, [0, 0]),
        (27, 1, [1, 0]),
    ] {
        let (pointer, length, _) = encoded(
            &mut store,
            &instance,
            memory,
            "recursive_chunk4_encoded",
            &base_arguments,
        );
        write_word(&mut store, memory, pointer, index, value);
        assert_eq!(
            call(
                &mut store,
                &instance,
                "recursive_accumulator4_decode_at",
                &[pointer, length],
                2,
            ),
            expected,
            "canonical mutation at word {index}",
        );
    }

    let (pointer, length, _) = encoded(
        &mut store,
        &instance,
        memory,
        "recursive_chunk4_encoded",
        &base_arguments,
    );
    assert_eq!(
        call(
            &mut store,
            &instance,
            "recursive_accumulator4_decode_at",
            &[pointer, length - 4],
            2,
        ),
        [0, 0],
    );
    assert_eq!(
        call(
            &mut store,
            &instance,
            "recursive_accumulator4_decode_at",
            &[pointer, length + 4],
            2,
        ),
        [0, 0],
    );
}

#[test]
fn sparse_quartic_interaction_root_matches_independent_port_oracle() {
    let bytes = compile_sparse_auth_fixture();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes)
        .expect("sparse interaction Wasm module should load");
    assert_eq!(module.imports().count(), 0, "interaction fixture must stay zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("interaction fixture should instantiate");
    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let expected_base_root = expected_sparse_trace_root(&point, &current);
    let expected_interaction_root =
        expected_sparse_interaction_root(&point, &current, expected_base_root);
    let mut mutated_interaction_root = expected_interaction_root;
    mutated_interaction_root[3] = bb_add(mutated_interaction_root[3], 1);
    let mut arguments = transition_arguments(&point, &current);
    arguments.extend(expected_interaction_root);
    arguments.extend(mutated_interaction_root);
    let words = call(
        &mut store,
        &instance,
        "fixed_transition4_sparse_interaction_root",
        &arguments,
        28,
    );
    assert_eq!(words[0], 1, "interaction trace must be valid");
    assert_eq!(words[1], 1, "Fe must accept the independent interaction root");
    assert_eq!(
        words[2], 0,
        "Fe must reject a directed interaction-root coefficient mutation",
    );
    assert_eq!(words[3], 0, "an invalid base commitment must fail closed");
    assert_eq!(
        words[4..12],
        expected_base_root,
        "interaction trace must retain its exact base commitment",
    );
    assert_eq!(
        words[12..20],
        expected_interaction_root,
        "Fe interaction root must match the independent quartic port reconstruction",
    );
    assert_eq!(
        words[20..28],
        [0; 8],
        "an invalid base commitment must return the canonical zero root",
    );
}
