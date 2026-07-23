use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

#[test]
fn staged_recursive_type_fn_payload_executes_through_wasm() {
    let source = r#"
struct Zero {}
struct Term<const I: i32> {}
struct Add<L, R> {}

const fn helper(_ i: usize) -> i32 {
    if i == 0 { 1 }
    else if i == 1 { 4 }
    else if i == 2 { 7 }
    else { 10 }
}
const fn payload(_ i: usize) -> i32 {
    let mut cursor: usize = 0
    let mut result: i32 = 0
    while cursor < 4 {
        if cursor == i { result = helper(cursor) }
        cursor = cursor + 1
    }
    result
}

recursive type fn Schedule<T, const N: usize>() -> (*) {
    match N {
        0 => Zero
        _ => Add<Term<{payload(N - 1)}>, Schedule<T, {N - 1}>>
    }
}

trait Eval { const VALUE: i32 }
impl Eval for Zero { const VALUE: i32 = 0 }
impl<const I: i32> Eval for Term<I> { const VALUE: i32 = I }
impl<L: Eval, R: Eval> Eval for Add<L, R> {
    const VALUE: i32 = L::VALUE + R::VALUE
}

pub fn staged_payload_entry() -> i32 {
    <Schedule<u8, 4> as Eval>::VALUE
}
"#;

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///staged_type_fn_payload_e2e.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "unexpected fixture diagnostics:\n{diagnostics}");
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("staged type-fn payload should compile to Wasm")
        .into_bytecode()
        .expect("Wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("staged payload emitted invalid Wasm");

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes).unwrap();
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let entry = instance
        .get_typed_func::<(), i32>(&mut store, "staged_payload_entry")
        .unwrap();
    assert_eq!(entry.call(&mut store, ()).unwrap(), 22);
}

#[test]
fn staged_recursive_type_fn_payload_executes_through_generic_methods() {
    let source = include_str!("fixtures/staged_type_fn_payload_generic_method.fe");
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///staged_type_fn_payload_generic_method.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "unexpected fixture diagnostics:\n{diagnostics}");
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("staged type-fn payload should compile through generic Eval methods")
        .into_bytecode()
        .expect("Wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("generic staged payload emitted invalid Wasm");

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes).unwrap();
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let entry = instance
        .get_typed_func::<(), i32>(&mut store, "staged_payload_entry")
        .unwrap();
    assert_eq!(entry.call(&mut store, ()).unwrap(), 22);
}

#[test]
fn invalid_generic_const_cast_is_rejected_before_runtime_arg_selection() {
    let source = r#"
struct Term<const I: usize> {}
trait Eval { fn eval() -> i32 }
impl<const I: usize> Eval for Term<I> {
    fn eval() -> i32 { I as i32 }
}
pub fn run() -> i32 { <Term<10> as Eval>::eval() }
"#;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///invalid_generic_const_cast.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.contains("cast is not provably lossless"));

    let error = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect_err("an invalid generic result ABI must fail before MIR argument selection");
    assert!(
        format!("{error:?}").contains("type checking left unresolved or invalid body operations"),
        "unexpected fail-closed error: {error:?}"
    );
}
