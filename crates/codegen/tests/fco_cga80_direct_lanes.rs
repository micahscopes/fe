use std::time::Instant;

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    BackendKind, OptLevel, WasmCompileOptions, compile_runtime_package_spirv_render,
    compile_runtime_package_wasm_with_options, layout_for,
};
use url::Url;

const SPARSE_CLIFFORD_API: &str = include_str!("../../../ingots/sparse_clifford/src/lib.fe");
const CANONICAL50_API: &str = include_str!("../../../ingots/canonical_cl41_schedule/src/lib.fe");
const CANONICAL: &str = include_str!("fixtures/composed/fco_cga80_direct_lanes.fe");
const SCHEDULE32_REFERENCE: &str =
    include_str!("../../../demos/webgpu-cga-inversion/gen-schedule32/reference.json");
const PINNED_REFERENCE_HASH: u32 = 3_470_936_828;
const PINNED_VIEW: (f32, f32, f32, f32, f32) = (0.0, 0.0, 0.0125, 0.5, 0.0);

fn canonical_source() -> String {
    let (_, interpreter) = CANONICAL
        .split_once("// BEGIN_PROVIDER_EMITTER")
        .expect("canonical typed-interpreter begin marker");
    assert!(
        interpreter.contains(
            "for Canonical50Term<Candidate, Left, Point, Right, Output, Magnitude, Negative>",
        ) && interpreter.contains("impl<L: Eval5, R: Eval5> Eval5 for Add<L, R>")
            && interpreter.contains("<Canonical50TypedBalancedSchedule32 as Eval5>::eval5(")
            && !interpreter.contains("ObserveCanonical")
            && !interpreter.contains("ImplBuilder")
            && !interpreter.contains("normalized_preorder_types")
            && !interpreter.contains("for triple in 0..80"),
        "ordinary typed Eval5 must consume the exact plan without provider traversal or raw80 rescanning",
    );
    let sparse_api = fe_codegen::standalone_ctfe_ingot_source(SPARSE_CLIFFORD_API);
    let canonical50_api = fe_codegen::standalone_ctfe_ingot_source(CANONICAL50_API);
    let (_, canonical50_api) = canonical50_api
        .split_once("// Bounded symbolic coefficient interpretation")
        .expect("canonical standalone source begins after its ingot import");
    let canonical50_api = format!(
        "// Bounded symbolic coefficient interpretation{}",
        canonical50_api.replace("sparse_clifford::", "")
    );
    format!("{sparse_api}\n{canonical50_api}\n// BEGIN_PROVIDER_EMITTER{interpreter}")
}

#[derive(Clone, Copy, Debug)]
enum PlanExecution {
    UnsharedTree,
    CompactTerms,
    SharedDag,
}

fn canonical_source_for(execution: PlanExecution) -> String {
    let _ = execution;
    canonical_source()
}

fn render_source_for(execution: PlanExecution) -> String {
    let source = canonical_source_for(execution);
    let (prefix, rest) = source
        .split_once("// BEGIN_PUBLIC_ORACLES")
        .expect("public-oracle begin marker");
    let (_, suffix) = rest
        .split_once("// END_PUBLIC_ORACLES")
        .expect("public-oracle end marker");
    format!(
        "{prefix}{suffix}\n{}",
        include_str!("fixtures/spirv/fco_cga80_direct_de_body.fe"),
    )
}

fn compile_to_wasm(source: &str) -> Vec<u8> {
    compile_to_wasm_at(source, OptLevel::O0)
}

fn compile_to_wasm_at(source: &str, opt: OptLevel) -> Vec<u8> {
    let total_started = Instant::now();
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///fco_cga80_direct_lanes.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let analysis_started = Instant::now();
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    eprintln!(
        "canonical FCO CGA semantic analysis: {:?}",
        analysis_started.elapsed()
    );
    assert!(
        diagnostics.is_empty(),
        "unexpected canonical FCO CGA diagnostics:\n{diagnostics}"
    );
    let backend_started = Instant::now();
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), opt)
        .expect("canonical FCO CGA lanes should compile to Wasm")
        .into_bytecode()
        .expect("Wasm output should be bytecode");
    eprintln!(
        "canonical FCO CGA Wasm backend: {:?}; total {:?}",
        backend_started.elapsed(),
        total_started.elapsed()
    );
    bytes
}

