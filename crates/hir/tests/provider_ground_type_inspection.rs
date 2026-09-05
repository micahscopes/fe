use fe_hir::{
    analysis::semantic::collect_semantic_borrow_diagnostic_vouchers, test_db::HirAnalysisTestDb,
};

#[test]
fn route_parse_constructors_do_not_require_copy_evidence() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "route_parse_unconstrained_constructors.fe".into(),
        r#"
fn absent<T>(_ value:T) -> std::web::RouteSegmentParse<T> {
    std::web::RouteSegmentParse::none(value)
}
fn present<T>(_ value:T) -> std::web::RouteSegmentParse<T> {
    std::web::RouteSegmentParse::matched(value)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

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
fn nested_fields_resolve_relative_module_paths() {
    use common::InputDb;
    use driver::DriverDataBase;
    use fe_hir::hir_def::HirIngot;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/provider_relative_fields");
    let url = url::Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let ingot = db.workspace().containing_ingot(&db, url).unwrap();
    for module in ingot.all_modules(&db) {
        let diagnostics = db.run_on_top_mod(*module).format_diags(&db);
        assert!(diagnostics.is_empty(), "{diagnostics}");
    }
}

#[test]
fn configured_derive_provider_may_be_named_through_a_generic_type_alias() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_generic_alias_facade.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait Inspect { type Out }
struct Program<T> {}
struct Compiler<P> {}
struct Payload {}
struct Yes {}
struct No {}
type Facade<T> = Compiler<Program<T>>

struct Target {}
impl<P> Derive<Inspect> for Compiler<P> {
    const fn derive<T>(ev: own Evidence<Inspect<T>>) -> Evidence<Inspect<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Inspect<T>>)
    {
        let out = builder.ty<No>()
        for nested in builder.provider_ty().normalized_preorder_types() {
            if builder.same_ty(nested.constructor(), builder.ty<Payload>()) {
                out = builder.ty<Yes>()
            }
        }
        builder.emit_assoc_ty("Out", out)
        builder.finish()
        ev
    }
}

derive Inspect for Target using Facade<Payload>
fn takes_yes(_ value: Yes) {}
fn proves_alias_configuration(value: <Target as Inspect>::Out) { takes_yes(value) }
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn provider_type_identity_distinguishes_generic_arguments() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_generic_argument_identity.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Inspect { type Out }
struct A {}
struct B {}
struct Pair<T> {}
struct Yes {}
struct No {}
struct Target {}
struct Inspector {}
impl Derive<Inspect> for Inspector {
    const fn derive<T>(ev: own Evidence<Inspect<T>>) -> Evidence<Inspect<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Inspect<T>>)
    {
        let out = builder.ty<No>()
        if builder.same_ty(builder.ty<Pair<A>>(), builder.ty<Pair<B>>()) {
            out = builder.ty<Yes>()
        }
        builder.emit_assoc_ty("Out", out)
        builder.finish()
        ev
    }
}
derive Inspect for Target using Inspector
fn takes_no(_ value: No) {}
fn proves_full_generic_identity(value: <Target as Inspect>::Out) { takes_no(value) }
"#,
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
fn normalized_postorder_and_persistent_sequences_support_structural_folds() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_normalized_postorder_fold.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait Inspect { type Out }
struct Term<const Candidate: usize> {}
struct Add<L, R> {}
struct Neg<A> {}
struct Yes {}
struct No {}
type GroundExpr = Add<Term<4>, Neg<Term<7>>>

