use fe_hir::test_db::HirAnalysisTestDb;

#[test]
fn reflected_const_candidate_can_narrow_into_an_exact_i32_term() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "reflected_const_candidate_i32.fe".into(),
        r#"
struct Term<const Candidate: usize> {}
struct ObservedTerm<const Candidate: i32> {}
trait Observe { type Out }
impl<const Candidate: usize> Observe for Term<Candidate> {
    type Out = ObservedTerm<{Candidate.downcast_truncate()}>
}
type Closed = <Term<49> as Observe>::Out
fn expects_exact(_ value: ObservedTerm<49>) {}
fn proves_exact(value: Closed) { expects_exact(value) }
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn nested_associated_type_normalizes_multiple_helper_computed_const_payloads() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "reflected_const_candidate_repeated_i32.fe".into(),
        r#"
struct Term<const Candidate: usize> {}
struct Zero {}
struct Add<L, R> {}
struct ObservedZero {}
struct ObservedTerm<
    const Candidate: i32,
    const Left: i32,
    const Point: i32,
    const Output: i32,
    const Magnitude: i32,
    const Negative: i32,
> {}
struct ObservedAdd<L, R> {}
const fn candidate_left(_ candidate: usize) -> i32 { 1 }
const fn candidate_point(_ candidate: usize) -> i32 { 2 }
const fn candidate_output(_ candidate: usize) -> i32 {
    (candidate + 1).downcast_truncate()
}
const fn candidate_magnitude(_ candidate: usize) -> i32 { 2 }
const fn candidate_negative(_ candidate: usize) -> i32 {
    (candidate & 1).downcast_truncate()
}
trait Observe { type Out }
impl<const Candidate: usize> Observe for Term<Candidate> {
    type Out = ObservedTerm<
        {Candidate.downcast_truncate()},
        {candidate_left(Candidate)},
        {candidate_point(Candidate)},
        {candidate_output(Candidate)},
        {candidate_magnitude(Candidate)},
        {candidate_negative(Candidate)},
    >
}
impl Observe for Zero { type Out = ObservedZero }
impl<L: Observe, R: Observe> Observe for Add<L, R> {
    // Keep the projected first argument visually separate from the generic
    // opener: `ObservedAdd<<L ...` is the shift token, not two `<` tokens.
    type Out = ObservedAdd<
        <L as Observe>::Out,
        <R as Observe>::Out,
    >
}
trait Eval { fn eval() -> i32 }
impl Eval for ObservedZero { fn eval() -> i32 { 0 } }
impl<
    const Candidate: i32,
    const Left: i32,
    const Point: i32,
    const Output: i32,
    const Magnitude: i32,
    const Negative: i32,
> Eval for ObservedTerm<Candidate, Left, Point, Output, Magnitude, Negative> {
    fn eval() -> i32 {
        Candidate + Left + Point + Output + Magnitude + Negative
    }
}
impl<L: Eval, R: Eval> Eval for ObservedAdd<L, R> {
    fn eval() -> i32 { <L as Eval>::eval() + <R as Eval>::eval() }
}
type ObservedPlan = <Add<Term<7>, Zero> as Observe>::Out
fn expects_exact(
    _ value: ObservedAdd<ObservedTerm<7, 1, 2, 8, 2, 1>, ObservedZero>,
) {}
fn proves_payloads(value: ObservedPlan) { expects_exact(value) }
fn proves_exact() -> i32 { <ObservedPlan as Eval>::eval() }
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn normalized_ground_type_inspection_rejects_type_arg_for_const_param() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_ground_type_arg_kind_mismatch.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Inspect {}
struct Zero {}
struct ConstOnly<const N: usize> {}
struct Target {}
struct Inspector {}
impl Derive<Inspect> for Inspector {
    const fn derive<T>(ev: own Evidence<Inspect<T>>) -> Evidence<Inspect<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Inspect<T>>)
    {
        for _ in builder.ty<ConstOnly<Zero>>().normalized_preorder_types() {}
        builder.finish()
        ev
    }
}
derive Inspect for Target using Inspector
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod);
    assert!(
        !diagnostics.is_empty(),
        "a type argument must not be accepted for a nominal const parameter"
    );
}

#[test]
fn imported_sparse_plan_reflects_exact_thirty_two_survivors() {
    use common::InputDb;
    use driver::DriverDataBase;
    use fe_hir::hir_def::HirIngot;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen/tests/fixtures/sparse_clifford_consumer_ingot");
    let url = url::Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let ingot = db.workspace().containing_ingot(&db, url).unwrap();
    let diagnostics = db.run_on_top_mod(ingot.root_mod(&db)).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
}

