//! Focused Wasm value-representation gates for ordinary Fe policy enums.
//!
//! Payload-free enums are compiler-derived integer tags and may be carried as
//! scalar leaves inside otherwise flattenable records. Payload enums remain
//! fail-closed until the canonical tagged-union value ABI is implemented.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

fn compile_to_wasm(name: &str, source: &str) -> Result<Vec<u8>, String> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .map_err(|error| error.to_string())?
        .into_bytecode()
        .ok_or_else(|| "Wasm backend did not return bytecode".to_owned())
}

#[test]
fn fieldless_enum_executes_as_a_nested_scalar_record_leaf() {
    let source = r#"
pub enum Mode {
    Fixed,
    Add,
    Scale,
}
impl Copy for Mode {}

impl Mode {
    pub fn is_scale(self) -> bool {
        match self {
            Self::Scale => true,
            _ => false,
        }
    }
}

pub struct Policy {
    mode: Mode,
    amount: f32,
}
impl Copy for Policy {}

pub fn apply(_ policy: Policy, current: f32) -> f32 {
    match policy.mode {
        Mode::Fixed => policy.amount,
        Mode::Add => current + policy.amount,
        Mode::Scale => current * policy.amount,
    }
}

pub fn construct_and_apply(current: f32) -> f32 {
    apply(Policy { mode: Mode::Scale, amount: 1.5 }, current)
}

pub fn method_receiver_is_scale(_ mode: Mode) -> bool {
    mode.is_scale()
}
"#;

    let wasm = compile_to_wasm("wasm_fieldless_enum.fe", source)
        .unwrap_or_else(|error| panic!("fieldless enum should compile:\n{error}"));
    wasmparser::validate(&wasm).expect("fieldless enum module should be valid Wasm");

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("module should instantiate");

    // The public record is flattened generically as (Mode tag, amount,
    // current); there is no Mode- or Policy-specific export adapter.
    let apply = instance
        .get_typed_func::<(i32, f32, f32), f32>(&mut store, "apply")
        .expect("flattened apply export");
    assert_eq!(apply.call(&mut store, (0, 9.0, 2.0)).unwrap(), 9.0);
    assert_eq!(apply.call(&mut store, (1, 3.25, 2.0)).unwrap(), 5.25);
    assert_eq!(apply.call(&mut store, (2, 1.5, 4.0)).unwrap(), 6.0);
    assert!(
        apply.call(&mut store, (99, 1.0, 2.0)).is_err(),
        "a host-supplied tag outside the closed Fe enum must trap"
    );

    let construct = instance
        .get_typed_func::<f32, f32>(&mut store, "construct_and_apply")
        .expect("enum construction export");
    assert_eq!(construct.call(&mut store, 8.0).unwrap(), 12.0);

    // A borrowed method receiver travels through Fe's provider-value lane.
    // Its fieldless enum remains the same canonical i32 value rather than
    // being mistaken for a linear-memory address.
    let method = instance
        .get_typed_func::<i32, i32>(&mut store, "method_receiver_is_scale")
        .expect("fieldless enum method receiver export");
    assert_eq!(method.call(&mut store, 0).unwrap(), 0);
    assert_eq!(method.call(&mut store, 2).unwrap(), 1);
    assert!(method.call(&mut store, 99).is_err());
}

#[test]
fn payload_enum_value_transport_remains_fail_closed() {
    let source = r#"
pub enum MaybeValue {
    None,
    Some(f32),
}

pub fn unwrap(_ value: own MaybeValue) -> f32 {
    match value {
        MaybeValue::None => 0.0,
        MaybeValue::Some(inner) => inner,
    }
}
"#;

    let error = compile_to_wasm("wasm_payload_enum.fe", source)
        .expect_err("payload enum value transport must fail closed");
    assert!(
        error.contains("payload enum") || error.contains("payload-enum"),
        "failure should name the unsupported payload-enum boundary:\n{error}"
    );
}
