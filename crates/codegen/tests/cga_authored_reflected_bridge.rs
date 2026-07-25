use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WasmCompileOptions, compile_runtime_package_wasm_with_options};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;

const AUTHORED_SOURCE: &str = include_str!("fixtures/recursive_clifford_consumer_ingot/src/lib.fe");
const REFLECTED_SOURCE: &str =
    include_str!("../../../demos/webgpu-cga-inversion/gen-schedule32/app/src/lib.fe");

type BladeArgs = (i32, f32, f32, f32, f32, f32, f32, f32, f32, f32);

#[derive(Debug)]
struct Artifact {
    label: &'static str,
    entry: &'static str,
    wasm: Vec<u8>,
    rmir_bytes: usize,
    rmir_calls: usize,
    f32_adds: usize,
    f32_muls: usize,
}

fn ingot_url(relative: &str) -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_ingot(label: &'static str, relative: &str, entry: &'static str) -> Artifact {
    let url = ingot_url(relative);
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "{label} ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .unwrap_or_else(|| panic!("{label} ingot"));
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected {label} diagnostics:\n{diagnostics}"
    );
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, entry)
        .unwrap_or_else(|error| panic!("{label} runtime package: {error}"));
    let rmir = mir::format_runtime_package(&db, &package);
    let wasm =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .unwrap_or_else(|error| panic!("{label} Wasm: {error}"))
            .bytes;
    wasmparser::validate(&wasm).unwrap_or_else(|error| panic!("{label} Wasm: {error}"));
    let (f32_adds, f32_muls) = wasm_f32_shape(&wasm);
    Artifact {
        label,
        entry,
        wasm,
        rmir_bytes: rmir.len(),
        rmir_calls: rmir.matches("call ").count(),
        f32_adds,
        f32_muls,
    }
}

fn wasm_f32_shape(bytes: &[u8]) -> (usize, usize) {
    let mut adds = 0;
    let mut muls = 0;
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let wasmparser::Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut operators = body.get_operators_reader().unwrap();
            while !operators.eof() {
                match operators.read().unwrap() {
                    wasmparser::Operator::F32Add => adds += 1,
                    wasmparser::Operator::F32Mul => muls += 1,
                    _ => {}
                }
            }
        }
    }
    (adds, muls)
}

fn raw_80(sphere: [f32; 4], point: [f32; 5]) -> [f32; 32] {
    let sphere_blades = [1usize, 2, 8, 16];
    let point_blades = [1usize, 2, 4, 8, 16];
    let mut out = [0.0; 32];
    for (li, &left) in sphere_blades.iter().enumerate() {
        for (pi, &middle) in point_blades.iter().enumerate() {
            for (ri, &right) in sphere_blades.iter().enumerate() {
                let mut negative = false;
                for (a, b) in [(left, middle), (left ^ middle, right)] {
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
                }
                let product = sphere[li] * point[pi] * sphere[ri];
                out[left ^ middle ^ right] += if negative { -product } else { product };
            }
        }
    }
    out
}

fn evaluate(artifact: &Artifact, cases: &[([f32; 4], [f32; 5])]) -> Vec<i32> {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &artifact.wasm).unwrap();
    assert!(
        module.imports().next().is_none(),
        "{} must be self-contained",
        artifact.label
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let coefficient = instance
        .get_typed_func::<BladeArgs, i32>(&mut store, artifact.entry)
        .unwrap();
    let mut outputs = Vec::with_capacity(cases.len() * 32);
    for &(sphere, point) in cases {
        for blade in 0..32 {
            outputs.push(
                coefficient
                    .call(
                        &mut store,
                        (
                            blade, sphere[0], sphere[1], sphere[2], sphere[3], point[0], point[1],
                            point[2], point[3], point[4],
                        ),
                    )
                    .unwrap(),
            );
        }
    }
    outputs
}

#[test]
fn ordinary_ingots_bridge_authored_recurrence_and_reflected_schedule32() {
    assert!(AUTHORED_SOURCE.contains("use sparse_clifford::{"));
    assert!(AUTHORED_SOURCE.contains("CliffordGp"));
    assert!(REFLECTED_SOURCE.contains("use sparse_clifford::{"));
    assert!(REFLECTED_SOURCE.contains("cga_schedule32_all_blades"));
    // These two assertions used to spell the invariant as
    // `derive Sandwich for ConformalVector using CanonicalCgaProvider` and
    // `impl Derive<Sandwich> for CanonicalCgaProvider`. Both spellings were
    // retired with the derive grammar, so they matched zero times and the test
    // was asserting the absence of syntax rather than the presence of a
    // property.
    //
    // The invariant itself is unchanged, and the artifact states it in its own
    // comment: the evidence view is "the same reflected aggregate Sandwich used
    // by the renderer ... without introducing a second plan or provider". So
    // assert exactly that: one aggregate, reached through one canonical plan.
    assert_eq!(
        REFLECTED_SOURCE
            .matches("impl Sandwich for ConformalVector")
            .count(),
        1,
        "the evidence selector must reuse one aggregate Sandwich, not add a second"
    );
    assert_eq!(
        REFLECTED_SOURCE
            .matches("Canonical50TypedBalancedSchedule32 as Eval5")
            .count(),
        1,
        "the reflected application must route through exactly one canonical plan"
    );
    for forbidden in ["include_str", "for triple in 0..80", "python"] {
        assert!(
            !AUTHORED_SOURCE.contains(forbidden),
            "authored consumer contains forbidden semantic reconstruction `{forbidden}`"
        );
        assert!(
            !REFLECTED_SOURCE.contains(forbidden),
            "reflected application contains forbidden semantic reconstruction `{forbidden}`"
        );
    }

    let authored = compile_ingot(
        "authored recurrence",
        "tests/fixtures/recursive_clifford_consumer_ingot",
        "authored_cl41_wasm",
    );
    let reflected = compile_ingot(
        "reflected Schedule32",
        "../../demos/webgpu-cga-inversion/gen-schedule32/app",
        "cga_schedule32_all_blades",
    );
    let cases = [
        ([0.5, -0.25, -0.875, 0.125], [2.0, 0.5, -0.75, 1.25, 1.75]),
        ([1.0, 0.5, -1.0, 0.25], [-0.5, 2.0, 0.25, -1.5, 1.0]),
    ];
    let expected: Vec<_> = cases
        .iter()
        .flat_map(|&(sphere, point)| raw_80(sphere, point))
        .map(|coefficient| (coefficient * 256.0) as i32)
        .collect();
    let authored_values = evaluate(&authored, &cases);
    let reflected_values = evaluate(&reflected, &cases);
    assert_eq!(
        authored_values, expected,
        "authored recurrence versus raw80"
    );
    assert_eq!(
        reflected_values, expected,
        "reflected Schedule32 versus raw80"
    );
    assert_eq!(
        authored_values, reflected_values,
        "ordinary-ingot implementations"
    );

    for artifact in [&authored, &reflected] {
        eprintln!(
            "{}: RMIR={} bytes/calls={}, Wasm={} bytes/f32.add={}/f32.mul={}",
            artifact.label,
            artifact.rmir_bytes,
            artifact.rmir_calls,
            artifact.wasm.len(),
            artifact.f32_adds,
            artifact.f32_muls,
        );
    }
}