#[test]
fn ground_type_inspection_exposes_constructor_and_ordered_args() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_ground_type_inspection.fe".into(),
        include_str!("fixtures/provider_ground_type_inspection/nominal_args.fe"),
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn ground_type_inspection_keeps_recursive_type_fn_alias_opaque() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_ground_type_fn_alias_limit.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait Inspect { type Out }
struct Zero {}
struct Term<const Candidate: usize> {}
struct Add<L, R> {}
struct Yes {}
struct No {}

recursive type fn Plan<const N: usize>() -> (*) {
    match N {
        0 => Zero
        _ => Add<Term<{N - 1}>, Plan<{N - 1}>>
    }
}

type GroundPlan = Plan<3>

struct Target {}
struct Inspector {}
impl Derive<Inspect> for Inspector {
    const fn derive<T>(ev: own Evidence<Inspect<T>>) -> Evidence<Inspect<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Inspect<T>>)
    {
        let out = builder.ty<No>()
        // Current ground-type inspection is deliberately source-syntactic:
        // it sees the GroundPlan alias, not its normalized Add/Term tree.
        for nested in builder.ty<GroundPlan>().preorder_types() {
            if builder.same_ty(nested.constructor(), builder.ty<Term>()) {
                out = builder.ty<Yes>()
            }
        }
        builder.emit_assoc_ty("Out", out)
        builder.finish()
        ev
    }
}

derive Inspect for Target using Inspector
fn takes_no(_ value: No) {}
fn proves_current_boundary(value: <Target as Inspect>::Out) { takes_no(value) }
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn normalized_ground_type_inspection_unfolds_recursive_plan_in_exact_order() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_normalized_ground_type_plan.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait Inspect { type Out }
struct Zero {}
struct Term<const Candidate: usize> {}
struct Add<L, R> {}
struct Yes {}
struct No {}

recursive type fn Plan<const N: usize>() -> (*) {
    match N {
        0 => Zero
        _ => Add<Term<{N - 1}>, Plan<{N - 1}>>
    }
}
type GroundPlan = Plan<3>

struct Target {}
struct Inspector {}
impl Derive<Inspect> for Inspector {
    const fn derive<T>(ev: own Evidence<Inspect<T>>) -> Evidence<Inspect<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Inspect<T>>)
    {
        let code = 0
        for nested in builder.ty<GroundPlan>().normalized_preorder_types() {
            if builder.same_ty(nested.constructor(), builder.ty<Term>()) {
                for arg in nested.generic_args() {
                    if arg.is_const() {
                        code = code * 10 + arg.const_value()
                    }
                }
            }
        }
        let out = builder.ty<No>()
        if code == 210 {
            out = builder.ty<Yes>()
        }
        builder.emit_assoc_ty("Out", out)
        builder.finish()
        ev
    }
}

derive Inspect for Target using Inspector
fn takes_yes(_ value: Yes) {}
fn proves_normalized_order(value: <Target as Inspect>::Out) { takes_yes(value) }
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn normalized_ground_type_inspection_fails_closed_on_forwarded_params() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_normalized_ground_type_forwarded.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Inspect { type Out }
struct Zero {}
struct Add<L, R> {}
recursive type fn Plan<F, const N: usize>() -> (*) {
    match N {
        0 => Zero
        _ => Add<F, Plan<F, {N - 1}>>
    }
}
type GroundPlan = Plan<Zero, 2>
struct Target {}
struct Inspector {}
impl Derive<Inspect> for Inspector {
    const fn derive<T>(ev: own Evidence<Inspect<T>>) -> Evidence<Inspect<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Inspect<T>>)
    {
        for _nested in builder.ty<GroundPlan>().normalized_preorder_types() {}
        builder.emit_assoc_ty("Out", builder.ty<Zero>())
        builder.finish()
        ev
    }
}
derive Inspect for Target using Inspector
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let rendered = fe_hir::test_db::format_diagnostics(&db, &db.run_on_top_mod(top_mod));
    assert!(
        rendered.contains("this construct is not supported in derive provider bodies"),
        "forwarded ground parameters must fail closed at the opt-in reflection call:\n{rendered}"
    );
}

#[test]
fn method_quote_supports_hygienic_local_let_block() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_quote_local_let.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait Compute {
    fn run(self, _ value: i32) -> i32
}

struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Compute<T>>,
        )
    {
        builder.emit_method(quote {
            fn run(self, _ value: i32) -> i32 {
                let value = value + value
                let shared = value + value
                shared + shared
            }
        })
        builder.finish()
        ev
    }
}

struct Target {}
derive Compute for Target using Provider

fn use_it(value: Target) -> i32 {
    value.run(2)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn provider_natural_range_and_integer_codegen_share_the_quote_dag() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_range_integer_codegen.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Compute { fn run(self, _ value: i32) -> i32 }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        let total = builder.int(0)
        for i in 0..3 {
            total = builder.add(total, builder.int(i))
        }
        let product = builder.mul(total, builder.int(4))
        let result = builder.sub(product, builder.neg(builder.int(2)))
        builder.emit_method("run", result)
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn provider_quote_integer_operators_preserve_hygienic_locals() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_quote_integer_codegen.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Compute { fn run(self, _ value: i32) -> i32 }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        builder.emit_method(quote {
            fn run(self, _ value: i32) -> i32 {
                let shared = value * 3
                shared - -2
            }
        })
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn provider_natural_range_hard_cap_fails_closed() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_range_cap.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        for i in 0..4097 {}
        builder.emit_method(quote { fn run(self) -> bool { true } })
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let rendered = fe_hir::test_db::format_diagnostics(&db, &db.run_on_top_mod(top_mod));
    assert!(
        rendered.contains("exceeded its compile-time execution budget"),
        "range cap must fail closed through the provider budget diagnostic:\n{rendered}"
    );
}

#[test]
fn provider_codegen_type_mismatches_fail_in_ordinary_type_checking() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_integer_codegen_type_mismatch.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        builder.emit_method("run", builder.int(7))
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let rendered = fe_hir::test_db::format_diagnostics(&db, &db.run_on_top_mod(top_mod));
    assert!(
        !rendered.is_empty(),
        "generated integer body must not bypass ordinary return-type checking"
    );
}

#[test]
fn provider_share_rejects_non_root_pure_expression_fail_closed() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_share_scope_reject.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Compute { fn run(self, _ value: bool) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        let value = builder.arg_ref("value")
        let effectful = builder.keccak(value)
        let shared = builder.share(effectful)
        builder.emit_method("run", shared)
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let rendered = fe_hir::test_db::format_diagnostics(&db, &db.run_on_top_mod(top_mod));
    assert!(
        rendered.contains("only accepts root-scope generated expressions"),
        "unsafe/effectful sharing must fail with the named scope diagnostic:\n{rendered}"
    );
}

#[test]
fn method_quote_local_let_rejects_typed_binding_fail_closed() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_quote_local_let_reject.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Compute { fn run(self, _ value: bool) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        builder.emit_method(quote {
            fn run(self, _ value: bool) -> bool {
                let shared: bool = value
                shared
            }
        })
        builder.finish()
        ev
    }
}

struct Target {}
derive Compute for Target using Provider
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = db.run_on_top_mod(top_mod);
    assert!(
        !diags.is_empty(),
        "typed quote-local bindings must be rejected rather than silently replayed"
    );
}

#[test]
fn provider_const_helper_also_drives_recursive_type_plan() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_const_helper_bridge.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

const fn keep(_ i: usize) -> bool { i == 1 }

struct Zero {}
struct Add<const I: usize, R> {}
struct Select<const Keep: usize, const I: usize, R> {}
trait SelectOut { type Out }
impl<const I: usize, R> SelectOut for Select<0, I, R> { type Out = R }
impl<const I: usize, R> SelectOut for Select<1, I, R> { type Out = Add<I, R> }
recursive type fn Plan<const N: usize>() -> (*) {
    match N {
        0 => Zero
        _ => <Select<
            { if keep(N - 1) { 1 } else { 0 } },
            {N - 1},
            Plan<{N - 1}>,
        > as SelectOut>::Out
    }
}
type Exact = Plan<3>
type Expected = Add<1, Zero>
fn exact(value: Exact) -> Expected { value }

trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        if keep(1) {
            builder.emit_method(quote { fn run(self) -> bool { true } })
        } else {
            builder.emit_method(quote { fn run(self) -> bool { false } })
        }
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn provider_const_helpers_may_nest() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_nested_const_helpers.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
const fn inner(_ x: bool) -> bool { !x }
const fn outer(_ x: bool) -> bool { inner(inner(x)) }
trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        if outer(true) {
            builder.emit_method(quote { fn run(self) -> bool { true } })
        } else {
            builder.emit_method(quote { fn run(self) -> bool { false } })
        }
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

