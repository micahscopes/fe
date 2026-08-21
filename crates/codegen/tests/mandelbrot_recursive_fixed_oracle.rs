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
const POSEIDON_WIDTH: usize = 16;

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

fn complex_words(value: &ComplexFx) -> Vec<u32> {
    let mut words = fixed_words(&value.real);
    words.extend(fixed_words(&value.imaginary));
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