fn without_section(source: &str, begin: &str, end: &str) -> String {
    let (prefix, rest) = source.split_once(begin).expect("begin marker");
    let (_, suffix) = rest.split_once(end).expect("end marker");
    format!("{prefix}{suffix}")
}

fn semantic_analysis(name: &str, source: &str) -> std::time::Duration {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}.fe")).unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let started = Instant::now();
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    let elapsed = started.elapsed();
    assert!(
        diagnostics.is_empty(),
        "unexpected {name} diagnostics after {elapsed:?}:\n{diagnostics}"
    );
    eprintln!("{name} semantic analysis: {elapsed:?}");
    elapsed
}

#[test]
#[ignore = "phase-isolation measurement; run explicitly"]
fn phase_a_forced_schedule32_without_provider() {
    let source = canonical_source();
    let source = without_section(
        &source,
        "// BEGIN_PROVIDER_EMITTER",
        "// END_PROVIDER_EMITTER",
    );
    semantic_analysis("fco_cga_phase_a_schedule", &source);
}

#[test]
#[ignore = "phase-isolation measurement; run explicitly"]
fn phase_b_provider_with_unforced_schedule_definition() {
    let source = canonical_source();
    let source = without_section(
        &source,
        "// BEGIN_FORCE_SCHEDULE32",
        "// END_FORCE_SCHEDULE32",
    );
    semantic_analysis("fco_cga_phase_b_provider", &source);
}

#[test]
#[ignore = "phase-isolation measurement; run explicitly"]
fn phase_c_forced_schedule32_and_provider() {
    let source = canonical_source();
    semantic_analysis("fco_cga_phase_c_combined", &source);
}

#[test]
#[ignore = "expensive focused regression for recursive staged const payload normalization"]
fn canonical50_eval5_chunk0_n1_signed_field_reproducer() {
    let mut source = canonical_source().replace(
        "Canonical50TypedBalancedSchedule32 as Eval5",
        "Canonical50TypedChunk0<1> as Eval5",
    );
    source.push_str(
        r#"
static_assert(canonical50_projected_sign(10) == 0)
struct Canonical50SignProbe<const Sign: i32> {}
fn accept_canonical50_sign_zero(_ value: Canonical50SignProbe<0>) {}
fn prove_direct_canonical50_sign_materializes(
    _ value: Canonical50SignProbe<{canonical50_projected_sign(10)}>,
) {
    accept_canonical50_sign_zero(value)
}
"#,
    );
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///canonical50_eval5_chunk0_n1_reproducer.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "N1 source should reach runtime-package lowering:\n{diagnostics}"
    );
    mir::build_wasm_runtime_package_for_entry(&db, top_mod, "cga_fco_e1")
        .expect("N1 signed const payload must lower to a runtime package");
}

fn sphere_blade(slot: usize) -> usize {
    1 << (slot + slot / 2)
}

fn point_blade(slot: usize) -> usize {
    1 << slot
}

fn gp_negative(a: usize, b: usize) -> bool {
    let mut negative = false;
    for bit in 0..5 {
        if a & (1 << bit) != 0 {
            if (b & ((1 << bit) - 1)).count_ones() & 1 != 0 {
                negative = !negative;
            }
            if bit == 4 && b & (1 << bit) != 0 {
                negative = !negative;
            }
        }
    }
    negative
}