fn assert_provider_helper_rejected(name: &str, source: &str) {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(name.into(), source);
    let (top_mod, _) = db.top_mod(file);
    let rendered = fe_hir::test_db::format_diagnostics(&db, &db.run_on_top_mod(top_mod));
    assert!(
        rendered.contains("this construct is not supported in derive provider bodies"),
        "unexpected diagnostics:\n{rendered}"
    );
}

#[test]
fn provider_const_helper_recursion_is_rejected() {
    assert_provider_helper_rejected(
        "provider_recursive_const_helper.fe",
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
const fn recurse(_ x: bool) -> bool { recurse(x) }
trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        if recurse(true) { builder.emit_method(quote { fn run(self) -> bool { true } }) }
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
}

#[test]
fn provider_const_helper_unsupported_body_is_rejected() {
    assert_provider_helper_rejected(
        "provider_unsupported_const_helper.fe",
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
const fn unsupported(_ x: bool) -> bool { [x][0] }
trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        if unsupported(true) { builder.emit_method(quote { fn run(self) -> bool { true } }) }
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
}

#[test]
fn provider_const_helper_depth_is_rejected() {
    let mut helpers = String::new();
    for i in 0..33 {
        let body = if i == 32 {
            "x".to_string()
        } else {
            format!("h{}(x)", i + 1)
        };
        helpers.push_str(&format!("const fn h{i}(_ x: bool) -> bool {{ {body} }}\n"));
    }
    let source = format!(
        r#"
use core::derive::{{Derive, Evidence, ImplBuilder, Reflect}}
{helpers}
trait Compute {{ fn run(self) -> bool }}
struct Provider {{}}
impl Derive<Compute> for Provider {{
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {{
        if h0(true) {{ builder.emit_method(quote {{ fn run(self) -> bool {{ true }} }}) }}
        builder.finish()
        ev
    }}
}}
struct Target {{}}
derive Compute for Target using Provider
"#
    );
    assert_provider_helper_rejected("provider_deep_const_helper.fe", &source);
}

#[test]
fn provider_non_const_helper_is_rejected() {
    assert_provider_helper_rejected(
        "provider_non_const_helper.fe",
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
fn effectful(_ x: bool) -> bool { x }
trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        if effectful(true) { builder.emit_method(quote { fn run(self) -> bool { true } }) }
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
}

#[test]
fn provider_effectful_const_helper_is_rejected() {
    assert_provider_helper_rejected(
        "provider_effectful_const_helper.fe",
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
const fn effectful(_ x: bool) -> bool uses (reflect: Reflect<bool>) { x }
trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        if effectful(true) { builder.emit_method(quote { fn run(self) -> bool { true } }) }
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
}

#[test]
fn provider_const_helper_supports_canonical_unsigned_arithmetic() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_const_helper_arithmetic.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
const fn canonical(_ x: usize) -> usize {
    let slot = (x / 4) % 5
    let blade = 1 << (slot + 1)
    let mixed = ((blade >> 1) & 7) ^ 3
    let wrapped = (1 << 256) | 3
    mixed * 2 + (10 - mixed) + (wrapped - 3)
}
trait Compute { fn run(self) -> bool }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        if canonical(8) == 17 {
            builder.emit_method(quote { fn run(self) -> bool { true } })
        } else {
            builder.emit_method(quote { fn run(self) -> bool { false } })
        }
        builder.finish()
        ev
    }
}
struct Target {}
derive Compute for Target using Provider
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

fn arithmetic_helper_source(expr: &str) -> String {
    format!(
        r#"
use core::derive::{{Derive, Evidence, ImplBuilder, Reflect}}
const fn bad() -> usize {{ {expr} }}
trait Compute {{ fn run(self) -> bool }}
struct Provider {{}}
impl Derive<Compute> for Provider {{
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {{
        if bad() == 0 {{ builder.emit_method(quote {{ fn run(self) -> bool {{ true }} }}) }}
        builder.finish()
        ev
    }}
}}
struct Target {{}}
derive Compute for Target using Provider
"#
    )
}

#[test]
fn provider_const_helper_arithmetic_edges_fail_closed() {
    for (name, expr) in [
        ("underflow", "0 - 1"),
        ("division_by_zero", "1 / 0"),
        ("remainder_by_zero", "1 % 0"),
        (
            "overflow",
            "115792089237316195423570985008687907853269984665640564039457584007913129639935 + 1",
        ),
    ] {
        let source = arithmetic_helper_source(expr);
        assert_provider_helper_rejected(&format!("provider_const_helper_{name}.fe"), &source);
    }
}
