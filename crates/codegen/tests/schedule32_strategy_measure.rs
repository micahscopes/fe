//! Reproducible reduced comparison of four Schedule32 publication/execution shapes.
//!
//! Run explicitly with:
//! `cargo test -p fe-codegen --test schedule32_strategy_measure -- --ignored --nocapture`
//!
//! Timings are observations, not assertions. Structural assertions distinguish
//! publishing the same recursive tree through FCO from genuine runtime DAG reuse.

use std::time::{Duration, Instant};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

const TREE_VOCABULARY: &str = r#"
struct Zero {}
struct Term {}
struct Add<L, R> {}
recursive type fn Schedule<const N: usize>() -> (*) {
    match N { 0 => Zero, _ => Add<Term, Schedule<{N - 1}>> }
}
trait Eval { fn eval(x: i32) -> i32 }
impl Eval for Zero {
    #[inline(always)]
    fn eval(x: i32) -> i32 { 0 }
}
impl Eval for Term {
    #[inline(always)]
    fn eval(x: i32) -> i32 { x }
}
impl<L: Eval, R: Eval> Eval for Add<L, R> {
    #[inline(always)]
    fn eval(x: i32) -> i32 {
        <L as Eval>::eval(x: x) + <R as Eval>::eval(x: x)
    }
}
"#;

fn recursive_tree() -> String {
    format!(
        "{TREE_VOCABULARY}\ntype Plan = Schedule<32>\n\
         pub fn run(x: i32) -> i32 {{ <Plan as Eval>::eval(x: x) }}\n"
    )
}

fn fco_published_tree() -> String {
    format!(
        r#"
use core::derive::{{Derive, Evidence, ImplBuilder, Reflect}}
{TREE_VOCABULARY}
type Plan = Schedule<32>
trait HasPlan {{ type Plan }}
struct Provider {{}}
impl Derive<HasPlan> for Provider {{
    const fn derive<T>(ev: own Evidence<HasPlan<T>>) -> Evidence<HasPlan<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<HasPlan<T>>,
        )
    {{
        builder.emit_assoc_ty("Plan", builder.ty<Plan>())
        builder.finish()
        ev
    }}
}}
struct Subject {{}}
derive HasPlan for Subject using Provider
type PublishedPlan = <Subject as HasPlan>::Plan
pub fn run(x: i32) -> i32 {{ <PublishedPlan as Eval>::eval(x: x) }}
"#
    )
}

fn fco_emitted_shared_dag() -> String {
    r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Execute32 { fn execute(_ x: i32) -> i32 }
struct SharedDagProvider {}
impl Derive<Execute32> for SharedDagProvider {
    const fn derive<T>(
        ev: own Evidence<Execute32<T>>,
    ) -> Evidence<Execute32<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Execute32<T>>,
        )
    {
        // Measured control: publish a known shared shape. This deliberately
        // does not claim to normalize or inspect Schedule<32>.
        builder.emit_method("execute", quote(x) {
            let x2 = x + x
            let x4 = x2 + x2
            let x8 = x4 + x4
            let x16 = x8 + x8
            x16 + x16
        })
        builder.finish()
        ev
    }
}
struct Published32 {}
derive Execute32 for Published32 using SharedDagProvider
pub fn run(x: i32) -> i32 {
    <Published32 as Execute32>::execute(x)
}
"#
    .to_string()
}

const COMPACT: &str = r#"
pub fn run(x: i32) -> i32 {
    let mut sum: i32 = 0
    let mut i: i32 = 0
    while i < 32 {
        sum = sum + x
        i = i + 1
    }
    sum
}
"#;

const SHARED_DAG: &str = r#"
pub fn run(x: i32) -> i32 {
    let x2: i32 = x + x
    let x4: i32 = x2 + x2
    let x8: i32 = x4 + x4
    let x16: i32 = x8 + x8
    x16 + x16
}
"#;

#[test]
fn fco_can_publish_an_explicit_shared_schedule32_body() {
    let source = fco_emitted_shared_dag();
    assert!(source.contains("builder.emit_method"));
    assert!(source.contains("let x16"));
    let measurement = measure("fco_emitted_shared_dag_smoke", source);
    assert!(
        measurement.rmir_calls <= 1,
        "the only permitted call-shaped residue is the exported facade calling its derived method"
    );
}

