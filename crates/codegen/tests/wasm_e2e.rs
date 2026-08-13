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
use fe_codegen::{
    BackendKind, OptLevel, WasmCompileOptions, compile_runtime_package_wasm_with_options,
    layout_for,
};
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

/// Compile one authored transition through the generic resident-actor wrapper.
/// This bypasses every surface/gallery projection so the acceptance below is
/// an independent consumer of the target-neutral lowering primitive.
fn compile_resident_actor_transition(
    name: &str,
    source: &str,
    authored_transition: &str,
    event_fields: usize,
    actor_fields: usize,
) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "resident actor fixture diagnostics:\n{diagnostics}"
    );
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, authored_transition)
        .expect("resident actor runtime package");
    let bytes = compile_runtime_package_wasm_with_options(
        &db,
        &package,
        WasmCompileOptions::default().with_resident_actor_transition(
            authored_transition,
            "fe_actor_transition_v1",
            "fe_actor_state_replace_v1",
            event_fields,
            vec![false; actor_fields],
        ),
    )
    .expect("resident actor Wasm")
    .bytes;
    wasmparser::validate(&bytes).expect("produced invalid resident actor Wasm");
    bytes
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

fn wasm_float_shape(bytes: &[u8]) -> (usize, usize, usize, usize, usize) {
    let mut adds = 0;
    let mut subs = 0;
    let mut muls = 0;
    let mut calls = 0;
    let mut loops = 0;
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let wasmparser::Payload::CodeSectionEntry(body) = payload.expect("valid wasm") {
            let mut reader = body.get_operators_reader().expect("operators");
            while !reader.eof() {
                match reader.read().expect("operator") {
                    wasmparser::Operator::F32Add => adds += 1,
                    wasmparser::Operator::F32Sub => subs += 1,
                    wasmparser::Operator::F32Mul => muls += 1,
                    wasmparser::Operator::Call { .. } => calls += 1,
                    wasmparser::Operator::Loop { .. } => loops += 1,
                    _ => {}
                }
            }
        }
    }
    (adds, subs, muls, calls, loops)
}

