//! End-to-end acceptance: the first genuinely-Fe-compiled wasm.
//!
//! Each test takes Fe source, compiles it Fe -> MIR -> Sonatina IR (Wasm32 ISA)
//! -> WAFFLE -> wasm bytes through `BackendKind::Wasm`, executes the bytes under
//! wasmtime, and asserts the result. The same source is also compiled through
//! the EVM backend (`BackendKind::Sonatina`) as the cross-backend twin: it
//! proves one Fe source lowers on both targets, and the wasm result is asserted
//! equal to the known EVM-semantics value (Fe integer arithmetic is identical
//! across backends; the EVM backend's value-correctness is covered by the full
//! EVM suite + byte-identity gate).
//!
//! R1 scope: scalar u64 arithmetic, a loop/phi (`sum_to`), and a call pair.
//! Non-overflowing values only (the WAFFLE translator fakes overflow flags as
//! 0; real checked semantics are R2).

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

/// Compile Fe source to wasm bytes through the wasm backend.
fn compile_to_wasm(name: &str, source: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .unwrap_or_else(|err| panic!("wasm compilation of `{name}` failed: {err}"));
    let bytes = output
        .into_bytecode()
        .expect("wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("produced invalid wasm");
    bytes
}

/// Compile Fe source through the wasm backend, expecting a fail-closed error.
fn compile_to_wasm_err(name: &str, source: &str) -> String {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect_err("wasm compilation should fail closed")
        .to_string()
}

/// Compile the same Fe source through the EVM backend (the cross-backend twin).
/// Returns the EVM runtime bytecode, proving one source lowers on both targets.
fn compile_to_evm(name: &str, source: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);

    BackendKind::Sonatina
        .create()
        .compile(
            &db,
            top_mod,
            layout_for(BackendKind::Sonatina),
            OptLevel::O0,
        )
        .unwrap_or_else(|err| panic!("evm twin compilation of `{name}` failed: {err}"))
        .into_bytecode()
        .expect("evm output should be bytecode")
}

fn instantiate(bytes: &[u8]) -> (wasmtime::Store<()>, wasmtime::Instance) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    (store, instance)
}

/// Collect the `(module, name)` of every function import in the emitted wasm,
/// scanned from the bytes with `wasmparser` (asserted, not assumed).
fn func_imports(bytes: &[u8]) -> Vec<(String, String)> {
    use wasmparser::{Payload, TypeRef};
    let mut imports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let Payload::ImportSection(reader) = payload.expect("valid wasm payload") {
            for import in reader.into_imports() {
                let import = import.expect("valid import entry");
                if let TypeRef::Func(_) = import.ty {
                    imports.push((import.module.to_string(), import.name.to_string()));
                }
            }
        }
    }
    imports
}

/// THE MILESTONE: `#[target(wasm)] pub fn add(a, b) -> a + b`, compiled Fe ->
/// wasm, executed under wasmtime, add(2, 3) == 5, and equal to the EVM twin.
#[test]
fn fe_add_runs_on_wasm_and_matches_evm_twin() {
    let source = "pub fn add(a: u64, b: u64) -> u64 { a + b }\n\
                  pub fn main() -> u64 { add(2, 3) }\n";

    // Cross-backend twin: the identical source also compiles to EVM.
    let evm = compile_to_evm("wasm_add.fe", source);
    assert!(!evm.is_empty(), "evm twin bytecode must be non-empty");
    // Fe `add(2, 3)` has the same integer semantics on both backends.
    let evm_twin_result: i64 = 5;

    let wasm = compile_to_wasm("wasm_add.fe", source);
    let (mut store, instance) = instantiate(&wasm);

    let add = instance
        .get_typed_func::<(i64, i64), i64>(&mut store, "add")
        .expect("`add` export should exist");
    let wasm_result = add.call(&mut store, (2, 3)).expect("add(2, 3) should run");
    assert_eq!(wasm_result, 5, "Fe->wasm add(2, 3) should be 5");
    assert_eq!(
        wasm_result, evm_twin_result,
        "Fe->wasm add(2, 3) must equal the EVM twin"
    );

    // A few more non-overflowing points.
    assert_eq!(add.call(&mut store, (40, 2)).unwrap(), 42);
    assert_eq!(add.call(&mut store, (0, 0)).unwrap(), 0);

    // `main()` calls `add(2, 3)` internally and returns 5.
    let main = instance
        .get_typed_func::<(), i64>(&mut store, "main")
        .expect("`main` export should exist");
    assert_eq!(main.call(&mut store, ()).unwrap(), 5, "main() should be 5");
}

