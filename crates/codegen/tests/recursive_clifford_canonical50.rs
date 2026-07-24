use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WasmCompileOptions, compile_runtime_package_wasm_with_options};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;

const SOURCE: &str = include_str!("fixtures/recursive_clifford_canonical50_ingot/src/lib.fe");
const PLANNER_SOURCE: &str = include_str!("../../../ingots/canonical_cl41_schedule/src/lib.fe");

fn gp_negative_cl41(a: usize, b: usize) -> bool {
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

fn sphere_pair_rank(a: usize, b: usize) -> usize {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    (0..lo).map(|left| 4 - left).sum::<usize>() + hi - lo
}

fn independent_canonical50() -> Vec<[i32; 7]> {
    let sphere_blades = [1usize, 2, 8, 16];
    let point_blades = [1usize, 2, 4, 8, 16];
    let mut coefficients = [0i32; 50];
    for (li, &left) in sphere_blades.iter().enumerate() {
        for (pi, &point) in point_blades.iter().enumerate() {
            for (ri, &right) in sphere_blades.iter().enumerate() {
                let candidate = sphere_pair_rank(li, ri) * 5 + pi;
                let negative =
                    gp_negative_cl41(left, point) ^ gp_negative_cl41(left ^ point, right);
                coefficients[candidate] += if negative { -1 } else { 1 };
            }
        }
    }

    coefficients
        .into_iter()
        .enumerate()
        .filter(|(_, coefficient)| *coefficient != 0)
        .map(|(candidate, coefficient)| {
            let pair = candidate / 5;
            let mut pairs = Vec::new();
            for left in 0..4 {
                for right in left..4 {
                    pairs.push((left, right));
                }
            }
            let (left, right) = pairs[pair];
            let point = candidate % 5;
            [
                candidate as i32,
                (sphere_blades[left] ^ point_blades[point] ^ sphere_blades[right]) as i32,
                sphere_blades[left] as i32,
                point_blades[point] as i32,
                sphere_blades[right] as i32,
                coefficient.unsigned_abs() as i32,
                i32::from(coefficient < 0),
            ]
        })
        .collect()
}

#[test]
fn public_recurrence_derives_exact_canonical50_schedule32() {
    assert_eq!(PLANNER_SOURCE.matches(".clifford_gp(").count(), 2);
    assert!(PLANNER_SOURCE.contains("use sparse_clifford::{"));
    assert!(PLANNER_SOURCE.contains("pub type Canonical50Schedule32 = SparsePlan<"));
    assert!(SOURCE.contains("use canonical_cl41_schedule::{"));
    assert_eq!(SOURCE.matches("const CANONICAL50_SIGN_").count(), 50);
    assert_eq!(SOURCE.matches("const CANONICAL50_OUTPUT_").count(), 50);
    assert_eq!(SOURCE.matches("= canonical50_projected_sign(").count(), 50);
    assert_eq!(SOURCE.matches("= candidate_output_blade_i32(").count(), 50);
    let runtime_projection = SOURCE
        .split("fn runtime_projected_sign")
        .nth(1)
        .expect("runtime scalar projection functions");
    assert!(!runtime_projection.contains("=> canonical50_projected_sign("));
    assert!(!runtime_projection.contains("=> candidate_output_blade("));
    for forbidden in [
        "gp_sign",
        "raw_",
        "triple",
        "support_gp",
        "SCHEDULE_KEEP",
        "python",
        "ImplBuilder",
        "include_str",
        "Term<0>",
    ] {
        assert!(
            !SOURCE.contains(forbidden),
            "dependent ingot must not reconstruct the schedule through `{forbidden}`"
        );
    }

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/recursive_clifford_canonical50_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("canonical50 ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics:\n{diagnostics}"
    );

    let package =
        mir::build_wasm_runtime_package_for_entry(&db, top_mod, "canonical50_schedule_field")
            .expect("canonical50 CTFE-derived runtime package");
    let runtime_ir = mir::format_runtime_package(&db, &package);
    for forbidden in [
        "Arith(BitAnd)",
        "Arith(BitOr)",
        "Arith(BitXor)",
        "Arith(Shl)",
        "Arith(Shr)",
        "Comp(NotEq)",
        "IntrinsicArith { op: Div",
        "IntrinsicArith { op: Rem",
        "carrier=int256",
    ] {
        assert!(
            !runtime_ir.contains(forbidden),
            "R1 inspector retained forbidden runtime operation `{forbidden}`"
        );
    }
    let wasm =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .unwrap_or_else(|error| panic!("canonical50 inspector Wasm: {error:?}\n{}", runtime_ir))
            .bytes;
    wasmparser::validate(&wasm).expect("canonical50 Wasm validates");

    let expected = independent_canonical50();
    assert_eq!(expected.len(), 32, "raw80 must reduce to Schedule32");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let field = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "canonical50_schedule_field")
        .unwrap();
    for (index, fields) in expected.iter().enumerate() {
        for (field_index, expected_value) in fields.iter().enumerate() {
            assert_eq!(
                field
                    .call(&mut store, (index as i32, field_index as i32))
                    .unwrap(),
                *expected_value,
                "schedule term {index}, field {field_index}"
            );
        }
    }
    assert_eq!(field.call(&mut store, (32, 0)).unwrap(), -1);
}