/// A non-surface actor proves that resident state is a general execution
/// mechanism, not a Mandelbrot/gallery special case. The authored Fe behavior
/// is executed once for each event; only the initial seed crosses from the
/// host. Every returned state is compared to an independently written Rust
/// transition over the same deterministic tape.
#[test]
fn scalar_actor_state_remains_resident_and_matches_independent_event_oracle() {
    let source = r#"
pub struct ComponentEvent {
    pub kind: u32,
    pub amount: i32,
    pub weight: f32,
}

pub struct CounterState {
    pub total: i32,
    pub accepted: u32,
    pub score: f32,
}

actor CounterActor {
    total: i32,
    accepted: u32,
    score: f32,

    fn react(self, event: own ComponentEvent) -> CounterState {
        let mut total: i32 = self.total
        let mut accepted: u32 = self.accepted
        if event.kind == 0 {
            total = total + event.amount
            accepted = accepted + 1
        } else {
            if event.kind == 1 {
                total = total - event.amount
                accepted = accepted + 2
            }
        }
        CounterState {
            total: total,
            accepted: accepted,
            score: self.score + event.weight,
        }
    }
}
"#;
    let wasm =
        compile_resident_actor_transition("resident_counter_actor.fe", source, "react", 3, 3);
    let (mut store, instance) = instantiate(&wasm);
    assert!(
        instance.get_func(&mut store, "react").is_none(),
        "the authored behavior name is private behind the fixed resident ABI"
    );
    let transition = instance
        .get_typed_func::<(i32, i32, f32), (i32, i32, f32)>(&mut store, "fe_actor_transition_v1")
        .expect("generic resident transition export");
    let replace = instance
        .get_typed_func::<(i32, i32, f32), ()>(&mut store, "fe_actor_state_replace_v1")
        .expect("generic resident state seed export");

    assert!(
        transition.call(&mut store, (0, 1, 0.5)).is_err(),
        "a resident actor must fail closed before complete state is seeded"
    );

    let mut expected = (17i32, 4u32, -2.0f32);
    replace
        .call(&mut store, (expected.0, expected.1 as i32, expected.2))
        .expect("seed complete actor state exactly once");

    let mut rng = 0x4d59_5df4u32;
    for step in 0..1024u32 {
        rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let kind = match step % 11 {
            0 => 0,
            1 => 1,
            _ => (rng >> 30) + 2,
        };
        let amount = ((rng >> 8) % 19) as i32;
        let weight = match rng & 3 {
            0 => -0.5,
            1 => 0.25,
            2 => 0.75,
            _ => 1.0,
        };

        // Independent oracle: this is intentionally not generated from, or
        // structurally compared with, the Fe source above.
        if kind == 0 {
            expected.0 += amount;
            expected.1 += 1;
        } else if kind == 1 {
            expected.0 -= amount;
            expected.1 += 2;
        }
        expected.2 += weight;

        let got = transition
            .call(&mut store, (kind as i32, amount, weight))
            .unwrap_or_else(|error| panic!("resident event {step} trapped: {error}"));
        assert_eq!(got.0, expected.0, "total differs at event {step}");
        assert_eq!(got.1 as u32, expected.1, "accepted differs at event {step}");
        assert_eq!(
            got.2.to_bits(),
            expected.2.to_bits(),
            "score differs at event {step}"
        );
    }
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
fn generic_integer_downcasts_execute_with_source_and_target_widths() {
    let source = r#"
use core::num::IntDowncast

pub fn truncate_u32(value: u32) -> u8 { value.downcast_truncate() }
pub fn truncate_i32(value: i32) -> i8 { value.downcast_truncate() }
pub fn widen_u8(value: u8) -> u32 { value.downcast_truncate() }
pub fn widen_i8(value: i8) -> i32 { value.downcast_truncate() }
pub fn unchecked_u32(value: u32) -> u8 { value.downcast_unchecked() }
"#;
    let wasm = compile_to_wasm("wasm_integer_downcasts.fe", source);
    assert!(
        func_imports(&wasm)
            .iter()
            .all(|(_, name)| name != "__int_truncate"),
        "integer truncation must lower to Wasm operators, not a host import"
    );
    let (mut store, instance) = instantiate(&wasm);
    let truncate_u32 = instance
        .get_typed_func::<i32, i32>(&mut store, "truncate_u32")
        .unwrap();
    let truncate_i32 = instance
        .get_typed_func::<i32, i32>(&mut store, "truncate_i32")
        .unwrap();
    let widen_u8 = instance
        .get_typed_func::<i32, i32>(&mut store, "widen_u8")
        .unwrap();
    let widen_i8 = instance
        .get_typed_func::<i32, i32>(&mut store, "widen_i8")
        .unwrap();
    let unchecked_u32 = instance
        .get_typed_func::<i32, i32>(&mut store, "unchecked_u32")
        .unwrap();

    assert_eq!(truncate_u32.call(&mut store, 257).unwrap(), 1);
    assert_eq!(truncate_i32.call(&mut store, -129).unwrap(), 127);
    assert_eq!(widen_u8.call(&mut store, 255).unwrap(), 255);
    assert_eq!(widen_i8.call(&mut store, 255).unwrap(), -1);
    assert_eq!(unchecked_u32.call(&mut store, 513).unwrap(), 1);
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

/// `abs`/`min`/`max`/`clamp` f32 intrinsics (`Fabs`/`Fmin`/`Fmax`/`Fclamp`),
/// executed end-to-end Fe source -> wasm -> wasmtime. Oracle values are
/// checked bit-exact against a plain Rust reference (normal values only; the
/// NaN/-0.0 cross-backend PINNED-semantics differential lives in Sonatina's
/// own test suite at `cranelift_backend.rs`, since that needs both the wasm
/// AND cranelift backends side by side on the same IR).
#[test]
fn f32_abs_min_max_clamp_intrinsics_execute_on_wasm() {
    let source = r#"
extern {
    fn __abs_f32(_: f32) -> f32
    fn __min_f32(_: f32, _: f32) -> f32
    fn __max_f32(_: f32, _: f32) -> f32
    fn __clamp_f32(_: f32, _: f32, _: f32) -> f32
}
pub fn abs(x: f32) -> f32 { __abs_f32(x) }
pub fn min(a: f32, b: f32) -> f32 { __min_f32(a, b) }
pub fn max(a: f32, b: f32) -> f32 { __max_f32(a, b) }
pub fn clamp(x: f32, lo: f32, hi: f32) -> f32 { __clamp_f32(x, lo, hi) }
"#;
    let wasm = compile_to_wasm("wasm_f32_abs_min_max_clamp.fe", source);
    assert!(
        func_imports(&wasm)
            .iter()
            .all(|(_, name)| !name.ends_with("_f32")),
        "f32 abs/min/max/clamp intrinsics must lower to Sonatina ops, not wasm host imports"
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
    for opcode in ["F32Abs", "F32Min", "F32Max"] {
        assert!(
            operators.contains(opcode),
            "generated wasm lacks {opcode} (branchy fallback?):\n{operators}"
        );
    }
    // `clamp` has no native wasm instruction; it must compose from exactly the
    // two native ops above (`min(max(x, lo), hi)`), never a branch.
    assert!(
        !operators.contains("If") && !operators.contains("Select"),
        "clamp must be branch-free (min/max composition), found a branch:\n{operators}"
    );

    let (mut store, instance) = instantiate(&wasm);
    let abs = instance
        .get_typed_func::<f32, f32>(&mut store, "abs")
        .expect("abs export");
    let min = instance
        .get_typed_func::<(f32, f32), f32>(&mut store, "min")
        .expect("min export");
    let max = instance
        .get_typed_func::<(f32, f32), f32>(&mut store, "max")
        .expect("max export");
    let clamp = instance
        .get_typed_func::<(f32, f32, f32), f32>(&mut store, "clamp")
        .expect("clamp export");

    for x in [-3.5f32, 3.5, 0.0, -0.0, -1.0, 1.0] {
        assert_eq!(abs.call(&mut store, x).unwrap(), x.abs(), "abs({x})");
    }
    for (a, b) in [(1.0f32, 2.0), (2.0, 1.0), (-5.0, 5.0), (3.0, 3.0)] {
        assert_eq!(
            min.call(&mut store, (a, b)).unwrap(),
            a.min(b),
            "min({a}, {b})"
        );
        assert_eq!(
            max.call(&mut store, (a, b)).unwrap(),
            a.max(b),
            "max({a}, {b})"
        );
    }
    for (x, lo, hi, expected) in [
        (5.0f32, 0.0, 1.0, 1.0),
        (-5.0, 0.0, 1.0, 0.0),
        (0.5, 0.0, 1.0, 0.5),
        (0.0, 0.0, 1.0, 0.0),
        (1.0, 0.0, 1.0, 1.0),
    ] {
        assert_eq!(
            clamp.call(&mut store, (x, lo, hi)).unwrap(),
            expected,
            "clamp({x}, {lo}, {hi})"
        );
    }
}

/// `floor`/`ceil`/`trunc`/`round` f32 intrinsics (`Ffloor`/`Fceil`/`Ftrunc`/
/// `Fround`), executed end-to-end Fe source -> wasm -> wasmtime. Oracle
/// values are checked bit-exact against a plain Rust reference
/// (`f32::round_ties_even`, NOT `f32::round`, for `round` -- ties-to-even is
/// the pinned semantics). The full NaN/-0.0/+-inf/ties differential across
/// wasm AND cranelift lives in Sonatina's own test suite
/// (`cranelift_backend.rs`'s `cross_backend_f32_rounding_differential`).
#[test]
fn f32_rounding_intrinsics_execute_on_wasm() {
    let source = r#"
extern {
    fn __floor_f32(_: f32) -> f32
    fn __ceil_f32(_: f32) -> f32
    fn __trunc_f32(_: f32) -> f32
    fn __round_f32(_: f32) -> f32
}
pub fn floor(x: f32) -> f32 { __floor_f32(x) }
pub fn ceil(x: f32) -> f32 { __ceil_f32(x) }
pub fn trunc(x: f32) -> f32 { __trunc_f32(x) }
pub fn round(x: f32) -> f32 { __round_f32(x) }
"#;
    let wasm = compile_to_wasm("wasm_f32_rounding.fe", source);
    assert!(
        func_imports(&wasm)
            .iter()
            .all(|(_, name)| !name.ends_with("_f32")),
        "f32 rounding intrinsics must lower to Sonatina ops, not wasm host imports"
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
    for opcode in ["F32Floor", "F32Ceil", "F32Trunc", "F32Nearest"] {
        assert!(
            operators.contains(opcode),
            "generated wasm lacks {opcode} (branchy fallback?):\n{operators}"
        );
    }
    // Every one of these ops is a single native instruction; none should
    // need a branch.
    assert!(
        !operators.contains("If") && !operators.contains("Select"),
        "rounding family must be branch-free (single native instruction each), found a branch:\n{operators}"
    );

    let (mut store, instance) = instantiate(&wasm);
    let floor = instance
        .get_typed_func::<f32, f32>(&mut store, "floor")
        .expect("floor export");
    let ceil = instance
        .get_typed_func::<f32, f32>(&mut store, "ceil")
        .expect("ceil export");
    let trunc = instance
        .get_typed_func::<f32, f32>(&mut store, "trunc")
        .expect("trunc export");
    let round = instance
        .get_typed_func::<f32, f32>(&mut store, "round")
        .expect("round export");

    for x in [
        0.0f32, -0.0, 1.0, -1.0, 0.5, 1.5, 2.5, 3.5, -0.5, -1.5, -2.5, -3.5, 3.14159, -3.14159,
        42.0, -42.0,
    ] {
        assert_eq!(floor.call(&mut store, x).unwrap(), x.floor(), "floor({x})");
        assert_eq!(ceil.call(&mut store, x).unwrap(), x.ceil(), "ceil({x})");
        assert_eq!(trunc.call(&mut store, x).unwrap(), x.trunc(), "trunc({x})");
        assert_eq!(
            round.call(&mut store, x).unwrap(),
            x.round_ties_even(),
            "round({x}) [ties-to-even]"
        );
    }

    // The exact roundTiesToEven answers, spelled out.
    assert_eq!(round.call(&mut store, 0.5).unwrap(), 0.0, "round(0.5) == 0");
    assert_eq!(round.call(&mut store, 1.5).unwrap(), 2.0, "round(1.5) == 2");
    assert_eq!(round.call(&mut store, 2.5).unwrap(), 2.0, "round(2.5) == 2");
    assert_eq!(
        round.call(&mut store, -0.5).unwrap().to_bits(),
        (-0.0f32).to_bits(),
        "round(-0.5) == -0"
    );
}

/// R1 integer compare matrix: the four ops the arm now derives (`>`, `!=`,
/// `>=`, `<=`) on BOTH `i32` and `u32`, executed on wasm against Rust's own
/// integer semantics as an independent oracle. The signed/unsigned distinction
/// is the point: the same 32-bit pattern is read signed by the `i32` funcs and
/// unsigned by the `u32` funcs, so any pattern with the top bit set must make
/// `<`/`>`/`<=`/`>=` DIVERGE between the two. `==`/`!=` stay sign-agnostic.
#[test]
fn fe_i32_u32_compare_matrix_runs_on_wasm_with_signedness() {
    let source = r#"
pub fn lt_i32(a: i32, b: i32) -> bool { a < b }
pub fn eq_i32(a: i32, b: i32) -> bool { a == b }
pub fn gt_i32(a: i32, b: i32) -> bool { a > b }
pub fn ne_i32(a: i32, b: i32) -> bool { a != b }
pub fn ge_i32(a: i32, b: i32) -> bool { a >= b }
pub fn le_i32(a: i32, b: i32) -> bool { a <= b }
pub fn lt_u32(a: u32, b: u32) -> bool { a < b }
pub fn eq_u32(a: u32, b: u32) -> bool { a == b }
pub fn gt_u32(a: u32, b: u32) -> bool { a > b }
pub fn ne_u32(a: u32, b: u32) -> bool { a != b }
pub fn ge_u32(a: u32, b: u32) -> bool { a >= b }
pub fn le_u32(a: u32, b: u32) -> bool { a <= b }
"#;
    let wasm = compile_to_wasm("wasm_int_compare_matrix.fe", source);

    // Structural guard: the sign-aware less-than every derived op is built from
    // must reach the width-correct signed AND unsigned wasm opcodes (`Slt` ->
    // `I32LtS`, `Lt` -> `I32LtU`). A regression to i64-only operands or a
    // sign-blind choice would drop one of these.
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
    for opcode in ["I32LtS", "I32LtU"] {
        assert!(
            operators.contains(opcode),
            "generated wasm lacks {opcode} (sign-aware i32 compare regressed):\n{operators}"
        );
    }

    let (mut store, instance) = instantiate(&wasm);

    // Sweep a spread of 32-bit patterns, several with the top bit set. Each
    // pattern is fed to both the i32 and u32 funcs as the SAME wasm i32 bits;
    // the oracle reads it signed for the `*_i32` funcs and unsigned for the
    // `*_u32` funcs.
    let patterns: [i32; 8] = [0, 1, 5, -1, -5, 7, i32::MIN, i32::MAX];
    for &a in &patterns {
        for &b in &patterns {
            let (ua, ub) = (a as u32, b as u32);
            let cases = [
                ("lt_i32", a < b),
                ("eq_i32", a == b),
                ("gt_i32", a > b),
                ("ne_i32", a != b),
                ("ge_i32", a >= b),
                ("le_i32", a <= b),
                ("lt_u32", ua < ub),
                ("eq_u32", ua == ub),
                ("gt_u32", ua > ub),
                ("ne_u32", ua != ub),
                ("ge_u32", ua >= ub),
                ("le_u32", ua <= ub),
            ];
            for (name, expected) in cases {
                let func = instance
                    .get_typed_func::<(i32, i32), i32>(&mut store, name)
                    .unwrap_or_else(|error| panic!("{name} export: {error}"));
                assert_eq!(
                    func.call(&mut store, (a, b)).unwrap(),
                    expected as i32,
                    "{name}({a:#010x}, {b:#010x})"
                );
            }
        }
    }

    // The load-bearing divergence, spelled out: 0xFFFFFFFF is -1 as i32 (so
    // `< 1`, not `> 1`) but 4294967295 as u32 (so `> 1`, not `< 1`). Same bits,
    // opposite answers, proving the inner `lt` picks Slt vs Lt by operand class.
    let gt_i32 = int_cmp(&instance, &mut store, "gt_i32");
    let gt_u32 = int_cmp(&instance, &mut store, "gt_u32");
    let lt_i32 = int_cmp(&instance, &mut store, "lt_i32");
    let lt_u32 = int_cmp(&instance, &mut store, "lt_u32");
    let ge_i32 = int_cmp(&instance, &mut store, "ge_i32");
    let le_u32 = int_cmp(&instance, &mut store, "le_u32");
    assert_eq!(
        gt_i32.call(&mut store, (-1, 1)).unwrap(),
        0,
        "-1 > 1 signed"
    );
    assert_eq!(
        gt_u32.call(&mut store, (-1, 1)).unwrap(),
        1,
        "0xFFFFFFFF > 1 unsigned"
    );
    assert_eq!(
        lt_i32.call(&mut store, (-1, 1)).unwrap(),
        1,
        "-1 < 1 signed"
    );
    assert_eq!(
        lt_u32.call(&mut store, (-1, 1)).unwrap(),
        0,
        "0xFFFFFFFF < 1 unsigned"
    );
    assert_eq!(
        ge_i32.call(&mut store, (-1, 1)).unwrap(),
        0,
        "-1 >= 1 signed"
    );
    assert_eq!(
        le_u32.call(&mut store, (-1, 1)).unwrap(),
        0,
        "0xFFFFFFFF <= 1 unsigned"
    );
}

/// Ordinary Fe logical negation reaches MIR as `UnOp::Not`. Execute both
/// truth-table values and a composed comparison under Wasmtime so this pins
/// language semantics, not merely the presence of a backend opcode.
#[test]
fn fe_bool_not_runs_on_wasm() {
    let source = r#"
pub fn negate(value: bool) -> bool { !value }
pub fn not_less(a: i32, b: i32) -> bool { !(a < b) }
pub fn main() -> u32 { if !false { 1 } else { 0 } }
"#;
    let wasm = compile_to_wasm("wasm_bool_not.fe", source);
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
    assert!(
        operators.contains("I32Eqz"),
        "generated Wasm lacks the exact bool-not operation:\n{operators}"
    );

    let (mut store, instance) = instantiate(&wasm);
    let negate = instance
        .get_typed_func::<i32, i32>(&mut store, "negate")
        .expect("negate export");
    assert_eq!(negate.call(&mut store, 0).unwrap(), 1);
    assert_eq!(negate.call(&mut store, 1).unwrap(), 0);

    let not_less = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "not_less")
        .expect("not_less export");
    for (a, b) in [(-2, -1), (-1, -2), (0, 0), (7, 9), (9, 7)] {
        assert_eq!(
            not_less.call(&mut store, (a, b)).unwrap(),
            (!(a < b)) as i32,
            "!({a} < {b})"
        );
    }

    let evm = compile_to_evm("wasm_bool_not.fe", source);
    assert!(
        !evm.is_empty(),
        "bool-not EVM twin bytecode must be non-empty"
    );
}

/// R2 bitwise: `& | ^ << >>` on i32 and u32 through the R1 wasm path, each
/// checked against a Rust oracle. and/or/xor result bits are signedness-blind;
/// `>>` is NOT (Sar for i32, Shr for u32), so both shift-right opcodes must
/// appear and the `(-1) >> 1` divergence is asserted. Shift amounts are in
/// range (0..31) only, the established R1 posture (oversize behavior is R2).
#[test]
fn fe_i32_u32_bitwise_matrix_runs_on_wasm_with_signedness() {
    let source = r#"
pub fn and_i32(a: i32, b: i32) -> i32 { a & b }
pub fn or_i32(a: i32, b: i32) -> i32 { a | b }
pub fn xor_i32(a: i32, b: i32) -> i32 { a ^ b }
pub fn shl_i32(a: i32, n: i32) -> i32 { a << n }
pub fn shr_i32(a: i32, n: i32) -> i32 { a >> n }
pub fn and_u32(a: u32, b: u32) -> u32 { a & b }
pub fn or_u32(a: u32, b: u32) -> u32 { a | b }
pub fn xor_u32(a: u32, b: u32) -> u32 { a ^ b }
pub fn shl_u32(a: u32, n: u32) -> u32 { a << n }
pub fn shr_u32(a: u32, n: u32) -> u32 { a >> n }
pub fn main() -> u32 {
    let a: u32 = 0xF0F0_F0F0
    let b: u32 = 0x0F0F_0F0F
    let m: u32 = (a & b) | (a ^ b)
    (m << 4) >> 4
}
"#;
    let wasm = compile_to_wasm("wasm_int_bitwise_matrix.fe", source);

    // Structural guard: the emitted wasm must carry all five bit opcodes plus
    // BOTH shift-right variants. A sign-blind `>>` choice would drop one of
    // `I32ShrS`/`I32ShrU`; a missing arm would drop `I32And`/`I32Or`/`I32Xor`.
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
    for opcode in ["I32And", "I32Or", "I32Xor", "I32Shl", "I32ShrS", "I32ShrU"] {
        assert!(
            operators.contains(opcode),
            "generated wasm lacks {opcode} (R2 bitwise arm regressed):\n{operators}"
        );
    }

    let (mut store, instance) = instantiate(&wasm);

    // and/or/xor: same bits fed to i32 and u32 funcs; the RESULT bits are
    // identical across signedness (these ops don't read sign), so the oracle
    // just compares bit patterns.
    let patterns: [i32; 7] = [
        0,
        1,
        0x0F0F_0F0F,
        0xAAAA_AAAAu32 as i32,
        -1,
        i32::MIN,
        i32::MAX,
    ];
    for &a in &patterns {
        for &b in &patterns {
            let (ua, ub) = (a as u32, b as u32);
            let cases = [
                ("and_i32", a & b),
                ("or_i32", a | b),
                ("xor_i32", a ^ b),
                ("and_u32", (ua & ub) as i32),
                ("or_u32", (ua | ub) as i32),
                ("xor_u32", (ua ^ ub) as i32),
            ];
            for (name, expected) in cases {
                let func = int_cmp(&instance, &mut store, name);
                assert_eq!(
                    func.call(&mut store, (a, b)).unwrap(),
                    expected,
                    "{name}({a:#010x}, {b:#010x})"
                );
            }
        }
    }

    // Shifts, in-range amounts only. `<<` is signedness-blind; `>>` picks Sar
    // (i32, sign-extending) vs Shr (u32, zero-filling) by operand class.
    let amounts: [i32; 5] = [0, 1, 7, 16, 31];
    for &a in &patterns {
        for &n in &amounts {
            let ua = a as u32;
            let un = n as u32;
            let cases = [
                ("shl_i32", a.wrapping_shl(un)),
                ("shr_i32", a.wrapping_shr(un)),
                ("shl_u32", ua.wrapping_shl(un) as i32),
                ("shr_u32", ua.wrapping_shr(un) as i32),
            ];
            for (name, expected) in cases {
                let func = int_cmp(&instance, &mut store, name);
                assert_eq!(
                    func.call(&mut store, (a, n)).unwrap(),
                    expected,
                    "{name}({a:#010x} by {n})"
                );
            }
        }
    }

    // The load-bearing signedness divergence, spelled out: 0xFFFFFFFF is -1 as
    // i32 (arithmetic `>>` keeps the sign, staying -1) but 4294967295 as u32
    // (logical `>>` fills zero, giving 0x7FFFFFFF). Same bits in, opposite bits
    // out, proving Sar vs Shr is chosen by operand class.
    let shr_i32 = int_cmp(&instance, &mut store, "shr_i32");
    let shr_u32 = int_cmp(&instance, &mut store, "shr_u32");
    assert_eq!(
        shr_i32.call(&mut store, (-1, 1)).unwrap(),
        -1,
        "-1 >> 1 signed (Sar)"
    );
    assert_eq!(
        shr_u32.call(&mut store, (-1, 1)).unwrap(),
        0x7FFF_FFFF,
        "0xFFFFFFFF >> 1 unsigned (Shr)"
    );
    // `<<` agrees bit-for-bit across widths: 1 << 31 = 0x80000000 either way.
    let shl_i32 = int_cmp(&instance, &mut store, "shl_i32");
    let shl_u32 = int_cmp(&instance, &mut store, "shl_u32");
    assert_eq!(
        shl_i32.call(&mut store, (1, 31)).unwrap(),
        i32::MIN,
        "1 << 31 i32"
    );
    assert_eq!(
        shl_u32.call(&mut store, (1, 31)).unwrap(),
        i32::MIN,
        "1 << 31 u32 (same bits)"
    );

    // Cross-backend twin: one source, both backends. `main` folds `& | ^ << >>`
    // into the EVM root object, and the EVM path already lowers all five
    // (lower_runtime.rs:4128-4146), so this is cheap and proves
    // one-source-two-backends for the R2 bitwise set.
    let evm = compile_to_evm("wasm_int_bitwise_matrix.fe", source);
    assert!(!evm.is_empty(), "evm twin bytecode must be non-empty");
}

/// The riff-cat kernel path witness: one blake3 G-mix column, rotr spelled as
/// shift/shift/or with literal amounts (16, 12, 8, 7), wrapping add via the
/// `WrappingAdd` intrinsic. Exercises XOR + both shifts + OR + wrapping Add
/// together against a Rust G oracle, including top-bit-set inputs where the
/// logical-shift choice in rotr is load-bearing.
#[test]
fn fe_blake3_g_mix_runs_on_wasm() {
    // rotr(x, n) = (x >> n) | (x << (32 - n)); u32 `>>` is logical (Shr).
    // Literal amounts only (O0 does not fold `32 - n`), so complements are
    // spelled directly: 16/16, 12/20, 8/24, 7/25.
    let source = r#"
pub fn g_mix(a: u32, b: u32, c: u32, d: u32, mx: u32, my: u32) -> u32 {
    let a1: u32 = a.wrapping_add(b).wrapping_add(mx)
    let da1: u32 = d ^ a1
    let d1: u32 = ((da1) >> 16) | ((da1) << 16)
    let c1: u32 = c.wrapping_add(d1)
    let bc1: u32 = b ^ c1
    let b1: u32 = ((bc1) >> 12) | ((bc1) << 20)
    let a2: u32 = a1.wrapping_add(b1).wrapping_add(my)
    let da2: u32 = d1 ^ a2
    let d2: u32 = ((da2) >> 8) | ((da2) << 24)
    let c2: u32 = c1.wrapping_add(d2)
    let bc2: u32 = b1 ^ c2
    let b2: u32 = ((bc2) >> 7) | ((bc2) << 25)
    (a2 ^ b2) ^ (c2 ^ d2)
}
"#;
    let wasm = compile_to_wasm("wasm_blake3_g_mix.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let g = instance
        .get_typed_func::<(i32, i32, i32, i32, i32, i32), i32>(&mut store, "g_mix")
        .expect("g_mix export");

    // Independent Rust oracle for one blake3 G column, same rotr spelling.
    fn rotr(x: u32, n: u32) -> u32 {
        (x >> n) | (x << (32 - n))
    }
    fn g_oracle(a: u32, b: u32, c: u32, d: u32, mx: u32, my: u32) -> u32 {
        let a1 = a.wrapping_add(b).wrapping_add(mx);
        let d1 = rotr(d ^ a1, 16);
        let c1 = c.wrapping_add(d1);
        let b1 = rotr(b ^ c1, 12);
        let a2 = a1.wrapping_add(b1).wrapping_add(my);
        let d2 = rotr(d1 ^ a2, 8);
        let c2 = c1.wrapping_add(d2);
        let b2 = rotr(b1 ^ c2, 7);
        a2 ^ b2 ^ c2 ^ d2
    }

    // Vectors include top-bit-set words so the logical-shift choice inside rotr
    // is exercised (an arithmetic `>>` would smear the sign bit and diverge).
    let vectors: [[u32; 6]; 4] = [
        [0, 0, 0, 0, 0, 0],
        [1, 2, 3, 4, 5, 6],
        [
            0x8000_0000,
            0xFFFF_FFFF,
            0x1234_5678,
            0x9ABC_DEF0,
            0xDEAD_BEEF,
            0xCAFE_BABE,
        ],
        [
            0x0102_0408,
            0x1020_4080,
            0xFEDC_BA98,
            0x7654_3210,
            0xA5A5_A5A5,
            0x5A5A_5A5A,
        ],
    ];
    for v in vectors {
        let expected = g_oracle(v[0], v[1], v[2], v[3], v[4], v[5]) as i32;
        let got = g
            .call(
                &mut store,
                (
                    v[0] as i32,
                    v[1] as i32,
                    v[2] as i32,
                    v[3] as i32,
                    v[4] as i32,
                    v[5] as i32,
                ),
            )
            .unwrap();
        assert_eq!(got, expected, "g_mix{v:08x?}");
    }
}

/// D1 const-aggregate seed reification: dec's exact escape shape. A CTFE'd
/// `filled(0.0)` produces a const-aggregate handle (`Ref{Const, AggregateValue}`)
/// that is seeded into a `let mut`, then reassigned via a MULTI-BLOCK method
/// taking `self` by value (so it is NOT inlined and the const handle reaches the
/// call as a value). Before D1 this reached `ty_for_class` as a `Ref{Const}` and
/// was rejected ("supports only scalar values"); reifying every reifiable
/// `ConstRef` unconditionally expands it to scalar leaves + `AggregateMake`.
/// Value assertions catch a wrong leaf order, which a compile-only test misses.
#[test]
fn wasm_const_aggregate_seed_flows_through_call_and_join() {
    let source = r#"
struct Leaf { v: f32 }
impl Copy for Leaf {}
struct Node { lo: Leaf, hi: Leaf }
impl Copy for Node {}

pub trait Fill { const fn filled(value: f32) -> Self }
impl Fill for Node {
    const fn filled(value: f32) -> Self {
        Node { lo: Leaf { v: value }, hi: Leaf { v: value } }
    }
}

pub trait Access {
    const fn set(self, index: i32, value: f32) -> Self
    const fn get(self, index: i32) -> f32
}
impl Access for Node {
    const fn set(self, index: i32, value: f32) -> Self {
        if index < 1 {
            Node { lo: Leaf { v: value }, hi: self.hi }
        } else {
            Node { lo: self.lo, hi: Leaf { v: value } }
        }
    }
    const fn get(self, index: i32) -> f32 {
        if index < 1 { self.lo.v } else { self.hi.v }
    }
}

// The written leaf reads back the runtime value.
pub fn seed_set_get(which: i32, value: f32) -> f32 {
    let mut t: Node = <Node as Fill>::filled(value: 0.0)
    t = t.set(index: which, value: value)
    t.get(index: which)
}

// The OTHER leaf must still read the seed 0.0 (proves the seed materialized,
// not garbage, and the join preserves the untouched field).
pub fn seed_untouched(which: i32, value: f32) -> f32 {
    let mut t: Node = <Node as Fill>::filled(value: 0.0)
    t = t.set(index: which, value: value)
    if which < 1 { t.get(index: 1) } else { t.get(index: 0) }
}
"#;
    let wasm = compile_to_wasm("wasm_const_aggregate_seed_arg.fe", source);
    let (mut store, instance) = instantiate(&wasm);

    let set_get = instance
        .get_typed_func::<(i32, f32), f32>(&mut store, "seed_set_get")
        .expect("seed_set_get export");
    let untouched = instance
        .get_typed_func::<(i32, f32), f32>(&mut store, "seed_untouched")
        .expect("seed_untouched export");

    for (which, value) in [(0i32, 3.5f32), (1, -2.0)] {
        assert_eq!(
            set_get.call(&mut store, (which, value)).unwrap(),
            value,
            "written leaf {which} should read back {value}"
        );
        assert_eq!(
            untouched.call(&mut store, (which, value)).unwrap(),
            0.0,
            "untouched leaf (opposite of {which}) should still be the seed 0.0"
        );
    }
}

/// Aggregate loop-carry over a reified const aggregate: a const-agg seed is
/// accumulated across a `while` back-edge (the shape dec's `d`/`star` operators
/// use on their `Slots` cochains), then a leaf read feeds a post-loop fmul.
/// Exercises D1's reification inside a loop, end to end under wasmtime.
#[test]
fn wasm_aggregate_loop_carry_then_scale() {
    let source = r#"
struct Leaf { v: f32 }
impl Copy for Leaf {}
struct Node { lo: Leaf, hi: Leaf }
impl Copy for Node {}

pub trait Fill { const fn filled(value: f32) -> Self }
impl Fill for Node {
    const fn filled(value: f32) -> Self {
        Node { lo: Leaf { v: value }, hi: Leaf { v: value } }
    }
}
pub trait Access {
    const fn set(self, index: i32, value: f32) -> Self
    const fn get(self, index: i32) -> f32
}
impl Access for Node {
    const fn set(self, index: i32, value: f32) -> Self {
        if index < 1 {
            Node { lo: Leaf { v: value }, hi: self.hi }
        } else {
            Node { lo: self.lo, hi: Leaf { v: value } }
        }
    }
    const fn get(self, index: i32) -> f32 {
        if index < 1 { self.lo.v } else { self.hi.v }
    }
}

pub fn accumulate(n: i32, x: f32) -> f32 {
    let mut acc: Node = <Node as Fill>::filled(value: 0.0)
    let mut t: i32 = 0
    while t < n {
        let cur: f32 = acc.get(index: 0)
        acc = acc.set(index: 0, value: cur + x)
        t = t + 1
    }
    acc.get(index: 0) * 0.5
}
"#;
    let wasm = compile_to_wasm("wasm_aggregate_loop_carry.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let f = instance
        .get_typed_func::<(i32, f32), f32>(&mut store, "accumulate")
        .expect("accumulate export");
    // n iterations add x each time: acc[0] = n*x; result = n*x*0.5.
    assert_eq!(
        f.call(&mut store, (4, 1.5)).unwrap(),
        4.0 * 1.5 * 0.5,
        "4 iters"
    );
    assert_eq!(
        f.call(&mut store, (0, 1.5)).unwrap(),
        0.0,
        "0 iters stays seed"
    );
}

fn int_cmp<'a>(
    instance: &wasmtime::Instance,
    store: &mut wasmtime::Store<()>,
    name: &'a str,
) -> wasmtime::TypedFunc<(i32, i32), i32> {
    instance
        .get_typed_func::<(i32, i32), i32>(store, name)
        .unwrap_or_else(|error| panic!("{name} export: {error}"))
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
fn closed_product_conditional_projection_executes_on_wasm() {
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
    let wasm = compile_to_wasm("wasm_aggregate_conditional_slot.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let select_left = instance
        .get_typed_func::<(i32, i64, i64, i64, i64), i64>(&mut store, "select_left")
        .expect("closed Pair transport should flatten to four scalar arguments");
    assert_eq!(
        select_left.call(&mut store, (1, 11, 12, 21, 22)).unwrap(),
        11
    );
    assert_eq!(
        select_left.call(&mut store, (0, 11, 12, 21, 22)).unwrap(),
        21
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
fn scalar_i32_to_f32_bitcasts_preserve_runtime_and_constant_bits_on_wasm() {
    let source = r#"
extern {
    const fn __bitcast<From, To>(_: From) -> To
}
pub fn bits_to_f32(_ bits: u32) -> f32 { __bitcast(bits) }
pub fn constant_bits_to_f32() -> f32 {
    let bits: u32 = 1065353216
    __bitcast(bits)
}
"#;
    let wasm = compile_to_wasm("scalar_i32_f32_bitcasts.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let bits_to_f32 = instance
        .get_typed_func::<i32, f32>(&mut store, "bits_to_f32")
        .expect("u32 -> f32 bitcast export");
    let constant = instance
        .get_typed_func::<(), f32>(&mut store, "constant_bits_to_f32")
        .expect("constant u32 -> f32 bitcast export");

    for bits in [0u32, 1, 0x3f80_0000, 0x8000_0000, 0x7f80_0000] {
        let value = bits_to_f32
            .call(&mut store, bits as i32)
            .expect("runtime u32 -> f32 bitcast");
        assert_eq!(value.to_bits(), bits);
    }
    assert_eq!(
        constant
            .call(&mut store, ())
            .expect("constant u32 -> f32 bitcast")
            .to_bits(),
        0x3f80_0000,
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
        .get_typed_func::<(i32, i32, f32, f32, f32, f32), i32>(&mut store, "mvt2_f32_helper_render")
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

#[test]
fn qcga3d_sparse_planner_fco_matches_independent_raw_expansion_on_wasm() {
    let fixture = include_str!("fixtures/spirv/qcga3d_sparse_planned_incidence.fe");
    assert!(
        fixture.contains("builder.ty<IncidencePlan12>().normalized_preorder_types()")
            && !fixture.contains("for candidate in 0..144")
            && !fixture.contains("builder.share("),
        "QCGA must traverse the exact reflected plan without candidate rediscovery \
         or claiming nonexistent product sharing",
    );
    let survivors = (0..144)
        .filter(|candidate| {
            let left_slot = candidate / 12;
            let right_slot = candidate % 12;
            let left_blade = if left_slot < 6 {
                left_slot
            } else {
                left_slot + 3
            };
            (left_blade == right_slot && left_blade < 3)
                || (left_blade >= 3 && left_blade < 9 && right_slot == left_blade + 6)
                || (left_blade >= 9 && right_slot + 6 == left_blade)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        survivors,
        [0, 13, 26, 45, 58, 71, 75, 88, 101, 114, 127, 140],
        "independent paper-null survivor order",
    );
    let product_keys = survivors
        .iter()
        .map(|candidate| (candidate / 12, candidate % 12))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        product_keys.len(),
        survivors.len(),
        "all twelve QCGA product keys are unique, so plan-level DAG fanout is zero",
    );

    let sparse_api = fe_codegen::standalone_ctfe_ingot_source(include_str!(
        "../../../ingots/sparse_clifford/src/lib.fe"
    ));
    let source = format!("{}\n{}", sparse_api, fixture,);
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///qcga3d_sparse_planned_incidence.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source));
    let file = db.workspace().get(&db, &url).expect("planner fixture");
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "planned QCGA semantic analysis failed:\n{diagnostics}"
    );
    let compile_entry = |entry: &str| {
        let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, entry)
            .unwrap_or_else(|err| panic!("runtime package for `{entry}` failed: {err}"));
        let bytes =
            compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
                .unwrap_or_else(|err| panic!("entry-rooted wasm for `{entry}` failed: {err}"))
                .bytes;
        wasmparser::validate(&bytes).expect("produced invalid entry-rooted wasm");
        bytes
    };
    let planned_wasm = compile_entry("qcga3d_incidence_planned");
    let raw_wasm = compile_entry("qcga3d_incidence_raw");
    let polynomial_wasm = compile_entry("qcga3d_incidence_polynomial");
    assert!(
        func_imports(&planned_wasm).is_empty() && func_imports(&raw_wasm).is_empty(),
        "planned sparse incidence fixture must have zero imports"
    );
    let planned_shape = wasm_float_shape(&planned_wasm);
    assert_eq!(
        (planned_shape.3, planned_shape.4),
        (1, 0),
        "entry plus one shared FCO aggregate helper must contain no runtime loop: {planned_shape:?}"
    );
    eprintln!("QCGA sparse planned Wasm shape (add,sub,mul,call,loop)={planned_shape:?}");
    let (mut planned_store, planned_instance) = instantiate(&planned_wasm);
    let (mut raw_store, raw_instance) = instantiate(&raw_wasm);
    let (mut polynomial_store, polynomial_instance) = instantiate(&polynomial_wasm);
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
    let planned = planned_instance
        .get_typed_func::<Inputs, f32>(&mut planned_store, "qcga3d_incidence_planned")
        .expect("CTFE/FCO planned sparse incidence ABI");
    let raw = raw_instance
        .get_typed_func::<Inputs, f32>(&mut raw_store, "qcga3d_incidence_raw")
        .expect("independent raw polynomial ABI");
    let polynomial = polynomial_instance
        .get_typed_func::<Inputs, f32>(&mut polynomial_store, "qcga3d_incidence_polynomial")
        .expect("independent fused polynomial ABI");
    let cases: [Inputs; 7] = [
        (
            3.0, 4.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -25.0,
        ),
        (
            0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -25.0,
        ),
        (
            2.0, -1.0, 2.0, 1.0, 2.0, 3.0, 0.0, 0.0, 0.0, -2.0, 8.0, -6.0, 0.0,
        ),
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
        (
            0.25, -0.75, 1.5, 0.85, 1.25, 0.65, 0.55, -0.40, 0.30, -0.16, 0.1375, -0.04, -0.979125,
        ),
        (
            -2.25, 0.5, 3.75, -1.5, 2.25, 0.125, -0.75, 1.5, -2.0, 0.375, -1.25, 2.5, 0.0625,
        ),
    ];
    for (case_index, inputs) in cases.into_iter().enumerate() {
        let planned_value = planned
            .call(&mut planned_store, inputs)
            .expect("planned incidence");
        let raw_value = raw.call(&mut raw_store, inputs).expect("raw incidence");
        let polynomial_value = polynomial
            .call(&mut polynomial_store, inputs)
            .expect("polynomial incidence");
        assert_eq!(
            planned_value.to_bits(),
            raw_value.to_bits(),
            "planned and raw incidence differ for case {case_index}: {planned_value} != {raw_value}"
        );
        assert!(
            (planned_value - polynomial_value).abs() <= 2.0e-5,
            "planned and fused polynomial differ for case {case_index}: \
             {planned_value} != {polynomial_value}"
        );
    }
}

#[test]
fn qcga3d_sparse_planned_render_preserves_current_frame_on_wasm() {
    let sparse_api = fe_codegen::standalone_ctfe_ingot_source(include_str!(
        "../../../ingots/sparse_clifford/src/lib.fe"
    ));
    let source = format!(
        "{}\n{}\n{}",
        sparse_api,
        include_str!("fixtures/spirv/qcga3d_sparse_planned_incidence.fe"),
        include_str!("fixtures/spirv/qcga3d_sparse_planned_render_body.fe"),
    );
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///qcga3d_sparse_planned_render.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source));
    let file = db
        .workspace()
        .get(&db, &url)
        .expect("planned render fixture");
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "planned QCGA render semantic analysis failed:\n{diagnostics}"
    );
    let package =
        mir::build_wasm_runtime_package_for_entry(&db, top_mod, "qcga3d_sparse_planned_render")
            .expect("planned QCGA render package");
    let planned_wasm =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .expect("planned QCGA render Wasm")
            .bytes;
    wasmparser::validate(&planned_wasm).expect("valid planned render Wasm");
    assert!(func_imports(&planned_wasm).is_empty());
    let planned_shape = wasm_float_shape(&planned_wasm);
    assert_eq!(planned_shape.4, 0, "planned render has no runtime loop");

    let old_source = include_str!("fixtures/spirv/qcga3d_rotated_quadric_render.fe");
    let old_wasm = compile_to_wasm("qcga3d_rotated_quadric_render.fe", old_source);
    let (mut planned_store, planned_instance) = instantiate(&planned_wasm);
    let (mut old_store, old_instance) = instantiate(&old_wasm);
    type PlannedInputs = (
        i32,
        i32,
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
        f32,
        f32,
    );
    let planned = planned_instance
        .get_typed_func::<PlannedInputs, i32>(&mut planned_store, "qcga3d_sparse_planned_render")
        .expect("planned render ABI");
    let old = old_instance
        .get_typed_func::<(i32, i32), i32>(&mut old_store, "qcga3d_rotated_quadric_render")
        .expect("current render ABI");
    let mut hash = 0x811c9dc5u32;
    for py in 0..128 {
        for px in 0..128 {
            let got = planned
                .call(
                    &mut planned_store,
                    (
                        px, py, 0.0, 0.0, -4.0, 3.24, 0.018, 0.85, 1.25, 0.65, 0.55, -0.40, 0.30,
                        -0.16, 0.1375, -0.04, -0.979125,
                    ),
                )
                .expect("planned pixel") as u32;
            let expected = old.call(&mut old_store, (px, py)).expect("current pixel") as u32;
            assert_eq!(
                got, expected,
                "planned/current frame mismatch at ({px},{py}): {got:#010x} != {expected:#010x}"
            );
            for byte in got.to_le_bytes() {
                hash = (hash ^ u32::from(byte)).wrapping_mul(0x01000193);
            }
        }
    }
    assert_eq!(hash, 2_368_784_280);
    eprintln!("QCGA planned frame bit-exact; Wasm shape (add,sub,mul,call,loop)={planned_shape:?}");
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
            let sign = if (swaps + metric_neg as u32) & 1 == 0 {
                1.0
            } else {
                -1.0
            };
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
                .call(
                    &mut store,
                    (px, py, a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]),
                )
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
            let sign = if (swaps + metric_neg as u32) & 1 == 0 {
                1.0
            } else {
                -1.0
            };
            out[left ^ right] += sign * a[left] * b[right];
        }
    }
    out
}

#[test]
fn generated_recursive_cl41_gp_f32_coefficients_execute_on_wasm() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spirv");
    let status = std::process::Command::new("python3")
        .arg(fixture_dir.join("gen_clifford_gp_f32_mvt5.py"))
        .arg("--check")
        .status()
        .expect("Cl(4,1) GP fixture generator should run");
    assert!(status.success(), "generated Cl(4,1) GP fixture is stale");
    let source = include_str!("fixtures/spirv/clifford_gp_recursive_f32_mvt5.fe");
    let wasm = compile_to_wasm("clifford_gp_recursive_f32_mvt5.fe", source);
    assert!(
        func_imports(&wasm).is_empty(),
        "recursive GP must not retain imports"
    );
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
        [
            dense_expected[0],
            dense_expected[1],
            dense_expected[4],
            dense_expected[17],
            dense_expected[31]
        ],
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
            args.extend(
                a.into_iter()
                    .chain(b)
                    .map(|value| wasmtime::Val::F32(value.to_bits())),
            );
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
    assert!(
        func_imports(&wasm).is_empty(),
        "authored generic sandwich must have zero imports"
    );
    let (mut store, instance) = instantiate(&wasm);
    let sandwich = instance
        .get_typed_func::<(i32, i32, f32, f32, f32, f32, f32), i32>(
            &mut store,
            "cga_sandwich_authored_generic_mvt5",
        )
        .expect("authored generic MvT<5> sandwich ABI");
    for (x, y, z, cx, cy) in [(2.5, 0.25, 0.0, 0.5, 0.25), (0.5, 2.25, 0.0, 0.5, 0.25)] {
        let mut sphere = [0.0; 32];
        let center2 = cx * cx + cy * cy;
        sphere[1] = cx;
        sphere[2] = cy;
        sphere[8] = center2 * 0.5 - 1.0;
        sphere[16] = center2 * 0.5;
        let expected = clifford_gp_cl41_oracle(
            clifford_gp_cl41_oracle(sphere, conformal_point_cl41(x, y, z)),
            sphere,
        );
        for (index, coefficient) in expected.into_iter().enumerate() {
            let got = sandwich
                .call(
                    &mut store,
                    ((index % 8) as i32, (index / 8) as i32, x, y, z, cx, cy),
                )
                .expect("authored generic CGA coefficient");
            assert_eq!(got, (coefficient * 256.0) as i32, "coefficient {index}");
        }
    }
}

#[test]
fn generated_recursive_cl41_cga_sandwich_executes_on_wasm() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spirv");
    let status = std::process::Command::new("python3")
        .arg(fixture_dir.join("gen_cga_sandwich_f32_mvt5.py"))
        .arg("--check")
        .status()
        .expect("CGA sandwich fixture generator should run");
    assert!(status.success(), "generated CGA sandwich fixture is stale");
    let source = include_str!("fixtures/spirv/cga_sandwich_recursive_f32_mvt5.fe");
    let wasm = compile_to_wasm("cga_sandwich_recursive_f32_mvt5.fe", source);
    assert!(
        func_imports(&wasm).is_empty(),
        "recursive sandwich must not retain imports"
    );
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
            assert_eq!(
                scaled.fract(),
                0.0,
                "coefficient {index} must be exactly observable"
            );
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
            sandwich
                .call(&mut store, &args, &mut results)
                .expect("recursive CGA sandwich coefficient execution");
            let wasmtime::Val::I32(word) = results[0] else {
                panic!("CGA sandwich Render result must be i32")
            };
            got_words[index] = word;
            assert_eq!(
                word,
                (expected[index] * 256.0) as i32,
                "coefficient {index}"
            );
        }
        for index in 0..32 {
            if ![1, 2, 4, 8, 16].contains(&index) {
                assert_eq!(got_words[index], 0, "off-vector blade {index}");
            }
        }
        let weight = expected[16] - expected[8];
        assert_ne!(weight, 0.0, "normalization case must be finite");
        assert_eq!(
            (
                expected[1] / weight,
                expected[2] / weight,
                expected[4] / weight
            ),
            expected_q,
        );
    }
}

#[test]
fn generated_support_cl41_cga_sandwich_executes_on_wasm() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spirv");
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
    let cases = [(2.5, 0.25, 0.0, 0.5, 0.25), (0.5, 2.25, 0.0, 0.5, 0.25)];
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
        let outputs = [
            expected[1] / weight,
            expected[2] / weight,
            expected[4] / weight,
            weight,
        ];

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
                return (shade + 88 * 256 + (255 - shade) * 65_536 - 16_777_216_i32) as u32;
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
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spirv");
    let status = std::process::Command::new("python3")
        .arg(fixture_dir.join("gen_cga_inversion_cyclide_recursive_support.py"))
        .arg("--check")
        .status()
        .expect("recursive-support cyclide generator should run");
    assert!(
        status.success(),
        "recursive-support cyclide fixture is stale"
    );

    let source = include_str!("fixtures/spirv/cga_inversion_cyclide_recursive_support.fe");
    let started = std::time::Instant::now();
    let wasm = compile_to_wasm("cga_inversion_cyclide_recursive_support.fe", source);
    eprintln!(
        "recursive-support cyclide Wasm: {} bytes compiled in {:?}",
        wasm.len(),
        started.elapsed()
    );
    assert!(
        func_imports(&wasm).is_empty(),
        "recursive-support cyclide must have zero imports"
    );
    let (mut store, instance) = instantiate(&wasm);
    let render = instance
        .get_typed_func::<(i32, i32, f32, f32, f32, f32, f32), i32>(
            &mut store,
            "cga_inversion_cyclide_recursive_support",
        )
        .expect("recursive-support ABI must be exactly two i32 builtins plus five f32 values");
    for py in 0..H {
        for px in 0..W {
            let got = render
                .call(
                    &mut store,
                    (
                        px, py, VALUES[0], VALUES[1], VALUES[2], VALUES[3], VALUES[4],
                    ),
                )
                .expect("recursive-support cyclide Wasm pixel") as u32;
            let expected = cga_recursive_support_scalar_oracle(
                px, py, VALUES[0], VALUES[1], VALUES[2], VALUES[3], VALUES[4],
            );
            assert_eq!(got, expected, "recursive-support Wasm pixel ({px},{py})");
        }
    }
}

