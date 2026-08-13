//! Independent semantic oracle for the Fe QCGA pencil distance-estimator view.
//!
//! The production `quadric_distance` and iterative `trace_quadric_de` execute
//! under Wasmtime. Rust evaluates the polynomial/gradient separately and uses
//! exact ray/quadratic roots for visibility. Artifact bytes and duplicated
//! marching code are deliberately not correctness oracles.

use std::path::Path;

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use url::Url;
use wasmtime::Val;

type Q = [f32; 10];

fn compile_gate() -> Vec<u8> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/qcga_pencil_de_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let ingot = db.workspace().containing_ingot(&db, url).unwrap();
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "QCGA DE oracle diagnostics:\n{diagnostics}"
    );
    let wasm = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("QCGA DE oracle should compile to Wasm")
        .into_bytecode()
        .expect("Wasm bytecode");
    wasmparser::validate(&wasm).expect("QCGA DE oracle Wasm should validate");
    wasm
}

fn call_f32(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    args: &[f32],
) -> f32 {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("missing `{name}`"));
    let args = args
        .iter()
        .map(|value| Val::F32(value.to_bits()))
        .collect::<Vec<_>>();
    let mut result = [Val::F32(0)];
    function.call(&mut *store, &args, &mut result).unwrap();
    match result[0] {
        Val::F32(bits) => f32::from_bits(bits),
        ref value => panic!("`{name}` result must be f32, got {value:?}"),
    }
}

fn call_i32(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    args: &[f32],
) -> i32 {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("missing `{name}`"));
    let args = args
        .iter()
        .map(|value| Val::F32(value.to_bits()))
        .collect::<Vec<_>>();
    let mut result = [Val::I32(0)];
    function.call(&mut *store, &args, &mut result).unwrap();
    match result[0] {
        Val::I32(value) => value,
        ref value => panic!("`{name}` result must be i32, got {value:?}"),
    }
}

fn polynomial(q: Q, p: [f32; 3]) -> f32 {
    let [x, y, z] = p;
    q[0] * x * x
        + q[1] * y * y
        + q[2] * z * z
        + q[3] * x * y
        + q[4] * x * z
        + q[5] * y * z
        + q[6] * x
        + q[7] * y
        + q[8] * z
        + q[9]
}

fn gradient(q: Q, [x, y, z]: [f32; 3]) -> [f32; 3] {
    [
        2.0 * q[0] * x + q[3] * y + q[4] * z + q[6],
        2.0 * q[1] * y + q[3] * x + q[5] * z + q[7],
        2.0 * q[2] * z + q[4] * x + q[5] * y + q[8],
    ]
}

fn distance(q: Q, p: [f32; 3]) -> f32 {
    let g = gradient(q, p);
    polynomial(q, p).abs()
        / (g[0] * g[0] + g[1] * g[1] + g[2] * g[2])
            .sqrt()
            .max(0.00001)
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2])
        .sqrt()
        .max(0.0000001);
    [v[0] / length, v[1] / length, v[2] / length]
}

/// Independently substitute `origin + t * direction` into the quadric and
/// return the nearest positive analytic root.
fn analytic_hit(q: Q, origin: [f32; 3], direction: [f32; 3]) -> Option<f32> {
    let d = normalize(direction);
    let a = q[0] * d[0] * d[0]
        + q[1] * d[1] * d[1]
        + q[2] * d[2] * d[2]
        + q[3] * d[0] * d[1]
        + q[4] * d[0] * d[2]
        + q[5] * d[1] * d[2];
    let b = 2.0 * q[0] * origin[0] * d[0]
        + 2.0 * q[1] * origin[1] * d[1]
        + 2.0 * q[2] * origin[2] * d[2]
        + q[3] * (origin[0] * d[1] + origin[1] * d[0])
        + q[4] * (origin[0] * d[2] + origin[2] * d[0])
        + q[5] * (origin[1] * d[2] + origin[2] * d[1])
        + q[6] * d[0]
        + q[7] * d[1]
        + q[8] * d[2];
    let c = polynomial(q, origin);
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 || a.abs() < 1e-8 {
        return None;
    }
    let root = discriminant.sqrt();
    let near = (-b - root) / (2.0 * a);
    let far = (-b + root) / (2.0 * a);
    [near, far]
        .into_iter()
        .filter(|value| *value >= 0.0)
        .min_by(|a, b| a.total_cmp(b))
}

