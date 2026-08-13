//! Independent acceptance suite for the `qcga_pencil` sketch, included by
//! `crates/codegen/tests/qcga_pencil_sketch.rs` as an ordinary workspace gate.
//! It intentionally lives outside the publishable demo ingot: the gallery's
//! provenance ledger must describe authored Fe, not its independent Rust
//! acceptance oracle.
//!
//! What it verifies (all Fe compiled to zero-import wasm, run under wasmtime;
//! no GPU work happens here and no GPU performance claim is made anywhere):
//!
//! 1. The sketch ingot analyzes clean.
//! 2. `defaults` serves nine points on the sphere-cone intersection curve.
//! 3. `solve` finds rank 8 there: the pencil survives; the independent f64
//!    oracle confirms both basis surfaces (and by exact linearity every
//!    member) pass through all nine points, and that the cylinder and the
//!    plane pair lie in the recovered pencil plane.
//! 4. NEGATIVE: nudging one point off the curve snaps rank to 9, degenerates
//!    the basis, and the old pencil demonstrably fails at the moved point.
//! 5. The vertex behavior projects onto the member (independent oracle at the
//!    emitted points), reports honest receipts, stays on its radial ray, and
//!    the oracle tolerance provably bites (a wrong point fails it).
//! 6. Topology: the sphere covers sheet 0 at t = 1 exactly and collapses
//!    sheet 1; the hyperboloid tears exactly along its asymptotic cone;
//!    NEGATIVE: an empty quadric emits no geometry at all.
//! 7. `mesh_oracle` and `probe` agree bit for bit; stream length pins
//!    VERTEX_COUNT.
//! 8. The canonical DE actor owns initialization, typed interaction, bounded
//!    arena reuse, and the only QCGA Pencil GPU entry after raster retirement.

use std::path::Path;

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    CanonicalExecution, CanonicalFieldLayout, CanonicalInterfaceManifest, CanonicalLane,
    CanonicalLayout, CanonicalShape, WasmCompileOptions, WebActorStageKind, WebBuildOptions,
    WebBundle, WebBundleMode, actor_gpu_program, actor_web_entry, canonical_lane_decl_from_entry,
    compile_runtime_package_wasm_with_options,
};
use hir::hir_def::{HirIngot, TopLevelMod};
use url::Url;

// Mesh constants mirrored from the sketch; `mesh_oracle_stream_matches_probe_lane`
// cross-checks them against the actual byte stream length.
const ROWS: u32 = 24;
const COLS: u32 = 48;
const SHEET_VERTS: u32 = ROWS * COLS * 6;
const VERTEX_COUNT: u32 = 2 * SHEET_VERTS;
const T_MAX: f64 = 12.0;

fn with_sketch<R>(run: impl FnOnce(&DriverDataBase, TopLevelMod) -> R) -> R {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demos/sketches/qcga_pencil");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "qcga_pencil ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("qcga_pencil ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "qcga_pencil sketch has diagnostics:\n{diagnostics}"
    );
    run(&db, top_mod)
}

fn with_de<R>(run: impl FnOnce(&DriverDataBase, TopLevelMod) -> R) -> R {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demos/sketches/qcga_pencil_de");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "qcga_pencil_de ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("qcga_pencil_de ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "qcga_pencil_de sketch has diagnostics:\n{diagnostics}"
    );
    run(&db, top_mod)
}

#[derive(Clone, Copy)]
enum V {
    F(f32),
    U(u32),
}

struct Lane {
    lane: CanonicalLane,
    store: wasmtime::Store<()>,
    memory: wasmtime::Memory,
    alloc: wasmtime::TypedFunc<(i32, i32), i32>,
    reset: wasmtime::TypedFunc<(), ()>,
    entry: wasmtime::TypedFunc<i32, i32>,
}

fn record_fields(layout: &CanonicalLayout) -> &[CanonicalFieldLayout] {
    match &layout.shape {
        CanonicalShape::Record { fields } => fields,
        other => panic!("expected record layout, found {other:?}"),
    }
}

fn uniquely_named_leaf_offset(layout: &CanonicalLayout, name: &str) -> Option<u32> {
    fn collect(layout: &CanonicalLayout, base: u32, name: &str, found: &mut Vec<u32>) {
        let CanonicalShape::Record { fields } = &layout.shape else {
            return;
        };
        for field in fields {
            let offset = base + field.offset;
            match &field.layout.shape {
                CanonicalShape::Record { .. } => collect(&field.layout, offset, name, found),
                _ if field.name == name => found.push(offset),
                _ => {}
            }
        }
    }

    let mut found = Vec::new();
    collect(layout, 0, name, &mut found);
    match found.as_slice() {
        [offset] => Some(*offset),
        [] => None,
        _ => panic!("canonical request has more than one leaf named `{name}`"),
    }
}