#[test]
fn generated_recursive_mvt5_f32_render_is_current_and_executes_on_wasm() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spirv");
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
    // `__abs_f32`/`__min_f32`/`__max_f32`/`__clamp_f32` graduated to
    // dedicated Sonatina lowering (`Fabs`/`Fmin`/`Fmax`/`Fclamp`); see
    // `f32_abs_min_max_clamp_intrinsics_execute_on_wasm` above for their
    // positive execution coverage. `__floor_f32`/`__ceil_f32`/`__trunc_f32`/
    // `__round_f32` similarly graduated (`Ffloor`/`Fceil`/`Ftrunc`/`Fround`);
    // see `f32_rounding_intrinsics_execute_on_wasm` below. `__rsqrt_f32`
    // remains deliberately unsupported.
    for (name, params, args) in [("__rsqrt_f32", "_: f32", "value")] {
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

/// `#[host_import(module = "fe:host")]` on an extern block names the generic
/// host namespace. The Wasm backend realizes `host_log` as
/// `("fe:host", "host_log")`
/// import instead of the flat `("fe", "host_log")` v0 default. The module string
/// threads HIR (block attribute propagated onto the extern `Func`) -> runtime
/// package -> the WasmBackend side table -> WAFFLE import emission. wasmtime
/// satisfies the import through a `Linker` bound at `("fe:host", "host_log")`.
#[test]
fn fe_host_import_module_attribute_names_module() {
    let source = "#[host_import(module = \"fe:host\")]\n\
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

#[test]
fn host_import_rejects_simultaneous_compatibility_alias() {
    let error = compile_to_wasm_err(
        "duplicate_host_import.fe",
        "#[host_import(module = \"fe:host\")]\n\
         #[wasm_import(module = \"legacy\")]\n\
         extern { pub unsafe fn host_log(value: u32) }\n\
         pub fn main() { host_log(1) }\n",
    );
    assert!(
        error.contains("declares both `#[host_import]`")
            && error.contains("`#[wasm_import]` compatibility alias"),
        "mixed generic and compatibility attributes must fail deterministically: {error}"
    );
}

/// Generated Web IDL bindings are ordinary Fe declarations: they pass through
/// the existing generic Wasm import path without codegen knowing `Window` or
/// any other Web API name.
#[test]
fn generated_webidl_binding_uses_generic_wasm_imports() {
    let world = fe_webidl_bindgen::parse(
        "interface Window { readonly attribute unsigned long innerWidth; };",
    )
    .expect("fixture Web IDL should parse");
    let adapter = fe_webidl_bindgen::build_adapter_plan(&world, "web-e2e", "fe:web")
        .expect("fixture should normalize to the generic host ABI");
    let transport = fe_webidl_bindgen::build_transport_plan(&adapter)
        .expect("fixture should produce a core-Wasm transport plan");
    let planned = transport
        .functions
        .iter()
        .find(|function| function.import_name == "window_get_inner_width")
        .expect("transport should contain the generated attribute getter");
    assert_eq!(
        planned.core,
        Some(fe_webidl_bindgen::CoreSignature {
            params: vec![fe_webidl_bindgen::CoreValueType::I32],
            results: vec![fe_webidl_bindgen::CoreValueType::I32],
        }),
        "transport planner should describe the executable receiver/result lanes"
    );
    let mut source = fe_webidl_bindgen::emit_fe_raw(&world, "fe:web")
        .expect("scalar Web IDL fixture should lower to the v0 ABI");
    assert!(source.contains("#[host_import(module = \"fe:web\")]"));
    assert!(
        !source.contains("wasm_import"),
        "generated Fe must use target-neutral host vocabulary"
    );
    source.push_str(
        "\npub fn read_inner_width(window: Window) -> u32 {\n\
         \x20   window_get_inner_width(window)\n\
         }\n",
    );

    let wasm = compile_to_wasm("generated_webidl_window.fe", &source);
    let imports = func_imports(&wasm);
    assert!(
        imports.contains(&("fe:web".to_owned(), "window_get_inner_width".to_owned())),
        "generated binding should be a normal namespaced Wasm import, found {imports:?}"
    );

    // Execute the same generic import path. The interface value is the
    // transport's borrowed i32 session token; neither MIR nor Wasm codegen
    // contains a Window- or WebIDL-specific case.
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    linker
        .func_wrap(
            "fe:web",
            "window_get_inner_width",
            |window_token: u32| -> u32 {
                assert_eq!(window_token, 17, "receiver token should cross as one i32");
                1440
            },
        )
        .expect("binding the generated Web IDL import should succeed");
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("generated Web IDL module should instantiate");
    let read_inner_width = instance
        .get_typed_func::<u32, u32>(&mut store, "read_inner_width")
        .expect("generated binding caller should be exported");
    assert_eq!(
        read_inner_width.call(&mut store, 17).unwrap(),
        1440,
        "Fe should pass the borrowed receiver token to the host and return its scalar result"
    );
}

/// The checked-in `std::web` facade remains an ordinary consumer of generated
/// host imports. This executes the currently honest resource/u32 subset without
/// teaching MIR or Wasm codegen any Web API names.
#[test]
fn std_web_safe_resource_facade_uses_generated_generic_imports() {
    let idl = include_str!("../../../ingots/std/web-minimal.webidl");
    let world = fe_webidl_bindgen::parse(idl).expect("minimal std Web IDL should parse");
    let generated =
        fe_webidl_bindgen::emit_fe_raw(&world, "fe:web").expect("resource/scalar IDL should emit");
    assert_eq!(
        generated,
        include_str!("../../../ingots/std/src/web/raw.fe"),
        "checked-in raw declarations must be generator output, never hand-maintained DOM bindings"
    );
    let adapter =
        fe_webidl_bindgen::emit_js_adapter(&world, "fe:web").expect("v0 adapter should emit");
    assert!(adapter.contains(
        "window_get_document(selfHandle) { return handles.insert(handles.get(selfHandle).document); }"
    ));
    assert!(adapter.contains(
        "document_get_document_element(selfHandle) { return handles.insert(handles.get(selfHandle).documentElement); }"
    ));

    let source = "\
use std::web\n\
\n\
pub fn safe_child_count(_ raw: own web::raw::Window) -> u32 {\n\
\x20   let window = web::Window::from_raw(raw)\n\
\x20   window.document().document_element().child_element_count()\n\
}\n";
    let wasm = compile_to_wasm("std_web_safe_facade.fe", source);
    let imports = func_imports(&wasm);
    for name in [
        "window_get_document",
        "document_get_document_element",
        "element_get_child_element_count",
    ] {
        assert!(
            imports.contains(&("fe:web".to_owned(), name.to_owned())),
            "safe facade should lower through generic import `{name}`, found {imports:?}"
        );
    }

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load facade module");
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    linker
        .func_wrap("fe:web", "window_get_document", |window: u32| -> u32 {
            assert_eq!(window, 17);
            23
        })
        .unwrap();
    linker
        .func_wrap(
            "fe:web",
            "document_get_document_element",
            |document: u32| -> u32 {
                assert_eq!(document, 23);
                41
            },
        )
        .unwrap();
    linker
        .func_wrap(
            "fe:web",
            "element_get_child_element_count",
            |element: u32| -> u32 {
                assert_eq!(element, 41);
                7
            },
        )
        .unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let call = instance
        .get_typed_func::<u32, u32>(&mut store, "safe_child_count")
        .expect("safe facade function should export one resource-token lane");
    assert_eq!(call.call(&mut store, 17).unwrap(), 7);
}

#[test]
fn generated_domstring_host_import_uses_flat_canonical_lanes() {
    let world = fe_webidl_bindgen::parse("interface Channel { DOMString echo(DOMString value); };")
        .expect("DOMString fixture should parse");
    let adapter = fe_webidl_bindgen::build_adapter_plan(&world, "string-e2e", "fe:web")
        .expect("DOMString fixture should normalize");
    let transport = fe_webidl_bindgen::build_transport_plan(&adapter)
        .expect("DOMString fixture should produce a transport plan");
    let planned = transport
        .functions
        .iter()
        .find(|function| function.import_name == "channel_echo")
        .expect("transport should contain Channel.echo");
    assert_eq!(
        planned.core,
        Some(fe_webidl_bindgen::CoreSignature {
            params: vec![
                fe_webidl_bindgen::CoreValueType::I32,
                fe_webidl_bindgen::CoreValueType::I32,
                fe_webidl_bindgen::CoreValueType::I32,
            ],
            results: vec![
                fe_webidl_bindgen::CoreValueType::I32,
                fe_webidl_bindgen::CoreValueType::I32,
            ],
        })
    );

    let mut source = fe_webidl_bindgen::emit_fe_flat_host_imports(&world, "fe:web")
        .expect("DOMString should lower to a generic flat host signature");
    source.push_str(
        "\npub fn call_echo(channel: Channel, value: own BrowserUtf16String) -> BrowserUtf16String {\n\
         \x20   channel_echo(channel, value)\n\
         }\n",
    );
    let wasm = compile_to_wasm("generated_webidl_domstring.fe", &source);
    assert!(func_imports(&wasm).contains(&("fe:web".to_owned(), "channel_echo".to_owned())));

    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, &wasm).expect("wasmtime should load DOMString wasm");
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    linker
        .func_wrap(
            "fe:web",
            "channel_echo",
            |channel: u32, ptr: u32, len: u32| -> (u32, u32) {
                assert_eq!(channel, 23);
                assert_eq!((ptr, len), (4096, 5));
                // This compiler-lane test returns the borrowed input descriptor.
                // Owned result allocation/post-return is tested at the codec
                // boundary and is not yet expressible by the safe Fe wrapper.
                (ptr, len)
            },
        )
        .expect("binding flat DOMString import should succeed");
    let instance = linker
        .instantiate(&mut store, &module)
        .expect("flat DOMString module should instantiate");
    let call_echo = instance
        .get_typed_func::<(u32, u32, u32), (u32, u32)>(&mut store, "call_echo")
        .expect("DOMString wrapper should expose flattened lanes");
    assert_eq!(
        call_echo.call(&mut store, (23, 4096, 5)).unwrap(),
        (4096, 5)
    );
}

#[test]
fn u32_iterator_enum_result_identifies_indirect_host_import_gap() {
    let source = r#"
use core::BrowserString

pub struct CountersIterator { handle: u32 }
pub enum CountersIteratorOption {
    None,
    Some { value: u32 }
}
pub enum CountersIteratorNext {
    Ok { value: CountersIteratorOption },
    Error { error: BrowserString }
}

#[host_import(module = "fe:web")]
extern {
    #[host_result(codec = "fe:host-wasm-codec/v1")]
    pub unsafe fn counters_iterator_next(
        self_: CountersIterator
    ) -> CountersIteratorNext
}

pub fn next_counter(value: CountersIterator) -> CountersIteratorNext {
    counters_iterator_next(value)
}
"#;
    let error = compile_to_wasm_err("generated_webidl_u32_iterator.fe", source);
    assert!(
        error.contains(
            "extern host import `counters_iterator_next` uses indirect host result codec \
             `fe:host-wasm-codec/v1`, but the Wasm backend is missing required capabilities: \
             realloc, post-return"
        ),
        "{error}"
    );
}

#[test]
fn indirect_host_result_rejects_non_aggregate_authored_return() {
    let source = r#"
#[host_import(module = "fe:web")]
extern {
    #[host_result(codec = "fe:host-wasm-codec/v1")]
    pub unsafe fn invalid_scalar_result() -> u32
}

pub fn call_invalid() -> u32 {
    invalid_scalar_result()
}
"#;
    let error = compile_to_wasm_err("invalid_indirect_host_result.fe", source);
    assert!(
        error.contains(
            "extern host import `invalid_scalar_result` declares an indirect host result, but \
             authored return type `u32` is not an aggregate"
        ),
        "{error}"
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
    assert_eq!(&memory.data(&store)[19..23], &0x78563412_u32.to_le_bytes());
}

#[test]
fn browser_ptr_u32_allocates_copies_and_roundtrips_on_wasm32_memory() {
    let source = r#"
use core::browser::{BrowserPtr, alloc_browser_bytes}

pub fn allocated_roundtrip(value: u32) -> u32 {
    let bytes = alloc_browser_bytes(8)
    let word: BrowserPtr<u32> = BrowserPtr::from_u32(bytes.address())
    let alias = word
    alias.write(value)
    word.read()
}

pub fn explicit_roundtrip(addr: u32, value: u32) -> u32 {
    let word: BrowserPtr<u32> = BrowserPtr::from_u32(addr)
    word.write(value)
    word.read()
}
"#;
    let wasm = compile_to_wasm("wasm_browser_ptr_u32.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("BrowserPtr allocation requires exported linear memory");
    let allocated = instance
        .get_typed_func::<i32, i32>(&mut store, "allocated_roundtrip")
        .expect("allocated_roundtrip export");
    let explicit = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "explicit_roundtrip")
        .expect("explicit_roundtrip export");

    assert_eq!(allocated.call(&mut store, 0x78563412).unwrap(), 0x78563412);
    assert_eq!(
        explicit.call(&mut store, (19, 0x10203040)).unwrap(),
        0x10203040
    );
    assert_eq!(&memory.data(&store)[19..23], &0x10203040_u32.to_le_bytes());
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
fn fe_worker_par_local_provider_executes() {
    // WAS a fail-closed test at the R2.1 aggregate (tuple) wall: `par.fork`
    // returns a `(u64, u64)`. The aggregate-flattening work removed that wall,
    // so the fork/join now compiles AND RUNS. Pinned by execution against the
    // same NTT8_OUTPUT oracle the sequential rail uses.
    let src = worker_par_src();
    let wasm = compile_to_wasm("wasm_worker_par.fe", &src);
    let (mut store, instance) = instantiate_task_pool(&wasm);
    let par_pair = instance
        .get_typed_func::<(i64, i64), i64>(&mut store, "par_pair")
        .expect("`par_pair` export should exist");
    for (k0, k1) in [(0usize, 1usize), (2, 5), (7, 7)] {
        let got = par_pair
            .call(&mut store, (k0 as i64, k1 as i64))
            .expect("Par fork/join should run through the task rail");
        let expected = (NTT8_OUTPUT[k0] as u64 + NTT8_OUTPUT[k1] as u64) % 12289;
        assert_eq!(
            got as u64, expected,
            "par_pair({k0}, {k1}) should be addmod_q of the two pinned NTT-8 coefficients"
        );
    }
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
/// the payload-enum wall. Fieldless enums have a compiler-derived scalar tag, but
/// `Result<E, A>` needs a tagged payload representation that the Wasm value lane
/// does not yet provide. The flagship type-checks in full (the anti-coloring /
/// typed-environment thesis holds); only its Wasm execution is walled.
#[test]
fn fe_ce9_result_flagship_fails_closed_at_enum_wall() {
    let err = compile_to_wasm_err("wasm_ce9_flagship.fe", CE9_FLAGSHIP_SRC);
    assert!(
        err.contains("payload enum value transport is not implemented"),
        "the Result-typed flagship should fail closed specifically at the Wasm \
         payload-enum transport boundary, got: {err}"
    );
}

/// CE-9 (secondary wall, pinned): control flow inside a capability (`uses`) body
/// fails closed even in scalar form - the provision environment becomes an aggregate
/// local across the branch. Straight-line capability bodies and pure branchy bodies
/// both lower fine; this is specific to control flow over a capability environment.
#[test]
fn fe_ce9_capability_body_control_flow_executes() {
    // WAS a fail-closed test: an `if` inside a capability body used to be
    // rejected at the aggregate provision-env local (R2). The
    // aggregate-flattening work (recursive values, owned record params, closed
    // products) removed that wall, so the program now compiles AND RUNS. Pinned
    // by execution rather than deleted, so the capability is a tested guarantee.
    let wasm = compile_to_wasm("wasm_ce9_cap_cf.fe", CE9_CAP_CONTROL_FLOW_SRC);
    let (mut store, instance) = instantiate_worker_and_clock(&wasm);
    let run = instance
        .get_typed_func::<i64, i64>(&mut store, "run")
        .expect("`run` export should exist");
    // n = BASE_CLOCK_MS = 1000, so `5 < n` holds and `branchy` yields
    // `raw` = fe_task(0, pair) = pair + 100 (not the `+ 1` else-arm).
    for pair in [5i64, 0, 42] {
        let got = run
            .call(&mut store, pair)
            .expect("a capability body with an `if` should fork/join and branch");
        assert_eq!(
            got as u64,
            pair as u64 + 100,
            "run({pair}) should take the `5 < n` arm and return raw = pair + 100"
        );
    }
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

/// A top-level `pub fn` that is reachable as a callee is still exported.
///
/// REGRESSION TEST for a bug fixed 2026-07-25. `wasm_runtime_root_candidate` admits
/// every `pub`, non-associated function of the entry module as a root
/// candidate, but `build_wasm_runtime_package_impl` then narrows to
/// `seed_funcs` by dropping candidates reachable as a callee, and only seeded
/// roots receive `RuntimeLinkage::Internal` -> `Linkage::Public`. That
/// narrowing is documented as safe because a callee-reachable candidate is
/// "bare-named via the export-everything policy (the R1 status quo)".
///
/// Sonatina `ac266c21` ("fix(wasm): keep private functions out of exports")
/// removed export-everything, gating exports on `Linkage::Public`. The
/// assumption the narrowing rests on is therefore false, and callee-reachable
/// `pub fn`s silently stopped being exported. Measured export surface for the
/// source below is `["memory", "main"]`; `add` is missing.
///
/// Fixed by decoupling "is exported" from "is a seeded root": every admitted
/// candidate in `entry_funcs` now gets public linkage on its single existing
/// instance, leaving root seeding untouched (seeding them would mint a second
/// scope-only-distinct instance and mangle both symbols).
#[test]
fn pub_fn_reachable_as_callee_is_still_exported() {
    let source = "pub fn add(a: u64, b: u64) -> u64 { a + b }\n\
                  pub fn main() -> u64 { add(2, 3) }\n";
    let wasm = compile_to_wasm("pub_callee_export.fe", source);
    let mut exports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        if let Ok(wasmparser::Payload::ExportSection(reader)) = payload {
            for export in reader {
                exports.push(export.expect("export should parse").name.to_string());
            }
        }
    }
    assert!(
        exports.iter().any(|name| name == "add"),
        "`pub fn add` should be exported even though `main` calls it; got {exports:?}"
    );
}

// ----------------------------------------------------------------------------
// Arrays rung STEP 1, Slice A: function-local `[u32; N]` array memory lowering.
// A local array indexed by a runtime variable compiles + runs on wasm; an
// out-of-bounds index traps; the u256-element and object-ref-signature shapes
// stay fail-closed.
// ----------------------------------------------------------------------------

/// THE SLICE A GATE: a function-local `[u32; 8]` filled by a while loop and read
/// at a runtime index compiles Fe -> wasm, runs under wasmtime for several `k`
/// (== a Rust oracle), and traps on an out-of-bounds index.
#[test]
fn local_u32_array_runtime_index_runs_on_wasm_and_traps_out_of_bounds() {
    let source = r#"
pub fn probe(k: u32) -> u32 {
    let mut a: [u32; 8] = [0; 8]
    let mut i: u32 = 0
    while i < 8 {
        a[i as usize] = i * i
        i = i + 1
    }
    a[k as usize]
}
"#;
    let wasm = compile_to_wasm("wasm_local_u32_array.fe", source);

    // The array is allocated in the canonical arena, so `MemAllocDynamic` lowers
    // to the synthesized (not imported) `fe_cabi_alloc`: the module must stay
    // self-contained (no function imports needed to instantiate).
    assert!(
        func_imports(&wasm).is_empty(),
        "the array probe should need no host imports; got {:?}",
        func_imports(&wasm)
    );

    let (mut store, instance) = instantiate(&wasm);
    let probe = instance
        .get_typed_func::<i32, i32>(&mut store, "probe")
        .expect("`probe` export should exist");

    // Rust oracle: a[i] = i*i for i in 0..8, so probe(k) == k*k for k in 0..8.
    for k in 0..8_i32 {
        let expected = k * k;
        let got = probe
            .call(&mut store, k)
            .unwrap_or_else(|err| panic!("probe({k}) should run: {err}"));
        assert_eq!(got, expected, "probe({k}) should be {expected}");
    }

    // Out-of-bounds indexes trap (wasm `unreachable`), the portable image of the
    // EVM revert an OOB index takes.
    for k in [8_i32, 9, 42, 1000] {
        assert!(
            probe.call(&mut store, k).is_err(),
            "probe({k}) is out of bounds and must trap"
        );
    }
}

/// A private fixed-array parameter is flattened at the call boundary and
/// materialized into independent target-layout memory for dynamic indexing.
/// `u8` arrays are intentionally packed at one byte per element, so this gate
/// rejects the tempting but incorrect assumption that every flattened leaf is
/// one Wasm-layout word apart.
#[test]
fn packed_u8_array_parameter_uses_target_layout_stride() {
    let source = r#"
fn pick(bytes: [u8; 4], index: u32) -> u8 {
    bytes[index as usize]
}

pub fn probe(a: u8, b: u8, c: u8, d: u8, index: u32) -> u8 {
    pick([a, b, c, d], index)
}
"#;
    let wasm = compile_to_wasm("wasm_packed_u8_array_parameter.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let probe = instance
        .get_typed_func::<(i32, i32, i32, i32, i32), i32>(&mut store, "probe")
        .expect("`probe` export should exist");

    for index in 0..4 {
        assert_eq!(
            probe.call(&mut store, (17, 33, 65, 129, index)).unwrap(),
            [17, 33, 65, 129][index as usize]
        );
    }
    assert!(probe.call(&mut store, (17, 33, 65, 129, 4)).is_err());
}

/// Fail-closed regression: a `[u256; N]` local array stays rejected (its element
/// is outside the wasm scalar envelope), with a named backend error.
#[test]
fn local_u256_array_stays_fail_closed_on_wasm() {
    let source = r#"
pub fn probe256(k: u32) -> u256 {
    let mut a: [u256; 4] = [0; 4]
    a[0] = 7
    a[k as usize]
}
"#;
    let error = compile_to_wasm_err("wasm_local_u256_array.fe", source);
    assert!(
        error.contains("wasm target")
            && (error.contains("scalar") || error.contains("not lowered")),
        "u256-element array should fail closed with a named error; got: {error}"
    );
}

/// Fail-closed regression: an object-ref (array) in an EXPORT signature stays
/// rejected. Change 1 only maps object-ref LOCALS to an i32 pointer; it does not
/// widen `ty_for_class`, which remains the signature admissibility SSOT.
#[test]
fn object_ref_array_in_signature_stays_fail_closed_on_wasm() {
    let source = r#"
pub fn takes_array(a: [u32; 4]) -> u32 {
    a[0] + a[1]
}
"#;
    let error = compile_to_wasm_err("wasm_object_ref_signature.fe", source);
    assert!(
        error.contains("wasm target"),
        "an array in an export signature should fail closed; got: {error}"
    );
}

// ----------------------------------------------------------------------------
// Arrays rung STEP 1, Slice A: adversarial bounds-safety + ownership gates
// (Codex NO-GO follow-ups). High-bit / oversized / overflowing `usize` indexes
// must trap or fail closed, never alias an in-bounds element; whole-array copies
// and host-region/local-array mixes fail closed.
// ----------------------------------------------------------------------------

/// A high-bit `u32` index (`0x80000000`, `0xffffffff`) is a valid u32 but far out
/// of bounds. The UNSIGNED bounds check must trap, never alias `a[low bits]`
/// (a signed comparison would treat these as negative and mis-handle them).
#[test]
fn local_u32_array_high_bit_index_traps_not_aliases() {
    let source = r#"
pub fn probe(k: u32) -> u32 {
    let mut a: [u32; 8] = [0; 8]
    let mut i: u32 = 0
    while i < 8 {
        a[i as usize] = i * i
        i = i + 1
    }
    a[k as usize]
}
"#;
    let wasm = compile_to_wasm("wasm_array_high_bit_index.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let probe = instance
        .get_typed_func::<i32, i32>(&mut store, "probe")
        .expect("`probe` export should exist");

    // Sanity: an in-range index still works.
    assert_eq!(probe.call(&mut store, 3).unwrap(), 9);

    for bits in [0x8000_0000_u32, 0xffff_ffff_u32] {
        assert!(
            probe.call(&mut store, bits as i32).is_err(),
            "index {bits:#x} is out of bounds and must TRAP, not alias an in-bounds slot"
        );
    }
}

/// A direct `usize` constant above `u32::MAX` cannot be a wasm32 index; narrowing
/// it to i32 would truncate its high bits and (for a low-bits-small value) slip
/// past the bounds check. The narrowing pass refuses (fails open), so the body
/// falls back to the 256-bit fail-closed path rather than miscompiling.
#[test]
fn local_u32_array_oversized_usize_const_index_fails_closed() {
    let source = r#"
pub fn probe() -> u32 {
    let mut a: [u32; 2] = [7; 2]
    let idx: usize = 4294967296
    a[idx]
}
"#;
    let error = compile_to_wasm_err("wasm_array_oversized_const_index.fe", source);
    assert!(
        error.contains("wasm target") || error.contains("256") || error.contains("usize"),
        "an oversized (> u32::MAX) usize const index must fail closed; got: {error}"
    );
}

/// Checked `usize` arithmetic that overflows the wasm32 pointer width must TRAP
/// (Fe's checked-overflow panic), never wrap to a small in-bounds index. With the
/// default checked-arithmetic mode, `hi + 1` for `hi == u32::MAX` overflows. The
/// array is filled via a loop (the supported zero-init + element-write shape) so
/// `a[i] == i`, making a wrong wrap-to-0 observable as reading `a[0] == 0`.
#[test]
fn local_u32_array_checked_usize_overflow_traps_not_aliases() {
    let source = r#"
pub fn probe(k: u32) -> u32 {
    let mut a: [u32; 4] = [0; 4]
    let mut j: u32 = 0
    while j < 4 {
        a[j as usize] = j
        j = j + 1
    }
    let hi: usize = k as usize
    let idx: usize = hi + 1
    a[idx]
}
"#;
    let wasm = compile_to_wasm("wasm_array_checked_usize_overflow.fe", source);
    let (mut store, instance) = instantiate(&wasm);
    let probe = instance
        .get_typed_func::<i32, i32>(&mut store, "probe")
        .expect("`probe` export should exist");

    // In range: hi = 1 -> idx = 2 -> a[2] == 2.
    assert_eq!(probe.call(&mut store, 1).unwrap(), 2);

    // hi = u32::MAX -> hi + 1 overflows -> trap (a wrap-to-0 would read a[0] == 0
    // and return Ok(0), so `is_err()` distinguishes the correct trap).
    assert!(
        probe.call(&mut store, 0xffff_ffff_u32 as i32).is_err(),
        "checked usize overflow (u32::MAX + 1) must trap, not alias a[0]"
    );
}

/// A whole-array copy (`let b = a`) must NOT pointer-alias the backing arena: a
/// later `a[0] = 9` must not be observable through `b`. The array is seeded from a
/// runtime parameter so the copy is not constant-folded away and the real
/// object-ref `Use` path is exercised. This is a required portable-Wasm value
/// operation: compilation must succeed and the result must be `seed`, never the
/// `9` that a shallow pointer alias would observe.
#[test]
fn local_u32_array_copy_deep_copies() {
    let source = r#"
pub fn probe(seed: u32) -> u32 {
    let mut a: [u32; 2] = [seed; 2]
    let mut b: [u32; 2] = a
    a[0] = 9
    b[0]
}
"#;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///wasm_array_copy.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("portable Wasm must support independent complete-value array copies");
    let bytes = output
        .into_bytecode()
        .expect("wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("produced invalid wasm");
    let (mut store, instance) = instantiate(&bytes);
    let probe = instance
        .get_typed_func::<i32, i32>(&mut store, "probe")
        .expect("`probe` export should exist");
    let seed = 4;
    assert_eq!(
        probe.call(&mut store, seed).unwrap(),
        seed,
        "array copy must be a DEEP copy (b independent of a), never a pointer alias \
         (an alias would return 9 after `a[0] = 9`)"
    );
}

/// Ownership contract: a function that BOTH allocates a local array AND uses a
/// direct host memory region (`MemPtr`) could grow the array over the host
/// region (the arena bumps up from byte 1024, ignorant of host-chosen fixed
/// addresses). Until a disjoint partition exists, that mix fails closed.
#[test]
fn host_region_plus_local_array_fails_closed() {
    let source = r#"
use core::MemPtr

fn write_u32(_ value: u32) uses (target: mut u32) { target = value }

pub fn mixed(_ ptr: MemPtr<u32>, value: u32) -> u32 {
    let mut a: [u32; 4] = [0; 4]
    a[0] = value
    with (ptr) {
        write_u32(a[0])
    }
    a[1]
}
"#;
    let error = compile_to_wasm_err("wasm_host_region_plus_array.fe", source);
    assert!(
        error.contains("host memory region"),
        "a host-region + local-array mix must fail closed with the ownership error; got: {error}"
    );
}

// ============================================================================
// Arrays rung STEP 1, Slice B: THE DUAL GATE. A ROLLED (loop-form) BN254 Fr
// CIOS Montgomery multiply computed over FUNCTION-LOCAL `[u32; N]` limb arrays
// indexed by loop variables is compiled to wasm, executed under wasmtime, and
// asserted BIT-IDENTICAL, limb-for-limb, to the fully-unrolled oracle-validated
// `field_mul_bn254_fr` on the same in-range field elements, at O0 AND O2 (so the
// optimizer's load/store/GVN pipeline runs over the new Mload/Mstore array
// pattern inside the gate). The loop-form is also checked directly against an
// independent num-bigint Montgomery oracle. If the loop-form matches the
// unrolled kernel bit-for-bit, the Slice A wasm local-array lowering is proven
// correct on a real Montgomery multiply (the Poseidon/MSM inner kernel).
// ============================================================================

use num_bigint::BigUint;

const SLICE_B_LIMB_BITS: usize = 13;
const SLICE_B_N: usize = 20;
const SLICE_B_UNROLLED_SRC: &str = include_str!("fixtures/spirv/field_mul_bn254_fr.fe");
const SLICE_B_LOOP_SRC: &str = include_str!("fixtures/spirv/field_mul_bn254_fr_loop.fe");

/// BN254 (alt_bn128) scalar field order Fr, parsed from decimal (never trusted
/// from a limb table), so the oracle is anchored to the canonical curve constant.
fn slice_b_bn254_fr_prime() -> BigUint {
    BigUint::parse_bytes(
        b"21888242871839275222246405745257275088548364400416034343698204186575808495617",
        10,
    )
    .expect("BN254 Fr decimal should parse")
}

/// Decompose a field element into `n` little-endian 13-bit limbs (u32 words).
fn slice_b_to_limbs(x: &BigUint, n: usize) -> Vec<u32> {
    let mask = BigUint::from(8191u32);
    (0..n)
        .map(|j| {
            let limb = (x >> (SLICE_B_LIMB_BITS * j)) & &mask;
            limb.to_u32_digits().first().copied().unwrap_or(0)
        })
        .collect()
}

/// The INDEPENDENT bigint oracle: the CIOS Montgomery product a*b*R^-1 mod p,
/// computed with num-bigint (which knows nothing of 13-bit limbs or CIOS), then
/// decomposed into `n` limbs. R^-1 is R^(p-2) mod p (Fermat; p prime).
fn slice_b_mont_oracle(a: &BigUint, b: &BigUint, p: &BigUint, n: usize) -> Vec<u32> {
    let r = BigUint::from(1u32) << (SLICE_B_LIMB_BITS * n);
    let rinv = r.modpow(&(p - BigUint::from(2u32)), p);
    let mont = (((a * b) % p) * &rinv) % p;
    slice_b_to_limbs(&mont, n)
}

/// A deterministic pseudo-random field element (xorshift64, no rand dep).
fn slice_b_next_field(s: &mut u64, p: &BigUint) -> BigUint {
    let mut x = BigUint::from(0u32);
    for _ in 0..5 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        x = (x << 64) | BigUint::from(*s);
    }
    x % p
}

/// Compile Fe source to wasm bytes through the wasm backend at a chosen opt level.
fn slice_b_compile_at(name: &str, source: &str, opt: OptLevel) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), opt)
        .unwrap_or_else(|err| panic!("wasm compilation of `{name}` at {opt:?} failed: {err}"))
        .into_bytecode()
        .expect("wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("produced invalid wasm");
    bytes
}

/// Execute the field-mul over all `n` limb indices (arg0 = k = limb index) for a
/// single (a, b). The kernel takes `2 + 2n` args (past wasmtime's typed-tuple
/// arity), so the untyped `Func::call` path is used.
fn slice_b_field_mul_limbs(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    fn_name: &str,
    a_limbs: &[u32],
    b_limbs: &[u32],
    n: usize,
) -> Vec<u32> {
    use wasmtime::Val;
    let f = instance
        .get_func(&mut *store, fn_name)
        .unwrap_or_else(|| panic!("`{fn_name}` export should exist"));
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let mut params: Vec<Val> = Vec::with_capacity(2 + 2 * n);
        params.push(Val::I32(k as i32));
        params.push(Val::I32(0));
        for &l in a_limbs {
            params.push(Val::I32(l as i32));
        }
        for &l in b_limbs {
            params.push(Val::I32(l as i32));
        }
        let mut results = [Val::I32(0)];
        f.call(&mut *store, &params, &mut results)
            .unwrap_or_else(|e| panic!("{fn_name}(k={k}) should run: {e:?}"));
        out.push(match results[0] {
            Val::I32(v) => v as u32,
            other => panic!("{fn_name} result must be i32, got {other:?}"),
        });
    }
    out
}

/// THE SLICE B DUAL GATE: the rolled loop-form BN254 Fr field-mul, compiled to
/// wasm and executed under wasmtime, is BIT-IDENTICAL limb-for-limb to the
/// unrolled `field_mul_bn254_fr` (and to the num-bigint Montgomery oracle) on the
/// canonical edges (incl. 0, 1, p-1 so p-1 x p-1 is the top-carry case), the
/// dense all-limbs-saturated value, the Montgomery anchors R and R^2, and a few
/// hundred deterministic pseudo-random operand pairs, at O0 AND O2.
#[test]
fn loop_form_bn254_fr_field_mul_matches_unrolled_kernel_on_wasm_at_o0_and_o2() {
    let p = slice_b_bn254_fr_prime();
    let n = SLICE_B_N;
    let one = BigUint::from(1u32);
    let two = BigUint::from(2u32);

    // Canonical edges. p-1 x p-1 (the maximal top-carry case) and 0 / 1 fall out
    // of the edge x edge cross product below.
    let mut edges: Vec<(String, BigUint)> = vec![
        ("0".into(), BigUint::from(0u32)),
        ("1".into(), one.clone()),
        ("2".into(), two.clone()),
        ("p-1".into(), &p - &one),
        ("p-2".into(), &p - &two),
        ("(p-1)/2".into(), (&p - &one) / &two),
    ];
    let mut dense = BigUint::from(0u32);
    for j in 0..n {
        dense |= BigUint::from(8191u32) << (SLICE_B_LIMB_BITS * j);
    }
    edges.push(("dense".into(), &dense % &p));
    let r = BigUint::from(1u32) << (SLICE_B_LIMB_BITS * n);
    edges.push(("R".into(), &r % &p));
    edges.push(("R^2".into(), (&r * &r) % &p));

    // Full product list: edge x edge (81 pairs) plus 256 pseudo-random pairs.
    let mut products: Vec<(String, BigUint, BigUint)> = Vec::new();
    for (na, a) in &edges {
        for (nb, b) in &edges {
            products.push((format!("{na} x {nb}"), a.clone(), b.clone()));
        }
    }
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    for idx in 0..256 {
        let a = slice_b_next_field(&mut seed, &p);
        let b = slice_b_next_field(&mut seed, &p);
        products.push((format!("rand{idx}"), a, b));
    }

    // Precompute limb decompositions + the bigint oracle once per product.
    let cases: Vec<(String, Vec<u32>, Vec<u32>, Vec<u32>)> = products
        .iter()
        .map(|(name, a, b)| {
            (
                name.clone(),
                slice_b_to_limbs(a, n),
                slice_b_to_limbs(b, n),
                slice_b_mont_oracle(a, b, &p, n),
            )
        })
        .collect();

    // Run the gate at BOTH opt levels: O0 (raw Mload/Mstore) and O2 (the sonatina
    // speed pipeline: inlining, GVN, load/store forwarding over the array memory).
    for opt in [OptLevel::O0, OptLevel::O2] {
        let loop_wasm = slice_b_compile_at("field_mul_bn254_fr_loop.fe", SLICE_B_LOOP_SRC, opt);
        let unrolled_wasm = slice_b_compile_at("field_mul_bn254_fr.fe", SLICE_B_UNROLLED_SRC, opt);

        // The loop kernel's limb arrays live in the synthesized canonical arena,
        // so the module must stay self-contained (no host imports to instantiate).
        assert!(
            func_imports(&loop_wasm).is_empty(),
            "[{opt:?}] the loop-form field-mul should need no host imports; got {:?}",
            func_imports(&loop_wasm)
        );

        let (mut loop_store, loop_instance) = instantiate(&loop_wasm);
        let (mut unrolled_store, unrolled_instance) = instantiate(&unrolled_wasm);

        for (name, al, bl, oracle) in &cases {
            let got_loop = slice_b_field_mul_limbs(
                &mut loop_store,
                &loop_instance,
                "field_mul_bn254_fr_loop",
                al,
                bl,
                n,
            );
            let got_unrolled = slice_b_field_mul_limbs(
                &mut unrolled_store,
                &unrolled_instance,
                "field_mul_bn254_fr",
                al,
                bl,
                n,
            );

            // THE DUAL GATE: the rolled loop-form is bit-identical to the
            // unrolled kernel, limb for limb.
            assert_eq!(
                got_loop, got_unrolled,
                "[{opt:?}] loop-form field_mul({name}) must be BIT-IDENTICAL to the unrolled \
                 kernel, limb for limb"
            );
            // Independent oracle on the loop-form directly.
            assert_eq!(
                &got_loop, oracle,
                "[{opt:?}] loop-form field_mul({name}) must equal the num-bigint Montgomery \
                 oracle a*b*R^-1 mod p"
            );
            // The unrolled twin also equals the oracle (anchors the comparison).
            assert_eq!(
                &got_unrolled, oracle,
                "[{opt:?}] unrolled field_mul({name}) must equal the num-bigint oracle"
            );
        }
        eprintln!(
            "  Slice B dual gate [{opt:?}]: rolled loop-form BN254 Fr field-mul == unrolled kernel \
             == num-bigint Montgomery oracle, limb-for-limb, over {} operand products (incl \
             p-1 x p-1 top-carry, dense-limb, R, R^2).",
            cases.len()
        );
    }
}

// ============================================================================
// Arrays rung STEP 1, Slice C: THE POSEIDON DUAL GATE. A ROLLED (loop-form)
// BN254 Fr circomlib Poseidon t=3 `hash2(left, right)`, computed over
// FUNCTION-LOCAL `[u32; N]` 13-bit x 20-limb Montgomery arrays (the CIOS montmul
// from Slice B, plus limbwise field-add + conditional subtract, an x^5 montmul
// chain, a rolled 3x3 MDS matmul, and a rolled 8-full/57-partial round schedule
// indexing a local Montgomery constant table), is compiled to wasm, executed
// under wasmtime, its 20 output limbs reassembled to a field element, and
// asserted bit-exact to BOTH (i) the checked-in circomlib vectors (which the
// u256 `const_poseidon.fe` form is static_assert-pinned to) AND (ii) an
// independent num-bigint PLAIN-FIELD Poseidon oracle driven by the SAME
// MDS / round constants parsed straight out of `const_poseidon.fe` (the u256
// form's own source of truth, using ordinary modular reduction, not CIOS/limbs).
// Run at O0 AND O2. If the loop-form matches on every vector, the loop-form limb
// Poseidon prover kernel is proven == circomlib == the u256 form on wasm.
// ============================================================================

const SLICE_C_POSEIDON_SRC: &str = include_str!("fixtures/spirv/poseidon_bn254_loop.fe");
const CONST_POSEIDON_SRC: &str = include_str!("../../fe/tests/fixtures/fe_test/const_poseidon.fe");

/// Extract the `0x..` field-element literals of a named `const` array block out of
/// `const_poseidon.fe` (bracket-matched from the block's opening `[`), so the
/// oracle's MDS / round constants come from the u256 form's OWN source, never a
/// re-transcription. Used only on the t=3 blocks (which precede the t=6 ones).
fn slice_c_parse_const_block(src: &str, name: &str) -> Vec<BigUint> {
    let bytes = src.as_bytes();
    let i = src.find(name).expect("named const block should exist");
    let j = i + src[i..].find('=').expect("block should have `=`");
    let k = j + src[j..].find('[').expect("block should have opening `[`");
    let mut depth = 0usize;
    let mut end = k;
    for (m, &b) in bytes.iter().enumerate().skip(k) {
        match b {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    end = m;
                    break;
                }
            }
            _ => {}
        }
    }
    let block = &src[k..=end];
    let mut out = Vec::new();
    let mut idx = 0usize;
    while let Some(p) = block[idx..].find("0x") {
        let s = idx + p + 2;
        let mut e = s;
        while e < block.len() && block.as_bytes()[e].is_ascii_hexdigit() {
            e += 1;
        }
        out.push(BigUint::parse_bytes(block[s..e].as_bytes(), 16).expect("hex limb"));
        idx = e;
    }
    out
}

/// The INDEPENDENT oracle: circomlib Poseidon t=3 in PLAIN modular arithmetic
/// (8 full + 57 partial rounds, x^5 S-box, 3x3 MDS), structurally unlike the
/// Montgomery-limb kernel. `mds` is 9 row-major entries; `rc` is 195
/// (65 rounds x 3). Mirrors `const_poseidon.fe`'s `permute3` / `hash2`.
fn slice_c_poseidon_oracle(
    left: &BigUint,
    right: &BigUint,
    p: &BigUint,
    mds: &[BigUint],
    rc: &[BigUint],
) -> BigUint {
    const FULL: usize = 8;
    const PART: usize = 57;
    const HALF: usize = FULL / 2;
    let pow5 = |x: &BigUint| -> BigUint {
        let x2 = (x * x) % p;
        let x4 = (&x2 * &x2) % p;
        (&x4 * x) % p
    };
    let zero = BigUint::from(0u32);
    let mut st = [zero.clone(), left % p, right % p];
    for r in 0..(FULL + PART) {
        for i in 0..3 {
            st[i] = (&st[i] + &rc[r * 3 + i]) % p;
        }
        let is_full = r < HALF || r >= HALF + PART;
        if is_full {
            for i in 0..3 {
                st[i] = pow5(&st[i]);
            }
        } else {
            st[0] = pow5(&st[0]);
        }
        let mut new = [zero.clone(), zero.clone(), zero.clone()];
        for row in 0..3 {
            let mut acc = zero.clone();
            for col in 0..3 {
                acc = (acc + &st[col] * &mds[row * 3 + col]) % p;
            }
            new[row] = acc;
        }
        st = new;
    }
    st[0].clone()
}

/// Reassemble little-endian 13-bit limbs (u32 words) into a field element.
fn slice_c_limbs_to_biguint(limbs: &[u32]) -> BigUint {
    let mut acc = BigUint::from(0u32);
    for (j, &l) in limbs.iter().enumerate() {
        acc |= BigUint::from(l) << (SLICE_B_LIMB_BITS * j);
    }
    acc
}

/// THE SLICE C POSEIDON DUAL GATE: the rolled loop-form limb Poseidon `hash2`,
/// compiled to wasm and run under wasmtime, equals the circomlib vectors AND the
/// plain-field u256-equivalent oracle, bit-exact, on a spread of operand pairs
/// (incl. the two circomlib pins, `p-1 x p-2`, and wide operands), at O0 AND O2.
///
/// The generated kernel is a single ~2.8k-statement function with sizeable local
/// constant tables; sonatina's straight-line lowering / opt passes recurse deeply
/// enough over it to exhaust a default libtest thread stack (SIGABRT stack
/// overflow under the plain CI invocation, which sets no `RUST_MIN_STACK`), so the
/// gate body runs on an explicitly wide-stacked worker thread. The kernel itself
/// is loop-only (no recursion); this is a compiler-stack accommodation for a large
/// generated function, not a change to what the kernel computes.
#[test]
fn loop_form_bn254_poseidon_hash2_matches_circomlib_and_u256_form_on_wasm_at_o0_and_o2() {
    std::thread::Builder::new()
        .stack_size(1 << 31)
        .spawn(slice_c_poseidon_dual_gate_body)
        .expect("spawn wide-stack worker for the Poseidon gate")
        .join()
        .expect("Poseidon dual-gate worker thread should not panic");
}

fn slice_c_poseidon_dual_gate_body() {
    let p = slice_b_bn254_fr_prime();
    let n = SLICE_B_N;
    let mds = slice_c_parse_const_block(CONST_POSEIDON_SRC, "POSEIDON_T3_MDS");
    let rc = slice_c_parse_const_block(CONST_POSEIDON_SRC, "POSEIDON_T3_ROUND_CONSTANTS");
    assert_eq!(mds.len(), 9, "t=3 MDS should be 3x3");
    assert_eq!(rc.len(), 195, "t=3 round constants should be 65x3");

    // The checked-in circomlib vectors: exactly the values the u256
    // `const_poseidon.fe` form is static_assert-pinned to (its `hash2(0,0)` /
    // `hash2(1,2)`), so matching these == matching the u256 form on those inputs.
    let pin_00 = BigUint::parse_bytes(
        b"2098f5fb9e239eab3ceac3f27b81e481dc3124d55ffed523a839ee8446b64864",
        16,
    )
    .unwrap();
    let pin_12 = BigUint::parse_bytes(
        b"115cc0f5e7d690413df64c6b9662e9cf2a3617f2743245519e19607a4417189a",
        16,
    )
    .unwrap();
    // Anchor the independent oracle to circomlib: the plain-field recomputation
    // reproduces the u256 form's pinned outputs, so it faithfully stands in for
    // "the u256 form" on the non-pinned vectors below.
    assert_eq!(
        slice_c_poseidon_oracle(&0u32.into(), &0u32.into(), &p, &mds, &rc),
        pin_00,
        "plain-field oracle must reproduce circomlib hash2(0,0)"
    );
    assert_eq!(
        slice_c_poseidon_oracle(&1u32.into(), &2u32.into(), &p, &mds, &rc),
        pin_12,
        "plain-field oracle must reproduce circomlib hash2(1,2)"
    );

    let one = BigUint::from(1u32);
    let two = BigUint::from(2u32);
    let vectors: Vec<(String, BigUint, BigUint)> = vec![
        ("hash2(0,0)".into(), 0u32.into(), 0u32.into()),
        ("hash2(1,2)".into(), 1u32.into(), 2u32.into()),
        ("hash2(3,4)".into(), 3u32.into(), 4u32.into()),
        ("hash2(7,7)".into(), 7u32.into(), 7u32.into()),
        ("hash2(p-1,p-2)".into(), &p - &one, &p - &two),
        (
            "hash2(12345,678910)".into(),
            12345u32.into(),
            678910u32.into(),
        ),
    ];

    for opt in [OptLevel::O0, OptLevel::O2] {
        let wasm = slice_b_compile_at("poseidon_bn254_loop.fe", SLICE_C_POSEIDON_SRC, opt);

        // The kernel's limb arrays live in the synthesized canonical arena, so the
        // module must stay self-contained (no host imports needed to instantiate).
        assert!(
            func_imports(&wasm).is_empty(),
            "[{opt:?}] the loop-form Poseidon should need no host imports; got {:?}",
            func_imports(&wasm)
        );

        let (mut store, instance) = instantiate(&wasm);

        for (name, left, right) in &vectors {
            let left_limbs = slice_b_to_limbs(left, n);
            let right_limbs = slice_b_to_limbs(right, n);
            // Reuse the Slice B limb driver: arg0 = k = output limb index, arg1 = 0,
            // then the 20 left + 20 right PLAIN input limbs.
            let got_limbs = slice_b_field_mul_limbs(
                &mut store,
                &instance,
                "poseidon_bn254_loop",
                &left_limbs,
                &right_limbs,
                n,
            );
            let got = slice_c_limbs_to_biguint(&got_limbs);
            let oracle = slice_c_poseidon_oracle(left, right, &p, &mds, &rc);

            // THE DUAL GATE, leg (ii): loop-form == the plain-field u256-equivalent
            // oracle (same MDS/round constants, plain modular arithmetic).
            assert_eq!(
                got, oracle,
                "[{opt:?}] loop-form Poseidon {name} must equal the plain-field u256-form oracle"
            );
        }

        // THE DUAL GATE, leg (i): the loop-form reproduces the checked-in circomlib
        // pins exactly (redundant with the anchored oracle above, but asserts the
        // literal published vectors directly against the wasm output).
        let h00 = slice_c_limbs_to_biguint(&slice_b_field_mul_limbs(
            &mut store,
            &instance,
            "poseidon_bn254_loop",
            &slice_b_to_limbs(&0u32.into(), n),
            &slice_b_to_limbs(&0u32.into(), n),
            n,
        ));
        let h12 = slice_c_limbs_to_biguint(&slice_b_field_mul_limbs(
            &mut store,
            &instance,
            "poseidon_bn254_loop",
            &slice_b_to_limbs(&1u32.into(), n),
            &slice_b_to_limbs(&2u32.into(), n),
            n,
        ));
        assert_eq!(
            h00, pin_00,
            "[{opt:?}] loop-form hash2(0,0) must equal the circomlib vector"
        );
        assert_eq!(
            h12, pin_12,
            "[{opt:?}] loop-form hash2(1,2) must equal the circomlib vector"
        );

        eprintln!(
            "  Slice C Poseidon dual gate [{opt:?}]: rolled loop-form limb Poseidon hash2 == \
             circomlib vectors == plain-field u256-form oracle, bit-exact, over {} vectors \
             (incl the two circomlib pins + p-1 x p-2).",
            vectors.len()
        );
    }
}

// ============================================================================
// Arrays rung STEP 1, Slice D: THE SERIAL POSEIDON-MERKLE ROOT BUILDER (the
// ungated wasm prover leg of Rollcall). A function-local `[u32; N*20]` tree
// buffer plus the PROVEN loop-form limb Poseidon `hash2` permutation (Slice C,
// reused VERBATIM as the node-combine primitive) builds a Poseidon-Merkle root
// over N leaves by hashing (even,odd) child pairs level-by-level, collapsing the
// levels IN PLACE up the local buffer, on wasm under wasmtime. The reassembled
// root is asserted bit-exact to an INDEPENDENT num-bigint Poseidon-Merkle-tree
// oracle (the same plain-field circomlib `hash2` as the Slice C oracle, same
// MDS / round constants parsed straight out of `const_poseidon.fe`), over random
// and edge leaf sets, at O0 AND O2, for depth 2 (N=4) and depth 3 (N=8). The
// pairing convention (parent = hash2(even child, odd child)) is EXACTLY the
// RollcallRegistry / `rollcall_e2e::build_tree` convention, so a root built here
// and a sibling path derived from it verify on-chain (see `rollcall_e2e`).
// ============================================================================

const SLICE_D_MERKLE4_SRC: &str = include_str!("fixtures/spirv/poseidon_merkle_root_loop.fe");
const SLICE_D_MERKLE8_SRC: &str = include_str!("fixtures/spirv/poseidon_merkle8_root_loop.fe");

/// INDEPENDENT oracle: build a Poseidon-Merkle root over `leaves` bottom-up with
/// the plain-field circomlib `hash2` (`slice_c_poseidon_oracle`), pairing
/// (even, odd) children -- the exact RollcallRegistry / `build_tree` convention.
fn slice_d_merkle_root_oracle(
    leaves: &[BigUint],
    p: &BigUint,
    mds: &[BigUint],
    rc: &[BigUint],
) -> BigUint {
    assert!(
        leaves.len().is_power_of_two() && leaves.len() >= 2,
        "leaf count must be a power of two >= 2"
    );
    let mut level: Vec<BigUint> = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len() / 2);
        for pair in level.chunks(2) {
            next.push(slice_c_poseidon_oracle(&pair[0], &pair[1], p, mds, rc));
        }
        level = next;
    }
    level.into_iter().next().expect("one root remains")
}

