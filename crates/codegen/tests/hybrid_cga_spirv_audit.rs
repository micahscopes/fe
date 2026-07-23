use common::InputDb;
use driver::DriverDataBase;
use url::Url;

#[test]
fn reduced_specialized_schedule_emits_call_free_browser_wgsl() {
    let source = r#"
struct Zero {}
struct Term<const I: i32> {}
struct Add<L, R> {}
const fn payload(_ i: usize) -> i32 {
    if i == 0 { 1 } else if i == 1 { 4 } else if i == 2 { 7 } else { 10 }
}
recursive type fn Schedule<const N: usize>() -> (*) {
    match N {
        0 => Zero
        _ => Add<Term<{payload(N - 1)}>, Schedule<{N - 1}>>
    }
}

trait Eval { fn eval(x: i32) -> i32 }
impl Eval for Zero {
    #[inline(always)]
    fn eval(x: i32) -> i32 { 0 }
}
impl<const I: i32> Eval for Term<I> {
    #[inline(always)]
    fn eval(x: i32) -> i32 { x + I }
}
impl<L: Eval, R: Eval> Eval for Add<L, R> {
    #[inline(always)]
    fn eval(x: i32) -> i32 {
        <L as Eval>::eval(x: x) + <R as Eval>::eval(x: x)
    }
}
pub fn staged_scalar_schedule4(x: i32) -> i32 {
    <Schedule<4> as Eval>::eval(x: x)
}
"#;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///cga_semantic_plan_hybrid_spirv_audit.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics:\n{diagnostics}"
    );

    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("reduced specialized schedule should build runtime MIR");
    let rmir = mir::format_runtime_package(&db, &package);
    let calls = rmir.matches("call ").count();
    assert!(
        calls > 0,
        "the source-level generic Eval call graph disappeared too early"
    );

    let artifact = fe_codegen::compile_runtime_package_spirv(&db, &package)
        .expect("bounded scalar Eval calls should inline before SPIR-V translation");
    let wgsl = artifact
        .wgsl
        .as_deref()
        .expect("SPIR-V compilation emits WGSL");
    assert!(
        !wgsl.contains("i64") && !wgsl.contains("u64") && !wgsl.contains("i256"),
        "browser WGSL must not contain wide integer types:\n{wgsl}"
    );
    let module = naga::front::wgsl::parse_str(wgsl)
        .unwrap_or_else(|error| panic!("emitted WGSL should reparse: {error:?}\n{wgsl}"));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap_or_else(|error| panic!("browser-profile WGSL validation failed: {error:?}"));
    assert!(
        module.functions.is_empty(),
        "all scalar Eval helpers must disappear before Naga; WGSL:\n{wgsl}"
    );
    assert_eq!(
        wgsl.matches("fn ").count(),
        1,
        "only the compute entry point should remain:\n{wgsl}"
    );
    assert!(
        wgsl.len() <= 20_000 && wgsl.lines().count() <= 500,
        "reduced specialized WGSL unexpectedly grew to {} bytes / {} lines",
        wgsl.len(),
        wgsl.lines().count(),
    );
}
