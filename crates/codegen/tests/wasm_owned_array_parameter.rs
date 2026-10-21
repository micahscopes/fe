//! A flattened value parameter may need local storage for dynamic indexing.
//! The first scalar argument is a value, never the address of that storage.
use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

const SOURCE: &str = r#"
pub fn pick_byte(_ values: own [u8; 4], _ index: u32) -> u32 {
    values[index as usize] as u32
}

fn pick<T: Copy, const N: usize>(_ values: own [T; N], _ index: usize) -> T {
    values[index]
}

pub fn pick_word(_ first: u32, _ index: u32) -> u32 {
    pick([first, 2147483648, 4294967295, 17], index as usize)
}

fn replace(mut values: own [u8; 4], _ index: usize, _ replacement: u8) -> u8 {
    values[index] = replacement
    values[index]
}

pub fn copy_and_replace(_ index: u32, _ replacement: u8) -> (u32, u32) {
    let original: [u8; 4] = [0, 127, 128, 255]
    let changed = replace(values: original, index as usize, replacement)
    (changed as u32, original[index as usize] as u32)
}
"#;

#[test]
fn indexed_owned_parameters_materialize_flat_leaves_without_aliasing() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///indexed_owned_parameters.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("owned array parameters must keep their flattened call ABI")
        .into_bytecode()
        .unwrap();
    wasmparser::validate(&bytes).unwrap();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let byte = instance
        .get_typed_func::<(i32, i32, i32, i32, i32), i32>(&mut store, "pick_byte")
        .expect("four byte leaves plus index, without a public pointer ABI");
    for first in 0..=255 {
        for (index, expected) in [first, 127, 128, 255].into_iter().enumerate() {
            assert_eq!(
                byte.call(&mut store, (first, 127, 128, 255, index as i32))
                    .unwrap(),
                expected
            );
        }
    }
    assert!(byte.call(&mut store, (0, 127, 128, 255, 4)).is_err());
    let word = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "pick_word")
        .unwrap();
    for first in [0, 1, 0x8765_4321u32, u32::MAX] {
        for (index, expected) in [first, 0x8000_0000, u32::MAX, 17].into_iter().enumerate() {
            assert_eq!(
                word.call(&mut store, (first as i32, index as i32)).unwrap() as u32,
                expected
            );
        }
    }
    assert!(word.call(&mut store, (0, 4)).is_err());
    let replace = instance
        .get_typed_func::<(i32, i32), (i32, i32)>(&mut store, "copy_and_replace")
        .unwrap();
    for (index, original) in [0, 127, 128, 255].into_iter().enumerate() {
        for replacement in [0, 127, 128, 255] {
            assert_eq!(
                replace
                    .call(&mut store, (index as i32, replacement))
                    .unwrap(),
                (replacement, original),
                "the callee edits its owned copy, preserving the caller's value"
            );
        }
    }
}
