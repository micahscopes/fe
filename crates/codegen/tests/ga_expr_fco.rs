//! Dual semantic/shape gate for the first typed GA expression-compiler slice.
//!
//! Correctness is established by executing the Fe-generated Wasm against an
//! independent Rust oracle. Generated WGSL shape is a separate performance
//! property; an operation count or byte match is never used as a semantic
//! oracle.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, compile_runtime_package_spirv_render, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;

fn algebraic_lane(l: [f32; 3], r: [f32; 3]) -> [f32; 3] {
    // Match the explicitly declared AlgebraicBalanced schedule, including its
    // association. Each repeated symbolic product is evaluated once in Fe and
    // represented twice here to model the two exact coefficient contributions.
    let p01 = l[0] * r[1];
    let n01 = l[1] * r[0];
    let p02 = l[0] * r[2];
    let n02 = l[2] * r[0];
    let p12 = l[1] * r[2];
    let n12 = l[2] * r[1];
    [
        (p01 + p01) + -(n01 + n01),
        (p02 + p02) + -(n02 + n02),
        (p12 + p12) + -(n12 + n12),
    ]
}

fn strict_dense_lane(l: [f32; 3], r: [f32; 3]) -> [f32; 3] {
    // Independent dense/source-tree interpretation of `(a ^ b) + (a ^ b)`.
    // This is intentionally allowed to last-bit differ from AlgebraicBalanced.
    let w01 = l[0] * r[1] - l[1] * r[0];
    let w02 = l[0] * r[2] - l[2] * r[0];
    let w12 = l[1] * r[2] - l[2] * r[1];
    [w01 + w01, w02 + w02, w12 + w12]
}

fn close_enough(got: f32, semantic: f32) -> bool {
    if got.to_bits() == semantic.to_bits() {
        return true;
    }
    if !got.is_finite() || !semantic.is_finite() {
        return got.is_nan() && semantic.is_nan();
    }
    let scale = got.abs().max(semantic.abs()).max(1.0);
    (got - semantic).abs() <= 8.0 * f32::EPSILON * scale
}

#[test]
fn typed_ga_expression_is_semantic_wasm_and_branch_free_browser_wgsl() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ga_expr_fco");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "typed GA fixture initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("typed GA fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "typed GA expression diagnostics:\n{diagnostics}"
    );

    // Semantic gate: execute the generated implementations, not a copied
    // source fragment, and compare each lane to the independently authored
    // AlgebraicBalanced schedule plus the dense semantic interpretation.
    let wasm = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("typed GA expression should compile to Wasm")
        .into_bytecode()
        .expect("Wasm output should be bytecode");
    wasmparser::validate(&wasm).expect("typed GA Wasm validates");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(
        module.imports().next().is_none(),
        "the GA evaluator must not depend on a host algebra implementation"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let component = |name: &str, store: &mut wasmtime::Store<()>| {
        instance
            .get_typed_func::<(f32, f32, f32, f32, f32, f32), f32>(&mut *store, name)
            .unwrap_or_else(|error| panic!("missing/mistyped `{name}`: {error}"))
    };
    let c0 = component("twice_wedge_c0", &mut store);
    let c1 = component("twice_wedge_c1", &mut store);
    let c2 = component("twice_wedge_c2", &mut store);

    let mut cases = vec![
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
        ([2.0, -3.0, 0.5], [0.25, 4.0, -1.5]),
        ([-0.0, 1.0, -2.0], [0.0, -3.0, 4.0]),
        ([0.0; 3], [0.0; 3]),
    ];
    let mut state = 0x8ac7_3d19_u32;
    let mut next = || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((state >> 9) as i32 as f32) / 16_384.0 - 512.0
    };
    for _ in 0..2_000 {
        cases.push(([next(), next(), next()], [next(), next(), next()]));
    }

    for (l, r) in cases {
        let args = (l[0], l[1], l[2], r[0], r[1], r[2]);
        let got = [
            c0.call(&mut store, args).unwrap(),
            c1.call(&mut store, args).unwrap(),
            c2.call(&mut store, args).unwrap(),
        ];
        let scheduled = algebraic_lane(l, r);
        assert_eq!(
            got.map(f32::to_bits),
            scheduled.map(f32::to_bits),
            "Fe/FCO schedule != independent emitted-policy oracle for l={l:?} r={r:?}"
        );
        let dense = strict_dense_lane(l, r);
        assert!(
            got.into_iter()
                .zip(dense)
                .all(|(actual, semantic)| close_enough(actual, semantic)),
            "algebraic schedule drifted from dense semantics for l={l:?} r={r:?}: \
             got={got:?}, dense={dense:?}"
        );
    }

    // Shape gate: compile the same Fe/FCO implementation through the browser
    // render backend and inspect properties that matter for one invocation.
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "twice_wedge_render")
        .expect("typed GA render runtime package");
    let artifact = compile_runtime_package_spirv_render(&db, &package)
        .expect("typed GA expression should compile through render SPIR-V");
    assert_eq!(artifact.words.first().copied(), Some(0x0723_0203));
    let wgsl = artifact.wgsl.expect("typed GA browser WGSL");
    let module = naga::front::wgsl::parse_str(&wgsl).expect("typed GA WGSL reparses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("typed GA WGSL validates with browser-default capabilities");
    assert_eq!(
        wgsl.matches("fn ").count(),
        2,
        "all expression/provider helpers must inline to vertex + fragment:\n{wgsl}"
    );
    assert!(
        !wgsl.contains("loop {") && !wgsl.contains("if (") && !wgsl.contains("switch"),
        "CTFE planning must leave no runtime plan control flow:\n{wgsl}"
    );
    assert_eq!(
        wgsl.matches(" * ").count(),
        7,
        "six survivor products plus the final display scale are the shape budget:\n{wgsl}"
    );
}
