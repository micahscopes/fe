//! Semantic substitution gate for the reusable finite GA compiler.
//!
//! The fixture closes one unchanged Fe provider over unrelated trees and
//! metrics. Rust independently interprets the exact authored trees; generated
//! byte shape is not a correctness oracle.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, compile_runtime_package_spirv_render, layout_for};
use hir::hir_def::HirIngot;
use std::{fs, path::Path};
use url::Url;

fn grade(blade: usize) -> usize {
    blade.count_ones() as usize
}

fn gp_blade(left: usize, right: usize, negative_squares: usize) -> (usize, bool) {
    let mut inversions = 0usize;
    for bit in 0..5 {
        if (left >> bit) & 1 == 1 {
            inversions += grade(right & ((1 << bit) - 1));
        }
    }
    let metric = grade(left & right & negative_squares);
    (left ^ right, ((inversions + metric) & 1) == 1)
}

fn add(left: &[f32], right: &[f32]) -> Vec<f32> {
    left.iter().zip(right).map(|(l, r)| *l + *r).collect()
}

fn sub(left: &[f32], right: &[f32]) -> Vec<f32> {
    left.iter().zip(right).map(|(l, r)| *l - *r).collect()
}

fn neg(value: &[f32]) -> Vec<f32> {
    value.iter().map(|value| -*value).collect()
}

fn reverse(value: &[f32]) -> Vec<f32> {
    value
        .iter()
        .enumerate()
        .map(|(blade, value)| {
            if (grade(blade) * grade(blade).saturating_sub(1) / 2) & 1 == 1 {
                -*value
            } else {
                *value
            }
        })
        .collect()
}

fn geometric(
    left: &[f32],
    right: &[f32],
    nonzero_squares: usize,
    negative_squares: usize,
) -> Vec<f32> {
    let mut output = vec![0.0; left.len()];
    for (l, lhs) in left.iter().enumerate() {
        for (r, rhs) in right.iter().enumerate() {
            let shared = l & r;
            if shared & nonzero_squares == shared {
                let (blade, negative) = gp_blade(l, r, negative_squares);
                let term = *lhs * *rhs;
                output[blade] += if negative { -term } else { term };
            }
        }
    }
    output
}

fn outer(left: &[f32], right: &[f32]) -> Vec<f32> {
    let mut output = vec![0.0; left.len()];
    for (l, lhs) in left.iter().enumerate() {
        for (r, rhs) in right.iter().enumerate() {
            if l & r == 0 {
                let (blade, negative) = gp_blade(l, r, 0);
                let term = *lhs * *rhs;
                output[blade] += if negative { -term } else { term };
            }
        }
    }
    output
}

fn scalar_product(
    left: &[f32],
    right: &[f32],
    nonzero_squares: usize,
    negative_squares: usize,
) -> Vec<f32> {
    let product = geometric(left, right, nonzero_squares, negative_squares);
    let mut output = vec![0.0; left.len()];
    output[0] = product[0];
    output
}

fn contraction(
    left: &[f32],
    right: &[f32],
    nonzero_squares: usize,
    negative_squares: usize,
    left_directed: bool,
) -> Vec<f32> {
    let mut output = vec![0.0; left.len()];
    for (l, lhs) in left.iter().enumerate() {
        for (r, rhs) in right.iter().enumerate() {
            let shared = l & r;
            let out = l ^ r;
            let grade_rule = if left_directed {
                grade(l) <= grade(r) && grade(out) == grade(r) - grade(l)
            } else {
                grade(r) <= grade(l) && grade(out) == grade(l) - grade(r)
            };
            if shared & nonzero_squares == shared && grade_rule {
                let (_, negative) = gp_blade(l, r, negative_squares);
                let term = *lhs * *rhs;
                output[out] += if negative { -term } else { term };
            }
        }
    }
    output
}

fn dual(value: &[f32]) -> Vec<f32> {
    let pseudoscalar = value.len() - 1;
    let mut output = vec![0.0; value.len()];
    for (blade, component) in value.iter().enumerate() {
        let complement = pseudoscalar ^ blade;
        let (_, negative) = gp_blade(blade, complement, 0);
        output[complement] = if negative { -*component } else { *component };
    }
    output
}

fn regressive(left: &[f32], right: &[f32]) -> Vec<f32> {
    dual(&outer(&dual(left), &dual(right)))
}

fn compact(value: &[f32], support: usize) -> Vec<f32> {
    value
        .iter()
        .enumerate()
        .filter_map(|(blade, value)| ((support >> blade) & 1 == 1).then_some(*value))
        .collect()
}

fn mixed_expected(args: [f32; 6]) -> Vec<f32> {
    let a = [0.0, args[0], args[1], 0.0, 0.0, 0.0, 0.0, 0.0];
    let b = [args[2], 0.0, 0.0, 0.0, args[3], 0.0, 0.0, 0.0];
    let c = [0.0, args[4], 0.0, 0.0, 0.0, 0.0, args[5], 0.0];
    let gp = geometric(&add(&a, &b), &reverse(&c), 7, 0);
    let mut projected = vec![0.0; 8];
    for blade in 0..8 {
        if grade(blade) == 1 {
            projected[blade] = gp[blade];
        }
    }
    compact(&add(&projected, &neg(&outer(&a, &c))), 158)
}

