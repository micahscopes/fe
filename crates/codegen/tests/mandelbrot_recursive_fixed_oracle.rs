//! Independent bigint gate for the field-neutral recursive Mandelbrot carrier.

#[cfg(debug_assertions)]
compile_error!(
    "mandelbrot_recursive_fixed_oracle requires Cargo --release; run with \
     --features expensive-release-oracles"
);

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    BackendKind, OptLevel, WasmCompileOptions, compile_runtime_package_wasm_with_options,
    layout_for,
};
use hir::hir_def::HirIngot;
use num_bigint::BigUint;
use p3_baby_bear::{BabyBear as P3BabyBear, default_babybear_poseidon2_16};
use p3_dft::{Radix2Dit, TwoAdicSubgroupDft};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_matrix::dense::RowMajorMatrix;
use p3_symmetric::Permutation;
use std::path::{Path, PathBuf};
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
const PRODUCTION_TRACE_ROWS: usize = 4_096;
const SPARSE_BASE_FIELDS: usize = 260;
const SPARSE_BASE_BROWSER_RECEIPT_DIR: &str = "MB2_SPARSE_BASE_BROWSER_RECEIPT_DIR";
const SPARSE_LDE_BROWSER_RECEIPT_DIR: &str = "MB2_SPARSE_LDE_BROWSER_RECEIPT_DIR";
const SPARSE_COMPOSITION_BIND_BROWSER_RECEIPT_DIR: &str =
    "MB2_SPARSE_COMPOSITION_BIND_BROWSER_RECEIPT_DIR";

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

fn expected_sparse_control_fields(task: [u32; 5]) -> [u32; 38] {
    let [kind, first, second, third, fourth] = task;
    let mut fields = [0u32; 38];
    fields[kind as usize] = 1;
    match kind {
        0 => {
            fields[32] = first;
            fields[33] = second;
            fields[34] = third;
            fields[37] = 1 << third;
        }
        1 => {
            fields[32] = first;
            fields[33] = second;
        }
        2 => fields[32] = first,
        3 => {
            fields[32] = first;
            fields[33] = second;
            fields[34] = fourth;
            fields[35] = third;
            fields[37] = 1 << fourth;
        }
        4 => {
            fields[32] = first;
            fields[33] = second;
        }
        5 => {
            fields[32] = first;
            fields[33] = second;
            fields[34] = third;
            fields[35] = u32::from(second >= LIMBS as u32);
            fields[36] = if second < LIMBS as u32 {
                second + 1
            } else if second < 2 * LIMBS as u32 - 1 {
                2 * LIMBS as u32 - 1 - second
            } else {
                0
            };
        }
        6 => {
            fields[32] = first;
            fields[33] = second;
            fields[35] = u32::from(second >= LIMBS as u32);
            fields[36] = if second < LIMBS as u32 {
                second + 1
            } else if second < 2 * LIMBS as u32 - 1 {
                2 * LIMBS as u32 - 1 - second
            } else {
                0
            };
        }
        7 => {
            fields[32] = first;
            fields[33] = second;
        }
        8 => fields[32] = first,
        9 => {
            fields[15 + second as usize] = 1;
            fields[32] = first;
            fields[33] = second;
            fields[34] = third;
            fields[35] = u32::from(first == 0);
        }
        10 => {
            fields[32] = first;
            fields[35] = u32::from(first == 0);
        }
        11 => fields[32] = first,
        12 => {
            fields[32] = first;
            fields[33] = second;
        }
        13 | 14 => {}
        _ => panic!("unknown sparse control task kind {kind}"),
    }
    if kind <= 2 {
        if first < 6 {
            fields[19] = 1;
            fields[20 + (first / 2) as usize] = 1;
        } else if first < 15 {
            fields[23 + ((first - 6) % 3) as usize] = 1;
        } else {
            fields[26 + ((first - 15) % 4) as usize] = 1;
        }
        fields[35] = u32::from(fields[19] == 1 || fields[25] == 1 || fields[29] == 1);
    }
    if kind == 0 || kind == 1 {
        fields[30] = u32::from(second + 2 == LIMBS as u32);
        fields[31] = u32::from(second + 1 == LIMBS as u32);
    }
    fields
}