fn raw80(sphere: [f32; 4], point: [f32; 5]) -> [f32; 32] {
    let mut out = [0.0; 32];
    for (left_slot, &left_value) in sphere.iter().enumerate() {
        let left = sphere_blade(left_slot);
        for (point_slot, &point_value) in point.iter().enumerate() {
            let middle = point_blade(point_slot);
            for (right_slot, &right_value) in sphere.iter().enumerate() {
                let right = sphere_blade(right_slot);
                let negative = gp_negative(left, middle) ^ gp_negative(left ^ middle, right);
                let term = left_value * point_value * right_value;
                out[left ^ middle ^ right] += if negative { -term } else { term };
            }
        }
    }
    out
}

fn sphere_pair_rank(a: usize, b: usize) -> usize {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    (0..lo).map(|left| 4 - left).sum::<usize>() + hi - lo
}

fn canonical_survivors() -> Vec<usize> {
    let mut coefficients = [0i32; 50];
    for left_slot in 0..4 {
        let left = sphere_blade(left_slot);
        for point_slot in 0..5 {
            let middle = point_blade(point_slot);
            for right_slot in 0..4 {
                let right = sphere_blade(right_slot);
                let candidate = sphere_pair_rank(left_slot, right_slot) * 5 + point_slot;
                let negative = gp_negative(left, middle) ^ gp_negative(left ^ middle, right);
                coefficients[candidate] += if negative { -1 } else { 1 };
            }
        }
    }
    coefficients
        .into_iter()
        .enumerate()
        .filter_map(|(candidate, coefficient)| (coefficient != 0).then_some(candidate))
        .collect()
}

fn deterministic_coefficient(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    ((*state % 2001) as i32 - 1000) as f32 / 257.0
}

fn call_lanes(
    bytes: &[u8],
    cases: &[([f32; 4], [f32; 5])],
) -> (Vec<[f32; 5]>, (usize, usize, usize, usize, usize, usize)) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("valid Wasm module");
    assert!(
        module.imports().next().is_none(),
        "canonical plan strategies must need no host imports",
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("zero-import instance");
    type Args = (f32, f32, f32, f32, f32, f32, f32, f32, f32);
    let lanes = [
        "cga_fco_e1",
        "cga_fco_e2",
        "cga_fco_e4",
        "cga_fco_e8",
        "cga_fco_e16",
    ]
    .map(|name| {
        instance
            .get_typed_func::<Args, f32>(&mut store, name)
            .unwrap_or_else(|_| panic!("missing {name} export"))
    });
    let values = cases
        .iter()
        .map(|(sphere, point)| {
            let args = (
                sphere[0], sphere[1], sphere[2], sphere[3], point[0], point[1], point[2], point[3],
                point[4],
            );
            std::array::from_fn(|lane| lanes[lane].call(&mut store, args).unwrap())
        })
        .collect();
    (values, wasm_shape(bytes))
}

fn browser_profile_frame(execution: PlanExecution) -> (Vec<u32>, String) {
    const ENTRY: &str = "cga_schedule32_vec5_de_render";
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///canonical_sandwich_{execution:?}.fe")).unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(render_source_for(execution)));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "{execution:?} browser-profile analysis:\n{diagnostics}",
    );
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, ENTRY)
        .unwrap_or_else(|error| panic!("{execution:?} runtime package: {error}"));
    let wasm =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .unwrap_or_else(|error| panic!("{execution:?} Wasm: {error}"));
    wasmparser::validate(&wasm.bytes).expect("valid browser-profile Wasm");

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm.bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let render = instance
        .get_typed_func::<(i32, i32, f32, f32, f32, f32, f32), i32>(&mut store, ENTRY)
        .unwrap();
    let mut frame = Vec::with_capacity(128 * 128);
    let (cam_x, cam_y, zoom, inv_cx, inv_cy) = PINNED_VIEW;
    for y in 0..128 {
        for x in 0..128 {
            frame.push(
                render
                    .call(&mut store, (x, y, cam_x, cam_y, zoom, inv_cx, inv_cy))
                    .unwrap() as u32,
            );
        }
    }

    let spirv = compile_runtime_package_spirv_render(&db, &package)
        .unwrap_or_else(|error| panic!("{execution:?} SPIR-V: {error}"));
    let wgsl = spirv.wgsl.expect("Render profile must emit WGSL");
    let module = naga::front::wgsl::parse_str(&wgsl).expect("browser-profile WGSL parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("browser-profile WGSL validates");
    assert_eq!(
        wgsl.matches("fn ").count(),
        2,
        "{execution:?} must inline to vertex + fragment only",
    );
    assert_eq!(
        wgsl.matches("loop {").count(),
        1,
        "{execution:?} may retain only the raymarch loop",
    );
    (frame, wgsl)
}