fn signed_expected(args: [f32; 5]) -> Vec<f32> {
    let x = [args[0], args[1], args[2], 0.0];
    let y = [0.0, args[3], args[4], 0.0];
    compact(
        &scalar_product(&sub(&x, &y), &reverse(&add(&x, &y)), 3, 2),
        1,
    )
}

fn dual_join_expected(args: [f32; 4]) -> Vec<f32> {
    let p = [0.0, 0.0, 0.0, args[0], 0.0, args[1], 0.0, 0.0];
    let q = [0.0, 0.0, 0.0, 0.0, 0.0, args[2], args[3], 0.0];
    compact(&add(&dual(&p), &regressive(&p, &q)), 22)
}

fn directed_expected(args: [f32; 7]) -> Vec<f32> {
    let l = [args[0], args[1], 0.0, args[2], 0.0, 0.0, 0.0, 0.0];
    let r = [0.0, args[3], args[4], args[5], 0.0, 0.0, 0.0, args[6]];
    compact(
        &add(
            &contraction(&l, &r, 7, 0, true),
            &contraction(&l, &r, 7, 0, false),
        ),
        223,
    )
}

#[test]
fn one_fe_provider_executes_unrelated_finite_ga_programs() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ga_expr_compile_fco");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let ingot = db.workspace().containing_ingot(&db, url).unwrap();
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "GA compiler diagnostics:\n{diagnostics}"
    );

    let wasm = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("finite GA programs compile to Wasm")
        .into_bytecode()
        .expect("Wasm bytecode");
    wasmparser::validate(&wasm).expect("finite GA Wasm validates");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(
        module.imports().next().is_none(),
        "no host algebra implementation"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let mixed = instance
        .get_typed_func::<(f32, f32, f32, f32, f32, f32), (f32, f32, f32, f32, f32)>(
            &mut store,
            "mixed_probe",
        )
        .expect("compact mixed-program ABI");
    let signed = instance
        .get_typed_func::<(f32, f32, f32, f32, f32), f32>(&mut store, "signed_scalar_probe")
        .expect("compact scalar-program ABI");
    let dual_join = instance
        .get_typed_func::<(f32, f32, f32, f32), (f32, f32, f32)>(&mut store, "dual_join_probe")
        .expect("compact dual/join ABI");
    let directed = instance
        .get_typed_func::<(f32, f32, f32, f32, f32, f32, f32), (f32, f32, f32, f32, f32, f32, f32)>(
            &mut store,
            "directed_probe",
        )
        .expect("compact directed-contraction ABI");

    let mut state = 0xa834_9d27_u32;
    let mut next = || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((state >> 10) as i32 as f32) / 65_536.0 - 32.0
    };
    for _ in 0..257 {
        let a = [next(), next(), next(), next(), next(), next()];
        let got = mixed.call(&mut store, a.into()).unwrap();
        let got = [got.0, got.1, got.2, got.3, got.4];
        assert_eq!(
            got.map(f32::to_bits).as_slice(),
            mixed_expected(a)
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            "mixed expression drifted for {a:?}",
        );

        let b = [next(), next(), next(), next(), next()];
        let got = signed.call(&mut store, b.into()).unwrap();
        assert_eq!(
            got.to_bits(),
            signed_expected(b)[0].to_bits(),
            "signed scalar expression drifted for {b:?}",
        );

        let c = [next(), next(), next(), next()];
        let got = dual_join.call(&mut store, c.into()).unwrap();
        let got = [got.0, got.1, got.2];
        assert_eq!(
            got.map(f32::to_bits).as_slice(),
            dual_join_expected(c)
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            "dual/join expression drifted for {c:?}",
        );

        let d = [next(), next(), next(), next(), next(), next(), next()];
        let got = directed.call(&mut store, d.into()).unwrap();
        let got = [got.0, got.1, got.2, got.3, got.4, got.5, got.6];
        assert_eq!(
            got.map(f32::to_bits).as_slice(),
            directed_expected(d)
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            "directed contractions drifted for {d:?}",
        );
    }

    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "finite_ga_render")
        .expect("finite GA render runtime package");
    let artifact = compile_runtime_package_spirv_render(&db, &package)
        .expect("finite GA expression compiles through render SPIR-V");
    assert_eq!(artifact.words.first().copied(), Some(0x0723_0203));
    let wgsl = artifact.wgsl.expect("finite GA browser WGSL");
    let module = naga::front::wgsl::parse_str(&wgsl).expect("finite GA WGSL reparses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("finite GA WGSL validates with browser-default capabilities");
    assert!(
        !wgsl.contains("loop {") && !wgsl.contains("if (") && !wgsl.contains("switch"),
        "compile-time GA planning must leave no runtime control flow:\n{wgsl}"
    );
}