#[test]
fn fe_f32_arithmetic_comparisons_and_neg_run_on_wasm() {
    let source = r#"
extern {
    fn __f32_from_i32(_: i32) -> f32
    fn __i32_from_f32(_: f32) -> i32
    fn __sqrt_f32(_: f32) -> f32
}
pub fn arith(a: f32, b: f32) -> f32 { -(((a + b) * (a - b)) / b) }
pub fn sqrt_round(value: i32) -> i32 {
    __i32_from_f32(__sqrt_f32(__f32_from_i32(value)))
}
pub fn eq(a: f32, b: f32) -> bool { a == b }
pub fn ne(a: f32, b: f32) -> bool { a != b }
pub fn lt(a: f32, b: f32) -> bool { a < b }
pub fn le(a: f32, b: f32) -> bool { a <= b }
pub fn gt(a: f32, b: f32) -> bool { a > b }
pub fn ge(a: f32, b: f32) -> bool { a >= b }
"#;
    let wasm = compile_to_wasm("wasm_f32_ops.fe", source);
    assert!(
        func_imports(&wasm)
            .iter()
            .all(|(_, name)| !name.ends_with("_f32")),
        "f32 language intrinsics must lower to Sonatina ops, not wasm host imports"
    );

    let operators = wasmparser::Parser::new(0)
        .parse_all(&wasm)
        .filter_map(|payload| match payload.expect("valid wasm") {
            wasmparser::Payload::CodeSectionEntry(body) => Some(
                body.get_operators_reader()
                    .expect("operator reader")
                    .into_iter()
                    .map(|op| format!("{:?}", op.expect("valid operator")))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    for opcode in [
        "F32Add",
        "F32Sub",
        "F32Mul",
        "F32Div",
        "F32Neg",
        "F32Sqrt",
        "F32ConvertI32S",
        "I32TruncSatF32S",
        "F32Eq",
        "F32Lt",
        "F32Le",
    ] {
        assert!(
            operators.contains(opcode),
            "generated wasm lacks {opcode}:\n{operators}"
        );
    }

    let (mut store, instance) = instantiate(&wasm);
    let arith = instance
        .get_typed_func::<(f32, f32), f32>(&mut store, "arith")
        .expect("arith export");
    assert_eq!(arith.call(&mut store, (5.0, 2.0)).unwrap(), -10.5);
    let sqrt_round = instance
        .get_typed_func::<i32, i32>(&mut store, "sqrt_round")
        .expect("sqrt_round export");
    assert_eq!(sqrt_round.call(&mut store, 81).unwrap(), 9);

    let nan = f32::from_bits(0x7fc0_1234);
    for (name, expected) in [
        ("eq", 0),
        ("ne", 1),
        ("lt", 0),
        ("le", 0),
        ("gt", 0),
        ("ge", 0),
    ] {
        let compare = instance
            .get_typed_func::<(f32, f32), i32>(&mut store, name)
            .unwrap_or_else(|error| panic!("{name} export: {error}"));
        assert_eq!(
            compare.call(&mut store, (nan, 1.0)).unwrap(),
            expected,
            "{name}(NaN, 1)"
        );
    }
    let gt = instance
        .get_typed_func::<(f32, f32), i32>(&mut store, "gt")
        .unwrap();
    let ge = instance
        .get_typed_func::<(f32, f32), i32>(&mut store, "ge")
        .unwrap();
    assert_eq!(gt.call(&mut store, (2.0, 1.0)).unwrap(), 1);
    assert_eq!(ge.call(&mut store, (2.0, 2.0)).unwrap(), 1);

    for (name, expected) in [("eq", 1), ("lt", 0), ("le", 1)] {
        let compare = instance
            .get_typed_func::<(f32, f32), i32>(&mut store, name)
            .unwrap_or_else(|error| panic!("{name} export: {error}"));
        assert_eq!(
            compare.call(&mut store, (0.0, -0.0)).unwrap(),
            expected,
            "{name}(+0, -0)"
        );
    }
}

#[test]
fn conditional_f32_value_materializes_into_wasm_result_slot() {
    let source = r#"
pub fn select_f32(flag: bool, when_true: f32, when_false: f32) -> f32 {
    if flag { when_true } else { when_false }
}
"#;
    let wasm = compile_to_wasm("wasm_conditional_f32_result.fe", source);
    let (mut store, instance) = instantiate(&wasm);

    let select = instance
        .get_typed_func::<(i32, f32, f32), f32>(&mut store, "select_f32")
        .expect("select_f32 export should preserve its typed f32 result");
    assert_eq!(select.call(&mut store, (1, 1.25, -3.5)).unwrap(), 1.25);
    assert_eq!(select.call(&mut store, (0, 1.25, -3.5)).unwrap(), -3.5);
}

#[test]
fn aggregate_conditional_slot_projection_fails_closed_on_wasm() {
    let source = r#"
struct Pair {
    left: u64,
    right: u64,
}

pub fn select_left(flag: bool, when_true: own Pair, when_false: own Pair) -> u64 {
    let selected: Pair = if flag { when_true } else { when_false }
    selected.left
}
"#;
    let error = compile_to_wasm_err("wasm_aggregate_conditional_slot.fe", source);
    assert!(
        error.contains("aggregate") || error.contains("unsupported place") || error.contains("R2"),
        "aggregate/projection slots must remain outside the whole-scalar slot boundary: {error}",
    );
}

/// D2 stage 1: `MvT<1> = Nd<Sc>` constructs a recursive two-leaf tree, projects
/// through it, copies it as a value, rebuilds it, and returns both distinct leaves
/// through Wasm multi-value, including across the private `make_mvt1` call.
/// Place-backed aggregate slots remain outside this stage.
#[test]
fn recursive_mvt1_construct_copy_project_return_executes_on_wasm() {
    let source = include_str!("fixtures/wasm_mvt1_runtime_probe.fe");
    let wasm = compile_to_wasm("wasm_mvt1_runtime_probe.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let probe = instance
        .get_typed_func::<(i32, i32), (i32, i32)>(&mut store, "mvt1_runtime_probe")
        .expect("MvT<1> export should have two scalar params and a flattened two-result return");
    assert_eq!(
        probe.call(&mut store, (17, 29)).expect("MvT<1> probe call"),
        (17, 29),
        "projection/copy/rebuild must preserve both distinct leaves",
    );
}

#[test]
fn generic_associated_const_type_converges_during_wasm_root_lowering() {
    // Checking the extern argument constructs the Copy impl environment.
    // Collecting Emit lowers its associated type and evaluates select(I),
    // whose body also consults that environment.  This is a legitimate
    // additive query cycle and must converge to the complete impl set.
    let source = r#"
const fn select(_ i: usize) -> usize { i + 1 }
extern { fn ext(_: f32) -> i32 }
struct Term<const I: usize> { value: i32 }
struct Choice<const I: usize> {}
trait Emit {
    type Out
    fn emit() -> i32
}
impl<const I: usize> Emit for Choice<I> {
    type Out = Term<{select(I)}>
    fn emit() -> i32 { 3 }
}
fn takes_term3(_ term: Term<3>) {}
fn projection_is_term3(term: <Choice<2> as Emit>::Out) { takes_term3(term) }
pub fn entry(value: f32) -> i32 {
    let _ = ext(value)
    <Choice<2> as Emit>::emit()
}
"#;
    let wasm = compile_to_wasm("generic_associated_const_type.fe", source);
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    linker.func_wrap("fe", "ext", |_value: f32| 0i32).unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let entry = instance
        .get_typed_func::<f32, i32>(&mut store, "entry")
        .unwrap();
    assert_eq!(entry.call(&mut store, 0.0).unwrap(), 3);
}

#[test]
fn generic_associated_const_cycle_recovery_does_not_invent_missing_impls() {
    let source = r#"
const fn select(_ i: usize) -> usize { i + 1 }
extern { fn ext(_: f32) -> i32 }
struct Term<const I: usize> {}
struct Choice<const I: usize> {}
trait Emit { type Out }
impl<const I: usize> Emit for Choice<I> { type Out = Term<{select(I)}> }
struct Missing {}
trait Needed { fn value() -> i32 }
pub fn entry(value: f32) -> i32 {
    let _ = ext(value)
    <Missing as Needed>::value()
}
"#;
    let error = compile_to_wasm_err("generic_associated_const_missing_impl.fe", source);
    assert!(
        error.contains("trait") || error.contains("impl") || error.contains("Needed"),
        "missing implementation must remain a normal compiler error: {error}",
    );
}

#[test]
fn recursive_mvt2_construct_copy_project_return_executes_on_wasm() {
    let source = include_str!("fixtures/wasm_mvt2_runtime_probe.fe");
    let wasm = compile_to_wasm("wasm_mvt2_runtime_probe.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let probe = instance
        .get_typed_func::<(i32, i32, i32, i32), (i32, i32, i32, i32)>(
            &mut store,
            "mvt2_runtime_probe",
        )
        .expect("MvT<2> export should take four scalar params and flatten to four results");
    assert_eq!(
        probe
            .call(&mut store, (11, 22, 33, 44))
            .expect("MvT<2> probe call"),
        (11, 22, 33, 44),
        "DFS construction/projection/copy/rebuild must preserve all four leaves",
    );
}

#[test]
fn recursive_mvt2_f32_render_executes_on_wasm() {
    let source = include_str!("fixtures/spirv/mvt2_f32_render.fe");
    let wasm = compile_to_wasm("wasm_mvt2_f32_render.fe", source);
    assert!(
        func_imports(&wasm).is_empty(),
        "f32 conversion and bitcast helpers must lower intrinsically"
    );
    let (mut store, instance) = instantiate(&wasm);
    let render = instance
        .get_typed_func::<(i32, i32, f32, f32, f32, f32), i32>(&mut store, "mvt2_f32_render")
        .expect("call-free MvT<2> f32 render export");
    let got = render
        .call(&mut store, (2, 3, 11.0, 22.0, 33.0, 44.0))
        .expect("MvT<2> f32 wasm execution") as u32;
    assert_eq!(got.to_le_bytes(), [13, 25, 121, 255]);
}

#[test]
fn recursive_mvt2_f32_helper_call_executes_on_wasm() {
    let source = include_str!("fixtures/spirv/mvt2_f32_helper_render.fe");
    let wasm = compile_to_wasm("wasm_mvt2_f32_helper_render.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let render = instance
        .get_typed_func::<(i32, i32, f32, f32, f32, f32), i32>(
            &mut store,
            "mvt2_f32_helper_render",
        )
        .expect("inlined recursive aggregate helper export");
    let got = render
        .call(&mut store, (2, 3, 11.0, 22.0, 33.0, 44.0))
        .expect("inlined MvT<2> helper execution") as u32;
    assert_eq!(got.to_le_bytes(), [46, 25, 55, 255]);
}

#[test]
fn recursive_mvt5_f32_nested_helper_executes_on_wasm() {
    let source = include_str!("fixtures/spirv/mvt5_f32_nested_helper_render.fe");
    let wasm = compile_to_wasm("wasm_mvt5_f32_nested_helper_render.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let render = instance
        .get_typed_func::<(i32, i32, f32, f32), i32>(&mut store, "mvt5_f32_nested_helper_render")
        .expect("inlined recursive MvT<5> helper export");
    let got = render
        .call(&mut store, (2, 3, 11.0, 22.0))
        .expect("inlined MvT<5> helper execution") as u32;
    assert_eq!(got.to_le_bytes(), [24, 14, 255, 255]);
}

#[test]
fn qcga3d_sparse_incidence_paths_compile_and_execute_on_wasm() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spirv");
    let oracle = std::process::Command::new("python3")
        .arg(fixture_dir.join("qcga3d_sparse_incidence_oracle.py"))
        .status()
        .expect("QCGA3D exact rational oracle should run");
    assert!(oracle.success(), "QCGA3D exact rational oracle failed");

    let source = include_str!("fixtures/spirv/qcga3d_sparse_incidence.fe");
    let wasm = compile_to_wasm("qcga3d_sparse_incidence.fe", source);
    assert!(
        func_imports(&wasm).is_empty(),
        "sparse incidence fixture must have zero imports"
    );
    let (mut store, instance) = instantiate(&wasm);
    type Inputs = (
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
    );
    let expanded = instance
        .get_typed_func::<Inputs, f32>(&mut store, "qcga3d_incidence_expanded")
        .expect("expanded sparse incidence ABI");
    let fused = instance
        .get_typed_func::<Inputs, f32>(&mut store, "qcga3d_incidence_fused")
        .expect("fused sparse incidence ABI");

    let kats: [(Inputs, f32); 5] = [
        (
            (
                3.0, 4.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -25.0,
            ),
            0.0,
        ),
        (
            (
                0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -25.0,
            ),
            -25.0,
        ),
        (
            (
                2.0, -1.0, 2.0, 1.0, 2.0, 3.0, 0.0, 0.0, 0.0, -2.0, 8.0, -6.0, 0.0,
            ),
            -6.0,
        ),
        (
            (
                2.0,
                -1.0,
                3.0,
                5.0,
                5.0,
                2.0,
                6.0,
                -4.0,
                2.0,
                -3.0,
                7.0,
                1.0,
                5.0 / 3.0,
            ),
            -22.0 / 3.0,
        ),
        (
            (
                1.0,
                2.0,
                -1.0,
                2.0,
                -1.0,
                3.0,
                1.0,
                -2.0,
                4.0,
                5.0,
                -3.0,
                2.0,
                1.0 / 7.0,
            ),
            -41.0 / 7.0,
        ),
    ];
    for (index, (inputs, expected)) in kats.into_iter().enumerate() {
        let expanded_value = expanded
            .call(&mut store, inputs)
            .expect("expanded incidence call");
        let fused_value = fused
            .call(&mut store, inputs)
            .expect("fused incidence call");
        assert!(
            (expanded_value - expected).abs() <= 2.0e-5,
            "expanded KAT {index}: {expanded_value} != {expected}"
        );
        assert!(
            (fused_value - expected).abs() <= 2.0e-5,
            "fused KAT {index}: {fused_value} != {expected}"
        );
        assert!(
            (expanded_value - fused_value).abs() <= 2.0e-5,
            "path mismatch for KAT {index}"
        );
    }
}

fn qcga3d_quadric_field_oracle(x: f32, y: f32, z: f32) -> f32 {
    let square = 0.85_f32 * x * x + 1.25_f32 * y * y + 0.65_f32 * z * z;
    let cross = 0.55_f32 * x * y - 0.40_f32 * x * z + 0.30_f32 * y * z;
    let linear = -0.16_f32 * x + 0.1375_f32 * y - 0.04_f32 * z;
    square + cross + linear - 0.979125_f32
}

fn qcga3d_pack_oracle(r: f32, g: f32, b: f32) -> u32 {
    (r as i32 + (g as i32) * 256 + (b as i32) * 65_536 - 16_777_216_i32) as u32
}

/// Independent CPU f32 rendering oracle for the fixed sparse-QCGA kernel.
/// It uses the implicit coefficient form directly; unlike the Fe hit gate it
/// never constructs PointSupport or DualQuadricSupport.
fn qcga3d_rotated_quadric_oracle(px: i32, py: i32) -> u32 {
    let fx = px as f32;
    let fy = py as f32;
    let sx = (fx - 63.5) * 0.018;
    let sy = (fy - 63.5) * -0.018;
    let inv_len = 1.0 / (sx * sx + sy * sy + 3.24_f32).sqrt();
    let (dx, dy, dz) = (sx * inv_len, sy * inv_len, 1.8 * inv_len);
    let (ox, oy, oz) = (0.0_f32, 0.0_f32, -4.0_f32);
    let f0 = qcga3d_quadric_field_oracle(ox, oy, oz);
    let fp = qcga3d_quadric_field_oracle(ox + dx, oy + dy, oz + dz);
    let fm = qcga3d_quadric_field_oracle(ox - dx, oy - dy, oz - dz);
    let alpha = (fp + fm) * 0.5 - f0;
    let beta = (fp - fm) * 0.5;
    let mut t = -1.0_f32;
    if alpha.abs() < 0.000001 {
        if beta.abs() >= 0.000001 {
            let linear_t = -f0 / beta;
            if linear_t >= 0.0 {
                t = linear_t;
            }
        }
    } else {
        let discriminant = beta * beta - 4.0 * alpha * f0;
        if discriminant >= 0.0 {
            let root = discriminant.sqrt();
            let signed_root = if beta < 0.0 { -root } else { root };
            let qroot = -0.5 * (beta + signed_root);
            if qroot.abs() >= 0.000001 {
                let (t0, t1) = (qroot / alpha, f0 / qroot);
                if t0 >= 0.0 {
                    t = t0;
                    if t1 >= 0.0 && t1 < t0 {
                        t = t1;
                    }
                } else if t1 >= 0.0 {
                    t = t1;
                }
            } else {
                let fallback = -beta / (2.0 * alpha);
                if fallback >= 0.0 {
                    t = fallback;
                }
            }
        }
    }
    if t >= 0.0 {
        let (x, y, z) = (ox + dx * t, oy + dy * t, oz + dz * t);
        // The CPU acceptance is direct coefficient evaluation, independently
        // equal to the Fe paper-null contraction by the committed exact gate.
        if qcga3d_quadric_field_oracle(x, y, z).abs() < 0.0002 {
            let nx = 1.70 * x + 0.55 * y - 0.40 * z - 0.16;
            let ny = 2.50 * y + 0.55 * x + 0.30 * z + 0.1375;
            let nz = 1.30 * z - 0.40 * x + 0.30 * y - 0.04;
            let n_inv = 1.0 / (nx * nx + ny * ny + nz * nz).sqrt();
            let (nnx, nny, nnz) = (nx * n_inv, ny * n_inv, nz * n_inv);
            let diffuse = (nnx * -0.45 + nny * 0.70 + nnz * -0.55).max(0.0);
            let rim = (1.0 - (-(nnx * dx + nny * dy + nnz * dz))).max(0.0);
            let shade = 0.18 + diffuse * 0.72;
            return qcga3d_pack_oracle(
                24.0 + 78.0 * shade + 65.0 * rim,
                28.0 + 178.0 * shade + 34.0 * rim,
                52.0 + 196.0 * shade + 55.0 * rim,
            );
        }
    }
    let horizon = (fy + 0.5) / 128.0;
    qcga3d_pack_oracle(
        8.0 + 10.0 * horizon,
        11.0 + 15.0 * horizon,
        25.0 + 28.0 * horizon,
    )
}

#[test]
fn qcga3d_rotated_quadric_renders_whole_frame_on_wasm() {
    const W: i32 = 128;
    const H: i32 = 128;
    let source = include_str!("fixtures/spirv/qcga3d_rotated_quadric_render.fe");
    let wasm = compile_to_wasm("qcga3d_rotated_quadric_render.fe", source);
    assert!(
        func_imports(&wasm).is_empty(),
        "QCGA render must have zero imports"
    );
    let (mut store, instance) = instantiate(&wasm);
    let render = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "qcga3d_rotated_quadric_render")
        .expect("QCGA render ABI must be (i32, i32) -> i32");
    let mut foreground = 0usize;
    let mut colors = std::collections::BTreeSet::new();
    for py in 0..H {
        for px in 0..W {
            let got = render.call(&mut store, (px, py)).expect("QCGA Wasm pixel") as u32;
            let expected = qcga3d_rotated_quadric_oracle(px, py);
            assert_eq!(got, expected, "QCGA Wasm pixel ({px},{py})");
            let background = qcga3d_rotated_quadric_oracle(0, py);
            if got != background {
                foreground += 1;
                colors.insert(got);
            }
        }
    }
    assert!(
        foreground > 1_800,
        "quadric must occupy a material part of the frame"
    );
    assert!(
        colors.len() > 64,
        "lighting must produce a visible color range"
    );
}

fn clifford_gp_cl11_oracle(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let mut out = [0.0; 4];
    for left in 0..4usize {
        for right in 0..4usize {
            let mut swaps = 0u32;
            for bit in 0..2usize {
                if (right >> bit) & 1 == 1 {
                    swaps += ((left >> (bit + 1)).count_ones()) as u32;
                }
            }
            let metric_neg = ((left & right) >> 1) & 1;
            let sign = if (swaps + metric_neg as u32) & 1 == 0 { 1.0 } else { -1.0 };
            out[left ^ right] += sign * a[left] * b[right];
        }
    }
    out
}

#[test]
fn recursive_cl11_gp_f32_coefficients_execute_on_wasm() {
    let source = include_str!("fixtures/spirv/clifford_gp_recursive_f32_mvt2.fe");
    let wasm = compile_to_wasm("clifford_gp_recursive_f32_mvt2.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let gp = instance
        .get_typed_func::<(i32, i32, f32, f32, f32, f32, f32, f32, f32, f32), i32>(
            &mut store,
            "clifford_gp_cl11_mvt2_render",
        )
        .expect("Cl(1,1) scalar GP Render export");
    let cases = [
        ([1.0, 0.0, 0.0, 0.0], [3.0, -2.0, 5.0, 7.0]),
        ([0.0, 1.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]),
        ([0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]),
        ([0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 1.0, 0.0]),
        ([0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]),
        ([1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0]),
        ([-2.0, 3.0, 1.0, -4.0], [4.0, -2.0, 1.0, 5.0]),
    ];
    for (a, b) in cases {
        let mut got = [0.0; 4];
        for (index, (px, py)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
            let word = gp
                .call(&mut store, (px, py, a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]))
                .expect("recursive Cl(1,1) GP coefficient execution");
            got[index] = word as f32;
        }
        assert_eq!(got, clifford_gp_cl11_oracle(a, b));
    }
}

fn clifford_gp_cl41_oracle(a: [f32; 32], b: [f32; 32]) -> [f32; 32] {
    let mut out = [0.0; 32];
    for left in 0..32usize {
        for right in 0..32usize {
            let mut swaps = 0u32;
            for bit in 0..5usize {
                if (right >> bit) & 1 == 1 {
                    swaps += (left >> (bit + 1)).count_ones();
                }
            }
            let metric_neg = ((left & right) >> 4) & 1;
            let sign = if (swaps + metric_neg as u32) & 1 == 0 { 1.0 } else { -1.0 };
            out[left ^ right] += sign * a[left] * b[right];
        }
    }
    out
}

#[test]
fn generated_recursive_cl41_gp_f32_coefficients_execute_on_wasm() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/spirv");
    let status = std::process::Command::new("python3")
        .arg(fixture_dir.join("gen_clifford_gp_f32_mvt5.py"))
        .arg("--check")
        .status()
        .expect("Cl(4,1) GP fixture generator should run");
    assert!(status.success(), "generated Cl(4,1) GP fixture is stale");
    let source = include_str!("fixtures/spirv/clifford_gp_recursive_f32_mvt5.fe");
    let wasm = compile_to_wasm("clifford_gp_recursive_f32_mvt5.fe", source);
    assert!(func_imports(&wasm).is_empty(), "recursive GP must not retain imports");
    let (mut store, instance) = instantiate(&wasm);
    let gp = instance
        .get_func(&mut store, "clifford_gp_cl41_mvt5_render")
        .expect("Cl(4,1) GP Render export");
    let mut cases = Vec::new();
    let mut dense_a = [0.0; 32];
    let mut dense_b = [0.0; 32];
    for i in 0..32 {
        dense_a[i] = (i + 1) as f32;
        dense_b[i] = (2 * i + 3) as f32;
    }
    let dense_expected = clifford_gp_cl41_oracle(dense_a, dense_b);
    assert_eq!(
        [dense_expected[0], dense_expected[1], dense_expected[4], dense_expected[17], dense_expected[31]],
        [268.0, 540.0, -2964.0, -3220.0, 2244.0],
        "dense Cl(4,1) oracle must retain the independent pinned components",
    );
    cases.push((dense_a, dense_b));
    for blade in [1usize, 2, 4, 8, 16, 31] {
        let mut a = [0.0; 32];
        let mut b = [0.0; 32];
        a[blade] = 1.0;
        b[blade] = 1.0;
        cases.push((a, b));
    }
    for (left, right) in [(1usize, 16usize), (16, 1)] {
        let mut a = [0.0; 32];
        let mut b = [0.0; 32];
        a[left] = 1.0;
        b[right] = 1.0;
        cases.push((a, b));
    }
    for (a, b) in cases {
        let expected = clifford_gp_cl41_oracle(a, b);
        let mut got = [0.0; 32];
        for index in 0..32 {
            let mut args = vec![
                wasmtime::Val::I32((index % 8) as i32),
                wasmtime::Val::I32((index / 8) as i32),
            ];
            args.extend(a.into_iter().chain(b).map(|value| wasmtime::Val::F32(value.to_bits())));
            let mut results = [wasmtime::Val::I32(0)];
            gp.call(&mut store, &args, &mut results)
                .expect("recursive Cl(4,1) GP coefficient execution");
            let wasmtime::Val::I32(word) = results[0] else {
                panic!("Cl(4,1) GP Render result must be i32")
            };
            got[index] = word as f32;
        }
        assert_eq!(got, expected);
    }
}

fn conformal_point_cl41(x: f32, y: f32, z: f32) -> [f32; 32] {
    let mut point = [0.0; 32];
    let radius2 = x * x + y * y + z * z;
    point[1] = x;
    point[2] = y;
    point[4] = z;
    point[8] = (radius2 - 1.0) * 0.5;
    point[16] = (radius2 + 1.0) * 0.5;
    point
}

#[test]
fn authored_generic_mvt5_cga_sandwich_executes_full_coefficient_frame_on_wasm() {
    let source = include_str!("fixtures/spirv/cga_sandwich_authored_generic_mvt5.fe");
    let wasm = compile_to_wasm("cga_sandwich_authored_generic_mvt5.fe", source);
    assert!(func_imports(&wasm).is_empty(), "authored generic sandwich must have zero imports");
    let (mut store, instance) = instantiate(&wasm);
    let sandwich = instance
        .get_typed_func::<(i32, i32, f32, f32, f32, f32, f32), i32>(
            &mut store, "cga_sandwich_authored_generic_mvt5",
        )
        .expect("authored generic MvT<5> sandwich ABI");
    for (x, y, z, cx, cy) in [
        (2.5, 0.25, 0.0, 0.5, 0.25),
        (0.5, 2.25, 0.0, 0.5, 0.25),
    ] {
        let mut sphere = [0.0; 32];
        let center2 = cx * cx + cy * cy;
        sphere[1] = cx;
        sphere[2] = cy;
        sphere[8] = center2 * 0.5 - 1.0;
        sphere[16] = center2 * 0.5;
        let expected = clifford_gp_cl41_oracle(
            clifford_gp_cl41_oracle(sphere, conformal_point_cl41(x, y, z)), sphere,
        );
        for (index, coefficient) in expected.into_iter().enumerate() {
            let got = sandwich.call(
                &mut store, ((index % 8) as i32, (index / 8) as i32, x, y, z, cx, cy),
            ).expect("authored generic CGA coefficient");
            assert_eq!(got, (coefficient * 256.0) as i32, "coefficient {index}");
        }
    }
}

#[test]
fn generated_recursive_cl41_cga_sandwich_executes_on_wasm() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/spirv");
    let status = std::process::Command::new("python3")
        .arg(fixture_dir.join("gen_cga_sandwich_f32_mvt5.py"))
        .arg("--check")
        .status()
        .expect("CGA sandwich fixture generator should run");
    assert!(status.success(), "generated CGA sandwich fixture is stale");
    let source = include_str!("fixtures/spirv/cga_sandwich_recursive_f32_mvt5.fe");
    let wasm = compile_to_wasm("cga_sandwich_recursive_f32_mvt5.fe", source);
    assert!(func_imports(&wasm).is_empty(), "recursive sandwich must not retain imports");
    let (mut store, instance) = instantiate(&wasm);
    let sandwich = instance
        .get_func(&mut store, "cga_sandwich_cl41_mvt5_render")
        .expect("recursive CGA sandwich Render export");
    let cases = [
        ((2.5, 0.0, 0.0), (0.5, -0.875, 0.125), (1.0, 0.0, 0.0)),
        ((0.5, 2.0, 0.0), (0.5, -0.875, 0.125), (0.5, 0.5, 0.0)),
        ((1.5, 1.0, 0.0), (0.5, -0.875, 0.125), (1.0, 0.5, 0.0)),
        ((2.0, 0.0, 0.0), (0.0, -1.0, 0.0), (0.5, 0.0, 0.0)),
    ];
    for ((x, y, z), (s1, s8, s16), expected_q) in cases {
        let mut sphere = [0.0; 32];
        sphere[1] = s1;
        sphere[8] = s8;
        sphere[16] = s16;
        let first = clifford_gp_cl41_oracle(sphere, conformal_point_cl41(x, y, z));
        let expected = clifford_gp_cl41_oracle(first, sphere);
        for (index, coefficient) in expected.iter().copied().enumerate() {
            let scaled = coefficient * 256.0;
            assert!(scaled.is_finite(), "coefficient {index} must be finite");
            assert_eq!(scaled.fract(), 0.0, "coefficient {index} must be exactly observable");
            assert_eq!(
                (scaled as i32) as f32,
                scaled,
                "coefficient {index} must fit the fixture's i32 observation"
            );
        }
        let mut got_words = [0i32; 32];
        for index in 0..32 {
            let args = [
                wasmtime::Val::I32((index % 8) as i32),
                wasmtime::Val::I32((index / 8) as i32),
                wasmtime::Val::F32(x.to_bits()),
                wasmtime::Val::F32(y.to_bits()),
                wasmtime::Val::F32(z.to_bits()),
                wasmtime::Val::F32(s1.to_bits()),
                wasmtime::Val::F32(s8.to_bits()),
                wasmtime::Val::F32(s16.to_bits()),
            ];
            let mut results = [wasmtime::Val::I32(0)];
            sandwich.call(&mut store, &args, &mut results)
                .expect("recursive CGA sandwich coefficient execution");
            let wasmtime::Val::I32(word) = results[0] else {
                panic!("CGA sandwich Render result must be i32")
            };
            got_words[index] = word;
            assert_eq!(word, (expected[index] * 256.0) as i32, "coefficient {index}");
        }
        for index in 0..32 {
            if ![1, 2, 4, 8, 16].contains(&index) {
                assert_eq!(got_words[index], 0, "off-vector blade {index}");
            }
        }
        let weight = expected[16] - expected[8];
        assert_ne!(weight, 0.0, "normalization case must be finite");
        assert_eq!(
            (expected[1] / weight, expected[2] / weight, expected[4] / weight),
            expected_q,
        );
    }
}

#[test]
fn generated_support_cl41_cga_sandwich_executes_on_wasm() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/spirv");
    let status = std::process::Command::new("python3")
        .arg(fixture_dir.join("gen_cga_sandwich_support_cl41.py"))
        .arg("--check")
        .status()
        .expect("support-specialized CGA fixture generator should run");
    assert!(status.success(), "support-specialized CGA fixture is stale");

    let source = include_str!("fixtures/spirv/cga_sandwich_support_cl41.fe");
    let wasm = compile_to_wasm("cga_sandwich_support_cl41.fe", source);
    assert!(
        func_imports(&wasm).is_empty(),
        "support-specialized sandwich must not retain imports"
    );
    let (mut store, instance) = instantiate(&wasm);
    let sandwich = instance
        .get_typed_func::<(i32, i32, f32, f32, f32, f32, f32), i32>(
            &mut store,
            "cga_sandwich_support_cl41",
        )
        .expect("support sandwich ABI must be exactly (i32,i32,f32,f32,f32,f32,f32)->i32");

    // Both cases use a nonzero-y dyadic center. Their exact binary inputs keep
    // the independent flat-Cl(4,1) oracle and generated operation tree directly
    // comparable, including the raw homogeneous weight.
    let cases = [
        (2.5, 0.25, 0.0, 0.5, 0.25),
        (0.5, 2.25, 0.0, 0.5, 0.25),
    ];
    for (x, y, z, cx, cy) in cases {
        let mut sphere = [0.0; 32];
        let center2 = cx * cx + cy * cy;
        sphere[1] = cx;
        sphere[2] = cy;
        sphere[8] = center2 * 0.5 - 1.0;
        sphere[16] = center2 * 0.5;
        let first = clifford_gp_cl41_oracle(sphere, conformal_point_cl41(x, y, z));
        let expected = clifford_gp_cl41_oracle(first, sphere);
        let weight = expected[16] - expected[8];
        let outputs = [expected[1] / weight, expected[2] / weight, expected[4] / weight, weight];

        for (index, expected_value) in outputs.into_iter().enumerate() {
            let px = (index % 2) as i32;
            let py = (index / 2) as i32;
            let got = sandwich
                .call(&mut store, (px, py, x, y, z, cx, cy))
                .expect("support-specialized CGA sandwich execution");
            assert_eq!(
                got as u32,
                (expected_value * 256.0) as i32 as u32,
                "output {index} for p=({x},{y},{z}), c=({cx},{cy},0)"
            );
        }
    }
}

fn cga_recursive_support_scalar_oracle(
    px: i32,
    py: i32,
    cam_x: f32,
    cam_y: f32,
    zoom: f32,
    inv_cx: f32,
    inv_cy: f32,
) -> u32 {
    let sx = (px as f32 - 64.0) * zoom;
    let sy = (py as f32 - 64.0) * zoom;
    let rz = 1.8_f32;
    let inv_len = 1.0 / (sx * sx + sy * sy + rz * rz).sqrt();
    let (rdx, rdy, rdz) = (sx * inv_len, sy * inv_len, rz * inv_len);
    let mut t = 0.0_f32;
    let mut i = 0_i32;
    while i < 72 {
        let x = cam_x + rdx * t;
        let y = cam_y + rdy * t;
        let z = -4.0 + rdz * t;
        let vx = x - inv_cx;
        let vy = y - inv_cy;
        let rho2 = vx * vx + vy * vy + z * z;
        let safe_rho2 = if rho2 < 0.0004 { 0.0004 } else { rho2 };
        let qx = inv_cx + vx / safe_rho2;
        let qy = inv_cy + vy / safe_rho2;
        let qz = z / safe_rho2;
        let tx = qx + 0.62;
        let ty = qy - 0.08;
        let ring_radius = (tx * tx + ty * ty).sqrt() - 0.58;
        let base = (ring_radius * ring_radius + qz * qz).sqrt() - 0.17;
        let distance = base * safe_rho2;
        t += distance * 0.18;
        if distance < 0.0022 {
            let shade = 38 + 24 * (i >> 3);
            if qy > 0.0 {
                return (shade + 88 * 256 + (255 - shade) * 65_536 - 16_777_216_i32)
                    as u32;
            }
            return (56 + shade * 256 + 224 * 65_536 - 16_777_216_i32) as u32;
        }
        i += 1;
    }
    (7 + 11 * 256 + 25 * 65_536 - 16_777_216_i32) as u32
}

#[test]
fn generated_recursive_support_cyclide_executes_full_frame_on_wasm() {
    const W: i32 = 128;
    const H: i32 = 128;
    const VALUES: [f32; 5] = [0.0, 0.0, 0.0125, 0.5, 0.0];
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/spirv");
    let status = std::process::Command::new("python3")
        .arg(fixture_dir.join("gen_cga_inversion_cyclide_recursive_support.py"))
        .arg("--check")
        .status()
        .expect("recursive-support cyclide generator should run");
    assert!(status.success(), "recursive-support cyclide fixture is stale");

    let source = include_str!("fixtures/spirv/cga_inversion_cyclide_recursive_support.fe");
    let started = std::time::Instant::now();
    let wasm = compile_to_wasm("cga_inversion_cyclide_recursive_support.fe", source);
    eprintln!(
        "recursive-support cyclide Wasm: {} bytes compiled in {:?}",
        wasm.len(),
        started.elapsed()
    );
    assert!(func_imports(&wasm).is_empty(), "recursive-support cyclide must have zero imports");
    let (mut store, instance) = instantiate(&wasm);
    let render = instance
        .get_typed_func::<(i32, i32, f32, f32, f32, f32, f32), i32>(
            &mut store,
            "cga_inversion_cyclide_recursive_support",
        )
        .expect("recursive-support ABI must be exactly two i32 builtins plus five f32 values");
    for py in 0..H {
        for px in 0..W {
            let got = render.call(
                &mut store,
                (px, py, VALUES[0], VALUES[1], VALUES[2], VALUES[3], VALUES[4]),
            ).expect("recursive-support cyclide Wasm pixel") as u32;
            let expected = cga_recursive_support_scalar_oracle(
                px, py, VALUES[0], VALUES[1], VALUES[2], VALUES[3], VALUES[4],
            );
            assert_eq!(got, expected, "recursive-support Wasm pixel ({px},{py})");
        }
    }
}

#[test]
fn generated_recursive_mvt5_f32_render_is_current_and_executes_on_wasm() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/spirv");
    let status = std::process::Command::new("python3")
        .arg(fixture_dir.join("gen_mvt5_f32_render.py"))
        .arg("--check")
        .status()
        .expect("python3 must run the MvT<5> fixture freshness check");
    assert!(status.success(), "generated MvT<5> fixture is stale");

    let source = include_str!("fixtures/spirv/mvt5_f32_render.fe");
    let wasm = compile_to_wasm("wasm_mvt5_f32_render.fe", source);
    assert!(func_imports(&wasm).is_empty());
    let (mut store, instance) = instantiate(&wasm);
    let render = instance
        .get_func(&mut store, "mvt5_f32_render")
        .expect("call-free MvT<5> scalar render export");
    for i in 0..32i32 {
        let mut args = vec![wasmtime::Val::I32(i % 8), wasmtime::Val::I32(i / 8)];
        args.extend((0..32).map(|leaf| {
            let value = (3 * leaf + 2) as f32;
            wasmtime::Val::F32(value.to_bits())
        }));
        let mut results = [wasmtime::Val::I32(0)];
        render
            .call(&mut store, &args, &mut results)
            .expect("MvT<5> wasm execution");
        let got = results[0].i32().expect("u32 result") as u32;
        let expected = ((2 * i + 1) * (3 * i + 2) + (1000 + i)) as u32;
        assert_eq!(got, expected, "DFS leaf {i} must retain its position");
    }
}

#[test]
fn conditional_f32_selection_feeds_loop_carry_and_both_exits_on_wasm() {
    let source = include_str!("fixtures/spirv/conditional_f32_loop_carry.fe");
    let wasm = compile_to_wasm("wasm_conditional_f32_loop_carry.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let run = instance
        .get_typed_func::<(i32, i32, f32, f32), i32>(&mut store, "conditional_f32_loop_carry")
        .expect("conditional_f32_loop_carry export");
    let normal = run.call(&mut store, (0, 0, 10.0, 60.0)).unwrap() as u32;
    let early = run.call(&mut store, (7, 0, 10.0, 60.0)).unwrap() as u32;
    assert_eq!(normal.to_le_bytes(), [40, 40, 40, 255]);
    assert_eq!(early.to_le_bytes(), [120, 120, 120, 255]);
}

#[test]
fn unsupported_f32_helpers_fail_closed_by_name() {
    for (name, params, args) in [
        ("__rsqrt_f32", "_: f32", "value"),
        ("__abs_f32", "_: f32", "value"),
        ("__min_f32", "_: f32, _: f32", "value, value"),
        ("__max_f32", "_: f32, _: f32", "value, value"),
        ("__floor_f32", "_: f32", "value"),
    ] {
        let source = format!(
            "extern {{ fn {name}({params}) -> f32 }}\npub fn probe(value: f32) -> f32 {{ {name}({args}) }}\n"
        );
        let error = compile_to_wasm_err(&format!("reject_{name}.fe"), &source);
        assert!(
            error.contains(name),
            "unsupported helper error must name `{name}`: {error}"
        );
        assert!(
            error.contains("dedicated Sonatina lowering")
                && error.contains("must not become an external call"),
            "unsupported helper `{name}` must fail closed before import emission: {error}"
        );
    }
}

/// `sum_to(n) = 0 + 1 + ... + (n-1)`: a loop with a loop-carried accumulator and
/// counter (phis inserted by Sonatina's SSA-variable machinery), compiled
/// Fe -> wasm and executed under wasmtime.
#[test]
fn fe_sum_to_loop_runs_on_wasm() {
    let source = "pub fn sum_to(n: u64) -> u64 {\n\
                  \x20   let mut acc: u64 = 0\n\
                  \x20   let mut i: u64 = 0\n\
                  \x20   while i < n {\n\
                  \x20       acc = acc + i\n\
                  \x20       i = i + 1\n\
                  \x20   }\n\
                  \x20   acc\n\
                  }\n\
                  pub fn main() -> u64 { sum_to(10) }\n";

    // Cross-backend twin.
    let evm = compile_to_evm("wasm_sum_to.fe", source);
    assert!(!evm.is_empty(), "evm twin bytecode must be non-empty");

    let wasm = compile_to_wasm("wasm_sum_to.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let sum_to = instance
        .get_typed_func::<i64, i64>(&mut store, "sum_to")
        .expect("`sum_to` export should exist");

    // sum_to(n) = n*(n-1)/2, all well within u64.
    for n in [0i64, 1, 5, 10, 100] {
        let expected = n * (n - 1) / 2;
        assert_eq!(
            sum_to.call(&mut store, n).unwrap(),
            expected,
            "sum_to({n}) should be {expected}"
        );
    }
}

/// R3.2 THE MILESTONE: a Fe `extern` host function becomes a real wasm import.
///
/// `extern { pub unsafe fn host_add(a, b) -> u64 }` is a non-builtin extern (no
/// Fe body, not a recognized runtime builtin), so it lowers to a DECLARED-
/// EXTERNAL runtime function with `Linkage::External` and no body, which the
/// WAFFLE backend (R3.1 pass-0) emits as a `("fe", "host_add")` wasm import.
/// `use_host` calls it; wasmtime satisfies the import through a `Linker` stub.
/// Because `host_add` has no body, the only way `use_host`/`main` can run at all
/// is via the emitted import, so a passing run proves the import path end to end.
#[test]
fn fe_extern_host_import_runs_on_wasm() {
    let source = "extern {\n\
                  \x20   pub unsafe fn host_add(a: u64, b: u64) -> u64\n\
                  }\n\
                  pub fn use_host(a: u64, b: u64) -> u64 { host_add(a, b) }\n\
                  pub fn main() -> u64 { use_host(2, 3) }\n";

    let wasm = compile_to_wasm("wasm_host_import.fe", source);

    // Scan the emitted bytes: the ("fe", "host_add") func import must be present.
    let imports = func_imports(&wasm);
    assert!(
        imports.contains(&("fe".to_string(), "host_add".to_string())),
        "expected a (\"fe\", \"host_add\") func import in the emitted wasm, found {imports:?}"
    );

    // Instantiate through a Linker that satisfies the import with a stub
    // (host_add(a, b) = a + b). The plain empty-imports `Instance::new` path used
    // by the other R1 tests cannot instantiate this module: it has an import.
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    linker
        .func_wrap("fe", "host_add", |a: u64, b: u64| a + b)
        .expect("binding the ('fe','host_add') host stub should succeed");
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("wasmtime should instantiate with the host import satisfied");

    // use_host(a, b) calls the host import; the stub returns a + b.
    let use_host = instance
        .get_typed_func::<(u64, u64), u64>(&mut store, "use_host")
        .expect("`use_host` export should exist");
    assert_eq!(
        use_host.call(&mut store, (2, 3)).unwrap(),
        5,
        "use_host(2, 3) should call the host import and return 5"
    );
    assert_eq!(use_host.call(&mut store, (40, 2)).unwrap(), 42);

    // main() calls use_host(2, 3) internally, which calls the host import.
    let main = instance
        .get_typed_func::<(), u64>(&mut store, "main")
        .expect("`main` export should exist");
    assert_eq!(main.call(&mut store, ()).unwrap(), 5, "main() should be 5");
}

/// R3.3 THE MILESTONE: `#[wasm_import(module = "fe:host")]` on an extern block
/// names the wasm import MODULE. `host_log` becomes a `("fe:host", "host_log")`
/// import instead of the flat `("fe", "host_log")` v0 default. The module string
/// threads HIR (block attribute propagated onto the extern `Func`) -> runtime
/// package -> the WasmBackend side table -> WAFFLE import emission. wasmtime
/// satisfies the import through a `Linker` bound at `("fe:host", "host_log")`.
#[test]
fn fe_wasm_import_module_attribute_names_module() {
    let source = "#[wasm_import(module = \"fe:host\")]\n\
                  extern {\n\
                  \x20   pub unsafe fn host_log(a: u64, b: u64) -> u64\n\
                  }\n\
                  pub fn use_host(a: u64, b: u64) -> u64 { host_log(a, b) }\n\
                  pub fn main() -> u64 { use_host(2, 3) }\n";

    let wasm = compile_to_wasm("wasm_import_module.fe", source);

    // The attribute's module is on the emitted import: ("fe:host", "host_log").
    let imports = func_imports(&wasm);
    assert!(
        imports.contains(&("fe:host".to_string(), "host_log".to_string())),
        "expected a (\"fe:host\", \"host_log\") func import in the emitted wasm, found {imports:?}"
    );
    // The flat "fe" module must NOT appear for this symbol (the attribute won).
    assert!(
        !imports.contains(&("fe".to_string(), "host_log".to_string())),
        "the attribute module should replace the flat \"fe\" default, found {imports:?}"
    );

    // Instantiate through a Linker bound at the attribute's module namespace.
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    linker
        .func_wrap("fe:host", "host_log", |a: u64, b: u64| a + b)
        .expect("binding the ('fe:host','host_log') host stub should succeed");
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("wasmtime should instantiate with the fe:host import satisfied");

    let main = instance
        .get_typed_func::<(), u64>(&mut store, "main")
        .expect("`main` export should exist");
    assert_eq!(
        main.call(&mut store, ()).unwrap(),
        5,
        "main() should call the fe:host import and return 5"
    );
}

// ===========================================================================
// R3.4b THE KEYSTONE: a Fe-compiled-to-wasm host program drives the WebGPU
// `Dispatch` + `Wait` capability import table against a wasmtime FAKE DEVICE and
// the pinned NTT-8 vectors land in wasm linear memory.
//
// The Fe host program is a PURE ORCHESTRATOR (the Path A-prime memory model,
// interop section 8): region handles are host-minted (the `main`/`main_begin`
// entries receive their `MemPtr<u32>`s as exported-fn parameters), so there is no
// `from_raw` and no Fe-side address arithmetic - every step is a raw import call.
// The fake device owns a host-side buffer table; `gpu_dispatch` is a pinned-vector
// table lookup (NO field arithmetic in the stub, so the oracle cannot lie).
// WebGPU EXECUTION is browser-only and never sandbox-verifiable; this run proves
// the CONTRACT (the import op-set and the memory model), not a GPU.
// ===========================================================================

/// The pinned size-8 forward-NTT probe input over `F_12289`
/// (`crates/fe/tests/fixtures/fe_test/ntt_exec.fe`).
const NTT8_INPUT: [u32; 8] = [5, 15, 39, 77, 129, 195, 275, 369];
/// The pinned size-8 forward-NTT output (the probe's pinned sample).
const NTT8_OUTPUT: [u32; 8] = [1104, 6528, 7157, 1035, 12081, 7898, 4772, 8621];

/// The Fe host program (host-minted handles, Path A-prime). Two entries over the
/// same straight-line sequence:
///   - `main`: create -> upload -> dispatch -> readback_begin -> wait, threading
///     BOTH `Dispatch` and `Wait`; when it returns, `output` is resident.
///   - `main_begin` + `on_ready`: the degraded/continuation twin. `main_begin`
///     composes WITHOUT `Wait` in scope (create..readback_begin only) and returns
///     the `Pending<()>`; the host later drives `on_ready(token)`, which completes
///     the resident copy. This proves degraded mode composes by construction and
///     that readback is honestly asynchronous (no copy until completion).
const WEBGPU_NTT8_SRC: &str = r#"
use core::MemPtr
use std::webgpu::{Dispatch, Wait, WebGpuBackend, KernelId, WorkerGpu, Pending}

// The pinned NTT-8 kernel is the page's pipeline-table index 0 (layout.json).
fn ntt8_on_gpu(_ input: MemPtr<u32>, _ output: MemPtr<u32>)
    uses (gpu: mut Dispatch<WebGpuBackend>, w: mut Wait<WebGpuBackend>)
{
    let buf = gpu.create(8)
    gpu.upload(input, 8, buf)
    gpu.dispatch(KernelId::new(0), 1)
    let p = gpu.readback_begin(buf, 8, output)
    w.wait(p)
}

pub fn main(_ input: MemPtr<u32>, _ output: MemPtr<u32>) {
    with (Dispatch<WebGpuBackend> = WorkerGpu {}, Wait<WebGpuBackend> = WorkerGpu {}) {
        ntt8_on_gpu(input, output)
    }
}

fn ntt8_begin(_ input: MemPtr<u32>, _ output: MemPtr<u32>) -> Pending<WebGpuBackend, ()>
    uses (gpu: mut Dispatch<WebGpuBackend>)
{
    let buf = gpu.create(8)
    gpu.upload(input, 8, buf)
    gpu.dispatch(KernelId::new(0), 1)
    gpu.readback_begin(buf, 8, output)
}

pub fn main_begin(_ input: MemPtr<u32>, _ output: MemPtr<u32>) -> Pending<WebGpuBackend, ()> {
    with (Dispatch<WebGpuBackend> = WorkerGpu {}) {
        ntt8_begin(input, output)
    }
}

fn wait_ready(_ pending: own Pending<WebGpuBackend, ()>) uses (w: mut Wait<WebGpuBackend>) {
    w.wait(pending)
}

pub fn on_ready(_ pending: own Pending<WebGpuBackend, ()>) {
    with (Wait<WebGpuBackend> = WorkerGpu {}) {
        wait_ready(pending)
    }
}
"#;

/// The wasmtime FAKE DEVICE: a host-side buffer table plus a pending-readback
/// table and an op-sequence log. It mirrors the `fe:webgpu` / `fe:async` import
/// op-set op-for-op. `gpu_dispatch` is a PINNED-VECTOR TABLE LOOKUP with no field
/// arithmetic: an unrecognized input traps, so the NTT-8 oracle is a lookup, not a
/// re-implementation of the transform.
#[derive(Default)]
struct FakeDevice {
    /// Buffer table: `buffers[handle]` is a device buffer of `u32` words.
    buffers: Vec<Vec<u32>>,
    /// Pending readbacks: `pending[token]` records the copy `wait`/`on_ready`
    /// will perform (source buffer, word count, destination byte offset).
    pending: Vec<PendingReadback>,
    /// The op-sequence log, one entry per serviced import call.
    log: Vec<&'static str>,
}

struct PendingReadback {
    src_buffer: usize,
    len_words: usize,
    dst_addr: usize,
}

/// Read `len` little-endian `u32` words from `mem` starting at byte offset `addr`.
fn read_words(mem: &[u8], addr: usize, len: usize) -> Vec<u32> {
    (0..len)
        .map(|i| {
            let off = addr + i * 4;
            u32::from_le_bytes(mem[off..off + 4].try_into().expect("4 bytes in range"))
        })
        .collect()
}

/// Write `words` as little-endian `u32`s into `mem` starting at byte offset `addr`.
fn write_words(mem: &mut [u8], addr: usize, words: &[u32]) {
    for (i, word) in words.iter().enumerate() {
        let off = addr + i * 4;
        mem[off..off + 4].copy_from_slice(&word.to_le_bytes());
    }
}

/// Build a `Linker` that services the `fe:webgpu` + `fe:async` import op-set with a
/// `FakeDevice`, and instantiate `wasm` against it. The `FakeDevice` lives in the
/// store data; host bodies reach both it and the instance's exported `memory`
/// through `Caller`.
fn instantiate_fake_device(wasm: &[u8]) -> (wasmtime::Store<FakeDevice>, wasmtime::Instance) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, FakeDevice::default());
    let mut linker = wasmtime::Linker::new(&engine);

    // gpu_buffer_create(len) -> handle: mint a fresh buffer handle.
    linker
        .func_wrap(
            "fe:webgpu",
            "gpu_buffer_create",
            |mut caller: wasmtime::Caller<'_, FakeDevice>, len: i32| -> i32 {
                let dev = caller.data_mut();
                dev.buffers.push(vec![0u32; len as usize]);
                dev.log.push("create");
                (dev.buffers.len() - 1) as i32
            },
        )
        .expect("bind gpu_buffer_create");

    // gpu_upload(src, len, dst): copy `len` words OUT of exported memory at byte
    // offset `src` into buffer `dst`.
    linker
        .func_wrap(
            "fe:webgpu",
            "gpu_upload",
            |mut caller: wasmtime::Caller<'_, FakeDevice>,
             src: i32,
             len: i32,
             dst: i32|
             -> Result<(), wasmtime::Error> {
                let memory = caller
                    .get_export("memory")
                    .and_then(wasmtime::Extern::into_memory)
                    .ok_or_else(|| wasmtime::Error::msg("instance has no exported `memory`"))?;
                let (mem, dev) = memory.data_and_store_mut(&mut caller);
                let words = read_words(mem, src as usize, len as usize);
                let dst = dst as usize;
                if dst >= dev.buffers.len() {
                    return Err(wasmtime::Error::msg("gpu_upload: unknown buffer handle"));
                }
                dev.buffers[dst] = words;
                dev.log.push("upload");
                Ok(())
            },
        )
        .expect("bind gpu_upload");

    // gpu_dispatch(kernel, groups): the pinned-vector table lookup. The single
    // uploaded buffer must equal the pinned NTT-8 input; replace it with the pinned
    // output. Any unrecognized input TRAPS (no field arithmetic here).
    linker
        .func_wrap(
            "fe:webgpu",
            "gpu_dispatch",
            |mut caller: wasmtime::Caller<'_, FakeDevice>,
             _kernel: i32,
             _groups: i32|
             -> Result<(), wasmtime::Error> {
                let dev = caller.data_mut();
                let hit = dev
                    .buffers
                    .iter_mut()
                    .find(|buf| buf.as_slice() == NTT8_INPUT.as_slice());
                match hit {
                    Some(buf) => {
                        *buf = NTT8_OUTPUT.to_vec();
                        dev.log.push("dispatch");
                        Ok(())
                    }
                    None => Err(wasmtime::Error::msg(
                        "gpu_dispatch: no buffer holds the pinned NTT-8 input (unrecognized \
                         kernel input; the stub does no field arithmetic)",
                    )),
                }
            },
        )
        .expect("bind gpu_dispatch");

    // gpu_readback_begin(src, len, dst) -> token: mint a token and RECORD the
    // pending copy WITHOUT touching memory (readback is honestly asynchronous).
    linker
        .func_wrap(
            "fe:webgpu",
            "gpu_readback_begin",
            |mut caller: wasmtime::Caller<'_, FakeDevice>, src: i32, len: i32, dst: i32| -> i32 {
                let dev = caller.data_mut();
                dev.pending.push(PendingReadback {
                    src_buffer: src as usize,
                    len_words: len as usize,
                    dst_addr: dst as usize,
                });
                dev.log.push("readback_begin");
                (dev.pending.len() - 1) as i32
            },
        )
        .expect("bind gpu_readback_begin");

    // wait(token): perform the recorded copy (buffer -> exported memory at dst).
    linker
        .func_wrap(
            "fe:async",
            "wait",
            |mut caller: wasmtime::Caller<'_, FakeDevice>,
             token: i32|
             -> Result<(), wasmtime::Error> {
                let memory = caller
                    .get_export("memory")
                    .and_then(wasmtime::Extern::into_memory)
                    .ok_or_else(|| wasmtime::Error::msg("instance has no exported `memory`"))?;
                let (mem, dev) = memory.data_and_store_mut(&mut caller);
                let token = token as usize;
                let pending = dev
                    .pending
                    .get(token)
                    .ok_or_else(|| wasmtime::Error::msg("wait: unknown pending token"))?;
                let words = dev.buffers[pending.src_buffer][..pending.len_words].to_vec();
                write_words(mem, pending.dst_addr, &words);
                dev.log.push("wait");
                Ok(())
            },
        )
        .expect("bind wait");

    let instance = linker
        .instantiate(&mut store, &module)
        .expect("wasmtime should instantiate with the fe:webgpu / fe:async imports satisfied");
    (store, instance)
}

