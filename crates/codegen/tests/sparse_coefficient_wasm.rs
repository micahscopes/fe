use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

const CL41_GRADE1_SOURCE: &str = include_str!("../../hir/tests/fixtures/sparse_cl41_grade1.fe");

const SOURCE: &str = r#"
struct Missing {}
struct Found<const Slot: usize> {}
struct Select<const Present: usize, const Slot: usize> {}

trait SelectOut { type Out }
impl<const Slot: usize> SelectOut for Select<0, Slot> { type Out = Missing }
impl<const Slot: usize> SelectOut for Select<1, Slot> { type Out = Found<Slot> }

const fn present(_ mask: usize, _ blade: usize) -> usize {
    (mask >> blade) & 1
}
const fn rank(_ mask: usize, _ blade: usize) -> usize {
    match blade {
        0 => 0
        1 => mask & 1
        2 => (mask & 1) + ((mask >> 1) & 1)
        3 => (mask & 1) + ((mask >> 1) & 1) + ((mask >> 2) & 1)
        4 => (mask & 1) + ((mask >> 1) & 1) + ((mask >> 2) & 1) + ((mask >> 3) & 1)
        5 => (mask & 1) + ((mask >> 1) & 1) + ((mask >> 2) & 1) + ((mask >> 3) & 1) + ((mask >> 4) & 1)
        6 => (mask & 1) + ((mask >> 1) & 1) + ((mask >> 2) & 1) + ((mask >> 3) & 1) + ((mask >> 4) & 1) + ((mask >> 5) & 1)
        _ => (mask & 1) + ((mask >> 1) & 1) + ((mask >> 2) & 1) + ((mask >> 3) & 1) + ((mask >> 4) & 1) + ((mask >> 5) & 1) + ((mask >> 6) & 1)
    }
}

recursive type fn FindA<const Blade: usize>() -> (*) {
    match Blade {
        0 => <Select<{present(146, 0)}, {rank(146, 0)}> as SelectOut>::Out
        1 => <Select<{present(146, 1)}, {rank(146, 1)}> as SelectOut>::Out
        2 => <Select<{present(146, 2)}, {rank(146, 2)}> as SelectOut>::Out
        3 => <Select<{present(146, 3)}, {rank(146, 3)}> as SelectOut>::Out
        4 => <Select<{present(146, 4)}, {rank(146, 4)}> as SelectOut>::Out
        5 => <Select<{present(146, 5)}, {rank(146, 5)}> as SelectOut>::Out
        6 => <Select<{present(146, 6)}, {rank(146, 6)}> as SelectOut>::Out
        7 => <Select<{present(146, 7)}, {rank(146, 7)}> as SelectOut>::Out
        _ => FindA<0>
    }
}

struct Compact4 { c0: i32, c1: i32, c2: i32, c3: i32 }
trait Coefficient { fn read(compact: Compact4) -> i32 }
impl Coefficient for Missing { fn read(compact: Compact4) -> i32 { 0 } }
impl Coefficient for Found<0> { fn read(compact: Compact4) -> i32 { compact.c0 } }
impl Coefficient for Found<1> { fn read(compact: Compact4) -> i32 { compact.c1 } }
impl Coefficient for Found<2> { fn read(compact: Compact4) -> i32 { compact.c2 } }
impl Coefficient for Found<3> { fn read(compact: Compact4) -> i32 { compact.c3 } }

#[inline(always)]
fn coefficient_at<Lookup: Coefficient>(value: Compact4) -> i32 {
    <Lookup as Coefficient>::read(compact: value)
}

pub fn sparse_present(c0: i32, c1: i32, c2: i32, c3: i32) -> i32 {
    coefficient_at<FindA<4>>(value: Compact4 { c0: c0, c1: c1, c2: c2, c3: c3 })
}
pub fn sparse_absent(c0: i32, c1: i32, c2: i32, c3: i32) -> i32 {
    coefficient_at<FindA<3>>(value: Compact4 { c0: c0, c1: c1, c2: c2, c3: c3 })
}
"#;

#[test]
fn ground_sparse_coefficient_access_compiles_to_wasm() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///sparse_coefficient_wasm.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("ground sparse coefficient access should compile")
        .into_bytecode()
        .expect("Wasm bytecode");
    assert!(bytes.starts_with(b"\0asm"));

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes).expect("valid Wasm module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("zero-import instance");
    let present = instance
        .get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "sparse_present")
        .expect("present coefficient export");
    let absent = instance
        .get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "sparse_absent")
        .expect("absent coefficient export");
    assert_eq!(
        present.call(&mut store, (11, 44, 77, 99)).unwrap(),
        44,
        "blade 4 has compact rank 1 in support [1, 4, 7]"
    );
    assert_eq!(
        absent.call(&mut store, (11, 44, 77, 99)).unwrap(),
        0,
        "missing blades read as zero without a runtime membership branch"
    );
}

#[test]
fn cl41_32_blade_grade_pruned_coefficients_execute_in_wasm() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///sparse_cl41_grade1.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(CL41_GRADE1_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("ground Cl(4,1) sparse access should compile")
        .into_bytecode()
        .expect("Wasm bytecode");

    let selector_arms = CL41_GRADE1_SOURCE
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.contains("=>")
                && line
                    .split_whitespace()
                    .next()
                    .is_some_and(|word| word.parse::<u32>().is_ok())
        })
        .count();
    assert_eq!(
        selector_arms, 32,
        "one explicit selector arm per Cl(4,1) blade"
    );
    assert!(
        CL41_GRADE1_SOURCE.len() < 10_000,
        "the explicit 32-blade source selector should remain inspectably small"
    );
    assert!(
        bytes.len() < 16_384,
        "ground specialization should not retain a large runtime selector: {} bytes",
        bytes.len()
    );
    eprintln!(
        "Cl(4,1) sparse selector: {selector_arms} arms, {} source bytes, {} Wasm bytes",
        CL41_GRADE1_SOURCE.len(),
        bytes.len()
    );

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes).expect("valid Wasm module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("zero-import instance");
    let e5 = instance
        .get_typed_func::<(i32, i32, i32, i32, i32), i32>(&mut store, "cl41_e5")
        .expect("e5 coefficient export");
    let bivector = instance
        .get_typed_func::<(i32, i32, i32, i32, i32), i32>(&mut store, "cl41_bivector_default_zero")
        .expect("default-zero coefficient export");
    assert_eq!(e5.call(&mut store, (10, 20, 30, 40, 50)).unwrap(), 50);
    assert_eq!(bivector.call(&mut store, (10, 20, 30, 40, 50)).unwrap(), 0);
}
