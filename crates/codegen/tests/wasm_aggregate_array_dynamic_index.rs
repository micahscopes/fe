//! Wasm regression gate for runtime indexing into an array whose element is a
//! recursively flattened nominal value. Scalar-array indexing was already
//! supported; proof-field code needs the same address calculation followed by
//! a whole aggregate load.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

const SOURCE: &str = r#"
struct Pair { left: u32, right: u32 }

impl Copy for Pair {}

pub fn pick(index: u32) -> (u32, u32) {
    let mut values: [Pair; 3] = [
        Pair { left: 0, right: 0 },
        Pair { left: 7, right: 11 },
        Pair { left: 13, right: 17 },
    ]
    values[0] = Pair { left: 3, right: 5 }
    let pair = values[index as usize]
    (pair.left, pair.right)
}
"#;

const CONST_SOURCE: &str = r#"
struct Pair { left: u32, right: u32 }

impl Copy for Pair {}

const VALUES: [Pair; 3] = [
    Pair { left: 3, right: 5 },
    Pair { left: 7, right: 11 },
    Pair { left: 13, right: 17 },
]

pub fn pick(index: u32) -> (u32, u32) {
    let pair = VALUES[index as usize]
    (pair.left, pair.right)
}
"#;

const NESTED_CONST_SOURCE: &str = r#"
const VALUES: [[u32; 2]; 3] = [
    [3, 5],
    [7, 11],
    [13, 17],
]

pub fn pick(row: u32, column: u32) -> u32 {
    let selected = VALUES[row as usize]
    selected[column as usize]
}

pub fn mutate_copy(row: u32, column: u32, replacement: u32) -> (u32, u32) {
    let mut selected = VALUES[row as usize]
    selected[column as usize] = replacement
    (selected[column as usize], VALUES[row as usize][column as usize])
}
"#;

const BORROWED_AGGREGATE_ARGUMENT_SOURCE: &str = r#"
struct Pair { left: u32, right: u32 }

impl Copy for Pair {}

fn combine(_ left: Pair, _ right: Pair) -> Pair {
    Pair {
        left: left.left + right.left,
        right: left.right + right.right,
    }
}

fn fold(_ values: mut [Pair; 3]) -> Pair {
    let mut digest = Pair { left: 0, right: 0 }
    let mut index: usize = 0
    while index < 3 {
        digest = combine(digest, values[index])
        index = index + 1
    }
    digest
}

pub fn sum() -> (u32, u32) {
    let mut values = [
        Pair { left: 3, right: 5 },
        Pair { left: 7, right: 11 },
        Pair { left: 13, right: 17 },
    ]
    let result = fold(mut values)
    (result.left, result.right)
}
"#;

fn compile(source: &str, name: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}.fe")).unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("aggregate-array probe should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("aggregate-array probe Wasm should validate");
    bytes
}

#[test]
fn dynamic_aggregate_array_index_executes_and_traps_out_of_bounds() {
    assert_pick_executes_and_traps(compile(SOURCE, "wasm_aggregate_array_dynamic_index"));
}

#[test]
fn dynamic_const_aggregate_array_index_executes_and_traps_out_of_bounds() {
    assert_pick_executes_and_traps(compile(
        CONST_SOURCE,
        "wasm_const_aggregate_array_dynamic_index",
    ));
}

#[test]
fn nested_dynamic_const_array_indexes_execute_and_trap_out_of_bounds() {
    let bytes = compile(
        NESTED_CONST_SOURCE,
        "wasm_nested_const_aggregate_array_dynamic_index",
    );
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let pick = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "pick")
        .expect("pick export");
    let mutate_copy = instance
        .get_typed_func::<(i32, i32, i32), (i32, i32)>(&mut store, "mutate_copy")
        .expect("mutate_copy export");
    for (row, column, expected) in [
        (0, 0, 3),
        (0, 1, 5),
        (1, 0, 7),
        (1, 1, 11),
        (2, 0, 13),
        (2, 1, 17),
    ] {
        assert_eq!(pick.call(&mut store, (row, column)).unwrap(), expected);
    }
    assert_eq!(
        mutate_copy.call(&mut store, (1, 0, 29)).unwrap(),
        (29, 7),
        "the selected row must be an independent Fe value"
    );
    assert_eq!(pick.call(&mut store, (1, 0)).unwrap(), 7);
    for (row, column) in [(3, 0), (0, 2), (-1, 0), (0, -1)] {
        assert!(
            pick.call(&mut store, (row, column)).is_err(),
            "out-of-bounds index ({row}, {column}) must trap"
        );
    }
}

#[test]
fn borrowed_dynamic_aggregate_array_element_passes_all_lanes_to_value_parameter() {
    let bytes = compile(
        BORROWED_AGGREGATE_ARGUMENT_SOURCE,
        "wasm_borrowed_aggregate_array_argument",
    );
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let sum = instance
        .get_typed_func::<(), (i32, i32)>(&mut store, "sum")
        .expect("sum export");
    assert_eq!(sum.call(&mut store, ()).unwrap(), (23, 33));
}

fn assert_pick_executes_and_traps(bytes: Vec<u8>) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let pick = instance
        .get_typed_func::<i32, (i32, i32)>(&mut store, "pick")
        .expect("pick export");
    for (index, expected) in [(0, (3, 5)), (1, (7, 11)), (2, (13, 17))] {
        assert_eq!(pick.call(&mut store, index).unwrap(), expected);
    }
    for index in [3, 4, 99, -1] {
        assert!(
            pick.call(&mut store, index).is_err(),
            "out-of-bounds index {index} must trap"
        );
    }
}