/// R3.4b THE KEYSTONE (LANDED): `main` drives create -> upload -> dispatch ->
/// readback_begin -> wait against the fake device, and the pinned NTT-8 outputs land
/// in wasm linear memory. The op-sequence log proves the exact import op-set was walked.
///
/// Path to green (over R3.4c's exported-param-entry enabler `build_wasm_runtime_package`):
/// (1) the fixture's `with (gpu: WorkerGpu {})` COLON syntax is not with-block grammar
/// (only `Key = value` / bare shorthand parse), so `with` silently fell back to a call
/// `with(...)`; corrected to the supported `with (Dispatch<WebGpuBackend> = WorkerGpu {},
/// ...)` keyed-by-trait form. (2) `MemPtr<u32>` classifies at the extern boundary as
/// `RawAddr { space: Memory }`, not the `Ref { Provider { Memory } }` the WIP admitted;
/// `wasm_lower::ty_for_class` + `runtime::is_wasm_import_boundary_class` corrected. (3)
/// Amendment 4's transport-newtype extension (architect ruling): the raw externs take
/// the single-`u32`-field capability newtypes (`WebGpuRef<u32, Global>`, `KernelId`,
/// `Pending<()>`) WHOLE, each transported as its one word, so the `WorkerGpu` bodies are
/// pure pass-through with ZERO field reads - no `RExpr::Load`/place needed, staying
/// inside the SSA-value-only wasm model. (4) host-import field NAME is the extern's base
/// op identifier (`mir::wasm_import_name`), decoupled from the mangled Sonatina symbol,
/// and per-effect-scope duplicate import instances dedup to one import per (module, op).
#[test]
fn fe_webgpu_ntt8_runs_on_wasm_fake_device() {
    let wasm = compile_to_wasm("wasm_webgpu_ntt8.fe", WEBGPU_NTT8_SRC);

    // The capability import op-set is on the emitted wasm, module-named per R3.3.
    let imports = func_imports(&wasm);
    for expected in [
        ("fe:webgpu", "gpu_buffer_create"),
        ("fe:webgpu", "gpu_upload"),
        ("fe:webgpu", "gpu_dispatch"),
        ("fe:webgpu", "gpu_readback_begin"),
        ("fe:async", "wait"),
    ] {
        assert!(
            imports.contains(&(expected.0.to_string(), expected.1.to_string())),
            "expected import {expected:?} in the emitted wasm, found {imports:?}"
        );
    }

    let (mut store, instance) = instantiate_fake_device(&wasm);
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("the emitted wasm should export `memory`");

    // Host-chosen regions, clear of the 1024-word bump base: input at byte 4096,
    // output at byte 8192.
    const INPUT_ADDR: i32 = 4096;
    const OUTPUT_ADDR: i32 = 8192;

    // The host writes the 8 input words into wasm memory.
    let mut input_bytes = [0u8; 32];
    for (i, word) in NTT8_INPUT.iter().enumerate() {
        input_bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    memory
        .write(&mut store, INPUT_ADDR as usize, &input_bytes)
        .expect("writing the input words should succeed");

    // Fe drives the whole sequence; `output` is resident when `main` returns.
    let main = instance
        .get_typed_func::<(i32, i32), ()>(&mut store, "main")
        .expect("`main` export should exist");
    main.call(&mut store, (INPUT_ADDR, OUTPUT_ADDR))
        .expect("main(input, output) should run the full dispatch sequence");

    // The 8 output words in wasm memory equal the pinned NTT-8 outputs.
    let mut output_bytes = [0u8; 32];
    memory
        .read(&store, OUTPUT_ADDR as usize, &mut output_bytes)
        .expect("reading the output words should succeed");
    let output: Vec<u32> = (0..8)
        .map(|i| u32::from_le_bytes(output_bytes[i * 4..i * 4 + 4].try_into().unwrap()))
        .collect();
    assert_eq!(
        output,
        NTT8_OUTPUT.to_vec(),
        "the Fe-compiled wasm should land the pinned NTT-8 outputs in linear memory \
         through the capability import table"
    );

    // The op-sequence log is exactly the ratified walk.
    assert_eq!(
        store.data().log,
        vec!["create", "upload", "dispatch", "readback_begin", "wait"],
        "the fake device should service exactly the ratified op-sequence"
    );
}

#[test]
fn raw_memory_scalar_roundtrips_are_byte_exact_on_wasm() {
    let source = r#"
use core::MemPtr

fn write_u8(_ value: u8) uses (target: mut u8) { target = value }
fn read_u8() -> u8 uses (target: u8) { target }
pub fn roundtrip_u8(_ ptr: MemPtr<u8>, value: u8) -> u8 {
    with (ptr) {
        write_u8(value)
        read_u8()
    }
}

fn write_u16(_ value: u16) uses (target: mut u16) { target = value }
fn read_u16() -> u16 uses (target: u16) { target }
pub fn roundtrip_u16(_ ptr: MemPtr<u16>, value: u16) -> u16 {
    with (ptr) {
        write_u16(value)
        read_u16()
    }
}

fn write_u32(_ value: u32) uses (target: mut u32) { target = value }
fn read_u32() -> u32 uses (target: u32) { target }
pub fn roundtrip_u32(_ ptr: MemPtr<u32>, value: u32) -> u32 {
    with (ptr) {
        write_u32(value)
        read_u32()
    }
}

fn write_u64(_ value: u64) uses (target: mut u64) { target = value }
fn read_u64() -> u64 uses (target: u64) { target }
pub fn roundtrip_u64(_ ptr: MemPtr<u64>, value: u64) -> u64 {
    with (ptr) {
        write_u64(value)
        read_u64()
    }
}

fn write_f32(_ value: f32) uses (target: mut f32) { target = value }
fn read_f32() -> f32 uses (target: f32) { target }
pub fn roundtrip_f32(_ ptr: MemPtr<f32>, value: f32) -> f32 {
    with (ptr) {
        write_f32(value)
        read_f32()
    }
}
"#;
    let wasm = compile_to_wasm("wasm_raw_memory_scalars.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("raw scalar access requires exported linear memory");

    assert_eq!(
        instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "roundtrip_u8")
            .unwrap()
            .call(&mut store, (17, 0xab))
            .unwrap(),
        0xab
    );
    assert_eq!(
        instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "roundtrip_u16")
            .unwrap()
            .call(&mut store, (18, 0xcdef))
            .unwrap(),
        0xcdef
    );
    assert_eq!(
        instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "roundtrip_u32")
            .unwrap()
            .call(&mut store, (20, 0x78563412))
            .unwrap(),
        0x78563412
    );
    assert_eq!(
        instance
            .get_typed_func::<(i32, i64), i64>(&mut store, "roundtrip_u64")
            .unwrap()
            .call(&mut store, (24, 0x0807060504030201))
            .unwrap(),
        0x0807060504030201
    );
    assert_eq!(
        instance
            .get_typed_func::<(i32, f32), f32>(&mut store, "roundtrip_f32")
            .unwrap()
            .call(&mut store, (32, -13.25))
            .unwrap(),
        -13.25
    );

    let bytes = memory.data(&store);
    assert_eq!(&bytes[17..18], &[0xab]);
    assert_eq!(&bytes[18..20], &0xcdef_u16.to_le_bytes());
    assert_eq!(&bytes[20..24], &0x78563412_u32.to_le_bytes());
    assert_eq!(&bytes[24..32], &0x0807060504030201_u64.to_le_bytes());
    assert_eq!(&bytes[32..36], &(-13.25_f32).to_le_bytes());
}