struct Target {}
struct Inspector {}
impl Derive<Inspect> for Inspector {
    const fn derive<T>(ev: own Evidence<Inspect<T>>) -> Evidence<Inspect<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Inspect<T>>)
    {
        // Postorder makes the normalized type tree an ordinary value stack:
        // Term<4>, Term<7>, Neg, Add. These sequence operations are persistent;
        // every assignment receives a fresh compile-time value.
        let stack = 0..0
        for nested in builder.ty<GroundExpr>().normalized_postorder_types() {
            if builder.same_ty(nested.constructor(), builder.ty<Term>()) {
                for arg in nested.generic_args() {
                    if arg.is_const() {
                        stack = stack.append(arg.const_value())
                    }
                }
            }
            if builder.same_ty(nested.constructor(), builder.ty<Neg>()) {
                let value = stack.last()
                stack = stack.without_last().append(value + 10)
            }
            if builder.same_ty(nested.constructor(), builder.ty<Add>()) {
                let rhs = stack.last()
                stack = stack.without_last()
                let lhs = stack.last()
                stack = stack.without_last().append(lhs * 100 + rhs)
            }
        }

        // Exercise indexed reads, concatenation, and functional replacement
        // independently of the expression fold.
        let probe = (0..0).append(3).append(8).concat(10..12)
        let probe = probe.replace(1, 9)
        let out = builder.ty<No>()
        if stack.len() == 1 && stack.at(0) == 417
            && probe.len() == 4 && probe.at(0) == 3
            && probe.at(1) == 9 && probe.at(2) == 10 && probe.at(3) == 11
        {
            out = builder.ty<Yes>()
        }
        builder.emit_assoc_ty("Out", out)
        builder.finish()
        ev
    }
}

derive Inspect for Target using Inspector
fn takes_yes(_ value: Yes) {}
fn proves_fold(value: <Target as Inspect>::Out) { takes_yes(value) }
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn persistent_sequence_bounds_fail_closed_at_the_access() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_sequence_bounds.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Inspect {}
struct Target {}
struct Inspector {}
impl Derive<Inspect> for Inspector {
    const fn derive<T>(ev: own Evidence<Inspect<T>>) -> Evidence<Inspect<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Inspect<T>>)
    {
        let _invalid = (0..1).at(1)
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
        "out-of-bounds compile-time sequence access must fail closed:\n{rendered}"
    );
}

#[test]
fn configured_provider_type_is_reflected_for_distinct_configurations() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_ground_configuration.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait Inspect { type Out }
struct Config<A> {}
struct Yes {}
struct No {}
struct Configured<P> {}

impl<P> Derive<Inspect> for Configured<P> {
    const fn derive<T>(ev: own Evidence<Inspect<T>>) -> Evidence<Inspect<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Inspect<T>>)
    {
        let out = builder.ty<No>()
        for configured in builder.provider_ty().normalized_preorder_types() {
            if builder.same_ty(configured.constructor(), builder.ty<Yes>()) {
                out = builder.ty<Yes>()
            }
        }
        builder.emit_assoc_ty("Out", out)
        builder.finish()
        ev
    }
}

struct TargetYes {}
struct TargetNo {}
derive Inspect for TargetYes using Configured<Config<Yes>>
derive Inspect for TargetNo using Configured<Config<No>>

fn takes_yes(_ value: Yes) {}
fn takes_no(_ value: No) {}
fn proves_yes(value: <TargetYes as Inspect>::Out) { takes_yes(value) }
fn proves_no(value: <TargetNo as Inspect>::Out) { takes_no(value) }
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn configured_provider_type_fails_closed_when_not_ground() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_open_configuration.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Inspect {}
struct Config<A> {}
struct Configured<P> {}
impl<P> Derive<Inspect> for Configured<P> {
    const fn derive<T>(ev: own Evidence<Inspect<T>>) -> Evidence<Inspect<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Inspect<T>>)
    {
        for _node in builder.provider_ty().normalized_preorder_types() {}
        builder.finish()
        ev
    }
}
struct OpenTarget<A> { value: A }
derive Inspect for OpenTarget<A> using Configured<Config<A>>
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let rendered = fe_hir::test_db::format_diagnostics(&db, &db.run_on_top_mod(top_mod));
    assert!(
        rendered.contains("this construct is not supported in derive provider bodies"),
        "open configured provider types must fail closed:\n{rendered}"
    );
}

#[test]
fn nested_nominal_fields_drive_hygienic_generated_access() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_nested_fields.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait ReadNested { fn read(_ value: Self) -> u32 }
struct Pair { left: u32, right: u32 }
struct Carrier { pair: Pair }
struct NestedReader {}

