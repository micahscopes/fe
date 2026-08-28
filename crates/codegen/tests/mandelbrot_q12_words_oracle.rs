//! Independent executable gate for the reusable Q12 Mandelbrot word stream.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;
use wasmtime::Val;

const SCALE: i64 = 4096;
const RADIUS_SQUARED_Q24: i64 = 67_108_864;

fn fixture_url() -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_q12_words_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "Q12 word-stream fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("Q12 word-stream fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected Q12 word-stream diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O2)
        .expect("Q12 word-stream fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("Q12 word-stream Wasm should validate");
    bytes
}

fn call(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    arguments: &[i32],
    result_count: usize,
) -> Vec<i32> {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("missing `{name}` export"));
    let params = arguments.iter().copied().map(Val::I32).collect::<Vec<_>>();
    let mut results = vec![Val::I32(0); result_count];
    function
        .call(&mut *store, &params, &mut results)
        .unwrap_or_else(|error| panic!("`{name}` should execute: {error:?}"));
    results
        .into_iter()
        .map(|value| match value {
            Val::I32(word) => word,
            other => panic!("`{name}` returned non-i32 lane {other:?}"),
        })
        .collect()
}

fn claim_is_valid(c_re: i32, c_im: i32, bound: u32) -> bool {
    (-8192..4096).contains(&c_re) && (-6144..6144).contains(&c_im) && bound <= 1_048_576
}

fn air_words(step: u32, z_re: i32, z_im: i32) -> [i32; 11] {
    let rr = i64::from(z_re) * i64::from(z_re);
    let ii = i64::from(z_im) * i64::from(z_im);
    let magnitude = rr + ii;
    let real_numerator = rr - ii;
    let imaginary_numerator = 2 * i64::from(z_re) * i64::from(z_im);
    let q_re = real_numerator >> 12;
    let q_im = imaginary_numerator >> 12;
    [
        step as i32,
        z_re,
        z_im,
        rr as i32,
        ii as i32,
        magnitude as i32,
        q_re as i32,
        (real_numerator - q_re * SCALE) as i32,
        q_im as i32,
        (imaginary_numerator - q_im * SCALE) as i32,
        i32::from(magnitude >= RADIUS_SQUARED_Q24),
    ]
}

fn stream_words(c_re: i32, c_im: i32, bound: u32, advances: u32) -> Vec<i32> {
    let valid = claim_is_valid(c_re, c_im, bound);
    let mut row = air_words(0, 0, 0);
    for _ in 0..advances {
        let terminal = row[10] == 1;
        if valid && !terminal && (row[0] as u32) < bound {
            row = air_words(row[0] as u32 + 1, row[6] + c_re, row[8] + c_im);
        }
    }
    let terminal = row[10] == 1;
    let can_advance = valid && !terminal && (row[0] as u32) < bound;
    let mut words = row.to_vec();
    words.extend([
        row[0],
        row[5],
        i32::from(valid),
        i32::from(valid) * row[10],
        i32::from(terminal),
        i32::from(can_advance),
    ]);
    words
}

#[test]
fn reusable_q12_words_match_independent_i64_recurrence() {
    let bytes = compile_wasm();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("Q12 word-stream module should load");
    assert!(
        module.imports().next().is_none(),
        "Q12 word-stream gate must remain zero-import",
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("Q12 word-stream module should instantiate");

    for (step, z_re, z_im) in [
        (0, 0, 0),
        (1, 3072, 0),
        (7, -4096, 2048),
        (19, 4095, -4095),
        (31, -8192, 0),
    ] {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "q12_air_words",
                &[step as i32, z_re, z_im],
                11,
            ),
            air_words(step, z_re, z_im),
            "Q12 AIR words differ at step {step}, z = ({z_re}, {z_im})",
        );
    }

    for (c_re, c_im, bound, advances) in [
        (3072, 0, 4, 0),
        (3072, 0, 4, 1),
        (3072, 0, 4, 8),
        (-3048, 2216, 255, 32),
        (0, 0, 3, 8),
        (-8192, -6144, 100, 2),
        (4096, 0, 100, 2),
        (0, 6144, 100, 2),
        (0, 0, 1_048_577, 2),
    ] {
        assert_eq!(
            call(
                &mut store,
                &instance,
                "q12_stream_words",
                &[c_re, c_im, bound as i32, advances as i32],
                17,
            ),
            stream_words(c_re, c_im, bound, advances),
            "Q12 stream differs for ({c_re}, {c_im}), bound {bound}, advances {advances}",
        );
    }
}
