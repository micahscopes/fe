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
fn shared_provider_expression_materializes_once_per_method_root() {
    let source = r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait Pair {
    fn a(_ x: i32, _ __fco_provider_share_2: i32) -> i32
    fn b(_ x: i32, _ __fco_provider_share_2: i32) -> i32
}
struct PairProvider {}
impl Derive<Pair> for PairProvider {
    const fn derive<T>(ev: own Evidence<Pair<T>>) -> Evidence<Pair<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Pair<T>>)
    {
        let x = builder.arg_ref("x")
        let square = builder.share(builder.mul(x, x))
        let fourth = builder.share(builder.mul(square, square))
        let doubled = builder.add(fourth, fourth)
        // Reusing one share handle across roots must materialize one hygienic
        // local independently in each method, never leak a prior root's local.
        builder.emit_method("a", doubled)
        builder.emit_method("b", doubled)
        builder.finish()
        ev
    }
}
struct Subject {}
derive Pair for Subject using PairProvider
pub fn run_a(x: i32) -> i32 { <Subject as Pair>::a(x, 123) }
pub fn run_b(x: i32) -> i32 { <Subject as Pair>::b(x, 456) }
"#;

    let wasm = compile_to_wasm(source);
    wasmparser::validate(&wasm).expect("shared provider expression emitted invalid Wasm");
    let mut muls = 0;
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        if let wasmparser::Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut ops = body.get_operators_reader().unwrap();
            while !ops.eof() {
                if matches!(ops.read().unwrap(), wasmparser::Operator::I32Mul) {
                    muls += 1;
                }
            }
        }
    }
    assert_eq!(
        muls, 4,
        "nested shares must emit two ordered multiplies per method root"
    );

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    for name in ["run_a", "run_b"] {
        let run = instance
            .get_typed_func::<i32, i32>(&mut store, name)
            .unwrap();
        assert_eq!(run.call(&mut store, 7).unwrap(), 4802);
    }
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
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("zero-import instance");
    let run = instance
        .get_typed_func::<i32, i32>(&mut store, "run")
        .expect("run export");

    // Kept terms are +1*x and -3*x, hence -2*x.
    assert_eq!(run.call(&mut store, 7).unwrap(), -14);
    assert_eq!(run.call(&mut store, -5).unwrap(), 10);
}

#[test]
fn provider_generated_generic_methods_and_associated_consts_run_on_wasm() {
    let source = r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

struct Sink { value: u32 }
impl core::marker::Copy for Sink {}

trait WordSink: core::marker::Copy {
    fn write(self, _ value: u32) -> Self
}
impl WordSink for Sink {
    fn write(self, _ value: u32) -> Self { Sink { value: self.value + value } }
}

trait Encode {
    const WORDS: u32
    fn encode<W: WordSink>(self, _ writer: W) -> W
}

struct Provider {}
impl Derive<Encode> for Provider {
    const fn derive<T>(ev: own Evidence<Encode<T>>) -> Evidence<Encode<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Encode<T>>)
    {
        let mut words = builder.int(0)
        let mut encode = quote(writer) { writer }
        for field in reflect.fields() {
            builder.require<Encode>(field.ty())
            words = builder.add(words, builder.trait_const(field.ty(), "WORDS"))
            encode = quote(writer) { self.${field}.encode(${encode}) }
        }
        builder.emit_const("WORDS", builder.ty<u32>(), words)
        builder.emit_method(quote {
            fn encode<W: WordSink>(self, writer: W) -> W { ${encode} }
        })
        builder.finish()
        ev
    }
}

impl Encode for u32 {
    const WORDS: u32 = 1
    fn encode<W: WordSink>(self, _ writer: W) -> W { writer.write(self) }
}

impl<T: Encode + core::marker::Copy, const N: usize> Encode for [T; N] {
    const WORDS: u32 = {
        let count: u32 = N.downcast_truncate()
        count * T::WORDS
    }

    #[inline(always)]
    fn encode<W: WordSink>(self, _ writer: W) -> W {
        let mut writer = writer
        let mut index: usize = 0
        while index < N {
            writer = self[index].encode(writer)
            index = index + 1
        }
        writer
    }
}

struct Pair { first: u32, second: u32 }
derive Encode for Pair using Provider

pub fn words() -> u32 { <Pair as Encode>::WORDS }
pub fn encode_pair() -> u32 {
    Pair { first: 7, second: 9 }.encode(Sink { value: 0 }).value
}
pub fn array_words() -> u32 { <[u32; 4] as Encode>::WORDS }
pub fn encode_array() -> u32 {
    let values: [u32; 4] = [1, 2, 3, 4]
    <[u32; 4] as Encode>::encode(values, Sink { value: 0 }).value
}
"#;

    let wasm = compile_to_wasm(source);
    wasmparser::validate(&wasm).expect("generic provider methods emitted invalid Wasm");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("valid Wasm module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("zero-import instance");
    let words = instance
        .get_typed_func::<(), i32>(&mut store, "words")
        .expect("words export");
    let encode_pair = instance
        .get_typed_func::<(), i32>(&mut store, "encode_pair")
        .expect("encode_pair export");
    let array_words = instance
        .get_typed_func::<(), i32>(&mut store, "array_words")
        .expect("array_words export");
    let encode_array = instance
        .get_typed_func::<(), i32>(&mut store, "encode_array")
        .expect("encode_array export");
    assert_eq!(words.call(&mut store, ()).unwrap(), 2);
    assert_eq!(encode_pair.call(&mut store, ()).unwrap(), 16);
    assert_eq!(array_words.call(&mut store, ()).unwrap(), 4);
    assert_eq!(encode_array.call(&mut store, ()).unwrap(), 10);
}
