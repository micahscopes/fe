use std::time::Instant;

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{layout_for, BackendKind, OptLevel};
use url::Url;

const SPARSE_CLIFFORD_API: &str = include_str!("../../../ingots/sparse_clifford/src/lib.fe");
const CANONICAL: &str = include_str!("fixtures/fco_cga80_direct_lanes.fe");

fn canonical_source() -> String {
    assert!(
        CANONICAL.contains("builder.ty<Schedule32>().normalized_preorder_types()")
            && !CANONICAL.contains("for triple in 0..80"),
        "the FCO provider must consume the typed plan rather than rescan raw80",
    );
    let sparse_api = fe_codegen::standalone_ctfe_ingot_source(SPARSE_CLIFFORD_API);
    format!("{sparse_api}\n{CANONICAL}")
}

#[derive(Clone, Copy, Debug)]
enum PlanExecution {
    UnsharedTree,
    CompactTerms,
    SharedDag,
}

fn canonical_source_for(execution: PlanExecution) -> String {
    let source = canonical_source();
    let shared_product =
        "let product = builder.share(builder.mul(builder.mul(left, point), right))";
    assert_eq!(
        source.matches(shared_product).count(),
        1,
        "the strategy seam must identify exactly one canonical product emission site",
    );
    match execution {
        PlanExecution::UnsharedTree => source.replace(
            shared_product,
            "let product = builder.mul(builder.mul(left, point), right)",
        ),
        PlanExecution::CompactTerms => source.replace(
            shared_product,
            "let product = builder.mul(builder.mul(left, point), right)\n\
             if magnitude(triple) == 2 { product = builder.share(product) }",
        ),
        PlanExecution::SharedDag => source,
    }
}

fn compile_to_wasm(source: &str) -> Vec<u8> {
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
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
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

fn canonical_survivors() -> Vec<usize> {
    let mut survivors = Vec::new();
    for triple in 0..80 {
        let left_slot = triple / 20;
        let point_slot = (triple / 4) % 5;
        let right_slot = triple % 4;
        let left = sphere_blade(left_slot);
        let middle = point_blade(point_slot);
        let right = sphere_blade(right_slot);
        let negative = gp_negative(left, middle) ^ gp_negative(left ^ middle, right);
        let reverse_negative = gp_negative(right, middle) ^ gp_negative(right ^ middle, left);
        if left_slot <= right_slot && negative == reverse_negative {
            survivors.push(triple);
        }
    }
    survivors
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

#[test]
fn one_reflected_plan_drives_tree_compact_terms_and_honest_shared_dag() {
    let survivors = canonical_survivors();
    assert_eq!(survivors.len(), 32);
    let repeated_product_edges = survivors
        .iter()
        .filter(|&&triple| triple / 20 != triple % 4)
        .count();
    assert_eq!(
        repeated_product_edges, 12,
        "only off-diagonal canonical terms reuse their product for magnitude two",
    );
    let unique_product_keys = survivors
        .iter()
        .map(|&triple| (triple / 20, (triple / 4) % 5, triple % 4))
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
                .matches("builder.ty<Schedule32>().normalized_preorder_types()")
                .count(),
            1,
            "{execution:?} must consume the one reflected Schedule32 witness",
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
    assert!(
        tree.3 > compact.3,
        "unshared tree must repeat magnitude-two product expressions",
    );
    assert_eq!(
        compact.3, dag.3,
        "with no cross-term duplicate keys, full DAG interning cannot remove more multiplies",
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
    assert_eq!(
        shape.3,
        5 * 64,
        "each of the five independent O0 oracle exports must materialize the same \
         32 canonical products with exactly two f32 multiplies each"
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