fn expected_sparse_control_rows() -> Vec<[u32; 38]> {
    let mut rows: Vec<[u32; 38]> = expected_sparse_transition_tasks(LIMBS as u32)
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

fn reference_poseidon_permutation_with<P>(
    permutation: &P,
    input: [u32; POSEIDON_WIDTH],
) -> [u32; POSEIDON_WIDTH]
where
    P: Permutation<[P3BabyBear; POSEIDON_WIDTH]>,
{
    let mut state = input.map(P3BabyBear::from_u32);
    permutation.permute_mut(&mut state);
    state.map(|value| value.as_canonical_u32())
}

fn reference_poseidon_digest_with<P>(permutation: &P, tag: &[u8; 4], fields: &[u32]) -> [u32; 8]
where
    P: Permutation<[P3BabyBear; POSEIDON_WIDTH]>,
{
    let mut message = vec![u32::from_be_bytes(*tag), fields.len() as u32];
    message.extend_from_slice(fields);
    let mut state = [0u32; POSEIDON_WIDTH];
    for block in message.chunks(8) {
        state[..block.len()].copy_from_slice(block);
        state = reference_poseidon_permutation_with(permutation, state);
    }
    state[..8].try_into().unwrap()
}

fn reference_poseidon_digest(tag: &[u8; 4], fields: &[u32]) -> [u32; 8] {
    reference_poseidon_digest_with(&default_babybear_poseidon2_16(), tag, fields)
}

fn reference_packed_u32_commitment(tag: &[u8; 4], words: &[u32]) -> [u32; 8] {
    let mut packed = BigUint::from(0u32);
    for (index, word) in words.iter().copied().enumerate() {
        packed |= BigUint::from(word) << (index * 32);
    }
    let field_count = (words.len() * 32).div_ceil(30);
    let mask = (BigUint::from(1u32) << 30usize) - BigUint::from(1u32);
    let fields = (0..field_count)
        .map(|index| {
            ((&packed >> (index * 30)) & &mask)
                .to_u32_digits()
                .first()
                .copied()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    let mut message = vec![u32::from_be_bytes(*tag), (words.len() * 32) as u32];
    message.extend(fields);
    let mut state = [0u32; POSEIDON_WIDTH];
    let permutation = default_babybear_poseidon2_16();
    for block in message.chunks(8) {
        state[..block.len()].copy_from_slice(block);
        state = reference_poseidon_permutation_with(&permutation, state);
    }
    state[..8].try_into().unwrap()
}

fn reference_security_floor_log2(value: u64) -> u64 {
    assert_ne!(value, 0);
    let integer_part = 63 - value.leading_zeros() as u64;
    let mut result = integer_part << 16;
    let mut mantissa = if integer_part >= 31 {
        value >> (integer_part - 31)
    } else {
        value << (31 - integer_part)
    };
    for bit in 0..16 {
        mantissa = (mantissa * mantissa) >> 31;
        if mantissa >= 1u64 << 32 {
            mantissa >>= 1;
            result |= 1u64 << (15 - bit);
        }
    }
    result
}

fn reference_security_ceil_log2(value: u64) -> u64 {
    let floor = reference_security_floor_log2(value);
    if value.is_power_of_two() {
        floor
    } else {
        floor + 1
    }
}

fn push_u64_words(words: &mut Vec<u32>, value: u64) {
    words.push(value as u32);
    words.push((value >> 32) as u32);
}

fn expected_recursive_security_profile_words() -> Vec<u32> {
    let target_bits = 100u32;
    let max_composed_proofs = 1_024u32;
    let extension_degree = 4u32;
    let trace_length = 4_096u32;
    let lde_length = 8_192u32;
    let composition_degree_bound = 4_095u32;
    let log_blowup = 1u32;
    let folding_arity = 2u32;
    let max_air_constraints = 8_192u32;
    let hash_collision_bits = 128u32;
    let query_pow_bits = 0u32;
    let commit_pow_bits = 0u32;
    let field_bits =
        reference_security_floor_log2(BABY_BEAR_MODULUS as u64) * extension_degree as u64;
    let numerator = field_bits + 94_548 + ((log_blowup as u64) << 16);
    let correction =
        reference_security_ceil_log2(numerator) - reference_security_floor_log2(field_bits);
    let bits_per_query = ((log_blowup as u64) << 16) - correction;
    let union_bits = reference_security_ceil_log2(max_composed_proofs as u64);
    let local_target = ((target_bits as u64) << 16) + union_bits;
    let query_count = local_target.div_ceil(bits_per_query) as u32;
    assert_eq!(query_count, 114);
    let query_bits = bits_per_query * query_count as u64;
    let air_bits = field_bits - reference_security_ceil_log2(max_air_constraints as u64);
    let commit_factor = (folding_arity as u64 - 1) * (lde_length as u64 + 1);
    let commit_bits = field_bits - reference_security_ceil_log2(commit_factor);
    let local_attained = query_bits
        .min(air_bits)
        .min(commit_bits)
        .min((hash_collision_bits as u64) << 16);
    let global_attained = local_attained - union_bits;

    let mut words = vec![
        1,
        1,
        target_bits,
        max_composed_proofs,
        BABY_BEAR_MODULUS,
        extension_degree,
        trace_length,
        lde_length,
        composition_degree_bound,
        log_blowup,
        folding_arity,
        max_air_constraints,
        hash_collision_bits,
        query_pow_bits,
        commit_pow_bits,
        query_count,
    ];
    for value in [
        field_bits,
        bits_per_query,
        local_target,
        query_bits,
        air_bits,
        commit_bits,
        local_attained,
        global_attained,
    ] {
        push_u64_words(&mut words, value);
    }
    words.extend([1, 312, 352, 16, 11, 691, 2, 2, 1, 1, 2, query_count]);
    assert_eq!(words.len(), 44);
    words
}

fn reference_digest_compress_with<P>(permutation: &P, left: [u32; 8], right: [u32; 8]) -> [u32; 8]
where
    P: Permutation<[P3BabyBear; POSEIDON_WIDTH]>,
{
    let mut state = [0u32; POSEIDON_WIDTH];
    state[..8].copy_from_slice(&left);
    state[8..].copy_from_slice(&right);
    reference_poseidon_permutation_with(permutation, state)[..8]
        .try_into()
        .unwrap()
}

fn reference_merkle_root_with<P>(permutation: &P, mut leaves: Vec<[u32; 8]>) -> [u32; 8]
where
    P: Permutation<[P3BabyBear; POSEIDON_WIDTH]>,
{
    assert!(!leaves.is_empty());
    assert!(leaves.len().is_power_of_two());
    while leaves.len() > 1 {
        leaves = leaves
            .chunks_exact(2)
            .map(|pair| reference_digest_compress_with(permutation, pair[0], pair[1]))
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

fn expected_sparse_control_plan(control: [u32; 38]) -> [u32; 3] {
    let major = control[32];
    [
        bb_mul(major, bb_sub(major, 1)),
        bb_mul(bb_sub(major, 2), bb_sub(major, 3)),
        bb_mul(bb_sub(major, 4), bb_sub(major, 5)),
    ]
}

fn expected_sparse_control_link_plan(current: [u32; 38], next: [u32; 38]) -> [u32; 68] {
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
        nodes[index] = bb_mul(current[left], next[right]);
    }
    for (index, (left, right)) in [(11, 12), (12, 12), (12, 11), (12, 13), (13, 14), (14, 14)]
        .into_iter()
        .enumerate()
    {
        nodes[23 + index] = bb_mul(current[left], next[right]);
    }
    nodes[29] = bb_mul(current[19], next[19]);
    nodes[30] = bb_mul(current[19], next[23]);
    nodes[31] = bb_mul(current[23], next[24]);
    nodes[32] = bb_mul(current[24], next[25]);
    nodes[33] = bb_mul(current[25], next[23]);
    nodes[34] = bb_mul(current[25], next[26]);
    nodes[35] = bb_mul(current[26], next[27]);
    nodes[36] = bb_mul(current[27], next[28]);
    nodes[37] = bb_mul(current[28], next[29]);
    nodes[38] = bb_mul(current[29], next[26]);
    nodes[39] = bb_mul(nodes[4], nodes[30]);
    nodes[40] = bb_mul(nodes[4], nodes[34]);

    let carry_reset = bb_sub(next[35], current[35]);
    let carry_delta = bb_sub(next[32], current[32]);
    nodes[41] = bb_mul(carry_reset, bb_sub(carry_reset, 1));
    nodes[42] = bb_mul(current[35], bb_sub(1, next[35]));
    nodes[43] = bb_mul(nodes[6], carry_reset);
    nodes[44] = bb_mul(bb_sub(1, carry_reset), bb_add(current[34], 1));
    nodes[45] = bb_mul(bb_sub(1, carry_reset), bb_mul(2, current[37]));
    nodes[46] = bb_mul(carry_delta, bb_sub(carry_delta, 1));
    nodes[47] = bb_mul(bb_sub(1, carry_delta), bb_add(current[33], 1));
    nodes[48] = bb_mul(nodes[8], carry_delta);

    let product_delta = bb_sub(next[32], current[32]);
    let same_product = bb_sub(1, product_delta);
    nodes[49] = bb_mul(product_delta, bb_sub(product_delta, 1));
    nodes[50] = bb_mul(same_product, current[35]);
    nodes[51] = bb_mul(nodes[50], bb_sub(1, next[35]));
    nodes[52] = bb_mul(same_product, bb_sub(next[35], current[35]));
    nodes[53] = bb_mul(nodes[12], nodes[52]);
    nodes[54] = bb_mul(same_product, bb_add(current[33], 1));
    nodes[55] = bb_mul(same_product, next[35]);
    nodes[56] = bb_mul(
        same_product,
        bb_sub(bb_add(current[36], 1), bb_mul(2, next[35])),
    );
    nodes[57] = bb_mul(nodes[12], product_delta);

    let linear_delta = bb_sub(next[33], current[33]);
    let retained = bb_sub(1, linear_delta);
    nodes[58] = bb_mul(linear_delta, bb_sub(linear_delta, 1));
    nodes[59] = bb_mul(retained, bb_add(current[34], 1));
    nodes[60] = bb_mul(nodes[19], linear_delta);
    nodes[61] = bb_mul(retained, current[15]);
    nodes[62] = bb_mul(retained, current[16]);
    nodes[63] = bb_mul(linear_delta, current[15]);
    nodes[64] = bb_mul(retained, current[17]);
    nodes[65] = bb_mul(linear_delta, current[16]);
    nodes[66] = bb_mul(retained, current[18]);
    nodes[67] = bb_mul(linear_delta, current[17]);
    nodes
}

fn expected_sparse_arithmetic_plan(control: [u32; 38], row: [u32; 6]) -> [u32; 23] {
    let two = 2;
    let right_flag_product = bb_mul(row[1], control[35]);
    let effective_right = bb_sub(bb_add(row[1], control[35]), bb_mul(two, right_flag_product));
    let sign_difference_product = bb_mul(row[0], effective_right);
    let signs_different = bb_sub(
        bb_add(row[0], effective_right),
        bb_mul(two, sign_difference_product),
    );
    let signs_same = bb_sub(1, signs_different);
    let select_right = row[2];
    let left_difference_sign = bb_mul(bb_sub(1, select_right), row[0]);
    let right_difference_sign = bb_mul(select_right, effective_right);
    let selected_difference_sign = bb_add(left_difference_sign, right_difference_sign);
    let same_sign_output = bb_mul(signs_same, row[0]);
    let different_sign_output = bb_mul(bb_sub(1, signs_same), selected_difference_sign);
    let selected_sign = bb_add(same_sign_output, different_sign_output);
    let output_sign = bb_mul(selected_sign, row[4]);
    let bit_value = bb_mul(row[0], row[0]);
    let bit_auxiliary = bb_mul(row[1], row[1]);
    let bit_accumulator_before = bb_mul(row[2], row[2]);
    let bit_accumulator_after = bb_mul(row[3], row[3]);
    let bit_state_before = bb_mul(row[4], row[4]);
    let bit_state_after = bb_mul(row[5], row[5]);
    let radix_state_value = bb_mul(row[4], row[0]);
    let radix_value_weight = bb_mul(row[0], control[37]);
    let radix_flag_value = bb_mul(control[35], row[0]);
    let radix_finish_auxiliary = bb_mul(row[1], bb_sub(1, radix_flag_value));
    let product_value_auxiliary = bb_mul(row[0], row[1]);
    let product_sign_product = bb_mul(row[3], row[5]);
    let product_sign_xor = bb_sub(bb_add(row[3], row[5]), bb_mul(two, product_sign_product));
    let product_finish_signed_output = bb_mul(product_sign_xor, row[1]);
    let selected_difference_adjustment = bb_mul(row[5], bb_sub(row[2], row[1]));
    let selected_difference = bb_add(row[1], selected_difference_adjustment);
    let selected_difference_magnitude = bb_mul(bb_sub(1, row[4]), selected_difference);
    let selected_right_magnitude = bb_mul(row[2], row[3]);
    [
        right_flag_product,
        sign_difference_product,
        left_difference_sign,
        right_difference_sign,
        same_sign_output,
        different_sign_output,
        output_sign,
        bit_value,
        bit_auxiliary,
        bit_accumulator_before,
        bit_accumulator_after,
        bit_state_before,
        bit_state_after,
        radix_state_value,
        radix_value_weight,
        radix_flag_value,
        radix_finish_auxiliary,
        product_value_auxiliary,
        product_sign_product,
        product_finish_signed_output,
        selected_difference_adjustment,
        selected_difference_magnitude,
        selected_right_magnitude,
    ]
}

fn expected_sparse_arithmetic_link_plan(
    control: [u32; 38],
    row: [u32; 6],
    next_control: [u32; 38],
) -> [u32; 18] {
    let current_radix = bb_add(control[0], bb_add(control[1], control[2]));
    let next_radix = bb_add(next_control[0], bb_add(next_control[1], next_control[2]));
    let radix_link = bb_mul(current_radix, next_radix);
    let carry_entry = bb_mul(control[2], next_control[3]);
    let current_carry = bb_add(control[3], control[4]);
    let next_carry = bb_add(next_control[3], next_control[4]);
    let carry_link = bb_mul(current_carry, next_carry);
    let carry_bit_pair = bb_mul(control[3], next_control[3]);
    let carry_reset = bb_mul(carry_bit_pair, bb_sub(next_control[35], control[35]));
    let carry_reset_accumulator = bb_mul(carry_reset, row[3]);
    let carry_reset_state_adjustment = bb_mul(carry_reset, bb_sub(row[3], row[5]));
    let product_entry = bb_mul(control[4], next_control[5]);
    let current_product = bb_add(control[5], control[6]);
    let next_product = bb_add(next_control[5], next_control[6]);
    let product_link = bb_mul(current_product, next_product);
    let product_terminal = bb_mul(control[6], next_control[7]);
    let round_start = bb_mul(bb_add(control[6], control[8]), next_control[7]);
    let round_link = bb_mul(control[7], bb_add(next_control[7], next_control[8]));
    let linear_sum_start = bb_mul(bb_add(control[8], control[10]), next_control[15]);
    let linear_left_start = bb_mul(control[15], next_control[16]);
    let linear_right_start = bb_mul(control[16], next_control[17]);
    let linear_sum_link = bb_mul(control[15], next_control[15]);
    let linear_left_link = bb_mul(control[16], next_control[16]);
    let linear_right_link = bb_mul(control[17], next_control[17]);
    [
        radix_link,
        carry_entry,
        carry_link,
        carry_bit_pair,
        carry_reset,
        carry_reset_accumulator,
        carry_reset_state_adjustment,
        product_entry,
        product_link,
        product_terminal,
        round_start,
        round_link,
        linear_sum_start,
        linear_left_start,
        linear_right_start,
        linear_sum_link,
        linear_left_link,
        linear_right_link,
    ]
}

fn expected_sparse_boundary_plan(control: [u32; 38], row: [u32; 6]) -> [u32; 14] {
    let output_node = bb_mul(bb_sub(control[32], 18), bb_inverse(4));
    let even_pair = bb_mul(bb_sub(output_node, 1), bb_sub(output_node, 3));
    let even_product = bb_mul(even_pair, bb_sub(bb_mul(2, output_node), 1));
    let even = bb_sub(0, bb_mul(even_product, bb_inverse(3)));
    let computed = bb_mul(control[29], bb_sub(1, even));
    let source_kind = bb_add(computed, control[22]);
    let digit_source = bb_mul(control[1], source_kind);
    let sign_source = bb_mul(control[2], source_kind);
    let limb_consumer = control[12];
    let sign_consumer = control[11];
    let consumers = bb_add(limb_consumer, sign_consumer);
    let computed_rank = bb_add(22, bb_mul(8, control[32]));
    let claimed_rank = bb_add(4, control[32]);
    [
        even_pair,
        even_product,
        computed,
        digit_source,
        sign_source,
        bb_mul(
            digit_source,
            bb_add(bb_mul(control[32], LIMBS as u32), control[33]),
        ),
        bb_mul(sign_source, bb_add(31 * LIMBS as u32, control[32])),
        bb_mul(
            limb_consumer,
            bb_add(bb_mul(computed_rank, LIMBS as u32), control[33]),
        ),
        bb_mul(sign_consumer, bb_add(31 * LIMBS as u32, computed_rank)),
        bb_mul(sign_source, row[1]),
        bb_mul(bb_add(digit_source, consumers), row[0]),
        bb_mul(
            limb_consumer,
            bb_add(bb_mul(claimed_rank, LIMBS as u32), control[33]),
        ),
        bb_mul(sign_consumer, bb_add(31 * LIMBS as u32, claimed_rank)),
        bb_mul(consumers, row[1]),
    ]
}

fn expected_sparse_round_plan(
    control: [u32; 38],
    next_control: [u32; 38],
    row: [u32; 6],
) -> [u32; 38] {
    let limbs = LIMBS as u32;
    let range_current = bb_mul(control[21], bb_sub(control[32], 2));
    let product_role = bb_add(bb_add(control[23], control[24]), control[25]);
    let range_product = bb_mul(product_role, bb_sub(control[32], 4));
    let range_rank = bb_add(range_current, range_product);
    let range_digit_address = bb_add(bb_mul(range_rank, limbs), control[33]);
    let range_bit_address = bb_add(
        11 * limbs,
        bb_add(
            bb_mul(range_rank, limbs * 13),
            bb_add(bb_mul(control[33], 13), control[34]),
        ),
    );

    let guard_role = bb_mul(control[0], control[23]);
    let guard_position = bb_mul(guard_role, control[30]);
    let guard_source = bb_mul(guard_position, next_control[1]);
    let low_role = bb_mul(control[1], control[23]);
    let low_source = bb_mul(low_role, control[31]);
    let high_role = bb_mul(control[1], control[24]);
    let high_source = bb_mul(high_role, next_control[0]);
    let retained_source = bb_add(low_source, high_source);
    let first_source = bb_add(guard_source, retained_source);
    let output_digit_source = bb_mul(control[1], control[25]);
    let output_finish_source = bb_mul(control[2], control[25]);
    let current_sign_source = bb_mul(control[2], control[21]);
    let round_consumer = control[7];
    let finish_consumer = control[8];

    let product_low_rank = bb_add(2, bb_mul(3, control[32]));
    let product_output_rank = bb_add(product_low_rank, 2);
    let retained_limb = bb_add(bb_sub(limbs, 1), control[33]);
    let retained_address = bb_add(bb_mul(product_low_rank, limbs), retained_limb);
    let output_digit_address = bb_add(bb_mul(product_output_rank, limbs), control[33]);
    let guard_address = bb_add(
        11 * limbs,
        bb_add(
            bb_mul(product_low_rank, limbs * 13),
            bb_add(bb_mul(limbs - 2, 13), 12),
        ),
    );
    let sign_base = 11 * limbs + 11 * limbs * 13;
    let output_sign_address = bb_add(sign_base, product_output_rank);
    let output_nonzero_address = bb_add(output_sign_address, 11);
    let left_rank = bb_mul(control[32], bb_sub(2, control[32]));
    let right_rank_pair = bb_mul(control[32], bb_sub(3, control[32]));
    let right_rank = bb_mul(right_rank_pair, bb_inverse(2));
    let left_sign_address = bb_add(sign_base, left_rank);
    let right_sign_address = bb_add(sign_base, right_rank);
    let range_sign_address = bb_add(sign_base, range_rank);
    let range_nonzero_address = bb_add(range_sign_address, 11);

    [
        range_current,
        range_product,
        guard_role,
        guard_position,
        guard_source,
        low_role,
        low_source,
        high_role,
        high_source,
        output_digit_source,
        output_finish_source,
        current_sign_source,
        left_rank,
        right_rank_pair,
        bb_mul(guard_source, range_bit_address),
        bb_mul(retained_source, range_digit_address),
        bb_mul(round_consumer, retained_address),
        bb_mul(finish_consumer, guard_address),
        bb_mul(bb_add(first_source, round_consumer), row[0]),
        bb_mul(finish_consumer, row[4]),
        bb_mul(output_digit_source, range_digit_address),
        bb_mul(output_finish_source, range_sign_address),
        bb_mul(round_consumer, output_digit_address),
        bb_mul(finish_consumer, output_sign_address),
        bb_mul(output_digit_source, row[0]),
        bb_mul(output_finish_source, row[1]),
        bb_mul(round_consumer, row[1]),
        bb_mul(finish_consumer, row[0]),
        bb_mul(output_finish_source, range_nonzero_address),
        bb_mul(finish_consumer, output_nonzero_address),
        bb_mul(output_finish_source, row[0]),
        bb_mul(finish_consumer, row[1]),
        bb_mul(current_sign_source, range_sign_address),
        bb_mul(finish_consumer, left_sign_address),
        bb_mul(current_sign_source, row[1]),
        bb_mul(finish_consumer, row[3]),
        bb_mul(finish_consumer, right_sign_address),
        bb_mul(finish_consumer, row[5]),
    ]
}

fn expected_sparse_linear_plan(
    control: [u32; 38],
    next_control: [u32; 38],
    row: [u32; 6],
    arithmetic_plan: [u32; 23],
) -> [u32; 52] {
    let limbs = LIMBS as u32;
    let zero = 0;
    let one = 1;
    let digit_address = |rank, limb| bb_add(bb_mul(rank, limbs), limb);
    let sign_address = |rank| bb_add(31 * limbs, rank);
    let nonzero_address = |rank| bb_add(31 * limbs + 31, rank);
    let borrow_address = |node, right| bb_add(31 * limbs + 62, bb_add(bb_mul(2, node), right));
    let same_address = |node| bb_add(31 * limbs + 70, node);
    let select_address = |node| bb_add(31 * limbs + 74, node);

    let point = control[20];
    let product = control[25];
    let product_index = bb_mul(bb_sub(control[32], 8), bb_inverse(3));
    let product_xy_pair = bb_mul(product_index, bb_sub(product_index, one));
    let product_xy = bb_mul(product_xy_pair, bb_inverse(2));
    let intermediate = bb_add(bb_add(control[26], control[27]), control[28]);
    let output_role = control[29];
    let node = control[32];
    let node_even_pair = bb_mul(bb_sub(node, one), bb_sub(node, 3));
    let node_even_product = bb_mul(node_even_pair, bb_sub(bb_mul(2, node), one));
    let node_even = bb_sub(zero, bb_mul(node_even_product, bb_inverse(3)));
    let left_even_rank = bb_mul(node_even, bb_add(8, bb_mul(3, node)));
    let left_odd_rank = bb_mul(bb_sub(one, node_even), bb_add(14, bb_mul(4, node)));
    let left_rank = bb_add(left_even_rank, left_odd_rank);
    let right_even_rank = bb_mul(
        node_even,
        bb_add(11, bb_mul(bb_mul(3, node), bb_inverse(2))),
    );
    let right_odd_rank = bb_mul(
        bb_sub(one, node_even),
        bb_mul(bb_sub(node, one), bb_inverse(2)),
    );
    let right_rank = bb_add(right_even_rank, right_odd_rank);
    let output_node = bb_mul(bb_sub(control[32], 18), bb_inverse(4));
    let output_even_pair = bb_mul(bb_sub(output_node, one), bb_sub(output_node, 3));
    let output_even_product = bb_mul(output_even_pair, bb_sub(bb_mul(2, output_node), one));
    let output_even = bb_sub(zero, bb_mul(output_even_product, bb_inverse(3)));

    let digit_source_kind = bb_add(bb_add(point, product), bb_add(intermediate, output_role));
    let digit_source = bb_mul(control[1], digit_source_kind);
    let product_weight = bb_mul(product, bb_add(one, product_xy));
    let output_digit_weight = bb_mul(output_role, bb_add(one, bb_mul(3, output_even)));
    let digit_source_coefficient = bb_mul(
        control[1],
        bb_add(
            bb_add(bb_mul(3, point), bb_mul(3, product_weight)),
            bb_add(bb_mul(2, intermediate), output_digit_weight),
        ),
    );
    let sign_source_kind = bb_add(bb_add(point, product), output_role);
    let sign_source = bb_mul(control[2], sign_source_kind);
    let output_sign_weight = bb_mul(output_role, bb_add(one, output_even));
    let sign_source_coefficient = bb_mul(
        control[2],
        bb_add(point, bb_add(product_weight, output_sign_weight)),
    );
    let nonzero_source = bb_mul(control[2], output_role);
    let left_borrow_role = bb_mul(control[9], control[16]);
    let left_borrow_source = bb_mul(left_borrow_role, next_control[17]);
    let right_borrow_role = bb_mul(control[9], control[17]);
    let right_borrow_source = bb_mul(right_borrow_role, next_control[18]);
    let limb_nonselect = bb_mul(
        control[9],
        bb_add(bb_add(control[15], control[16]), control[17]),
    );
    let limb_select = bb_mul(control[9], control[18]);
    let finish = control[10];
    let sum_rank = bb_add(15, bb_mul(4, node));
    let left_difference_rank = bb_add(sum_rank, one);
    let right_difference_rank = bb_add(sum_rank, 2);
    let output_rank = bb_add(sum_rank, 3);
    let nonselect_rank = bb_add(sum_rank, bb_add(control[16], bb_mul(2, control[17])));
    let source_digit_address = digit_address(control[32], control[33]);

    let first_source_address = bb_mul(digit_source, source_digit_address);
    let first_nonselect_address = bb_mul(limb_nonselect, digit_address(left_rank, control[34]));
    let first_select_address = bb_mul(limb_select, digit_address(sum_rank, control[34]));
    let first_finish_address = bb_mul(finish, sign_address(left_rank));
    let second_source_address = bb_mul(sign_source, sign_address(control[32]));
    let second_nonselect_address = bb_mul(limb_nonselect, digit_address(right_rank, control[34]));
    let second_select_address = bb_mul(
        limb_select,
        digit_address(left_difference_rank, control[34]),
    );
    let second_finish_address = bb_mul(finish, sign_address(right_rank));
    let third_source_address = bb_mul(nonzero_source, nonzero_address(control[32]));
    let third_nonselect_address =
        bb_mul(limb_nonselect, digit_address(nonselect_rank, control[34]));
    let third_select_address = bb_mul(
        limb_select,
        digit_address(right_difference_rank, control[34]),
    );
    let third_finish_address = bb_mul(finish, borrow_address(node, zero));
    let third_source_value = bb_mul(nonzero_source, row[0]);
    let third_nonselect_value = bb_mul(limb_nonselect, row[4]);
    let third_selected_value = bb_mul(bb_add(limb_select, finish), row[2]);
    let fourth_source_address = bb_mul(left_borrow_source, borrow_address(node, zero));
    let fourth_select_address = bb_mul(limb_select, digit_address(output_rank, control[34]));
    let fourth_finish_address = bb_mul(finish, borrow_address(node, one));
    let fifth_source_address = bb_mul(right_borrow_source, borrow_address(node, one));
    let fifth_select_address = bb_mul(limb_select, same_address(node));
    let fifth_finish_address = bb_mul(finish, nonzero_address(output_rank));
    let fifth_source_value = bb_mul(right_borrow_source, row[3]);
    let fifth_consumer_value = bb_mul(bb_add(limb_select, finish), row[4]);
    let sixth_select_address = bb_mul(limb_select, select_address(node));
    let sixth_finish_address = bb_mul(finish, sign_address(output_rank));
    let effective_right = bb_sub(bb_add(row[1], control[35]), bb_mul(2, arithmetic_plan[0]));
    let different = bb_sub(
        bb_add(row[0], effective_right),
        bb_mul(2, arithmetic_plan[1]),
    );
    let same = bb_sub(one, different);
    let seventh_address = bb_mul(finish, same_address(node));
    let seventh_value = bb_mul(finish, same);
    let eighth_address = bb_mul(finish, select_address(node));
    let eighth_value = bb_mul(finish, row[2]);

    [
        product_xy_pair,
        node_even_pair,
        node_even_product,
        left_even_rank,
        left_odd_rank,
        right_even_rank,
        right_odd_rank,
        output_even_pair,
        output_even_product,
        digit_source,
        product_weight,
        output_digit_weight,
        digit_source_coefficient,
        sign_source,
        output_sign_weight,
        sign_source_coefficient,
        nonzero_source,
        left_borrow_role,
        left_borrow_source,
        right_borrow_role,
        right_borrow_source,
        limb_nonselect,
        limb_select,
        first_source_address,
        first_nonselect_address,
        first_select_address,
        first_finish_address,
        second_source_address,
        second_nonselect_address,
        second_select_address,
        second_finish_address,
        third_source_address,
        third_nonselect_address,
        third_select_address,
        third_finish_address,
        third_source_value,
        third_nonselect_value,
        third_selected_value,
        fourth_source_address,
        fourth_select_address,
        fourth_finish_address,
        fifth_source_address,
        fifth_select_address,
        fifth_finish_address,
        fifth_source_value,
        fifth_consumer_value,
        sixth_select_address,
        sixth_finish_address,
        seventh_address,
        seventh_value,
        eighth_address,
        eighth_value,
    ]
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

fn bb_two_adic_root(log_order: u32) -> u32 {
    assert!(log_order <= 27);
    bb_pow(bb_pow(31, 15), 1 << (27 - log_order))
}

fn direct_baby_bear_ntt(values: &[u32], inverse: bool) -> Vec<u32> {
    assert!(values.len().is_power_of_two());
    let log_n = values.len().ilog2();
    let mut root = bb_two_adic_root(log_n);
    if inverse {
        root = bb_inverse(root);
    }
    let mut output = vec![0; values.len()];
    for (index, slot) in output.iter_mut().enumerate() {
        let point = bb_pow(root, index as u32);
        let mut power = 1;
        let mut sum = 0;
        for value in values {
            sum = bb_add(sum, bb_mul(*value, power));
            power = bb_mul(power, point);
        }
        *slot = sum;
    }
    if inverse {
        let scale = bb_inverse(values.len() as u32);
        for value in &mut output {
            *value = bb_mul(*value, scale);
        }
    }
    output
}

fn direct_baby_bear_coset_lde(values: &[u32], output_len: usize, shift: u32) -> Vec<u32> {
    let coefficients = direct_baby_bear_ntt(values, true);
    let log_n = output_len.ilog2();
    let root = bb_two_adic_root(log_n);
    (0..output_len)
        .map(|index| {
            let point = bb_mul(shift, bb_pow(root, index as u32));
            let mut power = 1;
            let mut sum = 0;
            for coefficient in &coefficients {
                sum = bb_add(sum, bb_mul(*coefficient, power));
                power = bb_mul(power, point);
            }
            sum
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ext4([u32; 4]);

impl Ext4 {
    const ZERO: Self = Self([0; 4]);

    fn from_words(words: &[u32]) -> Self {
        Self(
            words
                .try_into()
                .expect("quartic value must have four coefficients"),
        )
    }

    fn from_base(value: u32) -> Self {
        Self([value, 0, 0, 0])
    }

    fn add(self, other: Self) -> Self {
        Self(std::array::from_fn(|index| {
            bb_add(self.0[index], other.0[index])
        }))
    }

    fn sub(self, other: Self) -> Self {
        Self(std::array::from_fn(|index| {
            bb_sub(self.0[index], other.0[index])
        }))
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
                    bb_add(
                        bb_add(bb_mul(a[1], b[3]), bb_mul(a[2], b[2])),
                        bb_mul(a[3], b[1]),
                    ),
                ),
            ),
            bb_add(
                bb_add(bb_mul(a[0], b[1]), bb_mul(a[1], b[0])),
                bb_mul(nonresidue, bb_add(bb_mul(a[2], b[3]), bb_mul(a[3], b[2]))),
            ),
            bb_add(
                bb_add(
                    bb_add(bb_mul(a[0], b[2]), bb_mul(a[1], b[1])),
                    bb_mul(a[2], b[0]),
                ),
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

    fn pow(self, mut exponent: u32) -> Self {
        let mut base = self;
        let mut result = Self::from_base(1);
        while exponent != 0 {
            if exponent & 1 == 1 {
                result = result.mul(base);
            }
            base = base.mul(base);
            exponent >>= 1;
        }
        result
    }
}

fn expected_sparse_air_composition(point: Ext4, numerators: [Ext4; 4]) -> Ext4 {
    let one = Ext4::from_base(1);
    let last_trace_point = Ext4::from_base(bb_two_adic_root(2)).inverse();
    let trace_zerofier = point.pow(4).sub(one);
    let first_zerofier = point.sub(one);
    let last_zerofier = point.sub(last_trace_point);
    assert_ne!(trace_zerofier, Ext4::ZERO);
    assert_ne!(first_zerofier, Ext4::ZERO);
    assert_ne!(last_zerofier, Ext4::ZERO);
    let trace_inverse = trace_zerofier.inverse();
    numerators[0]
        .mul(trace_inverse)
        .add(numerators[1].mul(last_zerofier).mul(trace_inverse))
        .add(numerators[2].mul(first_zerofier.inverse()))
        .add(numerators[3].mul(last_zerofier.inverse()))
}

fn expected_sparse_public_positions() -> Vec<u32> {
    let tasks = expected_sparse_transition_tasks(LIMBS as u32);
    let mut positions = Vec::with_capacity(6 * (LIMBS + 1));
    for coordinate in 0..6u32 {
        for limb in 0..LIMBS as u32 {
            positions.push(
                tasks
                    .iter()
                    .position(|task| task[0] == 1 && task[1] == coordinate && task[2] == limb)
                    .expect("every public limb must occur in the independent task plan")
                    as u32,
            );
        }
        positions.push(
            tasks
                .iter()
                .position(|task| task[0] == 2 && task[1] == coordinate)
                .expect("every public sign must occur in the independent task plan")
                as u32,
        );
    }
    positions
}

fn expected_sparse_public_composition(
    point: &ComplexFx,
    current: &ComplexFx,
    next: &ComplexFx,
    evaluation_point: Ext4,
    opened_value: Ext4,
    opened_auxiliary: Ext4,
    fold: Ext4,
    mix: Ext4,
) -> Ext4 {
    let public = [
        &point.real,
        &point.imaginary,
        &current.real,
        &current.imaginary,
        &next.real,
        &next.imaginary,
    ];
    let positions = expected_sparse_public_positions();
    let trace_root = bb_two_adic_root(12);
    let mut result = Ext4::ZERO;
    let mut power = Ext4::from_base(1);
    for (coordinate, value) in public.into_iter().enumerate() {
        for (limb, expected) in limbs(value).into_iter().enumerate() {
            let row = positions[coordinate * (LIMBS + 1) + limb];
            let denominator = evaluation_point.sub(Ext4::from_base(bb_pow(trace_root, row)));
            assert_ne!(denominator, Ext4::ZERO, "public limb inverse pole");
            result = result.add(
                power
                    .mul(opened_value.sub(Ext4::from_base(expected)))
                    .mul(denominator.inverse()),
            );
            power = power.mul(fold);
        }
        let sign_row = positions[coordinate * (LIMBS + 1) + LIMBS];
        let denominator = evaluation_point.sub(Ext4::from_base(bb_pow(trace_root, sign_row)));
        assert_ne!(denominator, Ext4::ZERO, "public sign inverse pole");
        result = result.add(
            power
                .mul(opened_auxiliary.sub(Ext4::from_base(value.negative as u32)))
                .mul(denominator.inverse()),
        );
        power = power.mul(fold);
    }
    result.mul(mix)
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

fn expected_product_copy_ports(task: [u32; 5], row: [u32; 6]) -> [Option<ExpectedCopyPort>; 2] {
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

fn expected_sparse_product_address_plan(control: [u32; 38]) -> [u32; 17] {
    let limbs = LIMBS as u32;
    let radix_limb = control[1];
    let carry_finish = control[4];
    let product_term = control[5];
    let product_coefficient = control[6];
    let coordinate = control[19];
    let product_low = control[23];
    let product_high = control[24];
    let major = control[32];
    let minor = control[33];
    let step = control[34];
    let flag = control[35];

    let major01 = bb_mul(major, bb_sub(major, 1));
    let major45 = bb_mul(bb_sub(major, 4), bb_sub(major, 5));
    let current_numerator = bb_mul(major01, major45);
    let current = bb_mul(coordinate, bb_mul(current_numerator, bb_inverse(12)));
    let product_range = bb_add(product_low, product_high);
    let source_range_selector = bb_mul(radix_limb, bb_add(current, product_range));
    let source_range_coefficient = bb_mul(
        radix_limb,
        bb_add(bb_mul(current, 3 * limbs), product_range),
    );

    let product_node = bb_mul(bb_sub(bb_sub(major, 6), product_high), bb_inverse(3));
    let current_rank = bb_sub(major, 2);
    let product_rank = bb_add(2, bb_add(bb_mul(2, product_node), product_high));
    let range_current = bb_mul(current, current_rank);
    let range_product = bb_mul(product_range, product_rank);
    let range_rank = bb_add(range_current, range_product);
    let range_address = bb_add(bb_mul(range_rank, limbs), minor);
    let carry_address = bb_add(
        bb_add(bb_mul(8, limbs), bb_mul(bb_mul(2, limbs), major)),
        minor,
    );
    let source_range_address = bb_mul(source_range_selector, range_address);
    let carry_source_address = bb_mul(carry_finish, carry_address);

    let left_adjustment = bb_mul(flag, bb_sub(minor, limbs - 1));
    let left_index = bb_add(step, left_adjustment);
    let right_index = bb_sub(minor, left_index);
    let left_rank = bb_mul(major, bb_sub(2, major));
    let term_left_address = bb_add(bb_mul(left_rank, limbs), left_index);
    let right_rank_pair = bb_mul(major, bb_sub(3, major));
    let term_right_address = bb_add(
        bb_mul(bb_mul(right_rank_pair, bb_inverse(2)), limbs),
        right_index,
    );
    let coefficient_digit_rank = bb_add(2, bb_add(bb_mul(2, major), flag));
    let coefficient_digit_address = bb_add(
        bb_mul(coefficient_digit_rank, limbs),
        bb_sub(minor, bb_mul(flag, limbs)),
    );

    [
        major01,
        major45,
        current_numerator,
        current,
        source_range_selector,
        source_range_coefficient,
        range_current,
        range_product,
        source_range_address,
        carry_source_address,
        left_adjustment,
        left_rank,
        right_rank_pair,
        bb_mul(product_term, term_left_address),
        bb_mul(product_coefficient, coefficient_digit_address),
        bb_mul(product_term, term_right_address),
        bb_mul(product_coefficient, carry_address),
    ]
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

fn expected_round_copy_ports(task: [u32; 5], row: [u32; 6]) -> [Option<ExpectedCopyPort>; 5] {
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
                value: row[4],
                coefficient: -1,
            });
            ports[1] = Some(ExpectedCopyPort {
                address: round_sign_address(output_rank),
                value: row[0],
                coefficient: -1,
            });
            ports[2] = Some(ExpectedCopyPort {
                address: round_nonzero_address(output_rank),
                value: row[1],
                coefficient: -1,
            });
            ports[3] = Some(ExpectedCopyPort {
                address: round_sign_address(left_rank),
                value: row[3],
                coefficient: -1,
            });
            ports[4] = Some(ExpectedCopyPort {
                address: round_sign_address(right_rank),
                value: row[5],
                coefficient: -1,
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

fn expected_linear_copy_ports(task: [u32; 5], row: [u32; 6]) -> [Option<ExpectedCopyPort>; 8] {
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

fn expected_boundary_copy_ports(task: [u32; 5], row: [u32; 6]) -> [Option<ExpectedCopyPort>; 2] {
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
    let permutation = default_babybear_poseidon2_16();
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
            reference_poseidon_digest_with(&permutation, b"SL03", &fields)
        })
        .collect();
    reference_merkle_root_with(&permutation, leaves)
}

fn expected_interaction_challenges_for_domains(root: [u32; 8], domains: [&[u8; 4]; 8]) -> Vec<u32> {
    let mut words = vec![1];
    for tag in domains {
        words.extend(&reference_poseidon_digest(tag, &root)[..4]);
    }
    words
}

fn expected_sparse_interaction_challenges(root: [u32; 8]) -> Vec<u32> {
    expected_interaction_challenges_for_domains(
        root,
        [
            b"PB01", b"PG01", b"RB01", b"RG01", b"LB01", b"LG01", b"BB01", b"BG01",
        ],
    )
}

fn expected_sparse_lde_interaction_challenges(root: [u32; 8]) -> Vec<u32> {
    expected_interaction_challenges_for_domains(
        root,
        [
            b"PB02", b"PG02", b"RB02", b"RG02", b"LB02", b"LG02", b"BB02", b"BG02",
        ],
    )
}

fn expected_sparse_interaction_challenge_values(root: [u32; 8]) -> [Ext4; 8] {
    let words = expected_sparse_interaction_challenges(root);
    [
        Ext4::from_words(&words[1..5]),
        Ext4::from_words(&words[5..9]),
        Ext4::from_words(&words[9..13]),
        Ext4::from_words(&words[13..17]),
        Ext4::from_words(&words[17..21]),
        Ext4::from_words(&words[21..25]),
        Ext4::from_words(&words[25..29]),
        Ext4::from_words(&words[29..33]),
    ]
}

fn extend_ext4_words(destination: &mut Vec<u32>, value: Ext4) {
    destination.extend(value.0);
}

fn expected_sparse_base_row_words(
    controls: &[[u32; 38]],
    rows: &[[u32; 6]],
    index: usize,
) -> Vec<u32> {
    let padding_control = expected_sparse_control_fields([14, 0, 0, 0, 0]);
    let control = controls.get(index).copied().unwrap_or(padding_control);
    let next_control = controls.get(index + 1).copied().unwrap_or(padding_control);
    let row = rows.get(index).copied().unwrap_or([0; 6]);
    let arithmetic = expected_sparse_arithmetic_plan(control, row);
    let mut words = Vec::with_capacity(SPARSE_BASE_FIELDS);
    words.extend(control);
    words.extend(expected_sparse_control_plan(control));
    words.extend(expected_sparse_control_link_plan(control, next_control));
    words.extend(row);
    words.extend(arithmetic);
    words.extend(expected_sparse_arithmetic_link_plan(
        control,
        row,
        next_control,
    ));
    words.extend(expected_sparse_round_plan(control, next_control, row));
    words.extend(expected_sparse_linear_plan(
        control,
        next_control,
        row,
        arithmetic,
    ));
    words.extend(expected_sparse_boundary_plan(control, row));
    assert_eq!(words.len(), SPARSE_BASE_FIELDS, "sparse base row schema");
    words
}

fn expected_sparse_base_evaluation_words(point: &ComplexFx, current: &ComplexFx) -> Vec<u32> {
    let controls = expected_sparse_control_rows();
    let rows = expected_sparse_rows(point, current);
    let mut base_fields = Vec::with_capacity(4 * SPARSE_BASE_FIELDS);
    for index in 0..4 {
        base_fields.extend(expected_sparse_base_row_words(&controls, &rows, index));
    }
    assert_eq!(
        base_fields.len(),
        4 * SPARSE_BASE_FIELDS,
        "sparse base schema",
    );
    base_fields
}

fn expected_sparse_production_base_trace_words(point: &ComplexFx, current: &ComplexFx) -> Vec<u32> {
    let controls = expected_sparse_control_rows();
    let rows = expected_sparse_rows(point, current);
    assert_eq!(controls.len(), PRODUCTION_TRACE_ROWS);
    assert_eq!(rows.len(), PRODUCTION_TRACE_ROWS);
    let mut columns = vec![0; PRODUCTION_TRACE_ROWS * SPARSE_BASE_FIELDS];
    for row in 0..PRODUCTION_TRACE_ROWS {
        let words = expected_sparse_base_row_words(&controls, &rows, row);
        for (column, word) in words.into_iter().enumerate() {
            columns[column * PRODUCTION_TRACE_ROWS + row] = word;
        }
    }
    columns
}

fn plonky3_coset_lde_column_major(evaluations: &[u32], rows: usize, columns: usize) -> Vec<u32> {
    assert_eq!(evaluations.len(), rows * columns);
    assert!(rows.is_power_of_two());
    let row_major = (0..rows)
        .flat_map(|row| {
            (0..columns).map(move |column| P3BabyBear::from_u32(evaluations[column * rows + row]))
        })
        .collect::<Vec<_>>();
    let extended = Radix2Dit::<P3BabyBear>::default().coset_lde_batch(
        RowMajorMatrix::new(row_major, columns),
        1,
        P3BabyBear::from_u32(7),
    );
    let extended_rows = rows * 2;
    assert_eq!(extended.width, columns);
    assert_eq!(extended.values.len(), extended_rows * columns);
    let mut column_major = vec![0; extended_rows * columns];
    for row in 0..extended_rows {
        for column in 0..columns {
            column_major[column * extended_rows + row] =
                extended.values[row * columns + column].as_canonical_u32();
        }
    }
    column_major
}

fn expected_sparse_production_base_lde_root(lde: &[u32]) -> [u32; 8] {
    let permutation = default_babybear_poseidon2_16();
    let lde_rows = PRODUCTION_TRACE_ROWS * 2;
    assert_eq!(lde.len(), lde_rows * SPARSE_BASE_FIELDS);
    let mut leaves = Vec::with_capacity(lde_rows);
    for row in 0..lde_rows {
        let mut fields = Vec::with_capacity(4 + SPARSE_BASE_FIELDS);
        fields.extend([
            LIMBS as u32,
            PRODUCTION_TRACE_ROWS as u32,
            lde_rows as u32,
            row as u32,
        ]);
        for column in 0..SPARSE_BASE_FIELDS {
            fields.push(lde[column * lde_rows + row]);
        }
        leaves.push(reference_poseidon_digest_with(
            &permutation,
            b"LD01",
            &fields,
        ));
    }
    reference_merkle_root_with(&permutation, leaves)
}

fn expected_sparse_production_interaction_lde_root(
    lde: &[u32],
    base_lde_root: [u32; 8],
) -> [u32; 8] {
    const INTERACTION_FIELDS: usize = 152;
    let permutation = default_babybear_poseidon2_16();
    let lde_rows = PRODUCTION_TRACE_ROWS * 2;
    assert_eq!(lde.len(), lde_rows * INTERACTION_FIELDS);
    let mut leaves = Vec::with_capacity(lde_rows);
    for row in 0..lde_rows {
        let mut fields = Vec::with_capacity(4 + base_lde_root.len() + INTERACTION_FIELDS);
        fields.extend([
            LIMBS as u32,
            PRODUCTION_TRACE_ROWS as u32,
            lde_rows as u32,
            row as u32,
        ]);
        fields.extend(base_lde_root);
        for column in 0..INTERACTION_FIELDS {
            fields.push(lde[column * lde_rows + row]);
        }
        leaves.push(reference_poseidon_digest_with(
            &permutation,
            b"LD02",
            &fields,
        ));
    }
    reference_merkle_root_with(&permutation, leaves)
}

fn expected_sparse_production_interaction_trace_words(
    point: &ComplexFx,
    current: &ComplexFx,
    base_lde_root: [u32; 8],
) -> Vec<u32> {
    const INTERACTION_FIELDS: usize = 152;
    let words = expected_sparse_lde_interaction_challenges(base_lde_root);
    let challenges = [
        Ext4::from_words(&words[1..5]),
        Ext4::from_words(&words[5..9]),
        Ext4::from_words(&words[9..13]),
        Ext4::from_words(&words[13..17]),
        Ext4::from_words(&words[17..21]),
        Ext4::from_words(&words[21..25]),
        Ext4::from_words(&words[25..29]),
        Ext4::from_words(&words[29..33]),
    ];
    let controls = expected_sparse_control_rows();
    let mut tasks = expected_sparse_transition_tasks(LIMBS as u32);
    tasks.resize(PRODUCTION_TRACE_ROWS, [14, 0, 0, 0, 0]);
    let rows = expected_sparse_rows(point, current);
    let mut accumulators = [Ext4::ZERO; 4];
    let mut columns = vec![0; PRODUCTION_TRACE_ROWS * INTERACTION_FIELDS];
    for row_index in 0..PRODUCTION_TRACE_ROWS {
        let task = tasks[row_index];
        let row = rows[row_index];
        let (product_inverses, product_delta) = expected_ext4_interaction_row(
            expected_product_copy_ports(task, row),
            challenges[0],
            challenges[1],
        );
        let (round_inverses, round_delta) = expected_ext4_interaction_row(
            expected_round_copy_ports(task, row),
            challenges[2],
            challenges[3],
        );
        let (linear_inverses, linear_delta) = expected_ext4_interaction_row(
            expected_linear_copy_ports(task, row),
            challenges[4],
            challenges[5],
        );
        let (boundary_inverses, boundary_delta) = expected_ext4_interaction_row(
            expected_boundary_copy_ports(task, row),
            challenges[6],
            challenges[7],
        );

        let mut fields = Vec::with_capacity(INTERACTION_FIELDS);
        extend_ext4_words(&mut fields, accumulators[0]);
        for inverse in product_inverses {
            extend_ext4_words(&mut fields, inverse);
        }
        for node in expected_sparse_product_address_plan(controls[row_index]) {
            extend_ext4_words(&mut fields, Ext4::from_base(node));
        }
        extend_ext4_words(&mut fields, accumulators[1]);
        for inverse in round_inverses {
            extend_ext4_words(&mut fields, inverse);
        }
        extend_ext4_words(&mut fields, accumulators[2]);
        for inverse in linear_inverses {
            extend_ext4_words(&mut fields, inverse);
        }
        extend_ext4_words(&mut fields, accumulators[3]);
        for inverse in boundary_inverses {
            extend_ext4_words(&mut fields, inverse);
        }
        assert_eq!(fields.len(), INTERACTION_FIELDS);
        for (column, word) in fields.into_iter().enumerate() {
            columns[column * PRODUCTION_TRACE_ROWS + row_index] = word;
        }

        accumulators[0] = accumulators[0].add(product_delta);
        accumulators[1] = accumulators[1].add(round_delta);
        accumulators[2] = accumulators[2].add(linear_delta);
        accumulators[3] = accumulators[3].add(boundary_delta);
    }
    assert_eq!(accumulators, [Ext4::ZERO; 4]);
    columns
}

fn expected_sparse_air_prefix_words(
    point: &ComplexFx,
    current: &ComplexFx,
    base_root: [u32; 8],
) -> Vec<u32> {
    let controls = expected_sparse_control_rows();
    let rows = expected_sparse_rows(point, current);
    let tasks = expected_sparse_transition_tasks(LIMBS as u32);
    let [
        product_beta,
        product_gamma,
        round_beta,
        round_gamma,
        linear_beta,
        linear_gamma,
        boundary_beta,
        boundary_gamma,
    ] = expected_sparse_interaction_challenge_values(base_root);

    let mut words = vec![1, 1];
    words.extend(base_root);
    words.extend(expected_sparse_base_evaluation_words(point, current));

    let mut product_accumulator = Ext4::ZERO;
    let mut round_accumulator = Ext4::ZERO;
    let mut linear_accumulator = Ext4::ZERO;
    let mut boundary_accumulator = Ext4::ZERO;
    for index in 0..4 {
        let task = tasks[index];
        let row = rows[index];
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

        extend_ext4_words(&mut words, product_accumulator);
        for inverse in product_inverses {
            extend_ext4_words(&mut words, inverse);
        }
        for node in expected_sparse_product_address_plan(controls[index]) {
            extend_ext4_words(&mut words, Ext4::from_base(node));
        }
        extend_ext4_words(&mut words, round_accumulator);
        for inverse in round_inverses {
            extend_ext4_words(&mut words, inverse);
        }
        extend_ext4_words(&mut words, linear_accumulator);
        for inverse in linear_inverses {
            extend_ext4_words(&mut words, inverse);
        }
        extend_ext4_words(&mut words, boundary_accumulator);
        for inverse in boundary_inverses {
            extend_ext4_words(&mut words, inverse);
        }

        product_accumulator = product_accumulator.add(product_delta);
        round_accumulator = round_accumulator.add(round_delta);
        linear_accumulator = linear_accumulator.add(linear_delta);
        boundary_accumulator = boundary_accumulator.add(boundary_delta);
    }
    for accumulator in [
        product_accumulator,
        round_accumulator,
        linear_accumulator,
        boundary_accumulator,
    ] {
        extend_ext4_words(&mut words, accumulator);
    }
    assert_eq!(words.len(), 1_674, "sparse prefix schema must stay nominal");
    words
}

fn expected_sparse_base_lde_words(point: &ComplexFx, current: &ComplexFx) -> Vec<u32> {
    const BASE_FIELDS: usize = 260;
    const TRACE: usize = 4;
    const LDE: usize = 16;
    let evaluations = expected_sparse_base_evaluation_words(point, current);
    let mut words = vec![1];
    words.resize(1 + LDE * BASE_FIELDS, 0);
    for column in 0..BASE_FIELDS {
        let source = (0..TRACE)
            .map(|row| evaluations[row * BASE_FIELDS + column])
            .collect::<Vec<_>>();
        let extended = direct_baby_bear_coset_lde(&source, LDE, 7);
        for row in 0..LDE {
            words[1 + row * BASE_FIELDS + column] = extended[row];
        }
    }
    words
}

fn expected_sparse_air_lde_words(prefix: &[u32]) -> Vec<u32> {
    assert_eq!(prefix.len(), 1_674, "sparse prefix input must stay nominal");
    const BASE_FIELDS: usize = 260;
    const INTERACTION_FIELDS: usize = 152;
    const TRACE: usize = 4;
    const LDE: usize = 16;
    const BASE_START: usize = 10;
    const INTERACTION_START: usize = BASE_START + TRACE * BASE_FIELDS;

    let mut base = vec![0; LDE * BASE_FIELDS];
    for column in 0..BASE_FIELDS {
        let source = (0..TRACE)
            .map(|row| prefix[BASE_START + row * BASE_FIELDS + column])
            .collect::<Vec<_>>();
        let extended = direct_baby_bear_coset_lde(&source, LDE, 7);
        for row in 0..LDE {
            base[row * BASE_FIELDS + column] = extended[row];
        }
    }

    let mut interaction = vec![0; LDE * INTERACTION_FIELDS];
    for column in 0..INTERACTION_FIELDS {
        let source = (0..TRACE)
            .map(|row| prefix[INTERACTION_START + row * INTERACTION_FIELDS + column])
            .collect::<Vec<_>>();
        let extended = direct_baby_bear_coset_lde(&source, LDE, 7);
        for row in 0..LDE {
            interaction[row * INTERACTION_FIELDS + column] = extended[row];
        }
    }

    let mut words = vec![1];
    words.extend(base);
    words.extend(interaction);
    assert_eq!(words.len(), 6_593, "sparse LDE schema must stay nominal");
    words
}

fn expected_sparse_base_lde_root(lde: &[u32], mutate: bool) -> [u32; 8] {
    let permutation = default_babybear_poseidon2_16();
    const BASE_FIELDS: usize = 260;
    const LDE: usize = 16;
    assert!(lde.len() >= 1 + LDE * BASE_FIELDS);
    assert_eq!(lde[0], 1, "base LDE must be valid before commitment");
    let mut leaves = Vec::with_capacity(LDE);
    for index in 0..LDE {
        let mut fields = vec![LIMBS as u32, 4, LDE as u32, index as u32];
        let start = 1 + index * BASE_FIELDS;
        let mut row = lde[start..start + BASE_FIELDS].to_vec();
        if mutate && index == 0 {
            row[35] = bb_add(row[35], 1);
        }
        fields.extend(row);
        leaves.push(reference_poseidon_digest_with(
            &permutation,
            b"LD01",
            &fields,
        ));
    }
    reference_merkle_root_with(&permutation, leaves)
}

fn expected_sparse_production_interaction_prefix(
    point: &ComplexFx,
    current: &ComplexFx,
    base_lde_root: [u32; 8],
) -> Vec<u32> {
    let words = expected_sparse_lde_interaction_challenges(base_lde_root);
    let challenges = [
        Ext4::from_words(&words[1..5]),
        Ext4::from_words(&words[5..9]),
        Ext4::from_words(&words[9..13]),
        Ext4::from_words(&words[13..17]),
        Ext4::from_words(&words[17..21]),
        Ext4::from_words(&words[21..25]),
        Ext4::from_words(&words[25..29]),
        Ext4::from_words(&words[29..33]),
    ];
    let controls = expected_sparse_control_rows();
    let tasks = expected_sparse_transition_tasks(LIMBS as u32);
    let rows = expected_sparse_rows(point, current);
    let mut product_accumulator = Ext4::ZERO;
    let mut round_accumulator = Ext4::ZERO;
    let mut linear_accumulator = Ext4::ZERO;
    let mut boundary_accumulator = Ext4::ZERO;
    let mut fields = Vec::with_capacity(4 * 152);
    for index in 0..4 {
        let task = tasks[index];
        let row = rows[index];
        let (product_inverses, product_delta) = expected_ext4_interaction_row(
            expected_product_copy_ports(task, row),
            challenges[0],
            challenges[1],
        );
        let (round_inverses, round_delta) = expected_ext4_interaction_row(
            expected_round_copy_ports(task, row),
            challenges[2],
            challenges[3],
        );
        let (linear_inverses, linear_delta) = expected_ext4_interaction_row(
            expected_linear_copy_ports(task, row),
            challenges[4],
            challenges[5],
        );
        let (boundary_inverses, boundary_delta) = expected_ext4_interaction_row(
            expected_boundary_copy_ports(task, row),
            challenges[6],
            challenges[7],
        );

        extend_ext4_words(&mut fields, product_accumulator);
        for inverse in product_inverses {
            extend_ext4_words(&mut fields, inverse);
        }
        for node in expected_sparse_product_address_plan(controls[index]) {
            extend_ext4_words(&mut fields, Ext4::from_base(node));
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

        product_accumulator = product_accumulator.add(product_delta);
        round_accumulator = round_accumulator.add(round_delta);
        linear_accumulator = linear_accumulator.add(linear_delta);
        boundary_accumulator = boundary_accumulator.add(boundary_delta);
    }
    assert_eq!(fields.len(), 4 * 152);
    fields
}

fn expected_sparse_interaction_lde(prefix: &[u32]) -> Vec<u32> {
    const FIELDS: usize = 152;
    const TRACE: usize = 4;
    const LDE: usize = 16;
    assert_eq!(prefix.len(), TRACE * FIELDS);
    let mut output = vec![0; LDE * FIELDS];
    for column in 0..FIELDS {
        let source = (0..TRACE)
            .map(|row| prefix[row * FIELDS + column])
            .collect::<Vec<_>>();
        let extended = direct_baby_bear_coset_lde(&source, LDE, 7);
        for row in 0..LDE {
            output[row * FIELDS + column] = extended[row];
        }
    }
    output
}

fn expected_sparse_interaction_lde_root(
    lde: &[u32],
    base_lde_root: [u32; 8],
    mutate: bool,
) -> [u32; 8] {
    let permutation = default_babybear_poseidon2_16();
    const FIELDS: usize = 152;
    const LDE: usize = 16;
    assert_eq!(lde.len(), LDE * FIELDS);
    let mut leaves = Vec::with_capacity(LDE);
    for index in 0..LDE {
        let mut fields = vec![LIMBS as u32, 4, LDE as u32, index as u32];
        fields.extend(base_lde_root);
        let start = index * FIELDS;
        let mut row = lde[start..start + FIELDS].to_vec();
        if mutate && index == 0 {
            row[0] = bb_add(row[0], 1);
        }
        fields.extend(row);
        leaves.push(reference_poseidon_digest_with(
            &permutation,
            b"LD02",
            &fields,
        ));
    }
    reference_merkle_root_with(&permutation, leaves)
}

fn reference_sparse_multipath_sibling_count(width: u32, requests: &[u32]) -> usize {
    let mut indices = requests.to_vec();
    indices.sort_unstable();
    indices.dedup();
    assert!(!indices.is_empty());
    assert!(indices.iter().all(|&index| index < width));
    let mut siblings = 0usize;
    let mut width = width;
    while width > 1 {
        let mut next = Vec::new();
        let mut cursor = 0usize;
        while cursor < indices.len() {
            let index = indices[cursor];
            let paired =
                index & 1 == 0 && cursor + 1 < indices.len() && indices[cursor + 1] == index + 1;
            cursor += if paired { 2 } else { 1 };
            if !paired {
                siblings += 1;
            }
            next.push(index / 2);
        }
        indices = next;
        width /= 2;
    }
    siblings
}

fn pack_sparse_multipath_indices(indices: &[u32]) -> u32 {
    indices
        .iter()
        .copied()
        .chain(std::iter::repeat(0))
        .take(8)
        .enumerate()
        .fold(0, |packed, (index, value)| {
            packed | ((value & 15) << (index * 4))
        })
}

fn reference_digest_bind(tag: &[u8; 4], left: [u32; 8], right: [u32; 8]) -> [u32; 8] {
    let mut fields = Vec::with_capacity(16);
    fields.extend(left);
    fields.extend(right);
    reference_poseidon_digest(tag, &fields)
}

fn expected_sparse_air_lde_transcript(
    statement_words: &[u32],
    base: [u32; 8],
    interaction: [u32; 8],
) -> [u32; 8] {
    let statement = reference_poseidon_digest(b"AS01", statement_words);
    let roots = reference_digest_bind(b"AT01", base, interaction);
    reference_digest_bind(b"AT02", statement, roots)
}

fn expected_sparse_composition_root(values: &[Ext4]) -> [u32; 8] {
    let permutation = default_babybear_poseidon2_16();
    let leaves = values
        .iter()
        .map(|value| reference_poseidon_digest_with(&permutation, b"BC02", &value.0))
        .collect();
    reference_merkle_root_with(&permutation, leaves)
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

fn compile_fixture_entry(entry: &str) -> Vec<u8> {
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
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, entry)
        .unwrap_or_else(|error| panic!("recursive fixed entry {entry}: {error}"));
    let bytes =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .unwrap_or_else(|error| panic!("recursive fixed entry {entry} Wasm: {error}"))
            .bytes;
    wasmparser::validate(&bytes).expect("recursive fixed entry Wasm should validate");
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

fn compile_sparse_auth_fixture_entry(entry: &str) -> Vec<u8> {
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
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, entry)
        .unwrap_or_else(|error| panic!("sparse trace entry {entry}: {error}"));
    let bytes =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .unwrap_or_else(|error| panic!("sparse trace entry {entry} Wasm: {error}"))
            .bytes;
    wasmparser::validate(&bytes).expect("sparse trace entry Wasm should validate");
    bytes
}

fn compile_sparse_public_binding_fixture() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_sparse_public_binding_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "sparse public binding fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("sparse public binding fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected sparse public binding diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O2)
        .expect("sparse public binding fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit sparse public binding bytes");
    wasmparser::validate(&bytes).expect("sparse public binding Wasm should validate");
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

fn read_u32le_file(path: &Path) -> Vec<u32> {
    let bytes = std::fs::read(path)
        .unwrap_or_else(|error| panic!("read browser receipt {}: {error}", path.display()));
    assert_eq!(
        bytes.len() % 4,
        0,
        "browser receipt {} is not a u32 tape",
        path.display(),
    );
    bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("one little-endian u32")))
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

fn reset_canonical_arena(store: &mut wasmtime::Store<()>, instance: &wasmtime::Instance) {
    instance
        .get_typed_func::<(), ()>(&mut *store, "fe_cabi_reset")
        .expect("sparse proof fixture should export its canonical arena reset")
        .call(&mut *store, ())
        .expect("canonical arena reset should execute");
}

/// Read and copy one canonical return carrier before the next arena reset.
fn encoded_resetting(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    memory: wasmtime::Memory,
    function: &str,
    arguments: &[u32],
) -> (u32, u32, Vec<u32>) {
    reset_canonical_arena(store, instance);
    encoded(store, instance, memory, function, arguments)
}

#[test]
fn production_arithmetic_plan_matches_independent_nodes_and_rejects_mutations() {
    let entry = "fixed_transition4_sparse_arithmetic_plan_audit";
    let bytes = compile_fixture_entry(entry);
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes)
        .expect("focused arithmetic-plan Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "fixture must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("focused arithmetic-plan fixture should instantiate");

    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let tasks = expected_sparse_transition_tasks(LIMBS as u32);
    let index = tasks
        .iter()
        .position(|task| task[0] == 10)
        .expect("the production schedule must contain a signed-linear finish row");
    let controls = expected_sparse_control_rows();
    let rows = expected_sparse_rows(&point, &current);
    let expected = expected_sparse_arithmetic_plan(controls[index], rows[index]);

    for challenge in [3u32, 7, 31] {
        let mut arguments = transition_arguments(&point, &current);
        arguments.extend([index as u32, challenge, 0]);
        let baseline = call(&mut store, &instance, entry, &arguments, 25);
        assert_eq!(&baseline[..23], expected.as_slice());
        assert_eq!(
            baseline[23], 0,
            "the clean arithmetic plan must satisfy the AIR"
        );
        assert!(
            baseline[24] > 0,
            "the arithmetic plan must emit constraints"
        );

        for mutation in 1..=23u32 {
            let mut mutated_arguments = transition_arguments(&point, &current);
            mutated_arguments.extend([index as u32, challenge, mutation]);
            let mutated = call(&mut store, &instance, entry, &mutated_arguments, 25);
            assert_eq!(&mutated[..23], expected.as_slice());
            assert_eq!(mutated[23], 1, "plan node {mutation} must fail closed");
            assert_eq!(mutated[24], baseline[24]);
        }
    }
}

#[test]
fn production_arithmetic_link_plan_matches_independent_nodes_and_rejects_mutations() {
    let entry = "fixed_transition4_sparse_arithmetic_link_plan_audit";
    let bytes = compile_fixture_entry(entry);
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes)
        .expect("focused arithmetic-link-plan Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "fixture must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("focused arithmetic-link-plan fixture should instantiate");

    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let controls = expected_sparse_control_rows();
    let rows = expected_sparse_rows(&point, &current);
    let mut node_indices = [usize::MAX; 18];
    let mut semantic_indices = Vec::new();
    for index in 0..controls.len() - 1 {
        let expected =
            expected_sparse_arithmetic_link_plan(controls[index], rows[index], controls[index + 1]);
        let mut adds_index = false;
        for (node, value) in expected.into_iter().enumerate() {
            if value != 0 && node_indices[node] == usize::MAX {
                node_indices[node] = index;
                adds_index = true;
            }
        }
        if adds_index {
            semantic_indices.push(index);
        }
    }
    assert!(
        node_indices.iter().all(|index| *index != usize::MAX),
        "every arithmetic-link node must be exercised by the production trace",
    );

    for index in semantic_indices {
        let expected =
            expected_sparse_arithmetic_link_plan(controls[index], rows[index], controls[index + 1]);
        for challenge in [3u32, 7, 31, 127, 257] {
            let mut arguments = transition_arguments(&point, &current);
            arguments.extend([index as u32, challenge, 0]);
            let baseline = call(&mut store, &instance, entry, &arguments, 20);
            assert_eq!(&baseline[..18], expected.as_slice());
            assert_eq!(
                baseline[18], 0,
                "the clean arithmetic link must satisfy the AIR"
            );
            assert_eq!(
                baseline[19], 38,
                "twenty link equations plus eighteen plan nodes"
            );
        }
    }

    for (node, index) in node_indices.into_iter().enumerate() {
        let expected =
            expected_sparse_arithmetic_link_plan(controls[index], rows[index], controls[index + 1]);
        let mut rejected = false;
        for challenge in [3u32, 7, 31, 127, 257] {
            let mut arguments = transition_arguments(&point, &current);
            arguments.extend([index as u32, challenge, node as u32 + 1]);
            let mutated = call(&mut store, &instance, entry, &arguments, 20);
            assert_eq!(&mutated[..18], expected.as_slice());
            rejected |= mutated[18] == 1;
            assert_eq!(mutated[19], 38);
        }
        assert!(
            rejected,
            "arithmetic-link plan node {} must fail under the independent challenge set",
            node + 1,
        );
    }
}

#[test]
fn production_linear_plan_matches_independent_nodes_and_rejects_mutations() {
    let entry = "fixed_transition4_sparse_linear_plan_audit";
    let bytes = compile_fixture_entry(entry);
    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, bytes).expect("focused linear-plan Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "fixture must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("focused linear-plan fixture should instantiate");

    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let tasks = expected_sparse_transition_tasks(LIMBS as u32);
    let mut semantic_indices = Vec::new();
    for range in [0u32, 8, 15, 22] {
        semantic_indices.push(
            tasks
                .iter()
                .position(|task| task[0] == 1 && task[1] == range)
                .expect("the schedule must contain each signed-linear radix source"),
        );
    }
    for role in 0u32..4 {
        semantic_indices.push(
            tasks
                .iter()
                .position(|task| task[0] == 9 && task[1] == 0 && task[2] == role)
                .expect("the schedule must contain each signed-linear limb role"),
        );
    }
    semantic_indices.push(
        tasks
            .iter()
            .position(|task| task[0] == 10 && task[1] == 0)
            .expect("the schedule must contain a signed-linear finish row"),
    );
    let controls = expected_sparse_control_rows();
    let rows = expected_sparse_rows(&point, &current);

    for index in semantic_indices {
        let arithmetic = expected_sparse_arithmetic_plan(controls[index], rows[index]);
        let expected = expected_sparse_linear_plan(
            controls[index],
            controls[index + 1],
            rows[index],
            arithmetic,
        );
        for challenge in [3u32, 7, 31, 127, 257] {
            let mut arguments = transition_arguments(&point, &current);
            arguments.extend([index as u32, challenge, 0]);
            let baseline = call(&mut store, &instance, entry, &arguments, 54);
            assert_eq!(&baseline[..52], expected.as_slice());
            assert_eq!(
                baseline[52], 0,
                "the clean linear plan must satisfy the AIR"
            );
            assert_eq!(
                baseline[53], 68,
                "sixteen port constraints plus fifty-two plan nodes"
            );
        }

        for mutation in 1..=52u32 {
            let mut rejected = false;
            for challenge in [3u32, 7, 31, 127, 257] {
                let mut arguments = transition_arguments(&point, &current);
                arguments.extend([index as u32, challenge, mutation]);
                let mutated = call(&mut store, &instance, entry, &arguments, 54);
                assert_eq!(&mutated[..52], expected.as_slice());
                rejected |= mutated[52] == 1;
                assert_eq!(mutated[53], 68);
            }
            assert!(
                rejected,
                "linear plan node {mutation} must fail under the independent challenge set"
            );
        }
    }
}

#[test]
fn production_padding_terminal_is_linear_and_rejects_each_bus_imbalance() {
    let entry = "fixed_transition4_sparse_padding_terminal_audit";
    let bytes = compile_fixture_entry(entry);
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes)
        .expect("focused padding-terminal Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "fixture must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("focused padding-terminal fixture should instantiate");
    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };

    for challenge in [3u32, 7, 31, 127] {
        let mut clean = transition_arguments(&point, &current);
        clean.extend([17, 29, challenge, 0]);
        assert_eq!(
            call(&mut store, &instance, entry, &clean, 4),
            [0, 0, 166, 0],
            "the canonical padding row must have zero local deltas and a balanced terminal",
        );
        for mutation in 1..=4u32 {
            let mut mutated = transition_arguments(&point, &current);
            mutated.extend([17, 29, challenge, mutation]);
            assert_eq!(
                call(&mut store, &instance, entry, &mutated, 4),
                [0, 1, 166, 0],
                "terminal bus mutation {mutation} must fail exactly one linear balance",
            );
        }
    }
}

#[test]
fn production_round_plan_matches_independent_nodes_and_rejects_mutations() {
    let entry = "fixed_transition4_sparse_round_plan_audit";
    let bytes = compile_fixture_entry(entry);
    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, bytes).expect("focused round-plan Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "fixture must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("focused round-plan fixture should instantiate");

    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let tasks = expected_sparse_transition_tasks(LIMBS as u32);
    let semantic_indices = [
        tasks
            .iter()
            .position(|task| {
                task[0] == 0 && task[1] == 6 && task[2] + 2 == LIMBS as u32 && task[3] == 12
            })
            .expect("the schedule must contain a rounding guard source"),
        tasks
            .iter()
            .position(|task| task[0] == 1 && task[1] == 8)
            .expect("the schedule must contain a rounded output digit source"),
        tasks
            .iter()
            .position(|task| task[0] == 2 && task[1] == 2)
            .expect("the schedule must contain a current-sign source"),
        tasks
            .iter()
            .position(|task| task[0] == 7)
            .expect("the schedule must contain a round consumer"),
        tasks
            .iter()
            .position(|task| task[0] == 8)
            .expect("the schedule must contain a round finish consumer"),
    ];
    let controls = expected_sparse_control_rows();
    let rows = expected_sparse_rows(&point, &current);

    for index in semantic_indices {
        let expected =
            expected_sparse_round_plan(controls[index], controls[index + 1], rows[index]);
        for challenge in [3u32, 7, 31, 127, 257] {
            let mut arguments = transition_arguments(&point, &current);
            arguments.extend([index as u32, challenge, 0]);
            let baseline = call(&mut store, &instance, entry, &arguments, 40);
            assert_eq!(&baseline[..38], expected.as_slice());
            assert_eq!(baseline[38], 0, "the clean round plan must satisfy the AIR");
            assert_eq!(
                baseline[39], 48,
                "ten port constraints plus thirty-eight plan nodes"
            );
        }

        for mutation in 1..=38u32 {
            let mut rejected = false;
            for challenge in [3u32, 7, 31, 127, 257] {
                let mut arguments = transition_arguments(&point, &current);
                arguments.extend([index as u32, challenge, mutation]);
                let mutated = call(&mut store, &instance, entry, &arguments, 40);
                assert_eq!(&mutated[..38], expected.as_slice());
                rejected |= mutated[38] == 1;
                assert_eq!(mutated[39], 48);
            }
            assert!(
                rejected,
                "round plan node {mutation} must fail under the independent challenge set"
            );
        }
    }
}

#[test]
fn production_boundary_plan_matches_independent_nodes_and_rejects_mutations() {
    let entry = "fixed_transition4_sparse_boundary_plan_audit";
    let bytes = compile_fixture_entry(entry);
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes)
        .expect("focused boundary-plan Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "fixture must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("focused boundary-plan fixture should instantiate");

    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let tasks = expected_sparse_transition_tasks(LIMBS as u32);
    let source_index = tasks
        .iter()
        .position(|task| task[0] == 1 && task[1] == 22)
        .expect("the production schedule must contain a computed boundary source");
    let consumer_index = tasks
        .iter()
        .position(|task| task[0] == 12)
        .expect("the production schedule must contain a boundary consumer");
    let controls = expected_sparse_control_rows();
    let rows = expected_sparse_rows(&point, &current);

    for index in [source_index, consumer_index] {
        let expected = expected_sparse_boundary_plan(controls[index], rows[index]);
        for challenge in [3u32, 7, 31] {
            let mut arguments = transition_arguments(&point, &current);
            arguments.extend([index as u32, challenge, 0]);
            let baseline = call(&mut store, &instance, entry, &arguments, 16);
            assert_eq!(&baseline[..14], expected.as_slice());
            assert_eq!(
                baseline[14], 0,
                "the clean boundary plan must satisfy the AIR"
            );
            assert_eq!(
                baseline[15], 18,
                "four port constraints plus fourteen plan nodes"
            );
        }

        for mutation in 1..=14u32 {
            let mut rejected = false;
            for challenge in [3u32, 7, 31] {
                let mut mutated_arguments = transition_arguments(&point, &current);
                mutated_arguments.extend([index as u32, challenge, mutation]);
                let mutated = call(&mut store, &instance, entry, &mutated_arguments, 16);
                assert_eq!(&mutated[..14], expected.as_slice());
                rejected |= mutated[14] == 1;
                assert_eq!(mutated[15], 18);
            }
            assert!(
                rejected,
                "plan node {mutation} must fail under the independent challenge set"
            );
        }
    }
}

#[test]
fn recursive_committed_chunk_preserves_certified_boundaries() {
    let entry = "recursive_committed_leaf4_encoded";
    let bytes = compile_fixture_entry(entry);
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes)
        .expect("focused recursive committed-chunk Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "focused recursive committed-chunk fixture must remain zero-import",
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("focused recursive committed-chunk fixture should instantiate");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("focused recursive committed-chunk fixture should export linear memory");

    let claim = Claim {
        point: ComplexFx {
            real: zero(),
            imaginary: zero(),
        },
        bound: 32,
    };
    let steps = 9;
    let end = evaluate(&claim, steps);
    let start = Boundary {
        iteration: 0,
        z: ComplexFx {
            real: zero(),
            imaginary: zero(),
        },
        escaped: false,
    };
    let mut arguments = claim_args(&claim);
    arguments.push(steps);
    let (_, _, committed) = encoded(&mut store, &instance, memory, entry, &arguments);
    assert_eq!(
        committed,
        expected_committed_words(&claim, &start, &end, 1),
        "one recursive leaf must commit the complete certified chunk",
    );
}

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
                (first_sparse_kind(10), 7),
                (first_sparse_kind(10), 8),
                (first_sparse_kind(10), 9),
                (first_sparse_kind(10), 10),
                (first_sparse_kind(10), 11),
                (first_sparse_kind(10), 12),
                (first_sparse_kind(10), 13),
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
                    [0, 581_596, 4096],
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
                    assert_eq!(audit[1], 581_596);
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
                        90_113,
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
                        200_705,
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
                        282_625,
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
                        77_825,
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
                assert_eq!(audit[1], 90_113);
                assert_eq!(audit[2], 4_096);
            }
            for challenge in [3u32, 7, 31] {
                let mut baseline_arguments = arguments.clone();
                baseline_arguments.extend([challenge, u32::MAX]);
                assert_eq!(
                    call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_product_plan_node_audit",
                        &baseline_arguments,
                        3,
                    ),
                    [0, 21, 0],
                    "clean production product-address plan, challenge {challenge}",
                );
                for node in 0u32..17 {
                    let mut node_arguments = arguments.clone();
                    node_arguments.extend([challenge, node]);
                    let audit = call(
                        &mut store,
                        &instance,
                        "fixed_transition4_sparse_product_plan_node_audit",
                        &node_arguments,
                        3,
                    );
                    assert_eq!(audit[0], 1, "product plan node {node} must fail");
                    assert_eq!(audit[1], 21);
                    assert_ne!(
                        audit[2], 0,
                        "product plan node {node}, challenge {challenge}",
                    );
                }
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
                assert_eq!(audit[1], 200_705);
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
                assert_eq!(audit[1], 282_625);
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
                assert_eq!(audit[1], 77_825);
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
fn production_copy_buses_match_independent_port_oracles() {
    let bytes = compile_fixture();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes).expect("Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "fixture must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("zero-import oracle fixture should instantiate");
    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let arguments = transition_arguments(&point, &current);
    let expected_rows = expected_sparse_product_rows(&current);

    for (index, expected) in expected_rows.iter().enumerate() {
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

    let expected_constraints = 12 * LIMBS as u32 * LIMBS as u32 + 30 * LIMBS as u32 + 3;
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
            [0, expected_constraints, expected_rows.len() as u32],
            "clean sparse product and copy bus, challenge {challenge}",
        );
    }

    let (beta, gamma, fold_challenge, receipt_challenge) = (17u32, 29u32, 7u32, 31u32);
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
            90_113,
            4_096,
            expected_product_interaction_receipt(&point, &current, beta, gamma, receipt_challenge,),
        ],
        "product interaction must match the independent port oracle",
    );
    for mutation in [3u32, 6] {
        let mut mutated_arguments = arguments.clone();
        mutated_arguments.extend([beta, gamma, fold_challenge, receipt_challenge, mutation]);
        let audit = call(
            &mut store,
            &instance,
            "fixed_transition4_sparse_product_interaction_audit",
            &mutated_arguments,
            4,
        );
        if mutation == 6 {
            assert_eq!(
                audit[0], 1,
                "the coordinated mutation must fail only at the terminal",
            );
        } else {
            assert!(audit[0] > 0, "product interaction mutation {mutation}");
        }
        assert_eq!(audit[1], 90_113);
        assert_eq!(audit[2], 4_096);
    }

    for (label, function, constraints, expected_receipt) in [
        (
            "round",
            "fixed_transition4_sparse_round_interaction_audit",
            200_705u32,
            expected_round_interaction_receipt(&point, &current, beta, gamma, receipt_challenge),
        ),
        (
            "linear",
            "fixed_transition4_sparse_linear_interaction_audit",
            282_625,
            expected_linear_interaction_receipt(&point, &current, beta, gamma, receipt_challenge),
        ),
        (
            "boundary",
            "fixed_transition4_sparse_boundary_interaction_audit",
            77_825,
            expected_boundary_interaction_receipt(&point, &current, beta, gamma, receipt_challenge),
        ),
    ] {
        let mut baseline_arguments = arguments.clone();
        baseline_arguments.extend([beta, gamma, fold_challenge, receipt_challenge, 0]);
        assert_eq!(
            call(&mut store, &instance, function, &baseline_arguments, 4,),
            [0, constraints, 4_096, expected_receipt],
            "{label} interaction must match the independent port oracle",
        );
        for mutation in [3u32, 6] {
            let mut mutated_arguments = arguments.clone();
            mutated_arguments.extend([beta, gamma, fold_challenge, receipt_challenge, mutation]);
            let audit = call(&mut store, &instance, function, &mutated_arguments, 4);
            if mutation == 6 {
                assert_eq!(
                    audit[0], 1,
                    "the coordinated {label} mutation must fail only at the terminal",
                );
            } else {
                assert!(audit[0] > 0, "{label} interaction mutation {mutation}");
            }
            assert_eq!(audit[1], constraints);
            assert_eq!(audit[2], 4_096);
        }
    }
}

#[test]
fn production_product_quadratic_plan_rejects_every_committed_node() {
    let bytes = compile_fixture();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes).expect("Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "fixture must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("zero-import oracle fixture should instantiate");
    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let arguments = transition_arguments(&point, &current);

    for challenge in [3u32, 7, 31] {
        let mut baseline_arguments = arguments.clone();
        baseline_arguments.extend([challenge, u32::MAX]);
        assert_eq!(
            call(
                &mut store,
                &instance,
                "fixed_transition4_sparse_product_plan_node_audit",
                &baseline_arguments,
                3,
            ),
            [0, 21, 0],
            "clean production product-address plan, challenge {challenge}",
        );
        for node in 0u32..17 {
            let mut node_arguments = arguments.clone();
            node_arguments.extend([challenge, node]);
            let audit = call(
                &mut store,
                &instance,
                "fixed_transition4_sparse_product_plan_node_audit",
                &node_arguments,
                3,
            );
            assert_eq!(audit[0], 1, "product plan node {node} must fail");
            assert_eq!(audit[1], 21);
            assert_ne!(
                audit[2], 0,
                "product plan node {node}, challenge {challenge}",
            );
        }
    }
}

#[test]
fn sparse_public_composition_lowers_and_executes() {
    let bytes = compile_sparse_public_binding_fixture();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes)
        .expect("sparse public-composition Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "public fixture must stay zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("sparse public-composition fixture should instantiate");
    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 1, 2),
        imaginary: fixed(true, 1, 4),
    };
    let output = call(
        &mut store,
        &instance,
        "fixed_transition4_sparse_public_composition",
        &transition_arguments(&point, &current),
        56,
    );
    assert_eq!(output[0], 1, "public composition must execute");
    assert_eq!(output[1], 0, "statement mismatch must fail closed");

    let claim = Claim {
        point: point.clone(),
        bound: 8,
    };
    let start = Boundary {
        iteration: 3,
        z: current.clone(),
        escaped: escaped(&current),
    };
    let end = advance(&claim, &start);
    let statement = expected_committed_words(&claim, &start, &end, 1);
    let base = [11, 13, 17, 19, 23, 29, 31, 37];
    let interaction = [41, 43, 47, 53, 59, 61, 67, 71];
    let transcript = expected_sparse_air_lde_transcript(&statement, base, interaction);
    assert_eq!(
        output[2..10],
        transcript,
        "public constraints must derive from the independently reconstructed AIR transcript",
    );
    let fold = Ext4::from_words(&reference_poseidon_digest(b"PC01", &transcript)[..4]);
    let mix = Ext4::from_words(&reference_poseidon_digest(b"PM01", &transcript)[..4]);
    assert_ne!(fold, Ext4::ZERO, "PC01 must not erase later constraints");
    assert_ne!(mix, Ext4::ZERO, "PM01 must not erase public composition");
    assert_eq!(Ext4::from_words(&output[10..14]), fold);
    assert_eq!(Ext4::from_words(&output[14..18]), mix);
    assert_eq!(
        output[18..48],
        expected_sparse_public_positions(),
        "Fe public positions must match the independently enumerated sparse task plan",
    );

    let evaluation_point = Ext4([73, 79, 83, 89]);
    let opened_value = Ext4([97, 101, 103, 107]);
    let opened_auxiliary = Ext4([109, 113, 127, 131]);
    let expected = expected_sparse_public_composition(
        &point,
        &current,
        &end.z,
        evaluation_point,
        opened_value,
        opened_auxiliary,
        fold,
        mix,
    );
    assert_eq!(
        Ext4::from_words(&output[48..52]),
        expected,
        "public rational composition must match the independent field model",
    );
    let expected_mutation = expected_sparse_public_composition(
        &point,
        &current,
        &end.z,
        evaluation_point,
        opened_value.add(Ext4::from_base(1)),
        opened_auxiliary,
        fold,
        mix,
    );
    assert_eq!(
        Ext4::from_words(&output[52..56]),
        expected_mutation,
        "an opened value mutation must follow the same public quotient",
    );
    assert_ne!(
        expected_mutation, expected,
        "an opened value mutation must change the public contribution",
    );
}

#[test]
fn sparse_production_commitments_match_independent_port_oracle() {
    let entry = "fixed_transition4_sparse_production_composition_encoded";
    let bytes = compile_sparse_auth_fixture_entry(entry);
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes)
        .expect("focused production composition Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "production composition fixture must stay zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("production composition fixture should instantiate");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("production composition fixture should export linear memory");
    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let arguments = transition_arguments(&point, &current);
    let expected_lde = expected_sparse_base_lde_words(&point, &current);

    let mut production_compositions = Vec::new();
    for mutation in 0u32..=3 {
        let mut composition_arguments = arguments.clone();
        composition_arguments.push(mutation);
        let (_, _, actual) = encoded_resetting(
            &mut store,
            &instance,
            memory,
            "fixed_transition4_sparse_production_composition_encoded",
            &composition_arguments,
        );
        assert_eq!(actual.len(), 400, "production composition carrier width");
        assert_eq!(actual[0], 1, "production composition must be valid");

        let claim = Claim {
            point: point.clone(),
            bound: if mutation == 3 { 9 } else { 8 },
        };
        let start = Boundary {
            iteration: 3,
            z: current.clone(),
            escaped: escaped(&current),
        };
        let end = advance(&claim, &start);
        let statement_words = expected_committed_words(&claim, &start, &end, 1);
        assert_eq!(
            actual[1..32],
            statement_words,
            "recursive leaf statement mutation {mutation}",
        );

        let base_root = expected_sparse_base_lde_root(&expected_lde, mutation == 1);
        assert_eq!(actual[32], 1, "typed base root validity");
        assert_eq!(
            actual[33..41],
            base_root,
            "typed base root mutation {mutation}"
        );
        let interaction_prefix =
            expected_sparse_production_interaction_prefix(&point, &current, base_root);
        let interaction_lde = expected_sparse_interaction_lde(&interaction_prefix);
        let interaction_root =
            expected_sparse_interaction_lde_root(&interaction_lde, base_root, mutation == 2);
        assert_eq!(actual[41], 1, "typed interaction root validity");
        assert_eq!(actual[42], 1, "nested base root validity");
        assert_eq!(
            actual[43..51],
            base_root,
            "interaction root must retain its exact base dependency",
        );
        assert_eq!(
            actual[51..59],
            interaction_root,
            "typed interaction root mutation {mutation}",
        );

        let transcript =
            expected_sparse_air_lde_transcript(&statement_words, base_root, interaction_root);
        assert_eq!(
            actual[59..67],
            transcript,
            "AIR transcript mutation {mutation}"
        );
        let challenge = Ext4::from_words(&reference_poseidon_digest(b"BC01", &transcript)[..4]);
        assert_eq!(
            Ext4::from_words(&actual[67..71]),
            challenge,
            "composition challenge mutation {mutation}",
        );

        let mut composition_values = Vec::with_capacity(16);
        for evaluation in 0..16usize {
            let numerator_start = 71 + evaluation * 16;
            let numerators = std::array::from_fn(|family| {
                let start = numerator_start + family * 4;
                Ext4::from_words(&actual[start..start + 4])
            });
            let value_start = 327 + evaluation * 4;
            let value = Ext4::from_words(&actual[value_start..value_start + 4]);
            let point = Ext4::from_base(bb_mul(7, bb_pow(bb_two_adic_root(4), evaluation as u32)));
            assert_eq!(
                value,
                expected_sparse_air_composition(point, numerators),
                "production composition quotient, mutation {mutation}, evaluation {evaluation}",
            );
            composition_values.push(value);
        }
        let composition_root = expected_sparse_composition_root(&composition_values);
        assert_eq!(actual[391], 1, "composition root validity");
        assert_eq!(
            actual[392..400],
            composition_root,
            "composition root mutation {mutation} must match independent Plonky3",
        );
        production_compositions.push((
            base_root,
            interaction_root,
            transcript,
            challenge,
            composition_values,
            composition_root,
        ));
    }
    assert_ne!(
        production_compositions[1].0, production_compositions[0].0,
        "base mutation must change LD01",
    );
    assert_eq!(
        production_compositions[2].0, production_compositions[0].0,
        "interaction mutation must preserve LD01",
    );
    assert_ne!(
        production_compositions[2].1, production_compositions[0].1,
        "interaction mutation must change LD02",
    );
    assert_eq!(
        production_compositions[3].0, production_compositions[0].0,
        "statement mutation must preserve LD01",
    );
    assert_eq!(
        production_compositions[3].1, production_compositions[0].1,
        "statement mutation must preserve LD02",
    );
    for mutation in 1..=3 {
        assert_ne!(
            production_compositions[mutation].2, production_compositions[0].2,
            "mutation {mutation} must change the AIR transcript",
        );
        assert_ne!(
            production_compositions[mutation].3, production_compositions[0].3,
            "mutation {mutation} must change the composition challenge",
        );
        assert_ne!(
            production_compositions[mutation].4, production_compositions[0].4,
            "mutation {mutation} must change the composition codeword",
        );
        assert_ne!(
            production_compositions[mutation].5, production_compositions[0].5,
            "mutation {mutation} must change the composition root",
        );
    }
    let mut invalid_production_arguments = arguments.clone();
    invalid_production_arguments.push(4);
    let (_, invalid_length, invalid_words) = encoded_resetting(
        &mut store,
        &instance,
        memory,
        "fixed_transition4_sparse_production_composition_encoded",
        &invalid_production_arguments,
    );
    assert_eq!(invalid_length, 0, "unknown production mutation length");
    assert!(
        invalid_words.is_empty(),
        "unknown production mutation payload"
    );
}

#[test]
fn sparse_production_lde_codewords_match_independent_oracle() {
    let entry = "fixed_transition4_sparse_air_lde_encoded";
    let bytes = compile_sparse_auth_fixture_entry(entry);
    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, &bytes).expect("focused sparse LDE Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "fixture must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("focused sparse LDE fixture should instantiate");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("focused sparse LDE fixture should export linear memory");

    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let (_, _, actual) = encoded(
        &mut store,
        &instance,
        memory,
        entry,
        &transition_arguments(&point, &current),
    );
    let trace_root = expected_sparse_trace_root(&point, &current);
    let prefix = expected_sparse_air_prefix_words(&point, &current, trace_root);
    let mut expected = expected_sparse_air_lde_words(&prefix);
    expected.extend([1, 0]);
    assert_eq!(actual.len(), expected.len(), "sparse LDE carrier width");
    for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(
            actual, expected,
            "sparse production LDE word {index} must match its independent oracle",
        );
    }
}

#[test]
fn sparse_lde_multipaths_authenticate_production_codewords() {
    const BASE_FIELDS: usize = 260;
    const INTERACTION_FIELDS: usize = 152;
    let bytes = compile_sparse_auth_fixture_entry("fixed_transition4_sparse_lde_multipath_audit");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes)
        .expect("sparse LDE multipath Wasm module should load");
    assert_eq!(
        module.imports().count(),
        0,
        "sparse LDE multipath fixture must stay zero-import",
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("sparse LDE multipath fixture should instantiate");

    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let arguments = transition_arguments(&point, &current);
    let trace_root = expected_sparse_trace_root(&point, &current);
    let prefix = expected_sparse_air_prefix_words(&point, &current, trace_root);
    let lde = expected_sparse_air_lde_words(&prefix);
    let base_root = expected_sparse_base_lde_root(&lde, false);
    let interaction_prefix =
        expected_sparse_production_interaction_prefix(&point, &current, base_root);
    let interaction_lde = expected_sparse_interaction_lde(&interaction_prefix);
    let interaction_root = expected_sparse_interaction_lde_root(&interaction_lde, base_root, false);
    let requests = [12, 0, 4, 8, 3, 3];
    let canonical_indices = [0, 3, 4, 8, 12];
    let sibling_count = reference_sparse_multipath_sibling_count(16, &requests);

    let clean = call(
        &mut store,
        &instance,
        "fixed_transition4_sparse_lde_multipath_audit",
        &arguments,
        54,
    );
    assert_eq!(clean[0..3], [1, 1, 1], "both openings must be written");
    assert_eq!(clean[3], canonical_indices.len() as u32);
    assert_eq!(clean[4], sibling_count as u32);
    assert_eq!(clean[5], 1, "LD01 opening must authenticate");
    assert_eq!(clean[6], 1, "LD02 opening must be valid");
    assert_eq!(clean[7], canonical_indices.len() as u32);
    assert_eq!(clean[8], sibling_count as u32);
    assert_eq!(clean[9], 1, "LD02 opening must authenticate");
    assert_eq!(
        clean[10], 1,
        "matching paths must reconstruct a typed AIR row"
    );
    assert_eq!(clean[11], 1, "clean authenticated row must be accepted");
    assert_eq!(
        clean[12],
        pack_sparse_multipath_indices(&canonical_indices),
        "LD01 requests must normalize to one canonical index sequence",
    );
    assert_eq!(clean[13], clean[12], "LD01 and LD02 geometry must agree");
    assert_eq!(clean[14..22], base_root, "LD01 root must match Plonky3");
    assert_eq!(
        clean[22..30],
        base_root,
        "LD02 must retain the exact LD01 dependency",
    );
    assert_eq!(
        clean[30..38],
        interaction_root,
        "LD02 root must match Plonky3"
    );
    assert_eq!(
        clean[38],
        lde[1 + 4 * BASE_FIELDS],
        "opened LD01 value must be the requested LDE row",
    );
    assert_eq!(
        clean[39],
        interaction_lde[4 * INTERACTION_FIELDS],
        "opened LD02 value must be the requested LDE row",
    );
    assert_eq!(
        clean[40..42],
        [1, 1],
        "unused value capacity must stay zero"
    );

    assert_eq!(
        clean[42..54],
        [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        "one generated opening must accept cleanly and reject every directed or unknown mutation",
    );
}

#[test]
#[ignore = "requires an explicit real-Chrome composition-binding receipt"]
fn production_sparse_composition_binding_browser_workspace_matches_independent_reference() {
    let receipt_dir = std::env::var_os(SPARSE_COMPOSITION_BIND_BROWSER_RECEIPT_DIR)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!(
                "set {SPARSE_COMPOSITION_BIND_BROWSER_RECEIPT_DIR} to the directory emitted by the generic browser resource snapshot"
            )
        });
    let actual = read_u32le_file(&receipt_dir.join("workspace.u32le"));

    let base_root = [1, 2, 3, 4, 5, 6, 7, 8];
    let interaction_root = [9, 10, 11, 12, 13, 14, 15, 16];
    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let claim = Claim { point, bound: 16 };
    let start = Boundary {
        iteration: 0,
        z: current,
        escaped: false,
    };
    let end = advance(&claim, &start);
    let statement = expected_committed_words(&claim, &start, &end, 1);
    let air_transcript =
        expected_sparse_air_lde_transcript(&statement, base_root, interaction_root);

    let security_profile_words = expected_recursive_security_profile_words();
    let security_profile = reference_packed_u32_commitment(b"SP01", &security_profile_words);
    let mut security_binding = air_transcript.to_vec();
    security_binding.extend(security_profile);
    let security_transcript = reference_poseidon_digest(b"SP02", &security_binding);

    let mut expected = Vec::with_capacity(219);
    expected.extend(base_root);
    expected.extend(interaction_root);
    expected.extend(&statement);

    expected.push(1);
    expected.push(1);
    expected.extend(&statement);
    expected.push(1);
    expected.extend(base_root);
    expected.push(1);
    expected.push(1);
    expected.extend(base_root);
    expected.extend(interaction_root);
    expected.push(1);
    expected.extend(air_transcript);
    expected.push(1);
    expected.extend(security_transcript);

    expected.push(1);
    expected.push(1);
    expected.extend(base_root);
    expected.extend(security_transcript);

    expected.push(1);
    expected.extend(&reference_poseidon_digest(b"BC01", &security_transcript)[..4]);
    expected.extend(expected_sparse_lde_interaction_challenges(base_root));
    expected.push(1);
    expected.extend(&reference_poseidon_digest(b"PC01", &security_transcript)[..4]);
    expected.extend(&reference_poseidon_digest(b"PM01", &security_transcript)[..4]);

    expected.extend([0; 18]);
    expected.push(1);
    expected.extend(security_profile);
    expected.extend([1, 0]);
    assert_eq!(
        expected.len(),
        219,
        "independent composition workspace width"
    );
    assert_eq!(actual, expected, "real-Chrome composition workspace");
}

#[test]
#[ignore = "requires an explicit real-Chrome production proof receipt"]
fn production_sparse_proof_browser_words_match_independent_model() {
    let receipt_dir = std::env::var_os(SPARSE_BASE_BROWSER_RECEIPT_DIR)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!(
                "set {SPARSE_BASE_BROWSER_RECEIPT_DIR} to the directory emitted by the generic browser resource snapshot"
            )
        });
    let actual = read_u32le_file(&receipt_dir.join("base_trace.u32le"));
    let transition = read_u32le_file(&receipt_dir.join("transition_workspace.u32le"));
    let interaction_validity = read_u32le_file(&receipt_dir.join("lde_inverse_progress.u32le"));
    assert_eq!(
        actual.len(),
        PRODUCTION_TRACE_ROWS * SPARSE_BASE_FIELDS,
        "production base trace word count",
    );
    assert_eq!(
        transition.len(),
        218,
        "production transition workspace words"
    );
    assert_eq!(
        transition[217], 1,
        "all completed proof phases must be valid"
    );
    assert_eq!(
        &interaction_validity[..PRODUCTION_TRACE_ROWS],
        vec![1; PRODUCTION_TRACE_ROWS],
        "every production interaction row must be valid",
    );

    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let expected = expected_sparse_production_base_trace_words(&point, &current);
    if let Some((index, (actual, expected))) = actual
        .iter()
        .zip(&expected)
        .enumerate()
        .find(|(_, (actual, expected))| actual != expected)
    {
        let row = index % PRODUCTION_TRACE_ROWS;
        let column = index / PRODUCTION_TRACE_ROWS;
        let controls = expected_sparse_control_rows();
        let rows = expected_sparse_rows(&point, &current);
        panic!(
            "production sparse base trace differs at column {column}, row {row}: browser={actual}, independent={expected}, control={:?}, witness={:?}",
            controls[row], rows[row],
        );
    }
}

