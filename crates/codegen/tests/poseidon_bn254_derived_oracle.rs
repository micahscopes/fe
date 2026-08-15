//! Semantic gate for Fe-derived canonical Poseidon t=3 BN254 Fr parameters.
//! No generated parameter table appears in the implementation or this test.
//! A direct Rust implementation of the published Grain procedure first
//! reproduces the independently pinned plain-field constants, then every Fe
//! Wasm Montgomery limb is checked against a num-bigint conversion.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use num_bigint::BigUint;
use std::path::Path;
use url::Url;

const WIDTH: usize = 3;
const ROUND_CONSTANT_COUNT: usize = 195;
const MDS_ENTRY_COUNT: usize = 9;
const PARAMETER_COUNT: usize = ROUND_CONSTANT_COUNT + MDS_ENTRY_COUNT;
const LIMB_BITS: usize = 13;
const LIMBS: usize = 20;

const CANONICAL_POSEIDON: &str = include_str!("../../fe/tests/fixtures/fe_test/const_poseidon.fe");

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

struct ReferenceGrain {
    state: [u8; 80],
}

impl ReferenceGrain {
    fn canonical_t3() -> Self {
        let fields = [
            (1u128, 2usize),
            (0, 4),
            (254, 12),
            (3, 12),
            (8, 10),
            (57, 10),
            ((1u128 << 30) - 1, 30),
        ];
        let mut packed = 0u128;
        let mut width = 0usize;
        for (value, bits) in fields {
            packed = (packed << bits) | value;
            width += bits;
        }
        assert_eq!(width, 80);
        let mut state = [0u8; 80];
        for (index, bit) in state.iter_mut().enumerate() {
            *bit = ((packed >> (79 - index)) & 1) as u8;
        }
        let mut grain = Self { state };
        for _ in 0..160 {
            grain.step();
        }
        grain
    }

    fn step(&mut self) -> u8 {
        let next = self.state[62]
            ^ self.state[51]
            ^ self.state[38]
            ^ self.state[23]
            ^ self.state[13]
            ^ self.state[0];
        self.state.rotate_left(1);
        self.state[79] = next;
        next
    }

    fn admitted_bit(&mut self) -> u8 {
        loop {
            let selector = self.step();
            let data = self.step();
            if selector == 1 {
                return data;
            }
        }
    }

    fn bits(&mut self, count: usize) -> BigUint {
        let mut value = BigUint::from(0u32);
        for _ in 0..count {
            value = (value << 1) | BigUint::from(self.admitted_bit());
        }
        value
    }

    fn field_element(&mut self, prime: &BigUint) -> BigUint {
        loop {
            let value = self.bits(254);
            if &value < prime {
                return value;
            }
        }
    }
}

fn reference_parameters() -> Vec<BigUint> {
    let prime = bn254_fr_prime();
    let mut grain = ReferenceGrain::canonical_t3();
    let mut round_constants = Vec::with_capacity(ROUND_CONSTANT_COUNT);
    for _ in 0..ROUND_CONSTANT_COUNT {
        round_constants.push(grain.field_element(&prime));
    }

    let values = loop {
        let candidate: Vec<BigUint> = (0..2 * WIDTH).map(|_| grain.bits(254) % &prime).collect();
        let distinct = candidate
            .iter()
            .enumerate()
            .all(|(index, value)| candidate[..index].iter().all(|other| other != value));
        let denominators_nonzero = (0..WIDTH).all(|row| {
            (0..WIDTH).all(|column| {
                (&candidate[row] + &candidate[WIDTH + column]) % &prime != BigUint::from(0u32)
            })
        });
        if distinct && denominators_nonzero {
            break candidate;
        }
    };

    let mut parameters = round_constants;
    for row in 0..WIDTH {
        for column in 0..WIDTH {
            let denominator = (&values[row] + &values[WIDTH + column]) % &prime;
            parameters.push(denominator.modpow(&(&prime - BigUint::from(2u32)), &prime));
        }
    }
    parameters
}

fn compile_gate() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/poseidon_bn254_derived_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url), "gate ingot diagnostics");
    let ingot = db.workspace().containing_ingot(&db, url).unwrap();
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("derived Poseidon gate should compile")
        .into_bytecode()
        .expect("Wasm bytecode");
    wasmparser::validate(&bytes).expect("valid Wasm");
    bytes
}

fn direct_self_shrink(raw: u32) -> u32 {
    let mut outputs = 0u32;
    let mut count = 0u32;
    for pair in 0..9 {
        if (raw >> (pair * 2)) & 1 == 1 {
            outputs = (outputs << 1) | ((raw >> (pair * 2 + 1)) & 1);
            count += 1;
        }
    }
    outputs | (count << 16)
}

#[test]
fn fe_derivation_matches_canonical_parameters_and_exhaustive_self_shrinker() {
    let canonical_mds = parse_const_block(CANONICAL_POSEIDON, "POSEIDON_T3_MDS");
    let canonical_round_constants =
        parse_const_block(CANONICAL_POSEIDON, "POSEIDON_T3_ROUND_CONSTANTS");
    assert_eq!(canonical_mds.len(), MDS_ENTRY_COUNT);
    assert_eq!(canonical_round_constants.len(), ROUND_CONSTANT_COUNT);

    let reference = reference_parameters();
    assert_eq!(
        &reference[..ROUND_CONSTANT_COUNT],
        canonical_round_constants
    );
    assert_eq!(&reference[ROUND_CONSTANT_COUNT..], canonical_mds);

    let wasm = compile_gate();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(
        module.imports().next().is_none(),
        "gate must be zero-import Wasm"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let parameter_word = instance
        .get_typed_func::<(u32, u32), u32>(&mut store, "derived_parameter_word")
        .unwrap();
    let self_shrink = instance
        .get_typed_func::<u32, u32>(&mut store, "derived_self_shrink_9")
        .unwrap();
    let arena_reset = instance
        .get_typed_func::<(), ()>(&mut store, "fe_cabi_reset")
        .expect("materialized const lookup must expose the canonical arena reset");

    for raw in 0..(1 << 18) {
        assert_eq!(
            self_shrink.call(&mut store, raw).unwrap(),
            direct_self_shrink(raw),
            "self-shrinker mismatch for raw block {raw:#07x}"
        );
    }

    let prime = bn254_fr_prime();
    let radix = BigUint::from(1u32) << (LIMB_BITS * LIMBS);
    for (parameter, plain) in reference.iter().enumerate() {
        let montgomery = (plain * &radix) % &prime;
        for word in 0..LIMBS {
            let expected = ((&montgomery >> (word * LIMB_BITS))
                & BigUint::from((1u32 << LIMB_BITS) - 1))
            .to_u32_digits()
            .first()
            .copied()
            .unwrap_or(0);
            arena_reset.call(&mut store, ()).unwrap();
            let actual = parameter_word
                .call(&mut store, (parameter as u32, word as u32))
                .unwrap_or_else(|error| {
                    panic!("parameter {parameter}, Montgomery word {word} trapped: {error}")
                });
            assert_eq!(
                actual, expected,
                "parameter {parameter}, Montgomery word {word}"
            );
        }
    }
    assert_eq!(reference.len(), PARAMETER_COUNT);
}