fn rejection_diagnostics(source: &str) -> String {
    let fixture_parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let temp = tempfile::Builder::new()
        .prefix("fe-ga-expression-rejection-")
        .tempdir_in(fixture_parent)
        .expect("temporary rejection ingot");
    let root = temp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("fe.toml"),
        "[ingot]\nname = \"ga_expr_rejection\"\nversion = \"0.1.0\"\n\n\
         [dependencies]\ncore = \"../../../../../ingots/core\"\n\
         ga_expr = \"../../../../../ingots/ga_expr\"\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.fe"), source).unwrap();
    let url = Url::from_directory_path(root.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    driver::init_ingot(&mut db, &url);
    let ingot = db.workspace().containing_ingot(&db, url).unwrap();
    db.run_on_top_mod(ingot.root_mod(&db)).format_diags(&db)
}

#[test]
fn malformed_or_unsupported_programs_fail_closed() {
    let unknown = rejection_diagnostics(
        r#"
use ga_expr::{
    CompileGaF32, EvaluateGaF32, GaProgram, MultivectorInput,
    SignedOrthogonalMetric, Strict,
}
struct Coefficients { e0: f32 }
struct Unknown<A> {}
type Leaf = MultivectorInput<Coefficients, 2>
struct Operands { leaf: Coefficients }
derive EvaluateGaF32 for Operands using CompileGaF32<
    GaProgram<Unknown<Leaf>, SignedOrthogonalMetric<1, 1, 0>, Strict>
>
"#,
    );
    assert!(
        unknown.contains("failed to emit")
            || unknown.contains("doesn't implement")
            || unknown.contains("failed to derive")
            || unknown.contains("not all trait methods are implemented"),
        "an unknown operator must not disappear as a transparent wrapper:\n{unknown}"
    );

    let duplicate_identity = rejection_diagnostics(
        r#"
use ga_expr::{
    CompileGaF32, EvaluateGaF32, GaProgram, MultivectorInput,
    SignedOrthogonalMetric, Strict,
}
struct Coefficients { e0: f32 }
type Leaf = MultivectorInput<Coefficients, 2>
struct Operands { first: Coefficients, second: Coefficients }
derive EvaluateGaF32 for Operands using CompileGaF32<
    GaProgram<Leaf, SignedOrthogonalMetric<1, 1, 0>, Strict>
>
"#,
    );
    assert!(
        duplicate_identity.contains("failed to emit")
            || duplicate_identity.contains("doesn't implement")
            || duplicate_identity.contains("failed to derive")
            || duplicate_identity.contains("not all trait methods are implemented"),
        "ambiguous nominal leaf binding must fail closed:\n{duplicate_identity}"
    );

    let unsupported_policy = rejection_diagnostics(
        r#"
use ga_expr::{
    AlgebraicBalanced, CompileGaF32, EvaluateGaF32, GaProgram,
    MultivectorInput, SignedOrthogonalMetric,
}
struct Coefficients { e0: f32 }
type Leaf = MultivectorInput<Coefficients, 2>
struct Operands { leaf: Coefficients }
derive EvaluateGaF32 for Operands using CompileGaF32<
    GaProgram<Leaf, SignedOrthogonalMetric<1, 1, 0>, AlgebraicBalanced>
>
"#,
    );
    assert!(
        unsupported_policy.contains("failed to emit")
            || unsupported_policy.contains("doesn't implement")
            || unsupported_policy.contains("failed to derive")
            || unsupported_policy.contains("not all trait methods are implemented"),
        "an unimplemented numeric policy must fail closed:\n{unsupported_policy}"
    );

    let invalid_support = rejection_diagnostics(
        r#"
use ga_expr::{
    CompileGaF32, EvaluateGaF32, GaProgram, MultivectorInput,
    SignedOrthogonalMetric, Strict,
}
struct Coefficients { impossible_blade: f32 }
type Leaf = MultivectorInput<Coefficients, 16>
struct Operands { leaf: Coefficients }
derive EvaluateGaF32 for Operands using CompileGaF32<
    GaProgram<Leaf, SignedOrthogonalMetric<2, 3, 0>, Strict>
>
"#,
    );
    assert!(
        invalid_support.contains("not all trait methods are implemented"),
        "support outside the configured algebra must fail closed:\n{invalid_support}"
    );

    let invalid_metric = rejection_diagnostics(
        r#"
use ga_expr::{
    CompileGaF32, EvaluateGaF32, GaProgram, MultivectorInput,
    SignedOrthogonalMetric, Strict,
}
struct Coefficients { e0: f32 }
type Leaf = MultivectorInput<Coefficients, 2>
struct Operands { leaf: Coefficients }
derive EvaluateGaF32 for Operands using CompileGaF32<
    GaProgram<Leaf, SignedOrthogonalMetric<2, 1, 2>, Strict>
>
"#,
    );
    assert!(
        invalid_metric.contains("not all trait methods are implemented"),
        "a negative zero-square generator must fail closed:\n{invalid_metric}"
    );
}
