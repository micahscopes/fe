use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

const SOURCE: &str = r#"
struct Pair { x: f32, y: f32 }
impl Copy for Pair {}

#[inline(always)]
fn zero() -> Pair {
    Pair { x: 0.0, y: 0.0 }
}

#[inline(always)]
fn add(_ a: Pair, _ b: Pair) -> Pair {
    Pair { x: a.x + b.x, y: a.y + b.y }
}

pub fn one_traversal(x: f32) -> f32 {
    let value = add(zero(), Pair { x: x, y: x })
    value.x
}
"#;

#[test]
fn inline_constant_aggregate_is_reified_as_a_value() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///wasm_const_aggregate_inline.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics:\n{diagnostics}"
    );
    BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("constant aggregate in an inline value helper should stay value-carried");
}