#[derive(Debug)]
struct Measurement {
    strategy: &'static str,
    hir: Duration,
    package: Duration,
    backend: Duration,
    rmir_bytes: usize,
    rmir_calls: usize,
    wasm_bytes: usize,
}

fn measure(strategy: &'static str, source: String) -> Measurement {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///schedule32_{strategy}.fe")).unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);

    let started = Instant::now();
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    let hir = started.elapsed();
    assert!(diagnostics.is_empty(), "{strategy} HIR:\n{diagnostics}");

    let started = Instant::now();
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "run")
        .unwrap_or_else(|error| panic!("{strategy} package: {error}"));
    let package_time = started.elapsed();
    let rmir = mir::format_runtime_package(&db, &package);

    let started = Instant::now();
    let wasm = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .unwrap_or_else(|error| panic!("{strategy} Wasm: {error}"))
        .into_bytecode()
        .expect("Wasm bytecode");
    let backend = started.elapsed();
    wasmparser::validate(&wasm).unwrap();

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let run = instance
        .get_typed_func::<i32, i32>(&mut store, "run")
        .unwrap();
    assert_eq!(run.call(&mut store, 7).unwrap(), 224);

    Measurement {
        strategy,
        hir,
        package: package_time,
        backend,
        rmir_bytes: rmir.len(),
        rmir_calls: rmir.matches("call ").count(),
        wasm_bytes: wasm.len(),
    }
}

#[test]
#[ignore = "measurement harness; run explicitly and inspect reported timings"]
fn compare_schedule32_tree_compact_fco_and_actual_dag() {
    let tree_source = recursive_tree();
    let fco_source = fco_published_tree();
    assert!(tree_source.contains("<Plan as Eval>::eval"));
    assert!(fco_source.contains("<PublishedPlan as Eval>::eval"));
    assert!(
        fco_source.contains("builder.emit_assoc_ty(\"Plan\", builder.ty<Plan>())"),
        "FCO publishes Plan itself; it does not create a shared runtime graph"
    );
    assert_eq!(SHARED_DAG.matches("let x").count(), 4);
    assert!(!SHARED_DAG.contains("Schedule<32>"));

    let emitted_dag_source = fco_emitted_shared_dag();
    assert!(emitted_dag_source.contains("builder.emit_method"));
    assert!(emitted_dag_source.contains("let x16"));
    let measurements = [
        measure("recursive_tree", tree_source),
        measure("compact_loop", COMPACT.to_string()),
        measure("fco_published_tree", fco_source),
        measure("shared_dag", SHARED_DAG.to_string()),
        measure("fco_emitted_shared_dag", emitted_dag_source),
    ];
    assert_eq!(
        measurements[0].rmir_bytes, measurements[2].rmir_bytes,
        "FCO publication must not be mistaken for a different normalized graph"
    );
    assert_eq!(
        measurements[0].rmir_calls, measurements[2].rmir_calls,
        "direct and FCO-published trees must retain the same call graph"
    );
    assert!(measurements[0].rmir_calls > 0);
    assert_eq!(measurements[1].rmir_calls, 0);
    assert_eq!(measurements[3].rmir_calls, 0);
    assert!(
        measurements[4].rmir_calls < measurements[0].rmir_calls,
        "publishing an explicitly shared body must retain less call-shaped work than the tree"
    );
    eprintln!(
        "strategy                 HIR ms   package ms   backend ms   RMIR B   calls   Wasm B"
    );
    for m in measurements {
        eprintln!(
            "{:<24} {:>7.2} {:>12.2} {:>12.2} {:>8} {:>7} {:>8}",
            m.strategy,
            m.hir.as_secs_f64() * 1000.0,
            m.package.as_secs_f64() * 1000.0,
            m.backend.as_secs_f64() * 1000.0,
            m.rmir_bytes,
            m.rmir_calls,
            m.wasm_bytes,
        );
    }
}
