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

#[derive(Debug, Clone, Copy)]
struct DeReceipt {
    distance: f32,
    hit: i32,
    steps: i32,
    residual: f32,
}

fn call_de_receipt(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    args: &[f32],
) -> DeReceipt {
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("missing `{name}`"));
    let args = args
        .iter()
        .map(|value| Val::F32(value.to_bits()))
        .collect::<Vec<_>>();
    let mut result = [Val::F32(0), Val::I32(0), Val::I32(0), Val::F32(0)];
    function.call(&mut *store, &args, &mut result).unwrap();
    match result {
        [
            Val::F32(distance),
            Val::I32(hit),
            Val::I32(steps),
            Val::F32(residual),
        ] => DeReceipt {
            distance: f32::from_bits(distance),
            hit,
            steps,
            residual: f32::from_bits(residual),
        },
        other => panic!("`{name}` result must be PencilDeHit, got {other:?}"),
    }
}

fn call_interval_named(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    args: &[f32],
) -> (f32, f32, i32) {
    let function = instance
        .get_func(&mut *store, name)
        .expect("scene interval export");
    let args = args
        .iter()
        .map(|value| Val::F32(value.to_bits()))
        .collect::<Vec<_>>();
    let mut result = [Val::F32(0), Val::F32(0), Val::I32(0)];
    function.call(&mut *store, &args, &mut result).unwrap();
    match result {
        [Val::F32(start), Val::F32(end), Val::I32(active)] => {
            (f32::from_bits(start), f32::from_bits(end), active)
        }
        other => panic!("scene interval result has wrong shape: {other:?}"),
    }
}