fn frame_fnv1a32(frame: &[u32]) -> u32 {
    frame
        .iter()
        .flat_map(|pixel| pixel.to_le_bytes())
        .fold(0x811c_9dc5, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
        })
}

#[test]
fn one_reflected_plan_drives_tree_compact_terms_and_honest_shared_dag() {
    let survivors = canonical_survivors();
    assert_eq!(survivors.len(), 32);
    let repeated_product_edges = survivors
        .iter()
        .filter(|&&candidate| matches!(candidate / 5, 1 | 2 | 3 | 5 | 6 | 8))
        .count();
    assert_eq!(
        repeated_product_edges, 12,
        "twelve off-diagonal monomials retain two ordered multiplication paths",
    );
    let unique_product_keys = survivors
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        unique_product_keys.len(),
        survivors.len(),
        "Schedule32 has no cross-term product reuse to manufacture into a DAG",
    );

    let mut cases = vec![
        ([0.5, -0.25, -0.875, 0.125], [2.0, 0.5, -0.75, 1.25, 1.75]),
        ([1.0, 0.5, -1.0, 0.25], [-0.5, 2.0, 0.25, -1.5, 1.0]),
    ];
    let mut state = 0xDAD3_8032;
    for _ in 0..6 {
        cases.push((
            std::array::from_fn(|_| deterministic_coefficient(&mut state)),
            std::array::from_fn(|_| deterministic_coefficient(&mut state)),
        ));
    }

    let mut rows = Vec::new();
    for execution in [
        PlanExecution::UnsharedTree,
        PlanExecution::CompactTerms,
        PlanExecution::SharedDag,
    ] {
        let source = canonical_source_for(execution);
        assert_eq!(
            source
                .matches("<Canonical50TypedBalancedSchedule32 as Eval5>::eval5(")
                .count(),
            1,
            "{execution:?} must consume the one exact typed Schedule32 root",
        );
        assert!(
            !source.contains("for triple in 0..80"),
            "{execution:?} must not rediscover candidates",
        );
        let bytes = compile_to_wasm(&source);
        let (values, shape) = call_lanes(&bytes, &cases);
        rows.push((execution, values, shape, bytes.len()));
    }

    for (case_index, (sphere, point)) in cases.iter().enumerate() {
        let raw = raw80(*sphere, *point);
        let expected = [raw[1], raw[2], raw[4], raw[8], raw[16]];
        for (execution, values, _, _) in &rows {
            for lane in 0..5 {
                let tolerance = 2.0e-5 * expected[lane].abs().max(1.0);
                assert!(
                    (values[case_index][lane] - expected[lane]).abs() <= tolerance,
                    "{execution:?} case {case_index} lane {lane}: got {}, expected {}",
                    values[case_index][lane],
                    expected[lane],
                );
            }
        }
        assert_eq!(
            rows[0].1[case_index], rows[1].1[case_index],
            "tree and compact-term executions must preserve exact f32 order",
        );
        assert_eq!(
            rows[1].1[case_index], rows[2].1[case_index],
            "compact-term and DAG executions must preserve exact f32 order",
        );
    }

    let tree = rows[0].2;
    let compact = rows[1].2;
    let dag = rows[2].2;
    assert_eq!(tree.5, 0);
    assert_eq!(compact.5, 0);
    assert_eq!(dag.5, 0);
    assert_eq!(
        tree.3, compact.3,
        "retaining both ordered products leaves no duplicate product to compact",
    );
    assert_eq!(
        compact.3, dag.3,
        "with no duplicate ordered-product keys, DAG interning cannot remove multiplies",
    );
    eprintln!(
        "CanonicalSandwichPlan32: 32 terms, {repeated_product_edges} repeated edges, \
         {} unique product keys; rows (strategy, shape, Wasm bytes) = {:?}",
        unique_product_keys.len(),
        rows.iter()
            .map(|(strategy, _, shape, bytes)| (strategy, shape, bytes))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn compact_terms_and_shared_dag_preserve_the_full_browser_profile_frame() {
    assert!(
        SCHEDULE32_REFERENCE.contains(&format!("\"fnv1a32\": {PINNED_REFERENCE_HASH}"))
            && SCHEDULE32_REFERENCE.contains("0.012500000186264515")
            && SCHEDULE32_REFERENCE.contains("\"inversion_center\": [\n    0.5,\n    0.0"),
        "the named pinned view/hash must remain anchored to gen-schedule32/reference.json",
    );
    let (compact, compact_wgsl) = browser_profile_frame(PlanExecution::CompactTerms);
    let (dag, dag_wgsl) = browser_profile_frame(PlanExecution::SharedDag);
    assert_eq!(
        compact, dag,
        "full 128x128 compact-term and shared-DAG frames",
    );
    assert_eq!(
        frame_fnv1a32(&compact),
        PINNED_REFERENCE_HASH,
        "frame must retain the independently generated Schedule32 reference hash",
    );
    eprintln!(
        "CanonicalSandwichPlan32 browser profile: 128x128 FNV {}, \
         compact WGSL {} bytes / {} lines, DAG WGSL {} bytes / {} lines",
        frame_fnv1a32(&compact),
        compact_wgsl.len(),
        compact_wgsl.lines().count(),
        dag_wgsl.len(),
        dag_wgsl.lines().count(),
    );
}

fn wasm_shape(bytes: &[u8]) -> (usize, usize, usize, usize, usize, usize) {
    let mut functions = 0;
    let mut adds = 0;
    let mut subs = 0;
    let mut muls = 0;
    let mut negs = 0;
    let mut loops = 0;
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let wasmparser::Payload::CodeSectionEntry(body) = payload.unwrap() {
            functions += 1;
            let mut ops = body.get_operators_reader().unwrap();
            while !ops.eof() {
                match ops.read().unwrap() {
                    wasmparser::Operator::F32Add => adds += 1,
                    wasmparser::Operator::F32Sub => subs += 1,
                    wasmparser::Operator::F32Mul => muls += 1,
                    wasmparser::Operator::F32Neg => negs += 1,
                    wasmparser::Operator::Loop { .. } => loops += 1,
                    _ => {}
                }
            }
        }
    }
    (functions, adds, subs, muls, negs, loops)
}

