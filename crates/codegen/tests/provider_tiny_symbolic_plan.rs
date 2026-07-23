use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

fn compile_to_wasm(source: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///provider_tiny_symbolic_plan.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected tiny-plan diagnostics:\n{diagnostics}"
    );
    BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("tiny provider plan should compile to Wasm")
        .into_bytecode()
        .expect("Wasm output should be bytecode")
}

#[test]
fn ordinary_const_helpers_drive_typed_selection_and_direct_provider_arithmetic() {
    let source = r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

// One semantic source shared by the recursive type selector and the provider.
// These are formulas over the candidate index, not a survivor/expression table.
const fn keep_tag(_ triple: usize) -> usize {
    if triple != 1 { 1 } else { 0 }
}
const fn magnitude(_ triple: usize) -> usize { triple + 1 }
const fn negative(_ triple: usize) -> bool { triple > 1 }

struct Zero {}
struct Term<const I: usize> {}
struct Add<L, R> {}
struct Select<const K: usize, const I: usize, R> {}
trait SelectOut { type Out }
impl<const I: usize, R> SelectOut for Select<0, I, R> { type Out = R }
impl<const I: usize, R> SelectOut for Select<1, I, R> {
    type Out = Add<Term<I>, R>
}

recursive type fn Plan<const N: usize>() -> (*) {
    match N {
        0 => Zero
        _ => <Select<
            { keep_tag(N - 1) },
            {N - 1},
            Plan<{N - 1}>,
        > as SelectOut>::Out
    }
}

// A compile-time type equality witness: candidates 2 and 0 survive.
type TypedPlan = Plan<3>
type ExpectedPlan = Add<Term<2>, Add<Term<0>, Zero>>
fn exact_typed_plan(value: TypedPlan) -> ExpectedPlan { value }

trait Execute { fn execute(_ x: i32) -> i32 }
struct TinyProvider {}
impl Derive<Execute> for TinyProvider {
    const fn derive<T>(ev: own Evidence<Execute<T>>) -> Evidence<Execute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Execute<T>>)
    {
        let sum = builder.int(0)
        for triple in 0..3 {
            if keep_tag(triple) != 0 {
                let x = builder.arg_ref("x")
                let coefficient = builder.int(magnitude(triple))
                let term = builder.mul(x, coefficient)
                if negative(triple) {
                    term = builder.neg(term)
                }
                sum = builder.add(sum, term)
            }
        }
        builder.emit_method("execute", sum)
        builder.finish()
        ev
    }
}

struct Subject {}
derive Execute for Subject using TinyProvider

pub fn run(x: i32) -> i32 {
    <Subject as Execute>::execute(x)
}
"#;

    let wasm = compile_to_wasm(source);
    wasmparser::validate(&wasm).expect("tiny plan emitted invalid Wasm");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("valid Wasm module");
    assert!(
        module.imports().next().is_none(),
        "the direct arithmetic proof should need no host imports"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("zero-import instance");
    let run = instance
        .get_typed_func::<i32, i32>(&mut store, "run")
        .expect("run export");

    // Kept terms are +1*x and -3*x, hence -2*x.
    assert_eq!(run.call(&mut store, 7).unwrap(), -14);
    assert_eq!(run.call(&mut store, -5).unwrap(), 10);
}
