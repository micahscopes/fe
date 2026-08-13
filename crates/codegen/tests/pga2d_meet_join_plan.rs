//! Dual-gate for the gaplay PGA-2D meet/join plan (ingots/gaplay).
//!
//! The gaplay ingot adds ganja.js's `^` (meet) and `&` (join) for Cl(2,0,1) as
//! `Outer<Left,Right>` compiled by the shared `ga_expr::CompileGaF32` provider;
//! one kernel `pga2_wedge3` serves both operators. This test:
//!
//!   1. compiles the ingot through the real Fe drivers and shared provider;
//!   2. runs the planned kernel under wasmtime and checks it BIT-FOR-BIT against
//!      an independent hand-written cross product over directed + random f32
//!      cases (f32 arithmetic is IEEE754-deterministic, so the match is exact);
//!   3. checks Desargues' theorem through the SAME planned meet/join: the three
//!      corresponding-side intersections are collinear across the sweep, and the
//!      cross-point coordinates match an independent oracle.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;

fn compile_gate_ingot_to_wasm() -> Vec<u8> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pga2d_meet_join_gate_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "pga2d meet/join gate ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("pga2d meet/join gate ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected gate-ingot diagnostics (the plan static_asserts live here):\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("gaplay meet/join gate ingot should compile to wasm")
        .into_bytecode()
        .expect("wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("gate ingot wasm should validate");
    bytes
}

/// The independent hand-tabled outer product of two grade-1 PGA-2D vectors,
/// written directly (never transcribed from the Fe kernel). The three output
/// components are the antisymmetric pairings the plan must reproduce.
fn wedge3_oracle(l: [f32; 3], r: [f32; 3]) -> [f32; 3] {
    [
        l[0] * r[1] - l[1] * r[0], // e01
        l[0] * r[2] - l[2] * r[0], // e02
        l[1] * r[2] - l[2] * r[1], // e12
    ]
}

// The gaplay Desargues config constants, mirrored for the independent oracle.
const OX: f32 = 0.0;
const OY: f32 = 0.05;
const A: [f32; 2] = [-0.6, -0.4];
const B: [f32; 2] = [0.7, -0.45];
const C: [f32; 2] = [0.0, 0.75];
const FB: f32 = 1.45;
const FC: f32 = 0.72;

fn point_xy(x: f32, y: f32) -> [f32; 3] {
    [x, y, 1.0]
}
fn image(v: [f32; 2], t: f32) -> [f32; 3] {
    point_xy(OX + t * (v[0] - OX), OY + t * (v[1] - OY))
}
fn cross_oracle(sweep: f32) -> ([f32; 2], [f32; 2], [f32; 2]) {
    let a = point_xy(A[0], A[1]);
    let b = point_xy(B[0], B[1]);
    let c = point_xy(C[0], C[1]);
    let ap = image(A, sweep);
    let bp = image(B, sweep * FB);
    let cp = image(C, sweep * FC);
    // join(p, q) = wedge3(p, q); meet(a, b) = wedge3(a, b).
    let pa = wedge3_oracle(wedge3_oracle(b, c), wedge3_oracle(bp, cp));
    let pb = wedge3_oracle(wedge3_oracle(c, a), wedge3_oracle(cp, ap));
    let pc = wedge3_oracle(wedge3_oracle(a, b), wedge3_oracle(ap, bp));
    let world = |p: [f32; 3]| [p[0] / p[2], p[1] / p[2]];
    (world(pa), world(pb), world(pc))
}

