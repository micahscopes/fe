use fe_hir::test_db::HirAnalysisTestDb;

#[test]
fn ground_recursive_type_fn_materializes_a_term_add_zero_spine() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "type_schedule_probe.fe".into(),
        r#"
struct Zero {}
struct Term<const I: usize> {}
struct Add<L, R> {}

recursive type fn Schedule<const N: usize>() -> (*) {
    match N {
        0 => Zero
        _ => Add<Term<N>, Schedule<{N - 1}>>
    }
}

fn takes_schedule3(
    _ x: Add<Term<3>, Add<Term<2>, Add<Term<1>, Zero>>>
) {}

fn schedule3_is_its_ground_normal_form(x: Schedule<3>) {
    takes_schedule3(x)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn ground_projection_computes_term_blade_and_sign_args() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "type_schedule_projection_probe.fe".into(),
        r#"
struct End {}
struct Slot<const I: usize> {}
struct Cons<H, T> {}

struct Zero {}
struct Term<const Blade: usize, const Sign: usize> {}
struct Add<L, R> {}

recursive type fn Slots<const N: usize>() -> (*) {
    match N {
        0 => End
        _ => Cons<Slot<N>, Slots<{N - 1}>>
    }
}

const fn blade(_ i: usize) -> usize {
    (i * 3) % 8
}

const fn sign(_ i: usize) -> usize {
    i % 3
}

trait Emit {
    type Out
}

impl<const I: usize> Emit for Slot<I> {
    type Out = Term<{ blade(I) }, { sign(I) }>
}

trait Lower {
    type Out
}

impl Lower for End {
    type Out = Zero
}

impl<const I: usize, T: Lower> Lower for Cons<Slot<I>, T>
    where Slot<I>: Emit
{
    type Out = Add<
        <Slot<I> as Emit>::Out,
        <T as Lower>::Out
    >
}

fn takes_expected(
    _ x: Add<
        Term<4, 1>,
        Add<Term<1, 0>, Add<Term<6, 2>, Add<Term<3, 1>, Zero>>>
    >
) {}

fn projection_is_computed_normal_form(
    x: <Slots<4> as Lower>::Out
) {
    takes_expected(x)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn ground_numeric_tag_selects_zero_or_term() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "type_schedule_choice_probe.fe".into(),
        r#"
struct End {}
struct Slot<const I: usize> {}
struct Cons<H, T> {}
struct Zero {}
struct Term<const I: usize> {}
struct Add<L, R> {}

recursive type fn Slots<const N: usize>() -> (*) {
    match N {
        0 => End
        _ => Cons<Slot<N>, Slots<{N - 1}>>
    }
}

const fn keep_tag(_ i: usize) -> usize {
    i % 2
}

struct Choice<const Tag: usize, const I: usize> {}

trait Emit<Acc> {
    type Out
}

impl<const I: usize, Acc> Emit<Acc> for Choice<0, I> {
    type Out = Acc
}

impl<const I: usize, Acc> Emit<Acc> for Choice<1, I> {
    type Out = Add<Term<I>, Acc>
}

trait Lower<Acc> {
    type Out
}

impl<Acc> Lower<Acc> for End {
    type Out = Acc
}

impl<const I: usize, T: Lower<Acc>, Acc> Lower<Acc> for Cons<Slot<I>, T>
    where Choice<{ keep_tag(I) }, I>: Emit< <T as Lower<Acc>>::Out >
{
    type Out = <Choice<{ keep_tag(I) }, I> as Emit<
        <T as Lower<Acc>>::Out
    >>::Out
}

fn takes_expected(
    _ x: Add<Term<3>, Add<Term<1>, Zero>>
) {}

fn projection_is_computed_normal_form(x: <Slots<4> as Lower<Zero>>::Out) {
    takes_expected(x)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn ground_numeric_tag_normalizes_through_nested_const_fn() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "type_schedule_nested_const_choice_probe.fe".into(),
        r#"
struct Zero {}
struct One {}
struct Slot<const I: usize> {}
struct Choice<const Tag: usize> {}
const fn inner(_ a: usize, _ b: usize) -> usize { (a ^ b) & 1 }
const fn outer(_ i: usize) -> usize {
    let x = i / 2
    let y = i % 2
    inner(x, y)
}
trait Emit { type Out }
impl Emit for Choice<0> { type Out = Zero }
impl Emit for Choice<1> { type Out = One }
trait Lower { type Out }
impl<const I: usize> Lower for Slot<I> where Choice<{outer(I)}>: Emit {
    type Out = <Choice<{outer(I)}> as Emit>::Out
}
fn takes_one(_ x: One) {}
fn projection_is_one(x: <Slot<2> as Lower>::Out) { takes_one(x) }
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn provider_emit_assoc_ty_materializes_schedule_projection() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_schedule_probe.fe".into(),
        r#"
use core::derive::{Derive, Evidence, ImplBuilder, Reflect}

trait HasSchedule {
    type Schedule
}

struct End {}
struct Slot<const I: usize> {}
struct Cons<H, T> {}
struct Zero {}
struct Term<const I: usize> {}
struct Add<L, R> {}

recursive type fn Slots<const N: usize>() -> (*) {
    match N {
        0 => End
        _ => Cons<Slot<N>, Slots<{N - 1}>>
    }
}

const fn keep_tag(_ i: usize) -> usize {
    i % 2
}

struct Choice<const Tag: usize, const I: usize> {}

trait Emit<Acc> {
    type Out
}

impl<const I: usize, Acc> Emit<Acc> for Choice<0, I> {
    type Out = Acc
}

impl<const I: usize, Acc> Emit<Acc> for Choice<1, I> {
    type Out = Add<Term<I>, Acc>
}

trait Lower<Acc> {
    type Out
}

impl<Acc> Lower<Acc> for End {
    type Out = Acc
}

impl<const I: usize, T: Lower<Acc>, Acc> Lower<Acc> for Cons<Slot<I>, T>
    where Choice<{ keep_tag(I) }, I>: Emit< <T as Lower<Acc>>::Out >
{
    type Out = <Choice<{ keep_tag(I) }, I> as Emit<
        <T as Lower<Acc>>::Out
    >>::Out
}

struct ScheduleProvider {}

impl Derive<HasSchedule> for ScheduleProvider {
    const fn derive<T>(ev: own Evidence<HasSchedule<T>>) -> Evidence<HasSchedule<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<HasSchedule<T>>,
        )
    {
        builder.emit_assoc_ty(
            "Schedule",
            builder.ty< <Slots<4> as Lower<Zero>>::Out >(),
        )
        builder.finish()
        ev
    }
}

struct Target {}

derive HasSchedule for Target using ScheduleProvider

fn takes_expected(_ x: Add<Term<3>, Add<Term<1>, Zero>>) {}

fn generated_projection_is_computed_schedule(x: <Target as HasSchedule>::Schedule) {
    takes_expected(x)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}
