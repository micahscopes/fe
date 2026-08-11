//! Independent semantic oracle for the canonical Fe 3D distance-field demo.
//!
//! The real `raymarch::{scene_distance, trace_ray_distance}` functions compile
//! to Wasm and execute under Wasmtime. A separately written scalar Rust model
//! evaluates a directed 3D grid and camera rays. Comparisons are numerical and
//! semantic; generated source or artifact bytes are never a correctness oracle.

use std::path::Path;

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use url::Url;

const MAX_MARCH_STEPS: usize = 112;
const FAR_CLIP: f32 = 18.0;
const HIT_EPSILON: f32 = 0.0015;

fn compile_gate() -> Vec<u8> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raymarch_de_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let ingot = db.workspace().containing_ingot(&db, url).unwrap();
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "distance oracle diagnostics:\n{diagnostics}"
    );
    let wasm = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("distance oracle should compile to Wasm")
        .into_bytecode()
        .expect("Wasm bytecode");
    wasmparser::validate(&wasm).expect("distance oracle Wasm should validate");
    wasm
}

fn smooth_union(a: f32, b: f32, radius: f32) -> f32 {
    let blend = (0.5 + 0.5 * (b - a) / radius).clamp(0.0, 1.0);
    b + (a - b) * blend - radius * blend * (1.0 - blend)
}

fn sphere(x: f32, y: f32, z: f32, radius: f32) -> f32 {
    (x * x + y * y + z * z).sqrt() - radius
}

fn reference_distance(x: f32, y: f32, z: f32, morph: f32) -> f32 {
    let major = 0.72 + 0.18 * morph;
    let tube = 0.105 + 0.055 * (1.0 - morph);
    let ring_y = (((x * x + z * z).sqrt() - major).powi(2) + y * y).sqrt() - tube;
    let ring_x = (((y * y + z * z).sqrt() - major).powi(2) + x * x).sqrt() - tube;
    let ring_z = (((x * x + y * y).sqrt() - major).powi(2) + z * z).sqrt() - tube;
    let rings = smooth_union(smooth_union(ring_x, ring_y, 0.13), ring_z, 0.13);
    let core = sphere(x, y, z, 0.28 + 0.11 * morph);
    let body = smooth_union(rings, core, 0.18);
    let satellite_radius = 0.13 + 0.035 * morph;
    let satellite_a = sphere(x - 1.12, y - 0.16, z + 0.18, satellite_radius);
    let satellite_b = sphere(x + 1.12, y + 0.12, z - 0.18, satellite_radius);
    let sculpture = smooth_union(smooth_union(body, satellite_a, 0.11), satellite_b, 0.11);
    sculpture.min(y + 1.22)
}

fn normalize(direction: [f32; 3]) -> [f32; 3] {
    let length =
        (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2])
            .sqrt()
            .max(0.0000001);
    [
        direction[0] / length,
        direction[1] / length,
        direction[2] / length,
    ]
}

fn reference_trace(origin: [f32; 3], direction: [f32; 3], morph: f32) -> f32 {
    let direction = normalize(direction);
    let mut distance = 0.0f32;
    let mut done = false;
    for _ in 0..MAX_MARCH_STEPS {
        if !done {
            let x = origin[0] + direction[0] * distance;
            let y = origin[1] + direction[1] * distance;
            let z = origin[2] + direction[2] * distance;
            let estimate = reference_distance(x, y, z, morph);
            let epsilon = HIT_EPSILON * (1.0 + distance * 0.035);
            if estimate < epsilon {
                done = true;
            } else {
                distance += estimate * 0.78;
                if distance > FAR_CLIP {
                    done = true;
                }
            }
        }
    }
    distance
}

fn call_f32(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    name: &str,
    args: &[f32],
) -> f32 {
    use wasmtime::Val;
    let function = instance
        .get_func(&mut *store, name)
        .unwrap_or_else(|| panic!("missing `{name}`"));
    let values = args
        .iter()
        .copied()
        .map(|value| Val::F32(value.to_bits()))
        .collect::<Vec<_>>();
    let mut result = [Val::F32(0)];
    function.call(&mut *store, &values, &mut result).unwrap();
    match result[0] {
        Val::F32(bits) => f32::from_bits(bits),
        ref value => panic!("`{name}` result must be f32, got {value:?}"),
    }
}

fn assert_close(got: f32, want: f32, tolerance: f32, context: &str) {
    assert!(
        got.is_finite() && want.is_finite(),
        "{context}: non-finite result"
    );
    assert!(
        (got - want).abs() <= tolerance,
        "{context}: Fe/Wasm {got:.8} != independent Rust {want:.8}"
    );
}

#[test]
fn production_distance_and_ray_march_match_independent_geometry() {
    let wasm = compile_gate();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();

    let coordinates = [-1.75f32, -1.1, -0.52, 0.0, 0.47, 1.08, 1.8];
    let morphs = [0.0f32, 0.58, 1.0];
    let mut samples = 0usize;
    for morph in morphs {
        for &x in &coordinates {
            for &y in &coordinates {
                for &z in &coordinates {
                    let got = call_f32(&mut store, &instance, "de_distance", &[x, y, z, morph]);
                    let want = reference_distance(x, y, z, morph);
                    assert_close(
                        got,
                        want,
                        0.00002,
                        &format!("distance({x},{y},{z}; {morph})"),
                    );
                    samples += 1;
                }
            }
        }
    }
    assert!(call_f32(&mut store, &instance, "de_distance", &[0.0, 0.0, 0.0, 0.58]) < 0.0);
    assert_close(
        call_f32(
            &mut store,
            &instance,
            "de_distance",
            &[8.0, -1.22, 8.0, 0.58],
        ),
        0.0,
        0.000001,
        "far floor surface",
    );

    let rays = [
        ([0.0, 0.6, 4.6], [0.0, -0.12, -1.0], 0.58),
        ([3.8, 1.4, 2.8], [-1.0, -0.34, -0.74], 0.25),
        ([-4.1, 0.9, 2.2], [1.0, -0.18, -0.54], 0.82),
        ([0.0, 3.8, 5.0], [0.0, 0.45, -1.0], 0.58),
    ];
    for (index, (origin, direction, morph)) in rays.into_iter().enumerate() {
        let got = call_f32(
            &mut store,
            &instance,
            "de_trace",
            &[
                origin[0],
                origin[1],
                origin[2],
                direction[0],
                direction[1],
                direction[2],
                morph,
            ],
        );
        let want = reference_trace(origin, direction, morph);
        assert_close(got, want, 0.0002, &format!("ray {index}"));
    }
    eprintln!(
        "raymarch DE oracle: {samples} independently modeled field samples + {} rays green",
        rays.len(),
    );
}
