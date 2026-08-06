//! Slice 1 of the float-semantics type API
//! (`/workspace/mb2/FLOAT_SEMANTICS_TYPE_API_DESIGN.md`): THE POINT, proved
//! through the REAL Fe -> MIR -> Sonatina -> naga/SPIR-V pipeline (not just a
//! hand-built Sonatina IR module, see sonatina's own
//! `spirv_f32_min_relaxed_is_single_op_exact_min_is_not`).
//!
//! `Regular::assume(x).min(Regular::assume(y))` must lower to `FminRelaxed`,
//! which naga/SPIR-V emits as a SINGLE native `min(...)` call
//! (`MathFunction::Min`). Plain `f32` `x.min(y)` must keep paying the
//! pinned-exact ~15-20-op branch-free integer expansion. Both kernels are
//! call-free after `inline_spirv_calls` (the `Regular` wrapper -
//! `assume`/`get`/the `MinMax`/`Abs` trait dispatch - is zero-cost through
//! codegen, so it must vanish entirely, leaving nothing but the chosen
//! min/max op).

use common::InputDb;
use driver::DriverDataBase;
use url::Url;

const SPIRV_MAGIC: u32 = 0x0723_0203;

fn compile_source_to_wgsl(name: &str, source: &str) -> String {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "{name}: source diagnostics prevent compilation:\n{diagnostics}"
    );

    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .unwrap_or_else(|err| panic!("{name} should build a wasm runtime package: {err}"));
    let artifact = fe_codegen::compile_runtime_package_spirv(&db, &package)
        .unwrap_or_else(|err| panic!("{name} should compile to naga-validated SPIR-V: {err}"));
    assert_eq!(
        artifact.words[0], SPIRV_MAGIC,
        "{name}: words[0] must be the SPIR-V magic"
    );
    artifact
        .wgsl
        .unwrap_or_else(|| panic!("{name}: the naga backend should emit a WGSL side artifact"))
}

/// Plain `f32`, exact: `a.min(b)` must lower through `MinMax for f32` to
/// sonatina `Fmin`, which naga/SPIR-V lowers to the ~15-20-op branch-free
/// integer key-compare-and-select expansion. No native `min(` call.
///
/// The SPIR-V "kernel" ABI only accepts an i32 (u32 word) or i64 return
/// value (`SpirvBackend::compile_module`'s own envelope, same constraint
/// `spirv_e2e.rs`'s keystone tests document), so args/result are threaded
/// through `__f32_from_i32`/`__i32_from_f32` at the boundary -- the same
/// I32ToF32/F32ToI32 ABI-conversion trick the sonatina-level
/// `spirv_f32_min_relaxed_is_single_op_exact_min_is_not` test uses.
#[test]
fn f32_min_lowers_to_exact_expansion_no_native_min_call() {
    let source = "\
use core::ops::MinMax\n\
\n\
extern {\n\
\x20   fn __f32_from_i32(_: i32) -> f32\n\
\x20   fn __i32_from_f32(_: f32) -> i32\n\
}\n\
\n\
pub fn shade_min_exact(a: i32, b: i32) -> i32 {\n\
\x20   __i32_from_f32(__f32_from_i32(a).min(__f32_from_i32(b)))\n\
}\n";
    let wgsl = compile_source_to_wgsl("f32_min_exact.fe", source);
    assert!(
        !wgsl.contains("min("),
        "exact f32::min must NOT lower to a native `min(` call; got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("select("),
        "exact f32::min must lower to the branch-free select-based expansion; got:\n{wgsl}"
    );
    assert!(
        !wgsl.contains("if ") && !wgsl.contains("else") && !wgsl.contains("loop {"),
        "exact f32::min must stay branch-free (no if/else/loop); got:\n{wgsl}"
    );
    eprintln!("=== EXACT f32::min WGSL ===\n{wgsl}");
}

/// `Regular::assume(x).min(Regular::assume(y))`: THE POINT. Must lower
/// through `MinMax for Regular` to sonatina `FminRelaxed`, which naga/
/// SPIR-V lowers to a SINGLE native `min(...)` call (`MathFunction::Min`).
/// `Regular::assume`/`.get()` must vanish entirely (zero-cost newtype +
/// `inline_spirv_calls`), so this WGSL should be markedly shorter than the
/// exact kernel above and contain no `select(` from the min itself.
#[test]
fn regular_min_lowers_to_single_native_min_call() {
    let source = "\
use core::num::Regular\n\
use core::ops::MinMax\n\
\n\
extern {\n\
\x20   fn __f32_from_i32(_: i32) -> f32\n\
\x20   fn __i32_from_f32(_: f32) -> i32\n\
}\n\
\n\
pub fn shade_min_relaxed(a: i32, b: i32) -> i32 {\n\
\x20   let x: Regular = Regular::assume(__f32_from_i32(a))\n\
\x20   let y: Regular = Regular::assume(__f32_from_i32(b))\n\
\x20   __i32_from_f32(x.min(y).get())\n\
}\n";
    let wgsl = compile_source_to_wgsl("regular_min_relaxed.fe", source);
    assert!(
        wgsl.contains("min("),
        "Regular::min must lower to a native `min(` call; got:\n{wgsl}"
    );
    assert!(
        !wgsl.contains("if ") && !wgsl.contains("else") && !wgsl.contains("loop {"),
        "relaxed Regular::min must stay branch-free (no if/else/loop); got:\n{wgsl}"
    );
    eprintln!("=== RELAXED Regular::min WGSL ===\n{wgsl}");
}

/// SAFE-BY-DEFAULT: a naive `min(a, b)`/`max(a, b)`/`clamp(x, lo, hi)` on
/// plain `f32` is reachable ONLY through the exact path. Single entry
/// function (min+max+clamp combined into one f32 accumulator, mirroring
/// sonatina's own `spirv_f32_minmaxabsclamp_lowering_is_exact_and_branch_free`
/// shape) so there is no ambiguity about which function is "the kernel";
/// constructed the same way an attestation author would (no `Regular`/
/// `assume` anywhere in the source), making explicit that relaxed semantics
/// are unreachable without deliberately importing and constructing
/// `Regular`.
#[test]
fn naive_f32_min_is_never_silently_relaxed() {
    let source = "\
use core::ops::MinMax\n\
\n\
extern {\n\
\x20   fn __f32_from_i32(_: i32) -> f32\n\
\x20   fn __i32_from_f32(_: f32) -> i32\n\
}\n\
\n\
pub fn naive_minmaxclamp(ai: i32, bi: i32) -> i32 {\n\
\x20   let a: f32 = __f32_from_i32(ai)\n\
\x20   let b: f32 = __f32_from_i32(bi)\n\
\x20   let lo: f32 = a.min(b)\n\
\x20   let hi: f32 = a.max(b)\n\
\x20   let mid: f32 = a.clamp(lo, hi)\n\
\x20   __i32_from_f32(lo + hi + mid)\n\
}\n";
    let wgsl = compile_source_to_wgsl("naive_f32_minmaxclamp.fe", source);
    assert!(
        !wgsl.contains("min(") && !wgsl.contains("max("),
        "a naive min/max/clamp on plain f32 must never emit a native min(/max( call \
         (that would mean silent relaxation); got:\n{wgsl}"
    );
    assert!(
        wgsl.contains("select("),
        "naive f32 min/max/clamp must still lower to the exact select-based expansion; got:\n{wgsl}"
    );
}