#[test]
#[ignore = "requires an explicit real-Chrome production LDE receipt"]
fn production_sparse_lde_browser_codewords_match_independent_plonky3() {
    const INTERACTION_FIELDS: usize = 152;
    const LDE_ROWS: usize = PRODUCTION_TRACE_ROWS * 2;

    let receipt_dir = std::env::var_os(SPARSE_LDE_BROWSER_RECEIPT_DIR)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!(
                "set {SPARSE_LDE_BROWSER_RECEIPT_DIR} to the directory emitted by the generic browser resource snapshot"
            )
        });
    let transition = read_u32le_file(&receipt_dir.join("transition_workspace.u32le"));
    let actual_base_lde = read_u32le_file(&receipt_dir.join("lde_values.u32le"));
    let actual_interaction_lde = read_u32le_file(&receipt_dir.join("lde_progress.u32le"));

    assert_eq!(
        transition.len(),
        218,
        "production transition workspace words"
    );
    assert_eq!(
        transition[217], 1,
        "all completed production LDE phases must be valid",
    );
    assert_eq!(
        actual_base_lde.len(),
        LDE_ROWS * SPARSE_BASE_FIELDS,
        "production base LDE word count",
    );
    assert_eq!(
        actual_interaction_lde.len(),
        LDE_ROWS * INTERACTION_FIELDS,
        "production interaction LDE word count",
    );

    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let base_trace = expected_sparse_production_base_trace_words(&point, &current);
    let expected_base_lde =
        plonky3_coset_lde_column_major(&base_trace, PRODUCTION_TRACE_ROWS, SPARSE_BASE_FIELDS);
    if let Some((index, (actual, expected))) = actual_base_lde
        .iter()
        .zip(&expected_base_lde)
        .enumerate()
        .find(|(_, (actual, expected))| actual != expected)
    {
        let row = index % LDE_ROWS;
        let column = index / LDE_ROWS;
        panic!(
            "production base LDE differs at column {column}, row {row}: browser={actual}, Plonky3={expected}"
        );
    }

    let base_lde_root = expected_sparse_production_base_lde_root(&expected_base_lde);
    let interaction_trace =
        expected_sparse_production_interaction_trace_words(&point, &current, base_lde_root);
    let expected_interaction_lde = plonky3_coset_lde_column_major(
        &interaction_trace,
        PRODUCTION_TRACE_ROWS,
        INTERACTION_FIELDS,
    );
    if let Some((index, (actual, expected))) = actual_interaction_lde
        .iter()
        .zip(&expected_interaction_lde)
        .enumerate()
        .find(|(_, (actual, expected))| actual != expected)
    {
        let row = index % LDE_ROWS;
        let column = index / LDE_ROWS;
        panic!(
            "production interaction LDE differs at column {column}, row {row}: browser={actual}, Plonky3={expected}"
        );
    }
}