#[test]
fn planned_meet_join_matches_hand_tabled_kernel_and_proves_desargues() {
    let wasm = compile_gate_ingot_to_wasm();

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(
        module.imports().next().is_none(),
        "the gate wasm must be self-contained (zero imports)"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();

    let comp = |name: &str, store: &mut wasmtime::Store<()>| {
        instance
            .get_typed_func::<(f32, f32, f32, f32, f32, f32), f32>(&mut *store, name)
            .unwrap_or_else(|e| panic!("export `{name}` missing/mis-typed: {e}"))
    };
    let w0 = comp("wedge_c0", &mut store);
    let w1 = comp("wedge_c1", &mut store);
    let w2 = comp("wedge_c2", &mut store);

    let check_wedge = |l: [f32; 3], r: [f32; 3], store: &mut wasmtime::Store<()>| {
        let got = [
            w0.call(&mut *store, (l[0], l[1], l[2], r[0], r[1], r[2]))
                .unwrap(),
            w1.call(&mut *store, (l[0], l[1], l[2], r[0], r[1], r[2]))
                .unwrap(),
            w2.call(&mut *store, (l[0], l[1], l[2], r[0], r[1], r[2]))
                .unwrap(),
        ];
        let want = wedge3_oracle(l, r);
        assert_eq!(
            got.map(f32::to_bits),
            want.map(f32::to_bits),
            "planned wedge != hand-tabled oracle for l={l:?} r={r:?}: got {got:?}, want {want:?}"
        );
    };

    // Directed cases: basis wedges (sign + annihilation) and mixed vectors.
    let directed: [([f32; 3], [f32; 3]); 8] = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), // e0 ^ e1 = +e01
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0]), // e1 ^ e0 = -e01
        ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), // e0 ^ e2 = +e02
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]), // e1 ^ e2 = +e12
        ([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]), // e0 ^ e0 = 0
        ([2.0, -3.0, 0.5], [0.25, 4.0, -1.5]),
        ([-1.25, 0.75, 2.0], [3.0, -0.5, 0.125]),
        ([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]),
    ];
    for (l, r) in directed {
        check_wedge(l, r, &mut store);
    }

    // Deterministic random walk over f32 operands (same LCG discipline the
    // control-fn oracles use), each bit-exact.
    let mut s: u32 = 0x1234_5678;
    let mut rnd = || {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        // a smallish signed f32 with a fractional part, exactly representable-ish
        ((s >> 8) as i32 as f32) / 65_536.0 - 128.0
    };
    for _ in 0..4000 {
        let l = [rnd(), rnd(), rnd()];
        let r = [rnd(), rnd(), rnd()];
        check_wedge(l, r, &mut store);
    }

    // --- Desargues' theorem through the SAME planned meet/join. -------------
    let pax = comp1("desargues_pax", &instance, &mut store);
    let pay = comp1("desargues_pay", &instance, &mut store);
    let pbx = comp1("desargues_pbx", &instance, &mut store);
    let pby = comp1("desargues_pby", &instance, &mut store);
    let pcx = comp1("desargues_pcx", &instance, &mut store);
    let pcy = comp1("desargues_pcy", &instance, &mut store);

    // Sweep across the demo's interactive range; at every value the three
    // corresponding-side cross-points must (a) match the independent oracle and
    // (b) be collinear (the axis of perspectivity) to within 1e-3 world units.
    let mut sweeps = Vec::new();
    let mut t = 0.30f32;
    while t <= 1.61 {
        sweeps.push(t);
        t += 0.05;
    }
    for sweep in sweeps {
        let get = |f: &wasmtime::TypedFunc<f32, f32>, st: &mut wasmtime::Store<()>| {
            f.call(&mut *st, sweep).unwrap()
        };
        let pa = [get(&pax, &mut store), get(&pay, &mut store)];
        let pb = [get(&pbx, &mut store), get(&pby, &mut store)];
        let pc = [get(&pcx, &mut store), get(&pcy, &mut store)];

        let (opa, opb, opc) = cross_oracle(sweep);
        for (g, o) in [(pa, opa), (pb, opb), (pc, opc)] {
            assert!(
                (g[0] - o[0]).abs() < 1e-4 && (g[1] - o[1]).abs() < 1e-4,
                "planned Desargues cross-point {g:?} != oracle {o:?} at sweep={sweep}"
            );
        }

        // Collinearity: signed distance of pc to the axis through pa, pb.
        let dx = pb[0] - pa[0];
        let dy = pb[1] - pa[1];
        let len = (dx * dx + dy * dy).sqrt();
        assert!(len > 1e-3, "degenerate axis at sweep={sweep}");
        let dist = ((pc[0] - pa[0]) * dy - (pc[1] - pa[1]) * dx).abs() / len;
        assert!(
            dist < 1e-3,
            "Desargues FAILED: cross-point pc is {dist} off the axis at sweep={sweep} \
             (pa={pa:?} pb={pb:?} pc={pc:?})"
        );
    }
}

fn comp1(
    name: &str,
    instance: &wasmtime::Instance,
    store: &mut wasmtime::Store<()>,
) -> wasmtime::TypedFunc<f32, f32> {
    instance
        .get_typed_func::<f32, f32>(&mut *store, name)
        .unwrap_or_else(|e| panic!("export `{name}` missing/mis-typed: {e}"))
}
