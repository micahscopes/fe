use common::InputDb;
use driver::DriverDataBase;
use url::Url;

const BASE: &str = include_str!("fixtures/spirv/cga_schedule_ctfe_specialized_render.fe");

const VEC5_RENDER: &str = r#"
trait Eval5 {
    fn eval5(sphere: Sphere, point: Point) -> Vec5
}

struct Vec5 { e1: f32, e2: f32, e4: f32, e8: f32, e16: f32 }
impl Copy for Vec5 {}

#[inline(always)]
fn add5(_ a: Vec5, _ b: Vec5) -> Vec5 {
    Vec5 {
        e1: a.e1 + b.e1,
        e2: a.e2 + b.e2,
        e4: a.e4 + b.e4,
        e8: a.e8 + b.e8,
        e16: a.e16 + b.e16,
    }
}

impl Eval5 for Zero {
    #[inline(always)]
    fn eval5(sphere: Sphere, point: Point) -> Vec5 {
        Vec5 { e1: 0.0, e2: 0.0, e4: 0.0, e8: 0.0, e16: 0.0 }
    }
}

impl<
    const L: i32, const P: i32, const R: i32,
    const O: i32, const M: i32, const N: i32,
> Eval5 for Term<L, P, R, O, M, N>
    where Blade<L>: SphereCoefficient,
          Blade<P>: PointCoefficient,
          Blade<R>: SphereCoefficient,
{
    #[inline(always)]
    fn eval5(sphere: Sphere, point: Point) -> Vec5 {
        let product =
            <Blade<L> as SphereCoefficient>::read(value: sphere)
            * <Blade<P> as PointCoefficient>::read(value: point)
            * <Blade<R> as SphereCoefficient>::read(value: sphere)
        let scaled = if M == 1 { product } else { product + product }
        let signed = if N == 0 { scaled } else { -scaled }
        Vec5 {
            e1: if O == 1 { signed } else { 0.0 },
            e2: if O == 2 { signed } else { 0.0 },
            e4: if O == 4 { signed } else { 0.0 },
            e8: if O == 8 { signed } else { 0.0 },
            e16: if O == 16 { signed } else { 0.0 },
        }
    }
}

impl<L: Eval5, R: Eval5> Eval5 for Add<L, R> {
    #[inline(always)]
    fn eval5(sphere: Sphere, point: Point) -> Vec5 {
        add5(
            <L as Eval5>::eval5(sphere: sphere, point: point),
            <R as Eval5>::eval5(sphere: sphere, point: point),
        )
    }
}

pub fn schedule4_vec5_render(
    px: i32, py: i32,
    s1: f32, s2: f32, s8: f32, s16: f32,
    p1: f32, p2: f32, p4: f32, p8: f32, p16: f32,
) -> u32 {
    let sphere: Sphere = Cell {
        head: s1,
        tail: Cell {
            head: s2,
            tail: Cell { head: s8, tail: Cell { head: s16, tail: Nil {} } },
        },
    }
    let point: Point = Cell {
        head: p1,
        tail: Cell {
            head: p2,
            tail: Cell {
                head: p4,
                tail: Cell { head: p8, tail: Cell { head: p16, tail: Nil {} } },
            },
        },
    }
    let value = <SpecializedSandwich as Eval5>::eval5(sphere: sphere, point: point)
    __bitcast(__i32_from_f32((value.e1 + value.e2 + value.e4 + value.e8 + value.e16) * 256.0))
}
"#;

fn source() -> String {
    let base = BASE.replace(
        "type SpecializedSandwich = Schedule<32>",
        "type SpecializedSandwich = Schedule<4>",
    );
    let (before_proof, proof_and_rest) = base
        .split_once("struct NatZ {}")
        .expect("schedule proof marker");
    let (_, structs_and_rest) = proof_and_rest
        .split_once("struct Nil {}")
        .expect("runtime structs marker");
    let runtime = format!("struct Nil {{}}{structs_and_rest}");
    let (runtime_prefix, _) = runtime
        .split_once("trait Eval {")
        .expect("scalar interpreter marker");
    format!("{before_proof}{runtime_prefix}{VEC5_RENDER}")
}

#[test]
fn recursive_schedule4_vec5_return_compiles_to_browser_wgsl() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///schedule4_vec5_render.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics:\n{diagnostics}"
    );

    let package =
        mir::build_wasm_runtime_package(&db, top_mod).expect("Schedule4 Vec5 runtime package");
    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("Schedule4 Vec5 should compile through Render SPIR-V");
    assert_eq!(artifact.words.first().copied(), Some(0x0723_0203));
    let wgsl = artifact.wgsl.as_deref().expect("WGSL side artifact");
    let module = naga::front::wgsl::parse_str(wgsl).expect("WGSL reparses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("WGSL validates with browser-default capabilities");
    assert_eq!(
        wgsl.matches("fn ").count(),
        2,
        "recursive helpers should be fully inlined:\n{wgsl}"
    );
}