impl Derive<ReadNested> for NestedReader {
    const fn derive<T>(ev: own Evidence<ReadNested<T>>) -> Evidence<ReadNested<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<ReadNested<T>>)
    {
        let value = builder.arg_ref("value")
        let nested = value
        for outer in reflect.fields() {
            let inner_index: usize = 0
            for inner in outer.ty().fields() {
                if inner_index == 1 {
                    nested = builder.field_get(
                        builder.field_get(value, outer), inner,
                    )
                }
                inner_index = inner_index + 1
            }
        }
        builder.emit_method("read", nested)
        builder.finish()
        ev
    }
}

derive ReadNested for Carrier using NestedReader
fn proves_nested(value: Carrier) -> u32 {
    <Carrier as ReadNested>::read(value)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn reflected_fields_align_by_name_across_alias_normalized_records() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_field_name_alignment.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait ReadAligned { fn read(_ value: Self) -> u32 }
struct Basis { e0: Marker, e1: Marker, e2: Marker }
type BasisAlias = Basis
struct Marker {}
struct Coefficients { e2: u32, e0: u32 }
struct Carrier { coefficients: Coefficients }
struct AlignedReader {}

impl Derive<ReadAligned> for AlignedReader {
    const fn derive<T>(ev: own Evidence<ReadAligned<T>>) -> Evidence<ReadAligned<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<ReadAligned<T>>)
    {
        let value = builder.arg_ref("value")
        let result = value
        let basis = builder.ty<BasisAlias>()
        for configured in builder.ty<BasisAlias>().normalized_preorder_types() {
            if builder.same_ty(configured.constructor(), builder.ty<Basis>()) {
                basis = configured
            }
        }
        for outer in reflect.fields() {
            let coefficients = builder.field_get(value, outer)
            for coefficient in outer.ty().fields() {
                for generator in basis.fields() {
                    if coefficient.same_name(generator) {
                        result = builder.field_get(coefficients, coefficient)
                    }
                }
            }
        }
        builder.emit_method("read", result)
        builder.finish()
        ev
    }
}

derive ReadAligned for Carrier using AlignedReader
fn proves_alignment(value: Carrier) -> u32 {
    <Carrier as ReadAligned>::read(value)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn reflected_variant_names_generate_one_typed_match() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_variant_name_match.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait VariantName { const fn variant_name(self) -> String<31> }
struct VariantNameProvider {}

impl Derive<VariantName> for VariantNameProvider {
    const fn derive<T>(ev: own Evidence<VariantName<T>>) -> Evidence<VariantName<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<VariantName<T>>)
    {
        let mut arms = quote {}
        for variant in reflect.variants() {
            arms = quote {
                ${arms},
                ${variant}(ignored) => ${variant.name()}
            }
        }
        builder.emit_method("variant_name", quote { match self { ${arms} } })
        builder.finish()
        ev
    }
}

enum Direction { North, South, East, West }
derive VariantName for Direction using VariantNameProvider

const NORTH: String<31> = Direction::North.variant_name()
const SOUTH: String<31> = Direction::South.variant_name()
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn reflected_variants_generate_one_typed_method_call_chain() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_variant_method_call_chain.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

struct Input {}
impl Copy for Input {}
impl Input {
    fn equals_literal<const N: usize>(self, _ value: [u8; N]) -> bool { false }
}

struct Parsed<T> { value: T, valid: bool }
impl<T> Parsed<T> {
    fn none(_ value: T) -> Self { Self { value: value, valid: false } }
    fn select<const N: usize>(
        self,
        _ input: Input,
        _ segment: [u8; N],
        _ candidate: T,
    ) -> Self {
        if input.equals_literal(segment) {
            Self { value: candidate, valid: true }
        } else {
            self
        }
    }
}

trait Segment {
    fn parse(_ input: Input) -> Parsed<Self>
    fn begin_parse(self) -> Parsed<Self> { Parsed::none(self) }
}

struct SegmentProvider {}
impl Derive<Segment> for SegmentProvider {
    const fn derive<T>(ev: own Evidence<Segment<T>>) -> Evidence<Segment<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Segment<T>>)
    {
        let variants = reflect.variants()
        let fallback = builder.variant_init(variants.at(0))
        let input = builder.arg_ref("input")
        let mut parse = builder.call(fallback, "begin_parse")
        for variant in variants {
            parse = builder.call(
                parse,
                "select",
                input,
                builder.str(variant.name()),
                builder.variant_init(variant),
            )
        }
        builder.emit_method("parse", parse)
        builder.finish()
        ev
    }
}