#[test]
fn mem_ptr_ordinary_read_write_wrappers_do_not_recurse_on_wasm() {
    let source = r#"
use core::MemPtr

pub fn ordinary_roundtrip(_ ptr: MemPtr<u32>, value: u32) -> u32 {
    ptr.write(value)
    ptr.read()
}
"#;
    let wasm = compile_to_wasm("wasm_mem_ptr_ordinary_wrappers.fe", source);
    let mut function_index = func_imports(&wasm).len() as u32;
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        if let wasmparser::Payload::CodeSectionEntry(body) = payload.expect("valid wasm") {
            for operator in body.get_operators_reader().unwrap() {
                if let wasmparser::Operator::Call {
                    function_index: callee,
                } = operator.unwrap()
                {
                    assert_ne!(
                        callee, function_index,
                        "MemPtr wrapper call graph must not contain a direct self edge"
                    );
                }
            }
            function_index += 1;
        }
    }
    let (mut store, instance) = instantiate(&wasm);
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("MemPtr wrapper access requires exported linear memory");
    let roundtrip = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "ordinary_roundtrip")
        .expect("ordinary_roundtrip export");

    assert_eq!(
        roundtrip.call(&mut store, (19, 0x78563412)).unwrap(),
        0x78563412
    );
    assert_eq!(
        &memory.data(&store)[19..23],
        &0x78563412_u32.to_le_bytes()
    );
}

