use common::InputDb;
use driver::DriverDataBase;
use url::Url;

const PLAN: &str = r#"
struct Zero {}
struct Term<const O: i32> {}
struct Add<L, R> {}

const fn output(_ i: usize) -> i32 {
    if i == 0 { 1 } else if i == 1 { 2 } else if i == 2 { 4 } else { 16 }
}
recursive type fn Schedule<const N: usize>() -> (*) {
    match N {
        0 => Zero
        _ => Add<Term<{output(N - 1)}>, Schedule<{N - 1}>>
    }
}
"#;

const VEC5_RETURN: &str = r#"
struct Vec5 { e1: f32, e2: f32, e4: f32, e8: f32, e16: f32 }
impl Copy for Vec5 {}

trait EvalVec { fn eval(x: f32) -> Vec5 }
impl EvalVec for Zero {
    #[inline(always)]
    fn eval(x: f32) -> Vec5 {
        Vec5 { e1: 0.0, e2: 0.0, e4: 0.0, e8: 0.0, e16: 0.0 }
    }
}
impl<const O: i32> EvalVec for Term<O> {
    #[inline(always)]
    fn eval(x: f32) -> Vec5 {
        let v: f32 = x * __f32_from_i32(O)
        Vec5 {
            e1: if O == 1 { v } else { 0.0 },
            e2: if O == 2 { v } else { 0.0 },
            e4: if O == 4 { v } else { 0.0 },
            e8: if O == 8 { v } else { 0.0 },
            e16: if O == 16 { v } else { 0.0 },
        }
    }
}
impl<L: EvalVec, R: EvalVec> EvalVec for Add<L, R> {
    #[inline(always)]
    fn eval(x: f32) -> Vec5 {
        let a: Vec5 = <L as EvalVec>::eval(x: x)
        let b: Vec5 = <R as EvalVec>::eval(x: x)
        Vec5 {
            e1: a.e1 + b.e1, e2: a.e2 + b.e2, e4: a.e4 + b.e4,
            e8: a.e8 + b.e8, e16: a.e16 + b.e16,
        }
    }
}
extern {
    fn __f32_from_i32(_: i32) -> f32
    const fn __bitcast<From, To>(_: From) -> To
}
pub fn schedule4_vec_render(px: i32, py: i32, x: f32) -> u32 {
    let value: Vec5 = <Schedule<4> as EvalVec>::eval(x: x)
    let sum: f32 = value.e1 + value.e2 + value.e4 + value.e8 + value.e16
    let color: i32 = if sum + __f32_from_i32(px + py) > 0.0 { -16711936 }
        else { -65536 }
    __bitcast(color)
}
"#;

const TUPLE_RETURN: &str = r#"
trait EvalTuple { fn eval(x: f32) -> (f32, f32, f32, f32, f32) }
impl EvalTuple for Zero {
    #[inline(always)]
    fn eval(x: f32) -> (f32, f32, f32, f32, f32) {
        (0.0, 0.0, 0.0, 0.0, 0.0)
    }
}
impl<const O: i32> EvalTuple for Term<O> {
    #[inline(always)]
    fn eval(x: f32) -> (f32, f32, f32, f32, f32) {
        let v: f32 = x * __f32_from_i32(O)
        (
            if O == 1 { v } else { 0.0 },
            if O == 2 { v } else { 0.0 },
            if O == 4 { v } else { 0.0 },
            if O == 8 { v } else { 0.0 },
            if O == 16 { v } else { 0.0 },
        )
    }
}
impl<L: EvalTuple, R: EvalTuple> EvalTuple for Add<L, R> {
    #[inline(always)]
    fn eval(x: f32) -> (f32, f32, f32, f32, f32) {
        let (a1, a2, a4, a8, a16) = <L as EvalTuple>::eval(x: x)
        let (b1, b2, b4, b8, b16) = <R as EvalTuple>::eval(x: x)
        (a1 + b1, a2 + b2, a4 + b4, a8 + b8, a16 + b16)
    }
}
extern {
    fn __f32_from_i32(_: i32) -> f32
    const fn __bitcast<From, To>(_: From) -> To
}
pub fn schedule4_tuple_render(px: i32, py: i32, x: f32) -> u32 {
    let (e1, e2, e4, e8, e16) = <Schedule<4> as EvalTuple>::eval(x: x)
    let sum: f32 = e1 + e2 + e4 + e8 + e16
    let color: i32 = if sum + __f32_from_i32(px + py) > 0.0 { -16711936 }
        else { -65536 }
    __bitcast(color)
}
"#;

fn compile(label: &str, body: &str) -> String {
    assert_eq!(
        body.matches("<Schedule<4> as").count(),
        1,
        "{label} must have exactly one root Schedule4 traversal"
    );
    let source = format!("{PLAN}\n{body}");
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{label}.fe")).unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{label} HIR:\n{diagnostics}");
    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .unwrap_or_else(|error| panic!("{label} runtime package: {error}"));
    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .unwrap_or_else(|error| panic!("{label} Render SPIR-V: {error}"));
    let wgsl = artifact.wgsl.expect("Render compilation emits WGSL");
    let module = naga::front::wgsl::parse_str(&wgsl)
        .unwrap_or_else(|error| panic!("{label} WGSL reparsing: {error:?}\n{wgsl}"));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap_or_else(|error| panic!("{label} browser validation: {error:?}\n{wgsl}"));
    assert!(
        module.functions.is_empty(),
        "{label} must inline all evaluator helpers:\n{wgsl}"
    );
    assert_eq!(
        wgsl.matches("fn ").count(),
        2,
        "{label} must contain only vertex/fragment entry points:\n{wgsl}"
    );
    assert!(!wgsl.contains("i64") && !wgsl.contains("u64"));
    eprintln!(
        "{label}: {} bytes, {} lines, call-free and browser-valid",
        wgsl.len(),
        wgsl.lines().count()
    );
    wgsl
}

#[test]
fn schedule4_vec5_and_tuple_returns_reach_call_free_browser_wgsl() {
    let vec_wgsl = compile("schedule4_vec5_return", VEC5_RETURN);
    let tuple_wgsl = compile("schedule4_tuple_return", TUPLE_RETURN);
    eprintln!(
        "tuple/Vec5 WGSL size ratio: {:.3}",
        tuple_wgsl.len() as f64 / vec_wgsl.len() as f64
    );
    assert_eq!(
        vec_wgsl, tuple_wgsl,
        "the aggregate overlay should scalarize both source shapes to identical shaders"
    );
}