fn call_interval(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    args: &[f32],
) -> (f32, f32, i32) {
    call_interval_named(store, instance, "de_scene_interval", args)
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

/// Independent f64 broad-phase model for the production Fe sphere interval.
/// This is deliberately separate from the analytic quadric-root oracle below:
/// the interval only establishes where iteration is permitted.
fn sphere_interval(
    origin: [f32; 3],
    direction: [f32; 3],
    center: [f32; 3],
    radius: f64,
    max_distance: f64,
) -> (f64, f64, bool) {
    let direction = normalize(direction).map(f64::from);
    let offset =
        std::array::from_fn::<_, 3, _>(|axis| f64::from(origin[axis]) - f64::from(center[axis]));
    let half_b = offset
        .iter()
        .zip(direction)
        .map(|(left, right)| left * right)
        .sum::<f64>();
    let offset_squared = offset.iter().map(|value| value * value).sum::<f64>();
    let discriminant = half_b * half_b - (offset_squared - radius * radius);
    if discriminant < 0.0 {
        return (0.0, 0.0, false);
    }
    let root = discriminant.sqrt();
    let start = 0.0f64.max(-half_b - root);
    let end = max_distance.min(-half_b + root);
    (start, end, end > start)
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

    // Execute the exact production composition: the mobile work tier plus a
    // finite scene-domain interval. Rust derives the interval in f64 and the
    // quadric visibility from analytic roots; neither algorithm repeats Fe's
    // iterative distance-estimator loop.
    let center = [0.0f32, 0.0, 0.0];
    let mut full_steps = 0_i32;
    let mut clipped_steps = 0_i32;
    let mut rejected_intervals = 0;
    for (index, (q, origin, direction)) in rays.into_iter().enumerate() {
        let (want_start, want_end, want_active) =
            sphere_interval(origin, direction, center, 3.0, 18.0);
        let mut interval_args = origin.to_vec();
        interval_args.extend(direction);
        interval_args.extend(center);
        let (got_start, got_end, got_active) = call_interval(&mut store, &instance, &interval_args);
        assert_eq!(
            got_active == 1,
            want_active,
            "ray {index} interval activity"
        );
        if want_active {
            assert!(
                (f64::from(got_start) - want_start).abs() <= 0.00002
                    && (f64::from(got_end) - want_end).abs() <= 0.00002,
                "ray {index} Fe interval [{got_start}, {got_end}] != independent [{want_start}, {want_end}]",
            );
        } else {
            assert!(got_start.is_finite() && got_end.is_finite());
        }

        let mut full_args = q.to_vec();
        full_args.extend(origin);
        full_args.extend(direction);
        full_steps += call_i32(&mut store, &instance, "de_trace_steps", &full_args);

        let mut clipped_args = full_args;
        clipped_args.extend(center);
        clipped_args.extend([256.0, 256.0]);
        let receipt = call_de_receipt(
            &mut store,
            &instance,
            "de_clipped_quality_trace",
            &clipped_args,
        );
        assert!(receipt.steps >= 0 && receipt.steps <= 64);
        assert!(receipt.residual.is_finite());
        clipped_steps += receipt.steps;
        if !want_active {
            rejected_intervals += 1;
            assert_eq!(receipt.hit, 0, "ray {index} rejected domain cannot hit");
            assert_eq!(receipt.steps, 0, "ray {index} must skip the march entirely");
            continue;
        }

        let want_hit = analytic_hit(q, origin, direction).filter(|distance| {
            let distance = f64::from(*distance);
            distance >= want_start && distance <= want_end
        });
        match want_hit {
            Some(want) => {
                assert_eq!(receipt.hit, 1, "clipped ray {index} should hit at {want}");
                assert!(
                    (receipt.distance - want).abs() <= 0.02,
                    "clipped ray {index}: iterative Fe distance {} != analytic root {want}",
                    receipt.distance,
                );
            }
            None => assert_eq!(receipt.hit, 0, "clipped ray {index} is an analytic miss"),
        }
    }
    assert!(
        rejected_intervals >= 2,
        "fixture must exercise whole-loop rejection"
    );
    assert!(
        clipped_steps < full_steps,
        "scene clipping must reduce measured production iterations: clipped={clipped_steps}, full={full_steps}",
    );

    // The shader uses a cheaper form of the same sphere equation because its
    // resident PencilCamera already proves origin = center - forward * dist.
    // Exercise that exact exported decision independently across different
    // distances, orientations, hits, and whole-loop rejects.
    let camera_cases = [
        ([0.0, 0.0, 1.0], 4.0, [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 1.0], 4.0, [0.5, 0.0, 1.0]),
        ([0.0, 0.0, 1.0], 12.0, [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 1.0], 12.0, [0.5, 0.0, 1.0]),
        ([1.0, 0.0, 0.0], 8.0, [1.0, 0.2, 0.0]),
        ([1.0, 0.0, 0.0], 12.0, [1.0, 0.6, 0.0]),
    ];
    let mut camera_rejects = 0;
    for (index, (forward, camera_distance, direction)) in camera_cases.into_iter().enumerate() {
        let forward = normalize(forward);
        let direction = normalize(direction);
        let origin = forward.map(|component| -component * camera_distance);
        let (want_start, want_end, want_active) =
            sphere_interval(origin, direction, center, 3.0, 18.0);
        let mut args = direction.to_vec();
        args.extend(forward);
        args.push(camera_distance);
        let (got_start, got_end, got_active) =
            call_interval_named(&mut store, &instance, "de_camera_scene_interval", &args);
        assert_eq!(
            got_active == 1,
            want_active,
            "prepared-camera case {index} interval activity",
        );
        if want_active {
            assert!(
                (f64::from(got_start) - want_start).abs() <= 0.00002
                    && (f64::from(got_end) - want_end).abs() <= 0.00002,
                "prepared-camera case {index}: Fe [{got_start}, {got_end}] != independent [{want_start}, {want_end}]",
            );
        } else {
            camera_rejects += 1;
            assert!(got_start.is_finite() && got_end.is_finite());
        }
    }
    assert!(
        camera_rejects >= 2,
        "prepared-camera fixture must cover whole-loop rejects",
    );

    eprintln!(
        "QCGA DE oracle: {field_samples} independently evaluated fields + {} full/mobile/clipped analytic-root rays ({hits} full hits); clipped steps {clipped_steps} < {full_steps}, {rejected_intervals} generic + {camera_rejects} prepared-camera whole-loop skips; Fe-owned quality tiers green",
        rays.len(),
    );
}
