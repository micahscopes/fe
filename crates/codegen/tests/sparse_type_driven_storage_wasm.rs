use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

// The ideal representation is expressible and type-checks today: CTFE builds
// exactly N runtime cells. Wasm aggregate transport cannot lower Cell<T> yet,
// so the executable proof below uses the selected-shape alternative.
const RECURSIVE_STORAGE_SOURCE: &str = r#"
struct Nil {}
struct Cell<T> { head: i32, tail: T }

recursive type fn Storage<const N: usize>() -> (*) {
    match N {
        0 => Nil
        _ => Cell<Storage<{N - 1}>>
    }
}

fn takes_two(value: Storage<2>) {}
fn exact_two(value: Cell<Cell<Nil>>) { takes_two(value) }
fn takes_five(value: Storage<5>) {}
fn exact_five(value: Cell<Cell<Cell<Cell<Cell<Nil>>>>>) { takes_five(value) }
"#;

const RECURSIVE_EXEC_SOURCE: &str = r#"
struct Nil {}
struct Cell<T> { head: i32, tail: T }

recursive type fn Storage<const N: usize>() -> (*) {
    match N {
        0 => Nil
        _ => Cell<Storage<{N - 1}>>
    }
}

struct Missing {}
struct Here {}
struct Next<I> {}

trait Coefficient<S> { fn read(value: S) -> i32 }
impl<S> Coefficient<S> for Missing { fn read(value: S) -> i32 { 0 } }
impl<T> Coefficient<Cell<T>> for Here {
    fn read(value: Cell<T>) -> i32 { value.head }
}
impl<I, T> Coefficient<Cell<T>> for Next<I>
    where I: Coefficient<T>
{
    fn read(value: Cell<T>) -> i32 {
        <I as Coefficient<T>>::read(value: value.tail)
    }
}

fn coefficient<I: Coefficient<S>, S>(value: S) -> i32 {
    <I as Coefficient<S>>::read(value: value)
}

fn pass_two(value: Storage<2>) -> Storage<2> { value }

pub fn recursive_support2_second(a: i32, b: i32) -> i32 {
    coefficient<Next<Here>, Storage<2>>(
        value: pass_two(
            value: Cell { head: a, tail: Cell { head: b, tail: Nil {} } },
        ),
    )
}

pub fn recursive_support2_missing(a: i32, b: i32) -> i32 {
    coefficient<Missing, Storage<2>>(
        value: Cell { head: a, tail: Cell { head: b, tail: Nil {} } },
    )
}

pub fn recursive_support5_last(a: i32, b: i32, c: i32, d: i32, e: i32) -> i32 {
    coefficient<Next<Next<Next<Next<Here>>>>, Storage<5>>(
        value: Cell {
            head: a,
            tail: Cell {
                head: b,
                tail: Cell {
                    head: c,
                    tail: Cell {
                        head: d,
                        tail: Cell { head: e, tail: Nil {} },
                    },
                },
            },
        },
    )
}
"#;

const SOURCE: &str = r#"
struct Nil {}
struct Compact2 { c0: i32, c1: i32 }
struct Compact5 { c0: i32, c1: i32, c2: i32, c3: i32, c4: i32 }

recursive type fn Storage<const N: usize>() -> (*) {
    match N {
        0 => Nil
        2 => Compact2
        5 => Compact5
        _ => Storage<0>
    }
}

struct Missing {}
struct At<const I: usize> {}

trait Coefficient<S> {
    fn read(value: S) -> i32
}

impl<S> Coefficient<S> for Missing {
    fn read(value: S) -> i32 { 0 }
}