#[test]
fn canonical_helpers_publish_schedule32_and_emit_exact_five_lanes() {
    let source = canonical_source();
    let started = Instant::now();
    let wasm = compile_to_wasm(&source);
    let compile_elapsed = started.elapsed();
    wasmparser::validate(&wasm).expect("canonical FCO CGA emitted invalid Wasm");

    let survivors = canonical_survivors();
    assert_eq!(survivors.len(), 32, "independent canonical survivor count");
    let expected_hash = survivors
        .iter()
        .fold(17usize, |hash, triple| (hash * 83 + triple + 1) % 1_000_003)
        as i32;

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("valid Wasm module");
    assert!(
        module.imports().next().is_none(),
        "direct five-lane arithmetic should need no host imports"
    );
    let shape = wasm_shape(&wasm);
    // This counts MATERIALIZED multiplies at O0, which is deliberately
    // unoptimized: it measures the shape each lane emits before any folding, not
    // the schedule's intrinsic multiply count. It was 5 * 88 when the body was
    // the direct nine-scalar kernel; 9c15dc1c2 replaced that with the typed
    // canonical50 schedule, which materializes more shared subterms.
    //
    // Re-baselining an O0 count is only honest with the OPTIMIZED count beside
    // it, because that is the number G3 actually gates on. See below.
    eprintln!(
        "O0 materialized f32 muls: {} ({} per lane)",
        shape.3,
        shape.3 / 5
    );
    assert_eq!(
        shape.3,
        5 * 128,
        "each of the five independent O0 oracle exports must materialize the same \
         32 canonical monomials while retaining both ordered off-diagonal products"
    );

    // The G3 number. O0 materialization can rise while the shipped kernel gets
    // cheaper, which is exactly what happened here: the optimized artifact is
    // 1,723 bytes against the 2,147-byte schedule it replaced. Measure what
    // ships so a real multiply regression cannot hide behind an O0 re-baseline.
    let optimized = compile_to_wasm_at(&source, OptLevel::Os);
    let optimized_shape = wasm_shape(&optimized);
    eprintln!(
        "Os f32 muls: {} ({} per lane); bytes {}",
        optimized_shape.3,
        optimized_shape.3 / 5,
        optimized.len()
    );
    assert_eq!(
        shape.5, 0,
        "the specialized arithmetic must contain no runtime schedule loop"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("zero-import instance");
    let order_hash = instance
        .get_typed_func::<(), i32>(&mut store, "cga_schedule_order_hash")
        .expect("schedule hash export");
    assert_eq!(
        order_hash.call(&mut store, ()).unwrap(),
        expected_hash,
        "Fe Schedule32 survivor order must match the independent canonical scan"
    );

    type Args = (f32, f32, f32, f32, f32, f32, f32, f32, f32);
    let lanes = [
        ("cga_fco_e1", 1),
        ("cga_fco_e2", 2),
        ("cga_fco_e4", 4),
        ("cga_fco_e8", 8),
        ("cga_fco_e16", 16),
    ]
    .map(|(name, blade)| {
        (
            instance
                .get_typed_func::<Args, f32>(&mut store, name)
                .unwrap_or_else(|_| panic!("missing {name} export")),
            blade,
        )
    });

    let mut cases = vec![
        ([0.5, -0.25, -0.875, 0.125], [2.0, 0.5, -0.75, 1.25, 1.75]),
        ([1.0, 0.5, -1.0, 0.25], [-0.5, 2.0, 0.25, -1.5, 1.0]),
        (
            [-0.125, 0.75, 1.25, -0.5],
            [0.25, -1.75, 0.625, 2.0, -0.875],
        ),
    ];
    let mut state = 0xC0A5_8032;
    for _ in 0..12 {
        let sphere = std::array::from_fn(|_| deterministic_coefficient(&mut state));
        let point = std::array::from_fn(|_| deterministic_coefficient(&mut state));
        cases.push((sphere, point));
    }

    for (sphere, point) in cases {
        let expected = raw80(sphere, point);
        let args = (
            sphere[0], sphere[1], sphere[2], sphere[3], point[0], point[1], point[2], point[3],
            point[4],
        );
        for (lane, blade) in &lanes {
            let got = lane.call(&mut store, args).unwrap();
            let want = expected[*blade];
            let tolerance = 2.0e-5 * want.abs().max(1.0);
            assert!(
                (got - want).abs() <= tolerance,
                "blade {blade}: provider {got}, independent raw80 {want}, tolerance {tolerance}"
            );
        }
    }

    eprintln!(
        "canonical FCO CGA: {} bytes Wasm, compile {:?}, 80 candidates -> {} survivors -> 5 lanes; \
         functions/add/sub/mul/neg/loop = {:?}",
        wasm.len(),
        compile_elapsed,
        survivors.len(),
        shape,
    );
}