#[test]
fn raw_memory_record_fields_use_wasm_layout_offsets() {
    let source = r#"
use core::MemPtr

struct Mixed {
    a: u8,
    b: u32,
    c: u16,
    d: u64,
}

fn write_mixed(a: u8, b: u32, c: u16, d: u64) uses (record: mut Mixed) {
    record.a = a
    record.b = b
    record.c = c
    record.d = d
}

fn read_d() -> u64 uses (record: Mixed) {
    record.d
}

pub fn record_roundtrip(
    _ ptr: MemPtr<Mixed>,
    a: u8,
    b: u32,
    c: u16,
    d: u64,
) -> u64 {
    with (ptr) {
        write_mixed(a, b, c, d)
        read_d()
    }
}
"#;
    let wasm = compile_to_wasm("wasm_raw_memory_record_fields.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("record field access requires exported linear memory");
    let roundtrip = instance
        .get_typed_func::<(i32, i32, i32, i32, i64), i64>(&mut store, "record_roundtrip")
        .expect("record_roundtrip export");

    const BASE: i32 = 17;
    let d = 0x0807060504030201_i64;
    assert_eq!(
        roundtrip
            .call(&mut store, (BASE, 0xab, 0x78563412, 0xcdef, d))
            .unwrap(),
        d
    );
    // WASM_LAYOUT is an eight-byte word layout: each scalar field begins at
    // the next word, while Mstore itself writes only the scalar's true width.
    let bytes = &memory.data(&store)[BASE as usize..BASE as usize + 32];
    assert_eq!(&bytes[0..1], &[0xab]);
    assert_eq!(&bytes[1..8], &[0; 7]);
    assert_eq!(&bytes[8..12], &0x78563412_u32.to_le_bytes());
    assert_eq!(&bytes[12..16], &[0; 4]);
    assert_eq!(&bytes[16..18], &0xcdef_u16.to_le_bytes());
    assert_eq!(&bytes[18..24], &[0; 6]);
    assert_eq!(&bytes[24..32], &d.to_le_bytes());
}

/// R3.4b twin: the `on_ready` continuation lane. `main_begin` composes WITHOUT
/// `Wait` (create..readback_begin) and returns the `Pending<()>`; the output region
/// is UNCHANGED until the host drives `on_ready(token)`, which completes the copy
/// with the same token (the async-honesty assert + the continuation re-entry).
///
/// LANDED alongside `fe_webgpu_ntt8_runs_on_wasm_fake_device`: `main_begin` composes
/// WITHOUT `Wait` and returns the token, the output region stays UNCHANGED until the
/// host drives the exported `on_ready(token)` continuation, which completes the copy.
#[test]
fn fe_webgpu_ntt8_on_ready_continuation() {
    let wasm = compile_to_wasm("wasm_webgpu_ntt8.fe", WEBGPU_NTT8_SRC);
    let (mut store, instance) = instantiate_fake_device(&wasm);
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("the emitted wasm should export `memory`");

    const INPUT_ADDR: i32 = 4096;
    const OUTPUT_ADDR: i32 = 8192;

    let mut input_bytes = [0u8; 32];
    for (i, word) in NTT8_INPUT.iter().enumerate() {
        input_bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    memory
        .write(&mut store, INPUT_ADDR as usize, &input_bytes)
        .expect("writing the input words should succeed");

    // main_begin: create -> upload -> dispatch -> readback_begin, returns the token.
    // NO `wait`, so the copy has not happened yet.
    let main_begin = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "main_begin")
        .expect("`main_begin` export should exist");
    let token = main_begin
        .call(&mut store, (INPUT_ADDR, OUTPUT_ADDR))
        .expect("main_begin should run create..readback_begin");

    // Async-honesty: the output region is UNCHANGED before completion.
    let mut before = [0u8; 32];
    memory
        .read(&store, OUTPUT_ADDR as usize, &mut before)
        .expect("reading the output region should succeed");
    assert_eq!(
        before, [0u8; 32],
        "readback_begin must NOT touch memory: the output region stays unchanged \
         until the continuation completes"
    );
    assert_eq!(
        store.data().log,
        vec!["create", "upload", "dispatch", "readback_begin"],
        "main_begin should stop at readback_begin (no wait yet)"
    );

    // The host drives the exported continuation with the returned token; on_ready
    // completes the resident copy.
    let on_ready = instance
        .get_typed_func::<i32, ()>(&mut store, "on_ready")
        .expect("`on_ready` export should exist");
    on_ready
        .call(&mut store, token)
        .expect("on_ready(token) should complete the readback");

    // After the continuation, the pinned outputs are resident.
    let mut after = [0u8; 32];
    memory
        .read(&store, OUTPUT_ADDR as usize, &mut after)
        .expect("reading the output region should succeed");
    let output: Vec<u32> = (0..8)
        .map(|i| u32::from_le_bytes(after[i * 4..i * 4 + 4].try_into().unwrap()))
        .collect();
    assert_eq!(
        output,
        NTT8_OUTPUT.to_vec(),
        "on_ready(token) should land the pinned NTT-8 outputs in linear memory"
    );
    assert_eq!(
        store.data().log,
        vec!["create", "upload", "dispatch", "readback_begin", "wait"],
        "on_ready should complete the deferred wait with the right token"
    );
}

/// A two-function call pair compiled Fe -> wasm: `apply` calls `add`.
#[test]
fn fe_call_pair_runs_on_wasm() {
    let source = "pub fn add(a: u64, b: u64) -> u64 { a + b }\n\
                  pub fn apply(a: u64, b: u64) -> u64 { add(a, b) }\n\
                  pub fn main() -> u64 { apply(20, 22) }\n";

    let evm = compile_to_evm("wasm_call_pair.fe", source);
    assert!(!evm.is_empty(), "evm twin bytecode must be non-empty");

    let wasm = compile_to_wasm("wasm_call_pair.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let apply = instance
        .get_typed_func::<(i64, i64), i64>(&mut store, "apply")
        .expect("`apply` export should exist");
    assert_eq!(apply.call(&mut store, (20, 22)).unwrap(), 42);
    assert_eq!(apply.call(&mut store, (2, 3)).unwrap(), 5);
}

// ===========================================================================
// mb2 CE-5 (folds R3.6) THE CONTRACT: the worker TASK RAIL as the completion-cell
// rail's SECOND client. A Fe-compiled-to-wasm program forks/joins the size-8 NTT
// through the `spawn`/`Pending`/`wait` task rail (`std::worker`) against a wasmtime
// DEGENERATE ONE-WORKER POOL, and the recompiled fork-join NTT EQUALS the sequential
// oracle (`crates/fe/tests/fixtures/fe_test/ntt_exec.fe`, pinned `NTT8_OUTPUT`).
//
// The proof is a CONTRACT, not a real pool (R3): tasks are word-in / word-out leaves
// named by `TaskId` into the module's exported `fe_task` dispatch (NO closure
// shipping, R3), so the shared-buffer butterfly shape of the EVM `ntt_par_exec.fe`
// cannot cross the rail; the faithful task-rail realization is the independent DFT
// leaf `fe_task(0, k) -> X[k]` (cyclic convention `X[k] = sum_j x[j] w8^{jk} mod q`,
// the same convention `ntt_exec.fe` pins). The one-worker pool executes `fe_task`
// SYNCHRONOUSLY in-instance, stores the `u64` in the token's slot, marks it READY,
// `wait` returns it, and a double `task_result` traps host-side (the affine token's
// dynamic READY->CONSUMED backstop). Browser claims: NONE (this is the wasmtime
// contract; the bun 2-worker pool run is a DEFERRED follow-on, not this slice).
// ===========================================================================

/// The shared field-`F_q` arithmetic (`q = 12289`) plus the pinned probe input and
/// the NTT-8 coefficient leaf, all SCALAR `u64` (no linear memory: the wasm value
/// model, R2.0). The wasm R1 op envelope is `+`/`-`/`*` and the comparisons `<`/`==`
/// ONLY (`/`, `%`, shifts, bitwise are R2), so mod-`q` reduction is done by
/// SUBTRACTION and the twiddle powers are BAKED constants (no `powmod`, whose bit
/// loop needs `%`/`/`). Every product is `< q^2 ~ 1.5e8`, exact in `u64`. Prepended
/// to each worker fixture source below.
const WORKER_FIELD_ARITH: &str = r#"
// mod-q by repeated subtraction (only `<` and `-`): x < q^2, so at most q-1
// subtractions of q are needed; setting i = 12289 forces the loop to exit.
fn reduce_q(_ x: u64) -> u64 {
    let mut r = x
    let mut i: u64 = 0
    while i < 12289 {
        if r < 12289 {
            i = 12289
        } else {
            r = r - 12289
            i = i + 1
        }
    }
    r
}

fn addmod_q(_ a: u64, _ b: u64) -> u64 {
    let s = a + b
    if s < 12289 { s } else { s - 12289 }
}

fn mulmod_q(_ a: u64, _ b: u64) -> u64 {
    reduce_q(a * b)
}

// Baked twiddle powers w8^i mod q for i in 0..7 (w8 = 8246, q = 12289). powmod is
// R2 (its bit loop needs `%`/`/`), so the eight constants are inlined.
fn wpow(_ i: u64) -> u64 {
    if i == 0 { 1 }
    else if i == 1 { 8246 }
    else if i == 2 { 1479 }
    else if i == 3 { 5146 }
    else if i == 4 { 12288 }
    else if i == 5 { 4043 }
    else if i == 6 { 10810 }
    else { 7143 }
}

fn probe8(_ j: u64) -> u64 {
    if j == 0 { 5 }
    else if j == 1 { 15 }
    else if j == 2 { 39 }
    else if j == 3 { 77 }
    else if j == 4 { 129 }
    else if j == 5 { 195 }
    else if j == 6 { 275 }
    else { 369 }
}

// One INDEPENDENT NTT leaf: coefficient k of the forward size-8 NTT over F_q,
// cyclic convention X[k] = sum_j x[j] * w8^{jk} mod q (w8 = 8246). The twiddle
// wc = w8^{jk} is advanced by multiplying w8^k each j-step (no powmod). PURE scalar,
// no shared buffer: exactly the word-in / word-out task shape R3 sanctions.
fn ntt8_coeff(_ k: u64) -> u64 {
    let wk = wpow(k)
    let mut s: u64 = 0
    let mut wc: u64 = 1
    let mut j: u64 = 0
    while j < 8 {
        s = addmod_q(s, mulmod_q(probe8(j), wc))
        wc = mulmod_q(wc, wk)
        j = j + 1
    }
    s
}
"#;

/// THE TASK-RAIL fixture: `fe_task` dispatch + `Spawn`/`Wait` fork-join via the
/// `WorkerPool` provider. `coeff` is a single-leaf join; `coeff_pair` is the two-way
/// disjoint fork-join (N2's stage-split shape) holding TWO affine tokens at once.
const WORKER_NTT8_SPECIFIC: &str = r#"
// THE EXPORTED TASK TABLE (R3): the pool dispatches tasks by TaskId into this
// exported `fe_task(entry: u32, arg: u64)`. v1 table has ONE entry (TaskId 0 = the
// NTT-8 coefficient leaf, arg = k); a single-entry table needs no `entry` match, so
// it lowers today. Multi-entry dispatch would match `entry` (a u32), which needs the
// wasm i32-compare completion (R1 `==`/`<` currently lower i64 operands only) and is
// a separate codegen concern, not this slice.
pub fn fe_task(_ entry: u32, _ arg: u64) -> u64 {
    ntt8_coeff(arg)
}

fn coeff_via_rail(_ k: u64) -> u64
    uses (sp: mut Spawn<WasmBackend>, w: mut Wait<WasmBackend>)
{
    let token = sp.spawn(TaskId::new(0), k)   // fork: mint the task's completion cell
    w.wait(token)                             // join: block + fused deliver of X[k]
}

pub fn coeff(_ k: u64) -> u64 {
    with (Spawn<WasmBackend> = WorkerPool {}, Wait<WasmBackend> = WorkerPool {}) {
        coeff_via_rail(k)
    }
}

fn pair_via_rail(_ k0: u64, _ k1: u64) -> u64
    uses (sp: mut Spawn<WasmBackend>, w: mut Wait<WasmBackend>)
{
    // Two-way disjoint fork (N2's stage split): mint BOTH tokens, then join BOTH.
    // Two affine tokens are live simultaneously, each consumed exactly once (own).
    let t0 = sp.spawn(TaskId::new(0), k0)
    let t1 = sp.spawn(TaskId::new(0), k1)
    let r0 = w.wait(t0)
    let r1 = w.wait(t1)
    addmod_q(r0, r1)
}

pub fn coeff_pair(_ k0: u64, _ k1: u64) -> u64 {
    with (Spawn<WasmBackend> = WorkerPool {}, Wait<WasmBackend> = WorkerPool {}) {
        pair_via_rail(k0, k1)
    }
}

// CE-6 THE R2.1 PAYOFF: a two-token join through an await2-SHAPED own-tuple join.
// `mint` spawns one task and lets its `Pending<WasmBackend, u64>` token ESCAPE
// whole as its one `u32` word (the affine ruling: `spawn`'s token escapes); the
// host holds two such tokens and hands them back as a PAIR. `join2` is the
// await2-shaped join: it takes the `own (Pending, Pending)` tuple (R2.1: the
// two-word tuple PARAM flattens into two wasm params with per-element vars),
// consumes BOTH tokens out of the tuple by the destructuring `let (p1, p2) = pair`
// (R4.1: own-tuple consume is sound), waits both, and returns the `(u64, u64)`
// results as a wasm MULTI-VALUE result the host reads. This is the exact body of
// `core::pending::await2`, specialized to `WorkerPool::wait`: the generic `await2`
// itself cannot be Fe-CALLED on wasm because consuming a tuple FROM a call needs a
// multi-result wasm call (a fork-level gap beyond R2.1's interface scope), but its
// own-tuple param + tuple return lower at the export boundary, which is the payoff.
fn spawn_one(_ k: u64) -> Pending<WasmBackend, u64>
    uses (sp: mut Spawn<WasmBackend>)
{
    sp.spawn(TaskId::new(0), k)
}

pub fn mint(_ k: u64) -> Pending<WasmBackend, u64> {
    with (Spawn<WasmBackend> = WorkerPool {}) {
        spawn_one(k)
    }
}

pub fn join2(_ pair: own (Pending<WasmBackend, u64>, Pending<WasmBackend, u64>)) -> (u64, u64) {
    let mut w = WorkerPool {}
    let (p1, p2) = pair
    (w.wait(p1), w.wait(p2))
}
"#;

fn worker_ntt8_src() -> String {
    [
        "use std::worker::{Spawn, Wait, WorkerPool, TaskId, Pending}\n",
        "use std::wasm::WasmBackend\n",
        WORKER_FIELD_ARITH,
        WORKER_NTT8_SPECIFIC,
    ]
    .concat()
}

/// The LOCAL `Par<WasmBackend>` fork-join fixture: pure nullary thunks joined by the
/// sequential (left-then-right) `WorkerPool` `Par` provider. No host imports (pure).
const WORKER_PAR_SPECIFIC: &str = r#"
// A pure NTT-coefficient leaf as a nullary Fn<(), u64> thunk (the conal_par_fork
// shape). Effect-pure by construction (6-0009): no `uses` row, which is exactly why
// it CANNOT reach the task rail (that is `spawn`'s job).
struct CoeffThunk { k: u64 }
impl Fn<(), u64> for CoeffThunk {
    fn call(self, _ unit: own ()) -> u64 {
        ntt8_coeff(self.k)
    }
}

fn par_pair_local(_ k0: u64, _ k1: u64) -> u64
    uses (par: mut Par<WasmBackend>)
{
    let joined: (u64, u64) = par.fork(CoeffThunk { k: k0 }, CoeffThunk { k: k1 })
    addmod_q(joined.0, joined.1)
}

pub fn par_pair(_ k0: u64, _ k1: u64) -> u64 {
    with (Par<WasmBackend> = WorkerPool {}) {
        par_pair_local(k0, k1)
    }
}
"#;

fn worker_par_src() -> String {
    [
        "use std::worker::WorkerPool\n",
        "use std::wasm::WasmBackend\n",
        "use core::par::Par\n",
        "use core::Fn\n",
        WORKER_FIELD_ARITH,
        WORKER_PAR_SPECIFIC,
    ]
    .concat()
}

/// The one-shot CAS backstop fixture: drops to the raw `fe:worker` imports (any host
/// author could, at their own risk) and calls `task_result` TWICE on one token. Safe
/// Fe cannot double-consume (the affine `own` mode is a use-after-move type error);
/// this UNSAFE probe exercises the host-side dynamic READY->CONSUMED trap. `std` keeps
/// its `raw::task_result` private, so this is not std public surface.
const WORKER_CAS_SPECIFIC: &str = r#"
pub fn fe_task(_ entry: u32, _ arg: u64) -> u64 {
    ntt8_coeff(arg)
}

#[wasm_import(module = "fe:worker")]
extern {
    pub unsafe fn task_begin(_ entry: TaskId, _ arg: u64) -> Pending<WasmBackend, u64>
    pub unsafe fn wait<T>(_ pending: Pending<WasmBackend, T>)
    pub unsafe fn task_result<T>(_ pending: Pending<WasmBackend, T>) -> T
}

pub fn double_consume(_ k: u64) -> u64 {
    let token = task_begin(TaskId::new(0), k)
    wait(token)
    let a = task_result(token)   // consume 1: READY -> CONSUMED, delivers X[k]
    let b = task_result(token)   // consume 2: traps host-side (the CAS backstop)
    addmod_q(a, b)               // unreachable
}
"#;

fn worker_cas_src() -> String {
    [
        "use std::worker::{TaskId, Pending}\n",
        "use std::wasm::WasmBackend\n",
        WORKER_FIELD_ARITH,
        WORKER_CAS_SPECIFIC,
    ]
    .concat()
}

/// The DEGENERATE ONE-WORKER POOL: a token-slot table plus an op-sequence log. It
/// mirrors the `fe:worker` import op-set op-for-op. `task_begin` executes the
/// exported `fe_task` SYNCHRONOUSLY in-instance, stores the `u64`, and marks the slot
/// READY; `wait` is a no-op (already READY in the synchronous pool); `task_result`
/// reads the word and performs the one-shot `READY -> CONSUMED` CAS, trapping on a
/// second delivery of the same token.
#[derive(Default)]
struct FakeTaskPool {
    /// Token table: `slots[token]` holds a task's `u64` result and its CAS state.
    slots: Vec<TaskSlot>,
    /// The op-sequence log, one entry per serviced import call.
    log: Vec<&'static str>,
}

#[derive(Clone, Copy)]
struct TaskSlot {
    value: u64,
    consumed: bool,
}

/// Build a `Linker` that services the `fe:worker` import op-set with a `FakeTaskPool`
/// and instantiate `wasm` against it.
fn instantiate_task_pool(wasm: &[u8]) -> (wasmtime::Store<FakeTaskPool>, wasmtime::Instance) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, FakeTaskPool::default());
    let mut linker = wasmtime::Linker::new(&engine);

    // task_begin(entry, arg) -> token: the DEGENERATE one-worker pool. Execute the
    // exported `fe_task(entry, arg)` SYNCHRONOUSLY in-instance, store the u64, mark
    // the slot READY, and return the token index. Reentrant call into the instance.
    linker
        .func_wrap(
            "fe:worker",
            "task_begin",
            |mut caller: wasmtime::Caller<'_, FakeTaskPool>,
             entry: i32,
             arg: i64|
             -> Result<i32, wasmtime::Error> {
                let fe_task = caller
                    .get_export("fe_task")
                    .and_then(wasmtime::Extern::into_func)
                    .ok_or_else(|| wasmtime::Error::msg("the emitted wasm must export `fe_task`"))?
                    .typed::<(i32, i64), i64>(&caller)?;
                // Reentrant, synchronous execution of the task body in-instance.
                let result = fe_task.call(&mut caller, (entry, arg))?;
                let dev = caller.data_mut();
                dev.slots.push(TaskSlot {
                    value: result as u64,
                    consumed: false,
                });
                dev.log.push("task_begin");
                Ok((dev.slots.len() - 1) as i32)
            },
        )
        .expect("bind task_begin");

    // wait(token): block until the slot is READY. Synchronous pool: already READY, so
    // this is a no-op beyond a bounds check and the op log.
    linker
        .func_wrap(
            "fe:worker",
            "wait",
            |mut caller: wasmtime::Caller<'_, FakeTaskPool>,
             token: i32|
             -> Result<(), wasmtime::Error> {
                let dev = caller.data_mut();
                if token < 0 || token as usize >= dev.slots.len() {
                    return Err(wasmtime::Error::msg("wait: unknown pending token"));
                }
                dev.log.push("wait");
                Ok(())
            },
        )
        .expect("bind wait");

    // task_result(token) -> u64: read the result and perform the one-shot
    // READY -> CONSUMED CAS. A SECOND delivery of the same token TRAPS with the named
    // diagnostic (the affine task token's dynamic backstop).
    linker
        .func_wrap(
            "fe:worker",
            "task_result",
            |mut caller: wasmtime::Caller<'_, FakeTaskPool>,
             token: i32|
             -> Result<i64, wasmtime::Error> {
                let dev = caller.data_mut();
                let slot = dev
                    .slots
                    .get_mut(token as usize)
                    .ok_or_else(|| wasmtime::Error::msg("task_result: unknown pending token"))?;
                if slot.consumed {
                    return Err(wasmtime::Error::msg(
                        "fe:worker task_result trap: token already CONSUMED (one-shot \
                         READY->CONSUMED CAS violated; affine task token double-consumed)",
                    ));
                }
                slot.consumed = true;
                let value = slot.value;
                dev.log.push("task_result");
                Ok(value as i64)
            },
        )
        .expect("bind task_result");

    let instance = linker
        .instantiate(&mut store, &module)
        .expect("wasmtime should instantiate with the fe:worker imports satisfied");
    (store, instance)
}

