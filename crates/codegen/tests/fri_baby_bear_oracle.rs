//! Independent executable gate for the reusable BabyBear FRI pair ingot.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const MODULUS: u32 = 2_013_265_921;
const TWO_ADICITY: u32 = 27;
const EXT_NONRESIDUE: u32 = 11;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Ext4([u32; 4]);

impl Ext4 {
    const ZERO: Self = Self([0; 4]);

    fn reduced(words: [u32; 4]) -> Self {
        Self(words.map(|word| word % MODULUS))
    }

    fn from_base(word: u32) -> Self {
        Self([word % MODULUS, 0, 0, 0])
    }

    fn add(self, other: Self) -> Self {
        let modulus = u64::from(MODULUS);
        Self(core::array::from_fn(|index| {
            ((u64::from(self.0[index]) + u64::from(other.0[index])) % modulus) as u32
        }))
    }

    fn sub(self, other: Self) -> Self {
        let modulus = u64::from(MODULUS);
        Self(core::array::from_fn(|index| {
            ((u64::from(self.0[index]) + modulus - u64::from(other.0[index])) % modulus) as u32
        }))
    }

    fn mul(self, other: Self) -> Self {
        let modulus = u64::from(MODULUS);
        let mut output = [0u64; 4];
        for left in 0..4 {
            for right in 0..4 {
                let mut term = u64::from(self.0[left]) * u64::from(other.0[right]) % modulus;
                let mut degree = left + right;
                if degree >= 4 {
                    degree -= 4;
                    term = term * u64::from(EXT_NONRESIDUE) % modulus;
                }
                output[degree] = (output[degree] + term) % modulus;
            }
        }
        Self(output.map(|word| word as u32))
    }
}

fn pow_mod(mut base: u64, mut exponent: u32) -> u32 {
    let modulus = u64::from(MODULUS);
    base %= modulus;
    let mut result = 1u64;
    while exponent != 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus;
        }
        base = base * base % modulus;
        exponent >>= 1;
    }
    result as u32
}

fn reference_fold(positive: Ext4, negative: Ext4, challenge: Ext4, point: u32) -> Ext4 {
    let inverse_two = pow_mod(2, MODULUS - 2);
    let point_inverse = pow_mod(u64::from(point), MODULUS - 2);
    let even = positive.add(negative).mul(Ext4::from_base(inverse_two));
    let odd = positive
        .sub(negative)
        .mul(Ext4::from_base(inverse_two))
        .mul(Ext4::from_base(point_inverse));
    even.add(challenge.mul(odd))
}

fn root16() -> u32 {
    let maximal_root = pow_mod(31, 15);
    pow_mod(u64::from(maximal_root), 1 << (TWO_ADICITY - 4))
}

fn fixture_url() -> Url {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fri_baby_bear_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "FRI pair fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("FRI pair fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected FRI pair diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O2)
        .expect("FRI pair fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("FRI pair Wasm should validate");
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
    let params = arguments
        .iter()
        .map(|value| Val::I32(*value as i32))
        .collect::<Vec<_>>();
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

fn arguments(positive: Ext4, negative: Ext4, challenge: Ext4, tail: [u32; 2]) -> Vec<u32> {
    positive
        .0
        .into_iter()
        .chain(negative.0)
        .chain(challenge.0)
        .chain(tail)
        .collect()
}

#[test]
fn reusable_fri_schedule_derives_the_complete_16_to_1_placement() {
    let bytes = compile_wasm();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("FRI schedule module should load");
    assert!(
        module.imports().next().is_none(),
        "FRI schedule gate must remain zero-import",
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("FRI schedule module should instantiate");

    assert_eq!(
        call(&mut store, &instance, "fri_schedule16_metadata", &[], 8,),
        vec![4, 1, 5, 4, 15, 3, 3, 26],
    );
    let expected = [
        [1, 0, 1, 4, 16, 8, 0, 0, 15, 4, 2],
        [1, 1, 2, 3, 8, 4, 8, 15, 7, 2, 1],
        [1, 2, 3, 2, 4, 2, 12, 22, 3, 1, 0],
        [1, 3, 4, 1, 2, 1, 14, 25, 1, 0, 0],
    ];
    for (index, placement) in expected.into_iter().enumerate() {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "fri_schedule16_round",
                &[index as u32],
                11,
            ),
            placement,
            "wrong placement for FRI round {index}",
        );
        let round = index as u32 + 1;
        let decimal = [b'0' + (round / 10) as u8, b'0' + (round % 10) as u8];
        let tag =
            |prefix: [u8; 2]| u32::from_be_bytes([prefix[0], prefix[1], decimal[0], decimal[1]]);
        assert_eq!(
            call(
                &mut store,
                &instance,
                "fri_round_domains",
                &[index as u32],
                3,
            ),
            vec![tag(*b"FC"), tag(*b"FR"), tag(*b"FT")],
            "wrong transcript domains for FRI round {round}",
        );
        assert_eq!(
            call(
                &mut store,
                &instance,
                "fri_runtime_round_domains",
                &[round],
                3,
            ),
            vec![tag(*b"FC"), tag(*b"FR"), tag(*b"FT")],
            "runtime placement domains diverged for FRI round {round}",
        );
    }
    assert_eq!(
        call(&mut store, &instance, "fri_schedule16_round", &[4], 11,),
        vec![0; 11],
    );
    assert_eq!(
        call(&mut store, &instance, "fri_round_domains", &[4], 3,),
        vec![0; 3],
    );
}

