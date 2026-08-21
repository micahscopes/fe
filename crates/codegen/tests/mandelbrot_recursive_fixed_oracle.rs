//! Independent bigint gate for the field-neutral recursive Mandelbrot carrier.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use num_bigint::BigUint;
use p3_baby_bear::{BabyBear as P3BabyBear, default_babybear_poseidon2_16};
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
const PRODUCT_WITNESS_WORDS: usize = 24;
const LINEAR_WITNESS_WORDS: usize = 33;
const RANGE_WITNESS_WORDS: usize = LIMBS * LIMB_BITS * 2 + 1;
const TRANSITION_WITNESS_WORDS: usize = 1 + 3 * PRODUCT_WITNESS_WORDS + 4 * LINEAR_WITNESS_WORDS;
const POSEIDON_WIDTH: usize = 16;
const BABY_BEAR_MODULUS: u32 = 2_013_265_921;

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
    let mut output_limbs = [0u32; LIMBS];
    for index in 0..LIMBS {
        let retained = digits[LIMBS - 1 + index];
        let total = retained + round_carry;
        output_limbs[index] = total % LIMB_BASE;
        round_carry = total / LIMB_BASE;
    }
    let expected_output = multiply(left, right);
    assert_eq!(output_limbs, limbs(&expected_output));

    let mut words = vec![1];
    words.extend(&digits[..LIMBS]);
    words.extend(&carries[..LIMBS]);
    words.extend(&digits[LIMBS..]);
    words.extend(&carries[LIMBS..]);
    words.extend([round_up as u32, (round_carry == 1) as u32]);
    words.extend(fixed_words(&expected_output));
    assert_eq!(words.len(), PRODUCT_WITNESS_WORDS);
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
        assert_eq!(
            call(
                &mut store,
                &instance,
                "fixed_product4_residuals",
                &arguments,
                14,
            ),
            [0; 14],
            "BabyBear AIR residual case {case}",
        );
        for mutation in 0..=7 {
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
        for mutation in [1u32, 2, 3, 4, 7] {
            let mut mutated_arguments = arguments.clone();
            mutated_arguments.push(mutation);
            let expected = match mutation {
                2 => BABY_BEAR_MODULUS - LIMB_BASE,
                3 if expected_witness[17] == 0 => 1,
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