#[test]
fn production_qcga_de_matches_fields_and_independent_analytic_hits() {
    let wasm = compile_gate();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();

    let quadrics: [Q; 4] = [
        [1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0],
        [0.64, 1.44, 0.92, 0.0, 0.0, 0.0, -0.25, 0.18, -0.12, -1.0],
        [1.1, 0.82, 1.36, 0.24, -0.31, 0.19, 0.12, -0.08, 0.16, -1.2],
        [-3.7, -3.7, -3.7, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 3.7],
    ];
    let coordinates = [-2.1f32, -0.73, 0.0, 0.61, 1.9];
    let mut field_samples = 0;
    for q in quadrics {
        for &x in &coordinates {
            for &y in &coordinates {
                for &z in &coordinates {
                    let mut args = q.to_vec();
                    args.extend([x, y, z]);
                    let got = call_f32(&mut store, &instance, "de_distance", &args);
                    let want = distance(q, [x, y, z]);
                    assert!(got.is_finite() && want.is_finite());
                    assert!(
                        (got - want).abs() <= 0.00002,
                        "field {q:?} at ({x},{y},{z}): Fe {got} != Rust {want}",
                    );
                    field_samples += 1;
                }
            }
        }
    }

    let rays = [
        (quadrics[0], [0.0, 0.0, 4.0], [0.0, 0.0, -1.0]),
        (quadrics[0], [3.0, 0.0, 4.0], [0.0, 0.0, -1.0]),
        (quadrics[1], [0.3, 0.15, 4.5], [-0.04, -0.02, -1.0]),
        (quadrics[1], [3.2, 2.5, 4.0], [0.0, 0.0, -1.0]),
        (quadrics[2], [-0.8, 0.4, 4.8], [0.12, -0.05, -1.0]),
        (quadrics[2], [4.0, -3.0, 3.0], [0.0, 0.0, -1.0]),
        (quadrics[3], [0.0, 0.0, 4.0], [0.0, 0.0, -1.0]),
    ];
    let mut hits = 0;
    for (index, (q, origin, direction)) in rays.into_iter().enumerate() {
        let mut args = q.to_vec();
        args.extend(origin);
        args.extend(direction);
        let got_hit = call_i32(&mut store, &instance, "de_trace_hit", &args);
        let got_distance = call_f32(&mut store, &instance, "de_trace_distance", &args);
        match analytic_hit(q, origin, direction).filter(|distance| *distance <= 18.0) {
            Some(want) => {
                assert_eq!(got_hit, 1, "ray {index} should hit at {want}");
                assert!(
                    (got_distance - want).abs() <= 0.012,
                    "ray {index}: iterative Fe distance {got_distance} != analytic root {want}",
                );
                let d = normalize(direction);
                let point = [
                    origin[0] + d[0] * got_distance,
                    origin[1] + d[1] * got_distance,
                    origin[2] + d[2] * got_distance,
                ];
                assert!(
                    distance(q, point) <= 0.0045,
                    "ray {index} residual too large"
                );
                hits += 1;
            }
            None => assert_eq!(got_hit, 0, "ray {index} is an analytic miss"),
        }
    }

    for (width, height, march, refine) in [
        (128.0, 256.0, 64, 2),
        (256.0, 257.0, 96, 3),
        (384.0, 240.0, 96, 3),
        (385.0, 512.0, 128, 4),
    ] {
        assert_eq!(
            call_i32(
                &mut store,
                &instance,
                "de_quality_march_steps",
                &[width, height],
            ),
            march,
            "Fe must own the march tier for {width}x{height}",
        );
        assert_eq!(
            call_i32(
                &mut store,
                &instance,
                "de_quality_refinement_steps",
                &[width, height],
            ),
            refine,
            "Fe must own the refinement tier for {width}x{height}",
        );
    }

    // The mobile tier is not accepted merely because it performs less work:
    // execute the same rays through that exact production policy and retain
    // the independently derived analytic classification/root oracle.
    for (index, (q, origin, direction)) in rays.into_iter().enumerate() {
        let mut args = q.to_vec();
        args.extend(origin);
        args.extend(direction);
        args.extend([256.0, 256.0]);
        let got_hit = call_i32(&mut store, &instance, "de_quality_trace_hit", &args);
        let got_distance = call_f32(&mut store, &instance, "de_quality_trace_distance", &args);
        match analytic_hit(q, origin, direction).filter(|distance| *distance <= 18.0) {
            Some(want) => {
                assert_eq!(got_hit, 1, "mobile-tier ray {index} should hit at {want}");
                assert!(
                    (got_distance - want).abs() <= 0.02,
                    "mobile-tier ray {index}: iterative Fe distance {got_distance} != analytic root {want}",
                );
            }
            None => assert_eq!(got_hit, 0, "mobile-tier ray {index} is an analytic miss"),
        }
    }

    eprintln!(
        "QCGA DE oracle: {field_samples} independently evaluated fields + {} full/mobile analytic-root rays ({hits} hits) + Fe-owned quality tiers green",
        rays.len(),
    );
}