enum Direction { North, South, East, West }
derive Segment for Direction using SegmentProvider

fn parse_direction(input: Input) -> Parsed<Direction> { Direction::parse(input) }
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn route_segment_provider_keeps_definition_site_signature_paths() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_route_segment_signature_paths.fe".into(),
        r#"
use std::web::{RouteSegment, RouteSegmentProvider}

enum Direction { North, South }
impl Copy for Direction {}
derive RouteSegment for Direction using RouteSegmentProvider

fn parse_direction(
    input: std::text::Utf8View,
) -> std::web::RouteSegmentParse<Direction> {
    Direction::parse_route_segment(input)
}

fn select_direction(input: std::text::Utf8View) -> Direction {
    let parsed = Direction::parse_route_segment(input)
    if !parsed.is_valid() { return Direction::North }
    parsed.value()
}

fn write_direction(
    direction: Direction,
    destination: std::text::Utf8Buffer,
) -> std::text::Utf8Write {
    direction.write_route_segment(destination)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    let generated_parse = top_mod
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|func| {
            func.name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "parse_route_segment")
                && func.containing_impl_trait(&db).is_some()
        })
        .expect("missing generated RouteSegment impl method");
    let hir_return = generated_parse
        .ret_ty_hir(&db)
        .expect("generated parse method must retain its explicit result")
        .pretty_print(&db);
    let semantic_return = generated_parse.return_ty(&db).pretty_print(&db);
    assert_eq!(
        hir_return, "std::web::navigation::RouteSegmentParse<Self>",
        "generated result must retain the trait declaration's type-driven Self"
    );
    assert_eq!(
        semantic_return, "RouteSegmentParse<Direction>",
        "generated result must resolve the downstream route type"
    );

    let _select_direction = top_mod
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|func| {
            func.name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "select_direction")
        })
        .expect("missing select_direction");

    let semantic_diags = collect_semantic_borrow_diagnostic_vouchers(&db, top_mod);
    assert!(
        semantic_diags.is_empty(),
        "nested provider result types must lower through their typed accessors"
    );
}

#[test]
fn nested_generic_field_reflection_fails_closed_without_substitution() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_nested_generic_fields.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Inspect {}
struct Boxed<A> { value: A }
struct Carrier { boxed: Boxed<u32> }
struct Inspector {}
impl Derive<Inspect> for Inspector {
    const fn derive<T>(ev: own Evidence<Inspect<T>>) -> Evidence<Inspect<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Inspect<T>>)
    {
        for outer in reflect.fields() {
            for _inner in outer.ty().fields() {}
        }
        builder.finish()
        ev
    }
}
derive Inspect for Carrier using Inspector
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let rendered = fe_hir::test_db::format_diagnostics(&db, &db.run_on_top_mod(top_mod));
    assert!(
        rendered.contains("this construct is not supported in derive provider bodies"),
        "generic nested field reflection must fail closed:\n{rendered}"
    );
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
fn provider_generated_methods_retain_method_local_generics() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_method_local_generics.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait WordSink: core::marker::Copy {
    fn write(self, _ value: u32) -> Self
}

trait Encode {
    fn encode<W: WordSink>(self, _ writer: W) -> W
    fn decode<W: WordSink>(mut self, _ writer: W) -> W
}

struct Provider {}
impl Derive<Encode> for Provider {
    const fn derive<T>(ev: own Evidence<Encode<T>>) -> Evidence<Encode<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Encode<T>>,
        )
    {
        builder.emit_method(quote {
            fn encode<W: WordSink>(self, writer: W) -> W {
                writer.write(7)
            }
        })
        builder.emit_method(quote {
            fn decode<W: WordSink>(mut self, writer: W) -> W {
                writer.write(9)
            }
        })
        builder.finish()
        ev
    }
}

struct Target {}
derive Encode for Target using Provider

struct Sink { value: u32 }
impl core::marker::Copy for Sink {}
impl WordSink for Sink {
    fn write(self, _ value: u32) -> Self { Sink { value } }
}