/// Run the wasm merkle-root builder over all `n` output limb indices (arg0 = k),
/// passing the N leaves' PLAIN limbs (leaf j at args `[1 + j*n ..]`) as the rest.
/// Returns the root's `n` limbs. Untyped `Func::call` path (arity past 16).
fn slice_d_merkle_root_limbs(
    store: &mut wasmtime::Store<()>,
    instance: &wasmtime::Instance,
    fn_name: &str,
    leaf_limbs: &[Vec<u32>],
    n: usize,
) -> Vec<u32> {
    use wasmtime::Val;
    let f = instance
        .get_func(&mut *store, fn_name)
        .unwrap_or_else(|| panic!("`{fn_name}` export should exist"));
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let mut params: Vec<Val> = Vec::with_capacity(1 + leaf_limbs.len() * n);
        params.push(Val::I32(k as i32));
        for leaf in leaf_limbs {
            for &l in leaf {
                params.push(Val::I32(l as i32));
            }
        }
        let mut results = [Val::I32(0)];
        f.call(&mut *store, &params, &mut results)
            .unwrap_or_else(|e| panic!("{fn_name}(k={k}) should run: {e:?}"));
        out.push(match results[0] {
            Val::I32(v) => v as u32,
            other => panic!("{fn_name} result must be i32, got {other:?}"),
        });
    }
    out
}