/// CE-5 THE CONTRACT: the recompiled fork-join NTT-8 through the `spawn`/`wait` task
/// rail EQUALS the sequential oracle (`NTT8_OUTPUT`) under the one-worker pool. Every
/// coefficient is a task-rail leaf; the two-token join is the disjoint fork-join.
#[test]
fn fe_worker_ntt8_fork_join_equals_sequential_oracle() {
    let src = worker_ntt8_src();
    let wasm = compile_to_wasm("wasm_worker_ntt8.fe", &src);

    // The task-rail import op-set is on the emitted wasm, module-named per R3.3.
    let imports = func_imports(&wasm);
    for expected in [
        ("fe:worker", "task_begin"),
        ("fe:worker", "wait"),
        ("fe:worker", "task_result"),
    ] {
        assert!(
            imports.contains(&(expected.0.to_string(), expected.1.to_string())),
            "expected import {expected:?} in the emitted wasm, found {imports:?}"
        );
    }

    let (mut store, instance) = instantiate_task_pool(&wasm);
    // The pool dispatches into the module's exported `fe_task` task table.
    assert!(
        instance.get_func(&mut store, "fe_task").is_some(),
        "the emitted wasm must export the `fe_task` dispatch table"
    );

    // Every NTT-8 coefficient computed through the task rail equals the pinned oracle.
    let coeff = instance
        .get_typed_func::<i64, i64>(&mut store, "coeff")
        .expect("`coeff` export should exist");
    for (k, expected) in NTT8_OUTPUT.iter().enumerate() {
        let got = coeff
            .call(&mut store, k as i64)
            .expect("coeff(k) should run the spawn/wait rail");
        assert_eq!(
            got as u64, *expected as u64,
            "coeff({k}) through the task rail must equal the sequential oracle NTT8_OUTPUT[{k}]"
        );
    }

    // The op-sequence per leaf is exactly the ratified walk: spawn (task_begin), then
    // the fused wait (raw wait, then raw task_result). 8 coefficients = 8 * 3 ops.
    assert_eq!(
        store.data().log.len(),
        NTT8_OUTPUT.len() * 3,
        "each coefficient should walk task_begin + wait + task_result exactly once"
    );
    assert_eq!(
        &store.data().log[0..3],
        &["task_begin", "wait", "task_result"],
        "the fused wait should call raw wait then raw task_result after the spawn"
    );

    // The two-way disjoint fork-join (two affine tokens live at once): X[0] + X[1].
    let coeff_pair = instance
        .get_typed_func::<(i64, i64), i64>(&mut store, "coeff_pair")
        .expect("`coeff_pair` export should exist");
    let joined = coeff_pair
        .call(&mut store, (0, 1))
        .expect("coeff_pair should fork two tokens and join both");
    assert_eq!(
        joined as u64,
        ((NTT8_OUTPUT[0] as u64 + NTT8_OUTPUT[1] as u64) % 12289),
        "the two-token fork-join must equal (X[0] + X[1]) mod q"
    );
}