#[test]
#[ignore = "requires an explicit real-Chrome production interaction-root receipt"]
fn production_sparse_lde_browser_roots_match_independent_reference() {
    const INTERACTION_FIELDS: usize = 152;
    const LDE_ROWS: usize = PRODUCTION_TRACE_ROWS * 2;
    const TREE_NODES: usize = LDE_ROWS * 2 - 1;
    const ROOT_NODE: usize = TREE_NODES - 1;
    const ROOT_WORD: usize = ROOT_NODE * 8;

    let receipt_dir = std::env::var_os(SPARSE_LDE_BROWSER_RECEIPT_DIR)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!(
                "set {SPARSE_LDE_BROWSER_RECEIPT_DIR} to the directory emitted by the generic browser resource snapshot"
            )
        });
    let transition = read_u32le_file(&receipt_dir.join("transition_workspace.u32le"));
    let interaction_tree = read_u32le_file(&receipt_dir.join("base_trace.u32le"));
    let interaction_validity = read_u32le_file(&receipt_dir.join("lde_inverse_values.u32le"));

    assert_eq!(
        transition.len(),
        218,
        "production transition workspace words"
    );
    assert_eq!(transition[217], 1, "all commitment phases must be valid");
    assert!(interaction_tree.len() >= ROOT_WORD + 8);
    assert!(interaction_validity.len() >= ROOT_NODE + 1);

    let point = ComplexFx {
        real: fixed(true, 3, 4),
        imaginary: fixed(false, 1, 8),
    };
    let current = ComplexFx {
        real: fixed(false, 5, 4),
        imaginary: fixed(true, 3, 8),
    };
    let base_trace = expected_sparse_production_base_trace_words(&point, &current);
    let base_lde =
        plonky3_coset_lde_column_major(&base_trace, PRODUCTION_TRACE_ROWS, SPARSE_BASE_FIELDS);
    let base_root = expected_sparse_production_base_lde_root(&base_lde);
    let interaction_trace =
        expected_sparse_production_interaction_trace_words(&point, &current, base_root);
    let interaction_lde = plonky3_coset_lde_column_major(
        &interaction_trace,
        PRODUCTION_TRACE_ROWS,
        INTERACTION_FIELDS,
    );
    let interaction_root =
        expected_sparse_production_interaction_lde_root(&interaction_lde, base_root);

    assert_eq!(
        &transition[..8],
        &base_root,
        "retained browser LD01 root must match the independent reference",
    );
    assert_eq!(
        interaction_validity[ROOT_NODE], 1,
        "browser LD02 root node must be valid",
    );
    assert_eq!(
        &interaction_tree[ROOT_WORD..ROOT_WORD + 8],
        &interaction_root,
        "browser LD02 root must match the independent reference",
    );
}