#[test]
fn reusable_fri_pair_matches_independent_extension_field_arithmetic() {
    let bytes = compile_wasm();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("FRI pair module should load");
    assert!(
        module.imports().next().is_none(),
        "FRI pair gate must remain zero-import",
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("FRI pair module should instantiate");

    let vectors = [
        (Ext4::ZERO, Ext4::ZERO, Ext4::ZERO),
        (
            Ext4::reduced([1, 2, 3, 4]),
            Ext4::reduced([5, 6, 7, 8]),
            Ext4::reduced([9, 10, 11, 12]),
        ),
        (
            Ext4::reduced([MODULUS - 1, MODULUS - 2, 17, u32::MAX]),
            Ext4::reduced([998_244_353, 1_000_000_007, 42, 1_234_567_890]),
            Ext4::reduced([123_456_789, 987_654_321, MODULUS, MODULUS + 1]),
        ),
    ];

    for (positive, negative, challenge) in vectors {
        for point in [1, 7, 123_456_789, MODULUS - 1] {
            let expected = reference_fold(positive, negative, challenge, point);
            let mut expected_words = vec![1];
            expected_words.extend(expected.0);
            assert_eq!(
                call(
                    &mut store,
                    &instance,
                    "fri_pair_at_point",
                    &arguments(positive, negative, challenge, [point, 0])[..13],
                    5,
                ),
                expected_words,
                "point fold differs at point {point}",
            );
        }

        let zero_point = arguments(positive, negative, challenge, [0, 0]);
        assert_eq!(
            call(
                &mut store,
                &instance,
                "fri_pair_at_point",
                &zero_point[..13],
                5
            ),
            vec![0; 5],
            "checked point fold must reject zero",
        );

        for shift in [1, 7, 123_456_789, MODULUS - 1] {
            let root = root16();
            for index in 0..8 {
                let point = (u64::from(shift) * u64::from(pow_mod(u64::from(root), index))
                    % u64::from(MODULUS)) as u32;
                let expected = reference_fold(positive, negative, challenge, point);
                let args = arguments(positive, negative, challenge, [shift, index]);
                let mut checked = vec![1];
                checked.extend(expected.0);
                assert_eq!(
                    call(&mut store, &instance, "fri_pair16_checked", &args, 5),
                    checked,
                    "checked power-of-two fold differs at shift {shift}, index {index}",
                );
                assert_eq!(
                    call(&mut store, &instance, "fri_pair16_value", &args, 4),
                    expected.0,
                    "value power-of-two fold differs at shift {shift}, index {index}",
                );
            }
        }

        for [shift, index] in [[0, 0], [0, 7], [1, 8], [7, u32::MAX]] {
            let args = arguments(positive, negative, challenge, [shift, index]);
            assert_eq!(
                call(&mut store, &instance, "fri_pair16_checked", &args, 5),
                vec![0; 5],
                "checked power-of-two fold accepted invalid placement",
            );
            assert_eq!(
                call(&mut store, &instance, "fri_pair16_value", &args, 4),
                vec![0; 4],
                "value power-of-two fold accepted invalid placement",
            );
        }
    }
}