/// CE-6 THE R2.1 PAYOFF: a wasmtime two-token join driving TWO task completions
/// through an await2-shaped `own (Pending, Pending)` join. `mint(k)` spawns a task
/// and returns its escaping token (a `u32` slot); the host holds two tokens and
/// passes them as a PAIR to `join2`, whose two-word tuple PARAM lowers via R2.1
/// (flattened to two wasm params) and whose `(u64, u64)` result is a wasm
/// MULTI-VALUE return the host reads. The two delivered values EQUAL the pinned
/// sequential oracle `(NTT8_OUTPUT[0], NTT8_OUTPUT[1])`: both tokens' payloads flow
/// through the own-tuple param, distinctly and correctly. This is CE-5's
/// `coeff_pair` two-token join repackaged behind the await2 tuple boundary, proving
/// the `(Pending, Pending)` own-tuple param now lowers.
#[test]
fn fe_worker_await2_shaped_two_token_join_equals_oracle() {
    let src = worker_ntt8_src();
    let wasm = compile_to_wasm("wasm_worker_await2.fe", &src);
    let (mut store, instance) = instantiate_task_pool(&wasm);

    // Mint two task tokens (the host holds their escaping `u32` slots).
    let mint = instance
        .get_typed_func::<i64, i32>(&mut store, "mint")
        .expect("`mint` export should exist");
    let t0 = mint
        .call(&mut store, 0)
        .expect("mint(0) should spawn a task token");
    let t1 = mint
        .call(&mut store, 1)
        .expect("mint(1) should spawn a task token");

    // Join both tokens through the await2-shaped own-tuple join. The `(Pending,
    // Pending)` param arrives as two wasm params (R2.1); the `(u64, u64)` return is
    // a wasm multi-value result.
    let join2 = instance
        .get_typed_func::<(i32, i32), (i64, i64)>(&mut store, "join2")
        .expect("`join2` export should exist");
    let (r0, r1) = join2
        .call(&mut store, (t0, t1))
        .expect("join2 should consume the own-tuple of two tokens and deliver both payloads");

    assert_eq!(
        r0 as u64, NTT8_OUTPUT[0] as u64,
        "the first joined payload must equal the sequential oracle NTT8_OUTPUT[0]"
    );
    assert_eq!(
        r1 as u64, NTT8_OUTPUT[1] as u64,
        "the second joined payload must equal the sequential oracle NTT8_OUTPUT[1]"
    );

    // The op walk: two `mint`s (each task_begin), then `join2` waits both tokens
    // (each: raw wait + raw task_result). 2 begins + 2*(wait+task_result) = 6 ops.
    assert_eq!(
        store.data().log.len(),
        6,
        "two mints + a two-token join should walk task_begin x2 then (wait, task_result) x2"
    );
    assert_eq!(
        &store.data().log,
        &[
            "task_begin",
            "task_begin",
            "wait",
            "task_result",
            "wait",
            "task_result"
        ],
        "the await2-shaped join must wait+deliver each token exactly once, in order"
    );
}

/// CE-5/CE-6: the LOCAL `Par<WasmBackend>` provider is BUILT and TYPE-CHECKS (it
/// composes in `std::worker` and here at the Fe level); its wasm EXECUTION stays
/// fail-closed. R2.1 (CE-6) now lowers scalar-tuple params AND returns at function
/// BOUNDARIES, so `fork`'s own `(L, R)` return signature would lower; the wall the
/// `fork`-join hits is one rung deeper and unchanged by R2.1: the CALLER
/// (`par_pair_local`) must RECEIVE the `(u64, u64)` tuple FROM the `fork` call,
/// which needs a wasm MULTI-RESULT call (the WAFFLE Call path binds a single
/// result) - a fork-level gap beyond R2.1's interface scope. This test PINS that
/// the fork-join type-checks but its wasm lowering stops fail-closed at that
/// aggregate/call boundary, not anywhere unexpected. The order-independence oracle
/// is proven executably by the task rail's two-token `coeff_pair` and the
/// await2-shaped `join2` above; the local provider is the same semantics, deferred.
#[test]
fn fe_worker_par_local_provider_typechecks_wasm_exec_is_r2_1() {
    let src = worker_par_src();
    let err = compile_to_wasm_err("wasm_worker_par.fe", &src);
    assert!(
        err.contains("aggregate") || err.contains("single-scalar-field") || err.contains("R2"),
        "Par<WasmBackend> fork-join should fail-close at the R2.1 aggregate (tuple) wall, \
         got: {err}"
    );
}

/// CE-5: the one-shot `READY -> CONSUMED` CAS backstop. Safe Fe cannot double-consume
/// a token (affine `own`); this UNSAFE probe calls the raw `task_result` twice on one
/// token and the SECOND delivery TRAPS host-side with the named diagnostic.
#[test]
fn fe_worker_double_consume_traps_host_side() {
    let src = worker_cas_src();
    let wasm = compile_to_wasm("wasm_worker_cas.fe", &src);
    let (mut store, instance) = instantiate_task_pool(&wasm);

    let double_consume = instance
        .get_typed_func::<i64, i64>(&mut store, "double_consume")
        .expect("`double_consume` export should exist");
    let err = double_consume
        .call(&mut store, 0)
        .expect_err("a second task_result on one token must trap (the one-shot CAS backstop)");
    // The named diagnostic lives in the host-error cause chain under the wasm trap
    // backtrace; `{err:?}` renders the whole chain.
    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("already CONSUMED"),
        "expected the named READY->CONSUMED CAS trap in the error chain, got: {rendered}"
    );
}

// ===========================================================================
// CE-7: the EXTERNAL-COMPLETION clients (Timer + recv), the completion-cell rail's
// THIRD kind of client (after GPU readback and worker tasks). The proof the rail
// generalizes past GPU and fork children: three client families, ONE rail, zero new
// surface beyond the two begin ops.
// ===========================================================================

/// The fake broker's monotonic clock (ms). `host_now` returns it; `sleep_begin(ms)`
/// completes its cell with `BASE_CLOCK_MS + ms` as the wake timestamp.
const BASE_CLOCK_MS: u64 = 1000;
/// The posted host-event payload the fake broker completes a `recv_begin` cell with.
const POSTED_EVENT: u64 = 0xE7E7;

/// CE-7 THE HOST EXTERNAL-COMPLETION fixture: the `Timer` / `Recv` / `Wait` rail via
/// the `HostTimer` provider. `sleep_wake(ms)` mints a broker-completed timer cell and
/// waits it, delivering the wake timestamp; `recv_event` mints a broker-completed
/// host-event cell and waits it, delivering the posted payload; `clock_now` reads the
/// colorless clock. R2 acyclicity: the COMPLETER is the broker (the host linker fn),
/// never the Fe waiter, which only mints (begin) and blocks (wait).
const HOST_TIMER_SRC: &str = r#"
use std::host::{HostTimer, Timer, Recv, Wait}
use std::wasm::WasmBackend

fn sleep_via_rail(_ ms: u64) -> u64
    uses (t: mut Timer<WasmBackend>, w: mut Wait<WasmBackend>)
{
    let token = t.sleep_begin(ms)   // mint: the BROKER completes it, not us (R2)
    w.wait(token)                   // block + fused deliver of the wake timestamp
}

pub fn sleep_wake(_ ms: u64) -> u64 {
    with (Timer<WasmBackend> = HostTimer {}, Wait<WasmBackend> = HostTimer {}) {
        sleep_via_rail(ms)
    }
}

fn recv_via_rail() -> u64
    uses (r: mut Recv<WasmBackend>, w: mut Wait<WasmBackend>)
{
    let token = r.recv_begin()      // mint: the HOST completes it, not us (R2)
    w.wait(token)                   // block + fused deliver of the posted payload
}

pub fn recv_event() -> u64 {
    with (Recv<WasmBackend> = HostTimer {}, Wait<WasmBackend> = HostTimer {}) {
        recv_via_rail()
    }
}

fn now_via_rail() -> u64
    uses (t: mut Timer<WasmBackend>)
{
    t.now()
}

pub fn clock_now() -> u64 {
    with (Timer<WasmBackend> = HostTimer {}) {
        now_via_rail()
    }
}
"#;

/// The DEGENERATE HOST BROKER: a cell-slot table plus an op-sequence log. It mirrors
/// the `fe:host` import op-set op-for-op and completes each cell SYNCHRONOUSLY
/// in-instance (the sandbox has no real timers or message loop). `sleep_begin(ms)`
/// completes the cell with the wake timestamp `BASE_CLOCK_MS + ms`; `recv_begin`
/// completes with the posted event payload; `wait` reads the slot (already READY in
/// the synchronous broker) and delivers it; `host_now` reads the fake clock. THE
/// COMPLETER IS THIS HOST, external to the waiting agent, which is what preserves R2
/// acyclicity (mirroring CE-5's `FakeTaskPool`). The real async form (mint now,
/// complete after a genuine `setTimeout` over the SharedArrayBuffer rail) is the bun
/// page-lane's job, deferred with the browser batch.
#[derive(Default)]
struct FakeBroker {
    /// Cell table: `slots[token]` holds the `u64` value the BROKER wrote into the cell.
    slots: Vec<u64>,
    /// The op-sequence log, one entry per serviced import call.
    log: Vec<&'static str>,
}

/// Build a `Linker` that services the `fe:host` import op-set with a `FakeBroker` and
/// instantiate `wasm` against it.
fn instantiate_host_broker(wasm: &[u8]) -> (wasmtime::Store<FakeBroker>, wasmtime::Instance) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, FakeBroker::default());
    let mut linker = wasmtime::Linker::new(&engine);

    // host_now() -> ms: read the fake monotonic clock. Colorless: no cell, no wait.
    linker
        .func_wrap(
            "fe:host",
            "host_now",
            |mut caller: wasmtime::Caller<'_, FakeBroker>| -> Result<i64, wasmtime::Error> {
                caller.data_mut().log.push("host_now");
                Ok(BASE_CLOCK_MS as i64)
            },
        )
        .expect("bind host_now");

    // sleep_begin(ms) -> token: the DEGENERATE broker. Complete the cell SYNCHRONOUSLY
    // with the wake timestamp BASE_CLOCK_MS + ms (THE BROKER writes the value, NOT the
    // Fe waiter: R2 acyclicity), and return the token index.
    linker
        .func_wrap(
            "fe:host",
            "sleep_begin",
            |mut caller: wasmtime::Caller<'_, FakeBroker>,
             ms: i64|
             -> Result<i32, wasmtime::Error> {
                let dev = caller.data_mut();
                dev.slots.push(BASE_CLOCK_MS + ms as u64);
                dev.log.push("sleep_begin");
                Ok((dev.slots.len() - 1) as i32)
            },
        )
        .expect("bind sleep_begin");

    // recv_begin() -> token: complete the cell SYNCHRONOUSLY with the posted host-event
    // payload (again the BROKER/HOST writes the value, not the Fe waiter: R2).
    linker
        .func_wrap(
            "fe:host",
            "recv_begin",
            |mut caller: wasmtime::Caller<'_, FakeBroker>| -> Result<i32, wasmtime::Error> {
                let dev = caller.data_mut();
                dev.slots.push(POSTED_EVENT);
                dev.log.push("recv_begin");
                Ok((dev.slots.len() - 1) as i32)
            },
        )
        .expect("bind recv_begin");

    // wait(token) -> u64: block until the cell is READY (synchronous broker: already
    // READY) and deliver the u64 the broker wrote. This is the shared blocking-deliver
    // op of the rail (the `fe:async::wait<T> -> T` shape) at payload T = u64.
    linker
        .func_wrap(
            "fe:host",
            "wait",
            |mut caller: wasmtime::Caller<'_, FakeBroker>,
             token: i32|
             -> Result<i64, wasmtime::Error> {
                let dev = caller.data_mut();
                let value = *dev
                    .slots
                    .get(token as usize)
                    .ok_or_else(|| wasmtime::Error::msg("wait: unknown pending token"))?;
                dev.log.push("wait");
                Ok(value as i64)
            },
        )
        .expect("bind wait");

    let instance = linker
        .instantiate(&mut store, &module)
        .expect("wasmtime should instantiate with the fe:host imports satisfied");
    (store, instance)
}

/// CE-7 THE CONTRACT: the external-completion rail delivers through `begin -> wait`.
/// `sleep_begin -> wait` delivers the BROKER-written wake timestamp, `recv_begin ->
/// wait` delivers the posted host event, and `now` reads the clock. This proves the
/// SAME mint / wait / complete substrate serves a THIRD client family (neither GPU nor
/// a fork child), with the completer external to the waiter (R2 acyclicity). The real
/// async (genuine `setTimeout` parking on `Atomics.wait`) is the bun page-lane follow-on.
#[test]
fn fe_host_timer_recv_external_completion_rail() {
    let wasm = compile_to_wasm("wasm_host_timer.fe", HOST_TIMER_SRC);

    // The host-completion import op-set is on the emitted wasm, module-named per R3.3.
    let imports = func_imports(&wasm);
    for expected in [
        ("fe:host", "sleep_begin"),
        ("fe:host", "recv_begin"),
        ("fe:host", "wait"),
        ("fe:host", "host_now"),
    ] {
        assert!(
            imports.contains(&(expected.0.to_string(), expected.1.to_string())),
            "expected import {expected:?} in the emitted wasm, found {imports:?}"
        );
    }

    let (mut store, instance) = instantiate_host_broker(&wasm);

    // sleep_begin -> wait delivers the BROKER-written wake timestamp (R2: the broker
    // completes the cell, the Fe waiter never does). sleep_begin(16) with clock 1000
    // wakes at 1016.
    let sleep_wake = instance
        .get_typed_func::<i64, i64>(&mut store, "sleep_wake")
        .expect("`sleep_wake` export should exist");
    let woke = sleep_wake
        .call(&mut store, 16)
        .expect("sleep_wake should mint a timer cell and wait it");
    assert_eq!(
        woke as u64,
        BASE_CLOCK_MS + 16,
        "sleep_begin -> wait must deliver the broker's wake timestamp (R2: broker completes, not the waiter)"
    );

    // The rail walk for a sleep: mint (sleep_begin) then the fused blocking-deliver (wait).
    assert_eq!(
        &store.data().log,
        &["sleep_begin", "wait"],
        "the sleep rail walk should be begin then the fused wait"
    );

    // recv_begin -> wait delivers the posted host-event payload (the recv lane).
    let recv_event = instance
        .get_typed_func::<(), i64>(&mut store, "recv_event")
        .expect("`recv_event` export should exist");
    let got = recv_event
        .call(&mut store, ())
        .expect("recv_event should mint a host-event cell and wait it");
    assert_eq!(
        got as u64, POSTED_EVENT,
        "recv_begin -> wait must deliver the posted host event"
    );

    // now() reads the colorless clock: no cell minted, no wait.
    let clock_now = instance
        .get_typed_func::<(), i64>(&mut store, "clock_now")
        .expect("`clock_now` export should exist");
    let t = clock_now
        .call(&mut store, ())
        .expect("clock_now should read the host clock");
    assert_eq!(t as u64, BASE_CLOCK_MS, "now() reads the host clock");

    // Full op walk across all three calls: the two external-completion cells each walk
    // begin then wait, and `now` is a single colorless clock read (no cell, no wait).
    assert_eq!(
        &store.data().log,
        &["sleep_begin", "wait", "recv_begin", "wait", "host_now"],
        "the full walk should be sleep(begin,wait), recv(begin,wait), now"
    );
}