/// THE SLICE D MERKLE GATE: the serial loop-form Poseidon-Merkle root builder,
/// compiled to wasm and run under wasmtime, equals the independent num-bigint
/// Poseidon-Merkle-tree oracle bit-exact, over edge + pseudo-random leaf sets at
/// depth 2 (N=4) and depth 3 (N=8), at O0 AND O2. Runs on a wide-stack worker
/// thread for the same reason as the Slice C Poseidon gate (large generated
/// function; a compiler-stack accommodation, not a change to what it computes).
#[test]
fn serial_poseidon_merkle_root_matches_bigint_tree_oracle_on_wasm_at_o0_and_o2() {
    std::thread::Builder::new()
        .stack_size(1 << 31)
        .spawn(slice_d_merkle_gate_body)
        .expect("spawn wide-stack worker for the Merkle gate")
        .join()
        .expect("Merkle gate worker thread should not panic");
}

fn slice_d_merkle_gate_body() {
    let p = slice_b_bn254_fr_prime();
    let n = SLICE_B_N;
    let mds = slice_c_parse_const_block(CONST_POSEIDON_SRC, "POSEIDON_T3_MDS");
    let rc = slice_c_parse_const_block(CONST_POSEIDON_SRC, "POSEIDON_T3_ROUND_CONSTANTS");
    assert_eq!(mds.len(), 9, "t=3 MDS should be 3x3");
    assert_eq!(rc.len(), 195, "t=3 round constants should be 65x3");

    // Anchor the tree oracle's hash2 to circomlib (same pin as Slice C), so a
    // matching Merkle root means matching the u256 form's hash2 at every node.
    let pin_00 = BigUint::parse_bytes(
        b"2098f5fb9e239eab3ceac3f27b81e481dc3124d55ffed523a839ee8446b64864",
        16,
    )
    .unwrap();
    assert_eq!(
        slice_c_poseidon_oracle(&0u32.into(), &0u32.into(), &p, &mds, &rc),
        pin_00,
        "the tree oracle's node hash2 must reproduce circomlib hash2(0,0)"
    );

    let one = BigUint::from(1u32);
    let two = BigUint::from(2u32);

    // Deterministic pseudo-random leaf sets + edge sets, per depth.
    let mut seed: u64 = 0xD1B5_4A32_D192_ED03;
    let mut make_random_set = |count: usize| -> Vec<BigUint> {
        (0..count)
            .map(|_| slice_b_next_field(&mut seed, &p))
            .collect()
    };

    // depth 2 (N=4): edge set (0, 1, p-1, p-2), ascending, + 2 random sets.
    let sets4: Vec<(String, Vec<BigUint>)> = vec![
        (
            "edge4".into(),
            vec![0u32.into(), one.clone(), &p - &one, &p - &two],
        ),
        (
            "asc4".into(),
            vec![11u32.into(), 22u32.into(), 33u32.into(), 44u32.into()],
        ),
        ("rand4a".into(), make_random_set(4)),
        ("rand4b".into(), make_random_set(4)),
    ];
    // depth 3 (N=8): ascending + 1 random set.
    let sets8: Vec<(String, Vec<BigUint>)> = vec![
        ("asc8".into(), (1u32..=8u32).map(BigUint::from).collect()),
        ("rand8".into(), make_random_set(8)),
    ];

    // The build jobs: (fixture file, export fn name, source, opt, leaf sets).
    // N=4 (depth 2) is the primary deliverable, gated at O0 AND O2 (the O2 speed
    // pipeline runs GVN / load-store forwarding over the new tree-buffer memory).
    // N=8 (depth 3) is extra evidence that the level loop runs deeper (an added
    // intermediate collapse); it is gated at O0 (recompiling the larger function
    // at O2 is redundant for the depth claim and each compile of this ~2.9k-stmt
    // function is minutes-costly). Each generated function is loop-only.
    let jobs: Vec<(&str, &str, &str, OptLevel, &Vec<(String, Vec<BigUint>)>)> = vec![
        (
            "poseidon_merkle_root_loop.fe",
            "poseidon_merkle_root_loop",
            SLICE_D_MERKLE4_SRC,
            OptLevel::O0,
            &sets4,
        ),
        (
            "poseidon_merkle_root_loop.fe",
            "poseidon_merkle_root_loop",
            SLICE_D_MERKLE4_SRC,
            OptLevel::O2,
            &sets4,
        ),
        (
            "poseidon_merkle8_root_loop.fe",
            "poseidon_merkle8_root_loop",
            SLICE_D_MERKLE8_SRC,
            OptLevel::O0,
            &sets8,
        ),
    ];

    for (file, export, src, opt, src_sets) in jobs {
        let wasm = slice_b_compile_at(file, src, opt);
        // Self-contained: the tree buffer lives in the synthesized canonical arena.
        assert!(
            func_imports(&wasm).is_empty(),
            "[{opt:?}] the {export} Merkle builder should need no host imports; got {:?}",
            func_imports(&wasm)
        );

        let (mut store, instance) = instantiate(&wasm);
        for (name, leaves) in src_sets.iter() {
            let leaf_limbs: Vec<Vec<u32>> = leaves.iter().map(|x| slice_b_to_limbs(x, n)).collect();
            let got_limbs =
                slice_d_merkle_root_limbs(&mut store, &instance, export, &leaf_limbs, n);
            let got = slice_c_limbs_to_biguint(&got_limbs);
            let oracle = slice_d_merkle_root_oracle(leaves, &p, &mds, &rc);
            assert_eq!(
                got, oracle,
                "[{opt:?}] serial wasm Poseidon-Merkle root for {export}({name}) must equal \
                 the independent num-bigint Poseidon-Merkle-tree oracle"
            );
        }
        eprintln!(
            "  Slice D Merkle gate [{opt:?}]: serial wasm {export} root == num-bigint \
             Poseidon-Merkle-tree oracle, bit-exact, over {} leaf sets.",
            src_sets.len()
        );
    }
}