fn use_generated(value: Target, sink: Sink) -> u32 {
    value.encode(sink).value
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn provider_builder_borrow_constructs_explicit_ref_arguments() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_builder_borrow.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait Sink: core::marker::Copy { fn write(self, _ value: i32) -> Self }
trait Encode { fn encode<W: Sink>(ref self, _ writer: W) -> W }

impl Encode for i32 {
    fn encode<W: Sink>(ref self, _ writer: W) -> W { writer.write(self) }
}

struct Provider {}
impl Derive<Encode> for Provider {
    const fn derive<T>(ev: own Evidence<Encode<T>>) -> Evidence<Encode<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Encode<T>>)
    {
        let mut body = builder.arg_ref("writer")
        for field in reflect.fields() {
            builder.require<Encode>(field.ty())
            body = builder.trait_call(
                field.ty(),
                "encode",
                builder.borrow(builder.field_get(builder.self_ref(), field)),
                body,
            )
        }
        builder.emit_method("encode", body)
        builder.finish()
        ev
    }
}

struct Pair { left: i32, right: i32 }
derive Encode for Pair using Provider

struct Last { value: i32 }
impl core::marker::Copy for Last {}
impl Sink for Last {
    fn write(self, _ value: i32) -> Self { Self { value: value } }
}

fn use_generated(value: Pair) -> i32 {
    encode_value(ref value, Last { value: 0 }).value
}

fn encode_value<T: Encode, W: Sink>(_ value: ref T, _ writer: W) -> W {
    value.encode(writer)
}

fn copy_borrowed_bool(_ value: ref bool) -> bool { value }
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
    let semantic_diags = collect_semantic_borrow_diagnostic_vouchers(&db, top_mod);
    assert!(semantic_diags.is_empty());
}

#[test]
fn provider_builder_borrow_rejects_non_expression_values() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_builder_borrow_reject.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Compute { fn run(self) -> u32 }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        builder.emit_method("run", builder.borrow(builder.ty<u32>()))
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
        rendered.contains("this construct is not supported in derive provider bodies"),
        "non-expression borrow inputs must fail closed:\n{rendered}"
    );
}

#[test]
fn provider_builder_borrow_mut_constructs_explicit_mut_arguments() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_builder_borrow_mut.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait Clear { fn clear(mut self) }

impl Clear for i32 {
    fn clear(mut self) { self = 0 }
}

struct Provider {}
impl Derive<Clear> for Provider {
    const fn derive<T>(ev: own Evidence<Clear<T>>) -> Evidence<Clear<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Clear<T>>)
    {
        let field = reflect.fields().at(0)
        builder.require<Clear>(field.ty())
        builder.emit_method(
            "clear",
            builder.trait_call(
                field.ty(),
                "clear",
                builder.borrow_mut(builder.field_get(builder.self_ref(), field)),
            ),
        )
        builder.finish()
        ev
    }
}

struct Target { value: i32 }
derive Clear for Target using Provider

fn use_generated(value: mut Target) { value.clear() }
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
    let semantic_diags = collect_semantic_borrow_diagnostic_vouchers(&db, top_mod);
    assert!(semantic_diags.is_empty());
}

#[test]
fn provider_builder_borrow_mut_rejects_non_expression_values() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_builder_borrow_mut_reject.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Compute { fn run(self) -> u32 }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        builder.emit_method("run", builder.borrow_mut(builder.ty<u32>()))
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
        rendered.contains("this construct is not supported in derive provider bodies"),
        "non-expression mutable borrow inputs must fail closed:\n{rendered}"
    );
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
fn provider_float_literal_replays_through_ordinary_type_checking() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_float_literal_codegen.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Compute { fn zero(self) -> f32 }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        builder.emit_method("zero", builder.float(0.0))
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
fn provider_float_builder_rejects_non_float_input() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_float_literal_kind_reject.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}
trait Compute { fn zero(self) -> f32 }
struct Provider {}
impl Derive<Compute> for Provider {
    const fn derive<T>(ev: own Evidence<Compute<T>>) -> Evidence<Compute<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Compute<T>>)
    {
        builder.emit_method("zero", builder.float(0))
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
        "builder.float must fail closed when its value is not a float literal"
    );
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