// ===========================================================================
// CE-9: the Fe-idiomatic ZIO flagship (R7b), MEASURED on the wasm value model.
//
// R7b's flagship is a direct-style `quote_pipeline(pair) -> Result<StaleQuote, u64>
// uses (sp, w, t)` with a three-provider `with`-frame "runtime" (ZLayer without the
// DSL) and a `retry_task` combinator over reified tasks. It TYPE-CHECKS in full (the
// R6 anti-coloring / typed-environment thesis holds: no `.await`, calls are plain
// calls, `let` is bind, capabilities are one typed `uses` row, the runtime is a
// lexical `with`-frame, the typed error is `Result` + `#[error]`). But EXECUTING it
// on the wasm value model hits two walls the CE-9 rung and the section-7.4 de-risk
// table did not foresee (7.4 checked STRUCTS field-by-field - `Stepped` covered,
// `Combine2` flagged - but never the ENUM spine on which R7b is built):
//
//   1. THE ENUM WALL (fundamental, a seat decision). `Result<E, A>` is an enum;
//      `wasm_lower` fails closed on `Layout::Enum` (`single_scalar_field` returns
//      `None`, so `ty_for_class` errors) and `match`/switch terminators are R2. So a
//      `Result`-returning function's SIGNATURE alone cannot lower - the typed-error
//      arm of the flagship never reaches codegen. Enum lowering on wasm (a
//      tagged-union value repr + `match`/switch + construct/extract) is a NEW
//      R2-class codegen rung, well beyond CE-9's "worked example + small combinator"
//      mandate and beyond the R2.0/R2.1 slivers. Ruling needed before it is built.
//   2. THE CAPABILITY-BODY CONTROL-FLOW WALL (secondary). Even a SCALAR (no-Result)
//      pipeline fails closed the moment an `if` enters a `uses` body: the provision
//      environment is materialized as an aggregate local across the branch
//      ("not a value-carried scalar"; address-taken/aggregate locals are R2). PURE
//      bodies with the same control flow lower fine (the shipped `ntt8_coeff`
//      while/if), and STRAIGHT-LINE capability bodies lower fine (below), so this is
//      specific to control flow over a capability environment.
//
// WHAT DOES EXECUTE (pinned by `fe_ce9_three_capability_rail_executes` below): the
// direct-style three-capability rail in its STRAIGHT-LINE, scalar form - three
// distinct capabilities (`Spawn` + `Wait` + `Timer`), TWO import modules (`fe:worker`
// + `fe:host`) in ONE wasm module, a THREE-provider HETEROGENEOUS `with`-frame
// (`WorkerPool` + `HostTimer`) - forks/joins a task through the rail AND reads the
// clock, end to end through a combined worker+clock fake runtime. That isolates both
// walls to the enum and to capability-body control flow, NOT to the three-capability
// `uses` row or the multi-provider `with`-frame themselves (those lower and run).
// ===========================================================================

/// The REAL R7b flagship source: `Result<StaleQuote, u64>` threaded through a
/// direct-style body with the three-capability `uses (sp, w, t)` row, the
/// three-provider `with`-frame runtime, and the `retry_task` combinator. Kept
/// verbatim as the documented target; it TYPE-CHECKS (verified with `fe check`) and
/// is pinned to FAIL CLOSED at the enum wall on wasm by the test below.
const CE9_FLAGSHIP_SRC: &str = r#"
use core::result::Result
use core::pending::{Spawn, Wait, Timer, TaskId}
use std::worker::WorkerPool
use std::host::HostTimer
use std::wasm::WasmBackend

#[error]
struct StaleQuote { pub age_ms: u64 }

#[error]
struct Exhausted { pub tries: u32 }

fn precompute_weights(_ pair: u64) -> u64 { pair + 7 }
fn combine(_ local: u64, _ raw: u64) -> u64 { local + raw }

// ZIO[R, E, A] as a plain Fe function: R is the row, E the Err arm, A the Ok arm.
// No wrapper type, no flatMap chains, no for-comprehension. Direct style throughout:
// the fork is a plain call, the join is a plain call (no `.await`), `let` is bind.
fn quote_pipeline(_ pair: u64) -> Result<StaleQuote, u64>
    uses (sp: mut Spawn<WasmBackend>, w: mut Wait<WasmBackend>, t: mut Timer<WasmBackend>)
{
    let fetch = sp.spawn(TaskId::new(0), pair)   // fork the fiber
    let started = t.now()                        // read the clock (colorless)
    let local = precompute_weights(pair)         // pure: no row, checked (6-0009)
    let raw = w.wait(fetch)                       // join: the one colored op
    let age = t.now() - started
    if age > 5000 {
        return Result::Err(StaleQuote { age_ms: age })
    }
    Result::Ok(combine(local, raw))
}

// The "runtime" is a with-frame at the composition root: ZLayer without the DSL.
pub fn main(_ pair: u64) -> u64 {
    with (Spawn<WasmBackend> = WorkerPool {}, Wait<WasmBackend> = WorkerPool {},
          Timer<WasmBackend> = HostTimer {}) {
        quote_pipeline(pair).unwrap_or(default: 0)
    }
}

// The retry combinator over REIFIED tasks (the honest partial recovery for "no
// generic retry over arbitrary effects"): spawn+wait in a loop up to `tries`,
// returning Ok on first success (a nonzero result) else Err(Exhausted).
fn retry_task(_ task: TaskId, _ arg: u64, _ tries: u32) -> Result<Exhausted, u64>
    uses (sp: mut Spawn<WasmBackend>, w: mut Wait<WasmBackend>)
{
    let mut i: u32 = 0
    while i < tries {
        let token = sp.spawn(task, arg)
        let r = w.wait(token)
        if 0 < r {
            return Result::Ok(r)
        }
        i = i + 1
    }
    Result::Err(Exhausted { tries: tries })
}

pub fn retry_demo(_ arg: u64, _ tries: u32) -> u64 {
    with (Spawn<WasmBackend> = WorkerPool {}, Wait<WasmBackend> = WorkerPool {}) {
        retry_task(TaskId::new(0), arg, tries).unwrap_or(default: 0)
    }
}
"#;

/// A three-capability body with an `if` over a capability-derived value: pins the
/// secondary capability-body control-flow wall (aggregate provision-env local).
const CE9_CAP_CONTROL_FLOW_SRC: &str = r#"
use core::pending::{Spawn, Wait, Timer, TaskId}
use std::worker::WorkerPool
use std::host::HostTimer
use std::wasm::WasmBackend

pub fn fe_task(_ entry: u32, _ arg: u64) -> u64 { arg + 100 }

fn branchy(_ pair: u64) -> u64
    uses (sp: mut Spawn<WasmBackend>, w: mut Wait<WasmBackend>, t: mut Timer<WasmBackend>)
{
    let fetch = sp.spawn(TaskId::new(0), pair)
    let n = t.now()
    let raw = w.wait(fetch)
    if 5 < n { raw } else { raw + 1 }
}

pub fn run(_ pair: u64) -> u64 {
    with (Spawn<WasmBackend> = WorkerPool {}, Wait<WasmBackend> = WorkerPool {},
          Timer<WasmBackend> = HostTimer {}) {
        branchy(pair)
    }
}
"#;

/// The STRAIGHT-LINE three-capability rail that DOES lower and execute: three
/// distinct capabilities in one `uses` row, two import modules in one wasm module, a
/// three-provider heterogeneous `with`-frame. `run(pair)` forks `fe_task(0, pair)`
/// (`= pair + 100`), reads the clock, joins, and returns `clock + raw`.
const CE9_THREE_CAP_RAIL_SRC: &str = r#"
use core::pending::{Spawn, Wait, Timer, TaskId}
use std::worker::WorkerPool
use std::host::HostTimer
use std::wasm::WasmBackend

pub fn fe_task(_ entry: u32, _ arg: u64) -> u64 { arg + 100 }

fn pipeline(_ pair: u64) -> u64
    uses (sp: mut Spawn<WasmBackend>, w: mut Wait<WasmBackend>, t: mut Timer<WasmBackend>)
{
    let fetch = sp.spawn(TaskId::new(0), pair)   // fork the fiber (fe:worker)
    let started = t.now()                        // read the clock (fe:host, colorless)
    let raw = w.wait(fetch)                       // join: the one colored op (fe:worker)
    started + raw                                 // combine (straight line)
}

pub fn run(_ pair: u64) -> u64 {
    with (Spawn<WasmBackend> = WorkerPool {}, Wait<WasmBackend> = WorkerPool {},
          Timer<WasmBackend> = HostTimer {}) {
        pipeline(pair)
    }
}
"#;

/// A combined fake runtime: the `fe:worker` task pool (reusing `FakeTaskPool`'s
/// slot/CAS/log discipline) PLUS the `fe:host` clock (`host_now` -> `BASE_CLOCK_MS`).
/// This is the CE-5 `FakeTaskPool` and the CE-7 clock in ONE linker, servicing the
/// two import modules a three-capability program pulls into one wasm module.
fn instantiate_worker_and_clock(
    wasm: &[u8],
) -> (wasmtime::Store<FakeTaskPool>, wasmtime::Instance) {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, FakeTaskPool::default());
    let mut linker = wasmtime::Linker::new(&engine);

    // fe:worker::task_begin - execute the exported `fe_task` synchronously in-instance.
    linker
        .func_wrap(
            "fe:worker",
            "task_begin",
            |mut caller: wasmtime::Caller<'_, FakeTaskPool>,
             entry: i32,
             arg: i64|
             -> Result<i32, wasmtime::Error> {
                let fe_task = caller
                    .get_export("fe_task")
                    .and_then(wasmtime::Extern::into_func)
                    .ok_or_else(|| wasmtime::Error::msg("the emitted wasm must export `fe_task`"))?
                    .typed::<(i32, i64), i64>(&caller)?;
                let result = fe_task.call(&mut caller, (entry, arg))?;
                let dev = caller.data_mut();
                dev.slots.push(TaskSlot {
                    value: result as u64,
                    consumed: false,
                });
                dev.log.push("task_begin");
                Ok((dev.slots.len() - 1) as i32)
            },
        )
        .expect("bind task_begin");

    // fe:worker::wait - synchronous pool: already READY, a no-op beyond the log.
    linker
        .func_wrap(
            "fe:worker",
            "wait",
            |mut caller: wasmtime::Caller<'_, FakeTaskPool>,
             token: i32|
             -> Result<(), wasmtime::Error> {
                let dev = caller.data_mut();
                if token < 0 || token as usize >= dev.slots.len() {
                    return Err(wasmtime::Error::msg("wait: unknown pending token"));
                }
                dev.log.push("wait");
                Ok(())
            },
        )
        .expect("bind wait");

    // fe:worker::task_result - deliver the u64 with the one-shot READY->CONSUMED CAS.
    linker
        .func_wrap(
            "fe:worker",
            "task_result",
            |mut caller: wasmtime::Caller<'_, FakeTaskPool>,
             token: i32|
             -> Result<i64, wasmtime::Error> {
                let dev = caller.data_mut();
                let slot = dev
                    .slots
                    .get_mut(token as usize)
                    .ok_or_else(|| wasmtime::Error::msg("task_result: unknown pending token"))?;
                if slot.consumed {
                    return Err(wasmtime::Error::msg(
                        "task_result trap: token already CONSUMED",
                    ));
                }
                slot.consumed = true;
                let value = slot.value;
                dev.log.push("task_result");
                Ok(value as i64)
            },
        )
        .expect("bind task_result");

    // fe:host::host_now - the colorless clock read (the CE-7 broker's clock).
    linker
        .func_wrap(
            "fe:host",
            "host_now",
            |mut caller: wasmtime::Caller<'_, FakeTaskPool>| -> Result<i64, wasmtime::Error> {
                caller.data_mut().log.push("host_now");
                Ok(BASE_CLOCK_MS as i64)
            },
        )
        .expect("bind host_now");

    let instance = linker
        .instantiate(&mut store, &module)
        .expect("wasmtime should instantiate with the fe:worker + fe:host imports satisfied");
    (store, instance)
}

/// CE-9 THE STOP (pinned): the `Result`-typed R7b flagship FAILS CLOSED on wasm at
/// the enum wall. `Result<E, A>` is an enum; a `Result`-returning signature cannot
/// lower on the wasm value model (`single_scalar_field` rejects `Layout::Enum`). The
/// flagship type-checks in full (the anti-coloring / typed-environment thesis holds);
/// only its wasm EXECUTION is walled, on the typed-error arm.
#[test]
fn fe_ce9_result_flagship_fails_closed_at_enum_wall() {
    let err = compile_to_wasm_err("wasm_ce9_flagship.fe", CE9_FLAGSHIP_SRC);
    assert!(
        err.contains("single-scalar-field") || err.contains("one-field scalar newtype"),
        "the Result-typed flagship should fail closed at the wasm enum/aggregate wall \
         (enums are R2 on the wasm value model), got: {err}"
    );
}

/// CE-9 (secondary wall, pinned): control flow inside a capability (`uses`) body
/// fails closed even in scalar form - the provision environment becomes an aggregate
/// local across the branch. Straight-line capability bodies and pure branchy bodies
/// both lower fine; this is specific to control flow over a capability environment.
#[test]
fn fe_ce9_capability_body_control_flow_fails_closed() {
    let err = compile_to_wasm_err("wasm_ce9_cap_cf.fe", CE9_CAP_CONTROL_FLOW_SRC);
    assert!(
        err.contains("value-carried scalar") || err.contains("aggregate"),
        "an `if` inside a capability body should fail closed at the aggregate \
         provision-env local (R2), got: {err}"
    );
}

/// CE-9 WHAT EXECUTES: the direct-style THREE-CAPABILITY rail runs end to end. Three
/// distinct capabilities (`Spawn` + `Wait` + `Timer`) in one `uses` row, TWO import
/// modules (`fe:worker` + `fe:host`) in ONE wasm module, a THREE-provider
/// heterogeneous `with`-frame - fork/join a task through the rail AND read the clock,
/// no monad, no coloring, no `.await`. This proves the capability plumbing of R7b's
/// typed environment executes; only the enum arm and capability-body control flow are
/// the walls.
#[test]
fn fe_ce9_three_capability_rail_executes() {
    let wasm = compile_to_wasm("wasm_ce9_three_cap.fe", CE9_THREE_CAP_RAIL_SRC);

    // Two import modules in one wasm module: the fork/join rail AND the clock.
    let imports = func_imports(&wasm);
    for expected in [
        ("fe:worker", "task_begin"),
        ("fe:worker", "wait"),
        ("fe:worker", "task_result"),
        ("fe:host", "host_now"),
    ] {
        assert!(
            imports.contains(&(expected.0.to_string(), expected.1.to_string())),
            "expected import {expected:?} in the emitted wasm, found {imports:?}"
        );
    }

    let (mut store, instance) = instantiate_worker_and_clock(&wasm);
    assert!(
        instance.get_func(&mut store, "fe_task").is_some(),
        "the emitted wasm must export the `fe_task` dispatch table"
    );

    let run = instance
        .get_typed_func::<i64, i64>(&mut store, "run")
        .expect("`run` export should exist");

    // run(pair) = clock(BASE_CLOCK_MS) + fe_task(0, pair)(= pair + 100).
    for pair in [5i64, 0, 42] {
        let got = run
            .call(&mut store, pair)
            .expect("the three-capability rail should fork/join a task and read the clock");
        assert_eq!(
            got as u64,
            BASE_CLOCK_MS + (pair as u64 + 100),
            "run({pair}) should be clock + (pair + 100) through the three-capability rail"
        );
    }

    // The op walk for one `run`, in source order: spawn (task_begin), the colorless
    // clock read (host_now), then the fused join (wait, task_result). Three `run` calls.
    assert_eq!(
        &store.data().log[0..4],
        &["task_begin", "host_now", "wait", "task_result"],
        "one run should walk task_begin + host_now + (wait, task_result)"
    );
}