fn compile_lane(db: &DriverDataBase, top_mod: TopLevelMod, name: &str) -> Lane {
    let decl = canonical_lane_decl_from_entry(db, top_mod, name, name)
        .unwrap_or_else(|error| panic!("lane decl for `{name}`: {error}"));
    let manifest = CanonicalInterfaceManifest::build(vec![decl])
        .unwrap_or_else(|error| panic!("manifest for `{name}`: {error}"));
    let lane = manifest.lanes[0].clone();
    let package = mir::build_wasm_runtime_package_for_entry(db, top_mod, name)
        .unwrap_or_else(|error| panic!("runtime package for `{name}`: {error}"));
    let wasm = compile_runtime_package_wasm_with_options(
        db,
        &package,
        WasmCompileOptions::default().with_canonical_lane(lane.clone()),
    )
    .unwrap_or_else(|error| panic!("wasm for `{name}`: {error:?}"))
    .bytes;
    wasmparser::validate(&wasm).expect("lane wasm validates");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(
        module.imports().next().is_none(),
        "lane `{name}` wasm must be zero-import"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    let alloc = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .unwrap();
    let reset = instance
        .get_typed_func::<(), ()>(&mut store, "fe_cabi_reset")
        .unwrap();
    let entry = instance
        .get_typed_func::<i32, i32>(&mut store, &format!("fe_cabi_{name}"))
        .unwrap();
    Lane {
        lane,
        store,
        memory,
        alloc,
        reset,
        entry,
    }
}

impl Lane {
    fn call(&mut self, values: &[(String, V)]) -> Vec<u8> {
        self.reset.call(&mut self.store, ()).unwrap();
        let size = self.lane.request.size.max(4);
        let align = self.lane.request.align.max(4);
        let ptr = self
            .alloc
            .call(&mut self.store, (size as i32, align as i32))
            .unwrap();
        let mut bytes = vec![0_u8; size as usize];
        for (name, value) in values {
            let offset = uniquely_named_leaf_offset(&self.lane.request, name).unwrap_or_else(|| {
                panic!("lane `{}` has no request field `{name}`", self.lane.name)
            }) as usize;
            let word = match value {
                V::F(x) => x.to_bits(),
                V::U(x) => *x,
            };
            bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        self.memory
            .write(&mut self.store, ptr as usize, &bytes)
            .unwrap();
        let response = self.entry.call(&mut self.store, ptr).unwrap();
        let mut out = vec![0_u8; self.lane.response.size as usize];
        self.memory
            .read(&self.store, response as usize, &mut out)
            .unwrap();
        out
    }

    /// For `Bytes`-shaped responses: dereference the descriptor into the
    /// payload.
    fn bytes_payload(&mut self, response: &[u8]) -> Vec<u8> {
        let CanonicalShape::Bytes {
            pointer_offset,
            length_offset,
        } = self.lane.response.shape
        else {
            panic!("lane `{}` response is not bytes-shaped", self.lane.name)
        };
        let word = |bytes: &[u8], offset: u32| {
            u32::from_le_bytes(
                bytes[offset as usize..offset as usize + 4]
                    .try_into()
                    .unwrap(),
            )
        };
        let ptr = word(response, pointer_offset);
        let len = word(response, length_offset);
        let mut payload = vec![0_u8; len as usize];
        self.memory
            .read(&self.store, ptr as usize, &mut payload)
            .unwrap();
        payload
    }
}

fn resp_f32(lane: &Lane, bytes: &[u8], name: &str) -> f32 {
    let field = record_fields(&lane.lane.response)
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("lane `{}` has no response field `{name}`", lane.lane.name));
    let offset = field.offset as usize;
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn resp_u32(lane: &Lane, bytes: &[u8], name: &str) -> u32 {
    let field = record_fields(&lane.lane.response)
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("lane `{}` has no response field `{name}`", lane.lane.name));
    let offset = field.offset as usize;
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

const COEFF_NAMES: [&str; 10] = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];

fn read_quadric(lane: &Lane, bytes: &[u8], suffix: &str) -> [f32; 10] {
    let mut out = [0.0_f32; 10];
    for (slot, name) in COEFF_NAMES.iter().enumerate() {
        out[slot] = resp_f32(lane, bytes, &format!("{name}{suffix}"));
    }
    out
}

/// Independent oracle: the quadric polynomial in f64, canonical coefficient
/// order a..j, sharing nothing with the Fe planned contraction.
fn oracle(q: &[f32; 10], p: [f64; 3]) -> f64 {
    let q: Vec<f64> = q.iter().map(|&v| v as f64).collect();
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

/// Rust-side mirror of the sketch's vid decode, for direction-level
/// assertions. fmath's polynomial trig differs from libm by about 1e-3, so
/// every comparison built on this uses tolerances well above that.
fn direction(vid: u32) -> ([f64; 3], u32) {
    let sheet = vid / SHEET_VERTS;
    let rem = vid % SHEET_VERTS;
    let quad = rem / 6;
    let corner = rem % 6;
    let band = quad / COLS;
    let col = quad % COLS;
    let (dr, dc) = [(0, 0), (1, 0), (0, 1), (0, 1), (1, 0), (1, 1)][corner as usize];
    let theta = std::f64::consts::PI * (band + dr) as f64 / ROWS as f64;
    let phi = std::f64::consts::TAU * (col + dc) as f64 / COLS as f64;
    (
        [
            theta.sin() * phi.cos(),
            theta.cos(),
            theta.sin() * phi.sin(),
        ],
        sheet,
    )
}

fn point_field_names() -> Vec<String> {
    let mut names = Vec::new();
    for index in 0..9 {
        for axis in ["x", "y", "z"] {
            names.push(format!("p{index}{axis}"));
        }
    }
    names
}

fn solve_request(points: &[f32; 27], generation: u32) -> Vec<(String, V)> {
    let mut values = vec![("generation".to_string(), V::U(generation))];
    for (name, value) in point_field_names().into_iter().zip(points.iter()) {
        values.push((name, V::F(*value)));
    }
    values
}

/// A probe/mesh-oracle frame with an identity-ish camera: yaw = pitch = 0
/// (fmath sin/cos are within 2e-7 of exact at 0), centre and dolly explicit.
fn frame_values(
    generation: u32,
    lambda: f32,
    center: [f32; 3],
    dist: f32,
    pencil_free: f32,
    q0: &[f32; 10],
    q1: &[f32; 10],
) -> Vec<(String, V)> {
    let mut values = vec![
        ("generation".to_string(), V::U(generation)),
        ("lambda".to_string(), V::F(lambda)),
        ("yaw".to_string(), V::F(0.0)),
        ("pitch".to_string(), V::F(0.0)),
        ("dist".to_string(), V::F(dist)),
        ("cx".to_string(), V::F(center[0])),
        ("cy".to_string(), V::F(center[1])),
        ("cz".to_string(), V::F(center[2])),
        ("pencil_free".to_string(), V::F(pencil_free)),
    ];
    for (slot, name) in COEFF_NAMES.iter().enumerate() {
        values.push((format!("{name}0"), V::F(q0[slot])));
        values.push((format!("{name}1"), V::F(q1[slot])));
    }
    values
}

fn probe_vertex(
    lane: &mut Lane,
    vid: u32,
    lambda: f32,
    center: [f32; 3],
    q0: &[f32; 10],
    q1: &[f32; 10],
) -> Vec<u8> {
    let mut values = frame_values(7, lambda, center, 4.0, 0.0, q0, q1);
    values.push(("vid".to_string(), V::U(vid)));
    lane.call(&values)
}

const SPHERE: [f32; 10] = [1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0];
const HYPERBOLOID: [f32; 10] = [1.0, 1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0];
const EMPTY: [f32; 10] = [1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];

#[test]
fn defaults_lane_serves_points_on_sphere_and_cone() {
    with_sketch(|db, top_mod| {
        let mut defaults = compile_lane(db, top_mod, "defaults");
        let response = defaults.call(&[("generation".to_string(), V::U(3))]);
        assert_eq!(resp_u32(&defaults, &response, "generation"), 3);
        for index in 0..9 {
            let x = resp_f32(&defaults, &response, &format!("p{index}x")) as f64;
            let y = resp_f32(&defaults, &response, &format!("p{index}y")) as f64;
            let z = resp_f32(&defaults, &response, &format!("p{index}z")) as f64;
            let sphere = x * x + y * y + z * z - 1.0;
            let cone = z * z - x * x - y * y;
            assert!(
                sphere.abs() < 1e-5 && cone.abs() < 1e-5,
                "default point {index} must lie on the sphere-cone intersection \
                 (sphere {sphere:+.2e}, cone {cone:+.2e})"
            );
        }
    });
}

fn read_default_points(db: &DriverDataBase, top_mod: TopLevelMod) -> [f32; 27] {
    let mut defaults = compile_lane(db, top_mod, "defaults");
    let response = defaults.call(&[("generation".to_string(), V::U(1))]);
    let mut points = [0.0_f32; 27];
    for (slot, name) in point_field_names().into_iter().enumerate() {
        points[slot] = resp_f32(&defaults, &response, &name);
    }
    points
}

#[test]
fn nine_conspiring_points_leave_a_pencil() {
    with_sketch(|db, top_mod| {
        let points = read_default_points(db, top_mod);
        let mut solve = compile_lane(db, top_mod, "solve");
        let response = solve.call(&solve_request(&points, 11));
        assert_eq!(resp_u32(&solve, &response, "generation"), 11);
        let rank = resp_u32(&solve, &response, "rank");
        let residual = resp_f32(&solve, &response, "residual");
        assert_eq!(
            rank, 8,
            "nine points on the sphere-cone curve must impose only eight conditions"
        );
        assert!(residual < 1e-4, "solver receipt too large: {residual:e}");
        let q0 = read_quadric(&solve, &response, "0");
        let q1 = read_quadric(&solve, &response, "1");

        // Basis sanity: unit length, orthogonal.
        let dot = |u: &[f32; 10], v: &[f32; 10]| -> f64 {
            u.iter().zip(v).map(|(&a, &b)| a as f64 * b as f64).sum()
        };
        assert!((dot(&q0, &q0) - 1.0).abs() < 1e-4, "q0 must be unit");
        assert!((dot(&q1, &q1) - 1.0).abs() < 1e-4, "q1 must be unit");
        assert!(
            dot(&q0, &q1).abs() < 1e-3,
            "pencil basis must be orthogonal"
        );

        // Independent oracle: BOTH basis surfaces vanish at all nine points,
        // therefore (exact linearity, computed in f64) EVERY pencil member
        // does. This is the linear-incidence selling point, mechanically.
        for index in 0..9 {
            let p = [
                points[index * 3] as f64,
                points[index * 3 + 1] as f64,
                points[index * 3 + 2] as f64,
            ];
            let v0 = oracle(&q0, p);
            let v1 = oracle(&q1, p);
            assert!(
                v0.abs() < 1e-4 && v1.abs() < 1e-4,
                "point {index} must lie on both basis quadrics (q0 {v0:+.2e}, q1 {v1:+.2e})"
            );
        }

        // The recovered pencil plane must contain the two constructions the
        // configuration was built from: cylinder x^2+y^2-1/2 and plane pair
        // z^2-1/2. Membership = residue after projecting onto the basis.
        for (name, target) in [
            (
                "cylinder",
                [1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -0.5],
            ),
            (
                "plane pair",
                [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -0.5],
            ),
        ] {
            let target: [f32; 10] = target;
            let norm = dot(&target, &target).sqrt();
            let c0 = dot(&target, &q0) / norm;
            let c1 = dot(&target, &q1) / norm;
            let mut rest = 0.0_f64;
            for slot in 0..10 {
                let projected = c0 * q0[slot] as f64 + c1 * q1[slot] as f64;
                let residue = target[slot] as f64 / norm - projected;
                rest += residue * residue;
            }
            assert!(
                rest.sqrt() < 1e-3,
                "{name} must lie in the recovered pencil plane (residue {:e})",
                rest.sqrt()
            );
        }
    });
}

#[test]
fn perturbing_one_point_snaps_the_pencil_shut() {
    with_sketch(|db, top_mod| {
        let points = read_default_points(db, top_mod);
        let mut solve = compile_lane(db, top_mod, "solve");

        // Baseline: the conspiring configuration and its pencil basis.
        let free = solve.call(&solve_request(&points, 1));
        assert_eq!(resp_u32(&solve, &free, "rank"), 8);
        let q0 = read_quadric(&solve, &free, "0");
        let q1 = read_quadric(&solve, &free, "1");

        // Nudge the ninth point off the intersection curve.
        let mut perturbed = points;
        perturbed[24] += 0.08;
        let snapped = solve.call(&solve_request(&perturbed, 2));
        let rank = resp_u32(&solve, &snapped, "rank");
        assert_eq!(
            rank, 9,
            "one point off the curve must restore general position"
        );
        let unique0 = read_quadric(&solve, &snapped, "0");
        let unique1 = read_quadric(&solve, &snapped, "1");
        assert_eq!(
            unique0.map(f32::to_bits),
            unique1.map(f32::to_bits),
            "at rank 9 the solver must report the pencil as degenerate (q1 == q0)"
        );
        let residual = resp_f32(&solve, &snapped, "residual");
        assert!(
            residual < 1e-4,
            "unique-quadric receipt too large: {residual:e}"
        );
        for index in 0..9 {
            let p = [
                perturbed[index * 3] as f64,
                perturbed[index * 3 + 1] as f64,
                perturbed[index * 3 + 2] as f64,
            ];
            let v = oracle(&unique0, p);
            assert!(
                v.abs() < 1e-4,
                "perturbed point {index} must lie on the unique quadric ({v:+.2e})"
            );
        }

        // NEGATIVE: the old freedom is really gone. At most one member of the
        // original pencil can pass through the moved point, so the maximum
        // incidence magnitude over a lambda grid must be decisively nonzero.
        let moved = [
            perturbed[24] as f64,
            perturbed[25] as f64,
            perturbed[26] as f64,
        ];
        let mut worst = 0.0_f64;
        for step in 0..10 {
            let lambda = step as f64 / 10.0;
            let (w0, w1) = (
                (std::f64::consts::PI * lambda).cos(),
                (std::f64::consts::PI * lambda).sin(),
            );
            let member = oracle(&q0, moved) * w0 + oracle(&q1, moved) * w1;
            worst = worst.max(member.abs());
        }
        assert!(
            worst > 1e-3,
            "the original pencil must fail at the moved point (worst {worst:e})"
        );
    });
}

#[test]
fn vertex_behavior_projects_onto_the_member_and_reports_receipts() {
    with_sketch(|db, top_mod| {
        let points = read_default_points(db, top_mod);
        let mut solve = compile_lane(db, top_mod, "solve");
        let solved = solve.call(&solve_request(&points, 1));
        let q0 = read_quadric(&solve, &solved, "0");
        let q1 = read_quadric(&solve, &solved, "1");
        let center = [
            resp_f32(&solve, &solved, "cx"),
            resp_f32(&solve, &solved, "cy"),
            resp_f32(&solve, &solved, "cz"),
        ];

        let mut probe = compile_lane(db, top_mod, "probe");
        let mut hits = 0_u32;
        let mut misses = 0_u32;
        let mut checked_negative = false;
        // lambda 0 and 0.5 hit the basis surfaces themselves (fmath trig is
        // within 2e-7 of exact there), so the independent oracle applies at
        // full strength.
        for lambda in [0.0_f32, 0.5] {
            let active = if lambda == 0.0 { &q0 } else { &q1 };
            for vid in (0..VERTEX_COUNT).step_by(173) {
                let response = probe_vertex(&mut probe, vid, lambda, center, &q0, &q1);
                let missed = resp_u32(&probe, &response, "missed");
                if missed == 1 {
                    // The miss sentinel is exact and always clipped.
                    assert_eq!(
                        resp_f32(&probe, &response, "x").to_bits(),
                        0.0_f32.to_bits()
                    );
                    assert_eq!(
                        resp_f32(&probe, &response, "y").to_bits(),
                        0.0_f32.to_bits()
                    );
                    assert_eq!(
                        resp_f32(&probe, &response, "z").to_bits(),
                        2.0_f32.to_bits()
                    );
                    assert_eq!(
                        resp_f32(&probe, &response, "w").to_bits(),
                        1.0_f32.to_bits()
                    );
                    misses += 1;
                    continue;
                }
                hits += 1;
                let t = resp_f32(&probe, &response, "t") as f64;
                let h = [
                    resp_f32(&probe, &response, "hx") as f64,
                    resp_f32(&probe, &response, "hy") as f64,
                    resp_f32(&probe, &response, "hz") as f64,
                ];
                let w = resp_f32(&probe, &response, "w") as f64;
                let z = resp_f32(&probe, &response, "z") as f64;
                assert!(t > 0.0 && t <= T_MAX, "hit parameter out of range: {t}");
                assert!(
                    w > 0.0 && z >= 0.0 && z <= w,
                    "clip depth invariant: z {z}, w {w}"
                );

                // The emitted gradient is unit length and the fragment color
                // for this vertex's handoff is opaque.
                let n = [
                    resp_f32(&probe, &response, "nx") as f64,
                    resp_f32(&probe, &response, "ny") as f64,
                    resp_f32(&probe, &response, "nz") as f64,
                ];
                let nlen = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                assert!(
                    (nlen - 1.0).abs() < 1e-3,
                    "vid {vid}: vertex normal must be unit (|n| {nlen})"
                );
                let rgba = resp_u32(&probe, &response, "rgba");
                assert_eq!(rgba >> 24, 0xff, "vid {vid}: fragment alpha must be opaque");

                // Independent constraint check at the emitted point, with an
                // f32-cancellation budget that grows with the hit distance.
                let tolerance = 3e-3 * (1.0 + t * t);
                let value = oracle(active, h);
                assert!(
                    value.abs() < tolerance,
                    "vid {vid} lambda {lambda}: emitted point off the surface \
                     ({value:+.3e}, budget {tolerance:.1e})"
                );

                // The behavior's own receipt agrees and the magenta
                // self-accusation never fires on an honest frame.
                let receipt = resp_f32(&probe, &response, "residual") as f64;
                assert!(
                    receipt.abs() < 0.004 * (1.0 + t * t),
                    "vid {vid}: in-shader incidence receipt tripped ({receipt:+.3e})"
                );
                assert_eq!(
                    resp_f32(&probe, &response, "accuse").to_bits(),
                    0.0_f32.to_bits(),
                    "vid {vid}: self-accusation fired on an honest frame"
                );

                // The hit lies on this vertex's own radial ray.
                let (dir, _) = direction(vid);
                let rel = [
                    h[0] - center[0] as f64,
                    h[1] - center[1] as f64,
                    h[2] - center[2] as f64,
                ];
                let cross = [
                    rel[1] * dir[2] - rel[2] * dir[1],
                    rel[2] * dir[0] - rel[0] * dir[2],
                    rel[0] * dir[1] - rel[1] * dir[0],
                ];
                let cross_norm = (cross[0].powi(2) + cross[1].powi(2) + cross[2].powi(2)).sqrt();
                assert!(
                    cross_norm < 4e-3 * t.max(1.0),
                    "vid {vid}: hit off its radial ray (cross norm {cross_norm:e})"
                );

                // Test of the test: a deliberately wrong point must FAIL the
                // same oracle check, once, so the tolerance provably bites.
                if !checked_negative && t < 3.0 {
                    let wrong = [h[0] + 0.1, h[1], h[2]];
                    let off = oracle(active, wrong);
                    assert!(
                        off.abs() > tolerance,
                        "oracle tolerance does not bite: wrong point passed ({off:+.3e})"
                    );
                    checked_negative = true;
                }
            }
        }
        assert!(
            hits > 20,
            "the sweep must actually hit the surface (hits {hits})"
        );
        assert!(
            misses > 0,
            "the two-sheet template must also miss (misses {misses})"
        );
        assert!(checked_negative, "the negative oracle check must run");
    });
}

#[test]
fn topology_change_tears_the_mesh_along_the_asymptotic_cone() {
    with_sketch(|db, top_mod| {
        let mut probe = compile_lane(db, top_mod, "probe");
        let center = [0.0_f32; 3];

        // Closed member: the unit sphere from an interior centre. Sheet 0
        // covers it completely (radial distance exactly 1), sheet 1 has no
        // second forward intersection anywhere.
        for vid in (0..VERTEX_COUNT).step_by(89) {
            let response = probe_vertex(&mut probe, vid, 0.0, center, &SPHERE, &SPHERE);
            let missed = resp_u32(&probe, &response, "missed");
            let (_, sheet) = direction(vid);
            if sheet == 0 {
                assert_eq!(missed, 0, "sphere sheet 0 must hit everywhere (vid {vid})");
                let t = resp_f32(&probe, &response, "t") as f64;
                assert!(
                    (t - 1.0).abs() < 2e-3,
                    "unit sphere radial hit must sit at t = 1 (vid {vid}, t {t})"
                );
            } else {
                assert_eq!(missed, 1, "sphere sheet 1 must collapse (vid {vid})");
            }
        }

        // Open member: x^2 + y^2 - z^2 = 1. The mesh must tear: directions
        // near the asymptotic cone miss, equatorial directions hit, and every
        // sheet-0 miss is a near-cone direction. The tear IS the cone (in
        // DIRECTION space: dx^2 + dy^2 - dz^2 = 0 for a = b = 1, c = -1).
        let mut hyper_hits = 0_u32;
        let mut hyper_misses = 0_u32;
        for vid in (0..SHEET_VERTS).step_by(89) {
            let response = probe_vertex(&mut probe, vid, 0.0, center, &HYPERBOLOID, &HYPERBOLOID);
            let (dir, _) = direction(vid);
            let cone = dir[0] * dir[0] + dir[1] * dir[1] - dir[2] * dir[2];
            if resp_u32(&probe, &response, "missed") == 1 {
                hyper_misses += 1;
                assert!(
                    cone < 0.02,
                    "hyperboloid miss off the asymptotic cone (vid {vid}, cone {cone:+.3})"
                );
            } else {
                hyper_hits += 1;
                let t = resp_f32(&probe, &response, "t") as f64;
                assert!(t > 0.0 && t <= T_MAX);
            }
        }
        assert!(
            hyper_hits > 10 && hyper_misses > 10,
            "the hyperboloid must both hit and tear (hits {hyper_hits}, misses {hyper_misses})"
        );

        // NEGATIVE: an empty constraint draws nothing at all. x^2+y^2+z^2 = -1
        // has no real points, so every vertex on both sheets must emit the
        // clipped sentinel; there is no way to fudge a picture out of it.
        for vid in (0..VERTEX_COUNT).step_by(89) {
            let response = probe_vertex(&mut probe, vid, 0.0, center, &EMPTY, &EMPTY);
            assert_eq!(
                resp_u32(&probe, &response, "missed"),
                1,
                "empty quadric emitted geometry (vid {vid})"
            );
        }
    });
}

#[test]
fn mesh_oracle_stream_matches_probe_lane() {
    with_sketch(|db, top_mod| {
        let mut mesh = compile_lane(db, top_mod, "mesh_oracle");
        let values = frame_values(5, 0.0, [0.0; 3], 4.0, 0.0, &SPHERE, &SPHERE);
        let response = mesh.call(&values);
        let stream = mesh.bytes_payload(&response);
        assert_eq!(
            stream.len(),
            VERTEX_COUNT as usize * 20,
            "vertex stream must carry 20 bytes per vertex"
        );

        let vertex = |vid: u32| -> ([f32; 4], u32) {
            let base = vid as usize * 20;
            let word = |slot: usize| -> u32 {
                u32::from_le_bytes(
                    stream[base + slot * 4..base + slot * 4 + 4]
                        .try_into()
                        .unwrap(),
                )
            };
            (
                [
                    f32::from_bits(word(0)),
                    f32::from_bits(word(1)),
                    f32::from_bits(word(2)),
                    f32::from_bits(word(3)),
                ],
                word(4),
            )
        };

        // The stream and the single-vertex lane are the same Fe behaviors
        // compiled twice; the outputs must agree bit for bit. (If this ever
        // fires, wasm lowering became nondeterministic, which is itself a
        // finding.)
        let mut probe = compile_lane(db, top_mod, "probe");
        for vid in (0..VERTEX_COUNT).step_by(631) {
            let ([x, y, z, w], rgba) = vertex(vid);
            let single = probe_vertex(&mut probe, vid, 0.0, [0.0; 3], &SPHERE, &SPHERE);
            assert_eq!(
                [x.to_bits(), y.to_bits(), z.to_bits(), w.to_bits(), rgba],
                [
                    resp_f32(&probe, &single, "x").to_bits(),
                    resp_f32(&probe, &single, "y").to_bits(),
                    resp_f32(&probe, &single, "z").to_bits(),
                    resp_f32(&probe, &single, "w").to_bits(),
                    resp_u32(&probe, &single, "rgba"),
                ],
                "vid {vid}: mesh stream and probe lane disagree"
            );
        }

        // Sheet split: sphere sheet 0 rasterizable, sheet 1 fully clipped.
        for vid in (0..VERTEX_COUNT).step_by(97) {
            let ([_, _, z, w], _) = vertex(vid);
            if vid < SHEET_VERTS {
                assert!(w > 0.0 && z >= 0.0 && z <= w, "vid {vid} must be drawable");
            } else {
                assert_eq!((z, w), (2.0, 1.0), "vid {vid} must be the clipped sentinel");
            }
        }
    });
}

#[test]
fn lane_intents_declare_honest_placement() {
    with_sketch(|db, top_mod| {
        for name in ["defaults", "solve", "probe", "mesh_oracle"] {
            let decl = canonical_lane_decl_from_entry(db, top_mod, name, name)
                .unwrap_or_else(|error| panic!("lane decl for `{name}`: {error}"));
            assert_eq!(
                decl.intent.execution,
                CanonicalExecution::Wasm,
                "`{name}` must be a genuine wasm lane"
            );
        }
    });
}

/// The canonical DE actor owns initialization, interaction, and rendering.
/// This absorbs lifecycle/arena coverage formerly attached to the raster actor
/// while the independent solver and analytic-ray tests remain separate.
#[test]
fn canonical_de_actor_owns_the_complete_scene_lifecycle() {
    with_de(|db, top_mod| {
        let program = actor_gpu_program(db, top_mod)
            .expect("derive QCGA DE program")
            .expect("QCGA GPU actor");
        assert_eq!(program.actor, "PencilDistanceSurface");
        assert_eq!(program.stages.len(), 1);
        assert_eq!(program.stages[0].source_entry, "distance_surface");
        assert_eq!(program.stages[0].kind, WebActorStageKind::Fragment);
        assert_eq!(
            actor_web_entry(db, top_mod).expect("derive QCGA web entry"),
            Some(("distance_surface".to_owned(), WebBundleMode::Render)),
        );

        let decl =
            canonical_lane_decl_from_entry(db, top_mod, "distance_surface", "distance_surface");
        assert!(
            decl.is_err(),
            "the fragment stage must not masquerade as a message lane"
        );

        let bundle = WebBundle::compile(
            db,
            top_mod,
            WebBuildOptions::render("distance_surface", None),
        )
        .expect("QCGA canonical DE bundle");
        assert_eq!(bundle.manifest.passes.len(), 1);
        let pass = &bundle.manifest.passes[0];
        assert_eq!(pass.source_entry, "distance_surface");
        assert_eq!(pass.layout.bindings.len(), 1);
        assert_eq!(pass.layout.bindings[0].members.len(), 59);
        assert_eq!(
            pass.layout.bindings[0]
                .members
                .iter()
                .map(|member| member.name.as_str())
                .collect::<Vec<_>>(),
            [
                "lambda",
                "yaw",
                "pitch",
                "dist",
                "width",
                "height",
                "generation",
                "cx",
                "cy",
                "cz",
                "pencil_free",
                "a0",
                "b0",
                "c0",
                "d0",
                "e0",
                "f0",
                "g0",
                "h0",
                "i0",
                "j0",
                "a1",
                "b1",
                "c1",
                "d1",
                "e1",
                "f1",
                "g1",
                "h1",
                "i1",
                "j1",
                "p0x",
                "p0y",
                "p0z",
                "p1x",
                "p1y",
                "p1z",
                "p2x",
                "p2y",
                "p2z",
                "p3x",
                "p3y",
                "p3z",
                "p4x",
                "p4y",
                "p4z",
                "p5x",
                "p5y",
                "p5z",
                "p6x",
                "p6y",
                "p6z",
                "p7x",
                "p7y",
                "p7z",
                "p8x",
                "p8y",
                "p8z",
                "picked",
            ],
            "nested Fe state must recursively derive the shader member identities",
        );
        let surface = bundle.manifest.surface.as_ref().expect("QCGA view");
        assert_eq!(
            surface
                .params
                .iter()
                .map(|param| param.name.as_str())
                .collect::<Vec<_>>(),
            ["lambda", "yaw", "pitch", "dist", "width", "height"],
            "InitialState supplies complete defaults; view() exposes controls and typed extents",
        );
        assert!(bundle.manifest.control.is_none());
        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &bundle.wasm).expect("QCGA control Wasm");
        let mut store = wasmtime::Store::new(&engine, ());
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        let initialize = instance
            .get_func(&mut store, "fe_surface_initialize_v1")
            .expect("fixed Fe InitialState export");
        let mut state = vec![wasmtime::Val::F32(0); 59];
        state[6] = wasmtime::Val::I32(0);
        state[58] = wasmtime::Val::I32(0);
        initialize
            .call(&mut store, &[], &mut state)
            .expect("execute Fe solver-backed initialization");
        let f32_at = |values: &[wasmtime::Val], index: usize| match values[index] {
            wasmtime::Val::F32(bits) => f32::from_bits(bits),
            ref value => panic!("state leaf {index} is not f32: {value:?}"),
        };
        assert_eq!(f32_at(&state, 0).to_bits(), 0.15f32.to_bits());
        assert_eq!(f32_at(&state, 1).to_bits(), 0.6f32.to_bits());
        assert_eq!(f32_at(&state, 2).to_bits(), 0.35f32.to_bits());
        assert_eq!(f32_at(&state, 3).to_bits(), 4.0f32.to_bits());
        assert_eq!(f32_at(&state, 4).to_bits(), 512.0f32.to_bits());
        assert_eq!(f32_at(&state, 5).to_bits(), 512.0f32.to_bits());
        assert!(matches!(state[6], wasmtime::Val::I32(0)));
        assert_eq!(f32_at(&state, 10).to_bits(), 1.0f32.to_bits());
        let q0 = std::array::from_fn(|index| f32_at(&state, 11 + index));
        let q1 = std::array::from_fn(|index| f32_at(&state, 21 + index));
        for index in 0..9 {
            let point = [
                f32_at(&state, 31 + index * 3) as f64,
                f32_at(&state, 32 + index * 3) as f64,
                f32_at(&state, 33 + index * 3) as f64,
            ];
            assert!(oracle(&q0, point).abs() < 1e-4);
            assert!(oracle(&q1, point).abs() < 1e-4);
        }

        let replace = instance
            .get_func(&mut store, "fe_surface_state_replace_v1")
            .expect("resident state seed");
        replace.call(&mut store, &state, &mut []).unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();
        let alloc = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
            .unwrap();
        // Three complete raw SurfaceEvent records fit here. The host boundary
        // stays the fixed 52-byte fact transport; no QCGA identity or geometry
        // is added to it.
        let pointer = alloc.call(&mut store, (3 * 52, 4)).unwrap() as usize;
        let event_words = [
            0.0f32.to_bits(),
            0.0f32.to_bits(),
            25.0f32.to_bits(),
            0.0f32.to_bits(),
            0.0f32.to_bits(),
            0,
            1,
            1.0f32.to_bits(),
            512.0f32.to_bits(),
            512.0f32.to_bits(),
            0,
            0,
            0.0f32.to_bits(),
        ];
        let event_bytes = event_words
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        memory.write(&mut store, pointer, &event_bytes).unwrap();
        let transition = instance
            .get_func(&mut store, "fe_surface_transition_scheduled_v1")
            .expect("scheduled Fe navigation export");
        let mut next = vec![wasmtime::Val::F32(0); 59];
        next[6] = wasmtime::Val::I32(0);
        next[58] = wasmtime::Val::I32(0);
        transition
            .call(
                &mut store,
                &[wasmtime::Val::I32(pointer as i32), wasmtime::Val::I32(1)],
                &mut next,
            )
            .expect("execute FCO-derived QCGA navigation");
        assert_eq!(f32_at(&next, 0).to_bits(), f32_at(&state, 0).to_bits());
        assert_ne!(f32_at(&next, 1).to_bits(), f32_at(&state, 1).to_bits());
        assert_eq!(f32_at(&next, 2).to_bits(), f32_at(&state, 2).to_bits());
        assert_eq!(f32_at(&next, 3).to_bits(), f32_at(&state, 3).to_bits());
        assert_eq!(
            next[6..]
                .iter()
                .map(|value| format!("{value:?}"))
                .collect::<Vec<_>>(),
            state[6..]
                .iter()
                .map(|value| format!("{value:?}"))
                .collect::<Vec<_>>(),
            "navigation must preserve every solver-derived basis leaf exactly",
        );

        // Independent interaction receipt: derive p0's screen location from
        // the initialized Fe state, select it with a typed PointerDown, then
        // deliver PointerMove + PointerUp in one ordered resident batch. This
        // checks semantic state and incidence; generated bytes are irrelevant.
        replace.call(&mut store, &state, &mut []).unwrap();
        let point0 = [
            f32_at(&state, 31) as f64,
            f32_at(&state, 32) as f64,
            f32_at(&state, 33) as f64,
        ];
        let center = [
            f32_at(&state, 7) as f64,
            f32_at(&state, 8) as f64,
            f32_at(&state, 9) as f64,
        ];
        let yaw = f32_at(&state, 1) as f64;
        let pitch = f32_at(&state, 2) as f64;
        let dist = f32_at(&state, 3) as f64;
        let [wx, wy, wz] = std::array::from_fn(|axis| point0[axis] - center[axis]);
        let (cyaw, syaw) = (yaw.cos(), yaw.sin());
        let (cpit, spit) = (pitch.cos(), pitch.sin());
        let rx = cyaw * wx + syaw * wz;
        let rz0 = cyaw * wz - syaw * wx;
        let ry = cpit * wy - spit * rz0;
        let depth = spit * wy + cpit * rz0 + dist;
        let pointer_x = ((rx * 1.6 / depth) * 0.5 + 0.5) * 512.0;
        let pointer_y = (0.5 - (ry * 1.6 / depth) * 0.5) * 512.0;
        assert!(depth > 0.25 && pointer_x.is_finite() && pointer_y.is_finite());

        let raw_event = |kind: u32, dx: f32, dy: f32| {
            [
                (pointer_x as f32).to_bits(),
                (pointer_y as f32).to_bits(),
                dx.to_bits(),
                dy.to_bits(),
                0.0f32.to_bits(),
                0,
                1,
                2.0f32.to_bits(),
                512.0f32.to_bits(),
                512.0f32.to_bits(),
                kind,
                0,
                0.0f32.to_bits(),
            ]
        };
        let down_bytes = raw_event(8, 0.0, 0.0)
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        memory.write(&mut store, pointer, &down_bytes).unwrap();
        let mut selected = vec![wasmtime::Val::F32(0); 59];
        selected[6] = wasmtime::Val::I32(0);
        selected[58] = wasmtime::Val::I32(0);
        transition
            .call(
                &mut store,
                &[wasmtime::Val::I32(pointer as i32), wasmtime::Val::I32(1)],
                &mut selected,
            )
            .expect("Fe PointerDown must select the projected control point");
        assert!(matches!(selected[58], wasmtime::Val::I32(1)), "p0 enum tag");
        assert_eq!(
            selected[..58]
                .iter()
                .map(|value| format!("{value:?}"))
                .collect::<Vec<_>>(),
            state[..58]
                .iter()
                .map(|value| format!("{value:?}"))
                .collect::<Vec<_>>(),
            "selection changes only the typed PickedControl state",
        );

        let drag_release_bytes = raw_event(9, 12.0, -7.0)
            .into_iter()
            .chain(raw_event(10, 0.0, 0.0))
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        memory
            .write(&mut store, pointer, &drag_release_bytes)
            .unwrap();
        let mut dragged = vec![wasmtime::Val::F32(0); 59];
        dragged[6] = wasmtime::Val::I32(0);
        dragged[58] = wasmtime::Val::I32(0);
        transition
            .call(
                &mut store,
                &[wasmtime::Val::I32(pointer as i32), wasmtime::Val::I32(2)],
                &mut dragged,
            )
            .expect("ordered Fe PointerMove + PointerUp batch");
        assert!(matches!(dragged[6], wasmtime::Val::I32(1)), "one re-solve");
        assert!(
            matches!(dragged[58], wasmtime::Val::I32(0)),
            "release clears pick"
        );
        assert_ne!(
            [
                f32_at(&dragged, 31),
                f32_at(&dragged, 32),
                f32_at(&dragged, 33)
            ]
            .map(f32::to_bits),
            [f32_at(&state, 31), f32_at(&state, 32), f32_at(&state, 33)].map(f32::to_bits),
            "drag must move the selected point",
        );
        assert_eq!(
            dragged[34..58]
                .iter()
                .map(|value| format!("{value:?}"))
                .collect::<Vec<_>>(),
            state[34..58]
                .iter()
                .map(|value| format!("{value:?}"))
                .collect::<Vec<_>>(),
            "drag must preserve all eight unselected points exactly",
        );
        let dragged_q0 = std::array::from_fn(|index| f32_at(&dragged, 11 + index));
        let dragged_q1 = std::array::from_fn(|index| f32_at(&dragged, 21 + index));
        for index in 0..9 {
            let point = [
                f32_at(&dragged, 31 + index * 3) as f64,
                f32_at(&dragged, 32 + index * 3) as f64,
                f32_at(&dragged, 33 + index * 3) as f64,
            ];
            assert!(
                oracle(&dragged_q0, point).abs() < 2e-4 && oracle(&dragged_q1, point).abs() < 2e-4,
                "re-solved basis must contain dragged point {index}",
            );
        }

        // Browser regression for the reported `fe_cabi_alloc -> solve_pencil`
        // trap: repeatedly execute the allocation-heavy Fe drag/re-solve path
        // using the fixed host's canonical call epoch. Resident scalar state
        // must advance while every call reuses the same bounded arena range.
        let reset = instance
            .get_typed_func::<(), ()>(&mut store, "fe_cabi_reset")
            .expect("canonical arena reset");
        replace
            .call(&mut store, &selected, &mut [])
            .expect("restore selected control state for arena stress");
        let mut event_pointer = None;
        let mut post_solve_cursor = None;
        let mut bounded_pages = None;
        let initial_generation = match selected[6] {
            wasmtime::Val::I32(value) => value,
            ref value => panic!("generation is not i32: {value:?}"),
        };
        let mut stressed = selected.clone();
        for iteration in 0..256_i32 {
            reset
                .call(&mut store, ())
                .expect("begin canonical surface-call epoch");
            let current_pointer = alloc.call(&mut store, (52, 4)).unwrap();
            let move_bytes = raw_event(9, 0.01, -0.005)
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>();
            memory
                .write(&mut store, current_pointer as usize, &move_bytes)
                .unwrap();
            transition
                .call(
                    &mut store,
                    &[wasmtime::Val::I32(current_pointer), wasmtime::Val::I32(1)],
                    &mut stressed,
                )
                .expect("allocation-heavy Fe pencil solve stays live");
            assert!(
                matches!(stressed[6], wasmtime::Val::I32(value) if value == initial_generation + iteration + 1)
            );
            assert!(matches!(stressed[58], wasmtime::Val::I32(1)));

            let current_cursor = alloc.call(&mut store, (1, 1)).unwrap();
            let current_pages = memory.size(&store);
            match (event_pointer, post_solve_cursor, bounded_pages) {
                (None, None, None) => {
                    event_pointer = Some(current_pointer);
                    post_solve_cursor = Some(current_cursor);
                    bounded_pages = Some(current_pages);
                }
                (Some(pointer), Some(cursor), Some(pages)) => {
                    assert_eq!(current_pointer, pointer, "event allocation must be reused");
                    assert_eq!(
                        current_cursor, cursor,
                        "solve arena high-water mark must be stable"
                    );
                    assert_eq!(
                        current_pages, pages,
                        "surface calls must not grow Wasm memory"
                    );
                }
                _ => unreachable!(),
            }
        }
        reset
            .call(&mut store, ())
            .expect("close final canonical surface-call epoch");
        assert!(bundle.wgsl.contains("@vertex"), "{}", bundle.wgsl);
        assert!(bundle.wgsl.contains("@fragment"), "{}", bundle.wgsl);
    });
}
