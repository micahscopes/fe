//! Independent exactness gate for Fe-derived width-16 BabyBear Poseidon2.
//!
//! Fe derives every round constant from the shared Hades Grain construction.
//! The separately maintained Plonky3 implementation supplies both the
//! canonical constants and the permutation semantics.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use p3_baby_bear::{
    BABYBEAR_POSEIDON2_RC_16_EXTERNAL_FINAL, BABYBEAR_POSEIDON2_RC_16_EXTERNAL_INITIAL,
    BABYBEAR_POSEIDON2_RC_16_INTERNAL, BabyBear, default_babybear_poseidon2_16,
};
use p3_field::{PrimeCharacteristicRing, PrimeField32};
use p3_symmetric::Permutation;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const WIDTH: usize = 16;
const ROUND_CONSTANT_COUNT: usize = 8 * WIDTH + 13;

fn fixture_url() -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/poseidon_baby_bear_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn initialized_db() -> DriverDataBase {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "BabyBear Poseidon2 oracle fixture initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("BabyBear Poseidon2 oracle fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected BabyBear Poseidon2 fixture diagnostics:\n{diagnostics}"
    );
    db
}

fn compile_wasm() -> Vec<u8> {
    let db = initialized_db();
    let ingot = db
        .workspace()
        .containing_ingot(&db, fixture_url())
        .expect("BabyBear Poseidon2 oracle fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("BabyBear Poseidon2 fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("BabyBear Poseidon2 Wasm should validate");
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

fn reference_parameters() -> Vec<u32> {
    let mut parameters = Vec::with_capacity(ROUND_CONSTANT_COUNT);
    for round in BABYBEAR_POSEIDON2_RC_16_EXTERNAL_INITIAL {
        parameters.extend(round.map(|value| value.as_canonical_u32()));
    }
    parameters.extend(BABYBEAR_POSEIDON2_RC_16_INTERNAL.map(|value| value.as_canonical_u32()));
    for round in BABYBEAR_POSEIDON2_RC_16_EXTERNAL_FINAL {
        parameters.extend(round.map(|value| value.as_canonical_u32()));
    }
    assert_eq!(parameters.len(), ROUND_CONSTANT_COUNT);
    parameters
}

fn reference_permutation(input: [u32; WIDTH]) -> [u32; WIDTH] {
    let mut state = input.map(BabyBear::from_u32);
    default_babybear_poseidon2_16().permute_mut(&mut state);
    state.map(|value| value.as_canonical_u32())
}

#[test]
fn fe_derived_poseidon2_matches_plonky3_parameters_and_permutations() {
    let bytes = compile_wasm();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("Poseidon2 module should load");
    assert!(
        module.imports().next().is_none(),
        "Poseidon2 gate must remain zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("Poseidon2 module should instantiate");

    for (index, expected) in reference_parameters().into_iter().enumerate() {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "derived_parameter",
                &[index as u32],
                1,
            ),
            vec![expected],
            "round constant {index} differs from Plonky3",
        );
    }

    let mut sequential = [0u32; WIDTH];
    for (index, value) in sequential.iter_mut().enumerate() {
        *value = index as u32;
    }
    let inputs = [
        [0u32; WIDTH],
        sequential,
        [u32::MAX; WIDTH],
        [
            894_848_333,
            1_437_655_012,
            1_200_606_629,
            1_690_012_884,
            71_131_202,
            1_749_206_695,
            1_717_947_831,
            120_589_055,
            19_776_022,
            42_382_981,
            1_831_865_506,
            724_844_064,
            171_220_207,
            1_299_207_443,
            227_047_920,
            1_783_754_913,
        ],
        [
            1,
            17,
            257,
            65_537,
            123_456_789,
            2_013_265_920,
            2_013_265_921,
            2_013_265_922,
            0x8000_0000,
            0x7fff_ffff,
            0x1357_9bdf,
            0x2468_ace0,
            31,
            27,
            16,
            13,
        ],
    ];

    for input in inputs {
        let expected = reference_permutation(input);
        assert_eq!(
            call(
                &mut store,
                &instance,
                "poseidon2_permute16",
                &input,
                WIDTH,
            ),
            expected,
            "Fe Poseidon2 differs from Plonky3 for {input:?}",
        );
    }
}