impl Coefficient<Compact2> for At<0> { fn read(value: Compact2) -> i32 { value.c0 } }
impl Coefficient<Compact2> for At<1> { fn read(value: Compact2) -> i32 { value.c1 } }
impl Coefficient<Compact5> for At<0> { fn read(value: Compact5) -> i32 { value.c0 } }
impl Coefficient<Compact5> for At<1> { fn read(value: Compact5) -> i32 { value.c1 } }
impl Coefficient<Compact5> for At<2> { fn read(value: Compact5) -> i32 { value.c2 } }
impl Coefficient<Compact5> for At<3> { fn read(value: Compact5) -> i32 { value.c3 } }
impl Coefficient<Compact5> for At<4> { fn read(value: Compact5) -> i32 { value.c4 } }

fn coefficient<I: Coefficient<S>, S>(value: S) -> i32 {
    <I as Coefficient<S>>::read(value: value)
}

pub fn support2_second(a: i32, b: i32) -> i32 {
    coefficient<At<1>, Storage<2>>(
        value: Compact2 { c0: a, c1: b },
    )
}

pub fn support2_missing(a: i32, b: i32) -> i32 {
    coefficient<Missing, Storage<2>>(
        value: Compact2 { c0: a, c1: b },
    )
}

pub fn support5_last(a: i32, b: i32, c: i32, d: i32, e: i32) -> i32 {
    coefficient<At<4>, Storage<5>>(
        value: Compact5 { c0: a, c1: b, c2: c, c3: d, c4: e },
    )
}
"#;

#[test]
fn recursive_support_sized_storage_type_checks_for_two_lengths() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///recursive_support_sized_storage.fe").unwrap();
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(RECURSIVE_STORAGE_SOURCE.to_string()),
    );
    let file = db.workspace().get(&db, &url).unwrap();
    let diagnostics = db.run_on_top_mod(db.top_mod(file)).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "recursive support-sized storage should type-check:\n{diagnostics}"
    );
}

#[test]
fn type_driven_selected_storage_executes_for_two_supports() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///sparse_type_driven_storage_wasm.fe").unwrap();
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
        .expect("type-driven recursive storage should compile")
        .into_bytecode()
        .expect("Wasm bytecode");
    assert!(bytes.starts_with(b"\0asm"));
    assert!(
        bytes.len() < 900,
        "ground storage/access specialization should remain compact: {} bytes",
        bytes.len()
    );

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes).expect("valid Wasm module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("zero imports");
    let support2_second = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "support2_second")
        .expect("support2 export");
    let support2_missing = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "support2_missing")
        .expect("missing export");
    let support5_last = instance
        .get_typed_func::<(i32, i32, i32, i32, i32), i32>(&mut store, "support5_last")
        .expect("support5 export");
    assert_eq!(support2_second.call(&mut store, (11, 22)).unwrap(), 22);
    assert_eq!(support2_missing.call(&mut store, (11, 22)).unwrap(), 0);
    assert_eq!(
        support5_last
            .call(&mut store, (10, 20, 30, 40, 50))
            .unwrap(),
        50
    );
    eprintln!(
        "type-driven Storage<2>/Storage<5>: {} Wasm bytes",
        bytes.len()
    );
}

#[test]
fn recursive_support_sized_storage_executes_through_wasm() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///recursive_support_sized_storage_exec.fe").unwrap();
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(RECURSIVE_EXEC_SOURCE.to_string()),
    );
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
        .expect("recursive closed-product storage should compile")
        .into_bytecode()
        .expect("Wasm bytecode");

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bytes).expect("valid Wasm module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("zero imports");
    let second = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "recursive_support2_second")
        .expect("support2 export");
    let missing = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "recursive_support2_missing")
        .expect("missing export");
    let last = instance
        .get_typed_func::<(i32, i32, i32, i32, i32), i32>(&mut store, "recursive_support5_last")
        .expect("support5 export");
    assert_eq!(second.call(&mut store, (11, 22)).unwrap(), 22);
    assert_eq!(missing.call(&mut store, (11, 22)).unwrap(), 0);
    assert_eq!(last.call(&mut store, (10, 20, 30, 40, 50)).unwrap(), 50);
    eprintln!(
        "recursive Storage<2>/Storage<5>: {} Wasm bytes",
        bytes.len()
    );
}
