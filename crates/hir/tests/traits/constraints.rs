use std::path::Path;

use common::diagnostics::{CompleteDiagnostic, cmp_complete_diagnostics};
use dir_test::{Fixture, dir_test};
use fe_hir::{
    hir_def::TopLevelMod,
    test_db::{HirAnalysisTestDb, initialize_test_analysis_pass},
};

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/test_files/constraints",
    glob: "*.fe"
)]
fn constraints_standalone(fixture: Fixture<&str>) {
    let mut db = HirAnalysisTestDb::default();
    let path = Path::new(fixture.path());
    let file_name = path.file_name().and_then(|file| file.to_str()).unwrap();
    let file = db.new_stand_alone(file_name.into(), fixture.content());
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

fn diagnostics_for<'db>(
    db: &'db HirAnalysisTestDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<CompleteDiagnostic> {
    let mut manager = initialize_test_analysis_pass();
    let mut diags: Vec<_> = manager
        .run_on_module(db, top_mod)
        .into_iter()
        .map(|diag| diag.to_complete(db))
        .collect();
    diags.sort_by(cmp_complete_diagnostics);
    diags
}

fn assert_unsatisfied_bound(diags: &[CompleteDiagnostic], expected: &str) {
    assert!(
        diags.iter().any(|diag| {
            diag.message == "trait bound is not satisfied"
                && diag
                    .sub_diagnostics
                    .iter()
                    .any(|sub| sub.message.contains(expected))
        }),
        "expected unsatisfied bound containing `{expected}`, got diagnostics: {diags:#?}"
    );
}

fn assert_required_by_bound(diags: &[CompleteDiagnostic], callable_name: &str) {
    let expected = format!("required by this bound on `{callable_name}`");
    assert!(
        diags.iter().any(|diag| {
            diag.sub_diagnostics
                .iter()
                .any(|sub| sub.message.contains(&expected))
        }),
        "expected call-bound note containing `{expected}`, got diagnostics: {diags:#?}"
    );
}

#[test]
fn free_function_calls_check_instantiated_constraints_without_effects() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "free_function_calls_check_instantiated_constraints_without_effects.fe".into(),
        r#"
trait Other {}

fn needs<T>(x: T)
where
    T: Other
{}

fn caller() {
    let x: u8 = 1
    needs(x)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_unsatisfied_bound(&diags, "`u8` doesn't implement `Other`");
}

#[test]
fn free_function_calls_accept_satisfied_constraints_without_effects() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "free_function_calls_accept_satisfied_constraints_without_effects.fe".into(),
        r#"
trait Other {}

impl Other for u8 {}

fn needs<T>(x: T)
where
    T: Other
{}

fn caller() {
    let x: u8 = 1
    needs(x)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn free_function_call_constraints_defer_through_if_join() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "free_function_call_constraints_defer_through_if_join.fe".into(),
        r#"
extern {
    fn todo() -> !
}

trait Other {}

fn make<T>() -> T
where
    T: Other
{
    todo()
}

fn caller(flag: bool) {
    let one: u8 = 1
    let x = if flag { make() } else { one }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_unsatisfied_bound(&diags, "`u8` doesn't implement `Other`");
}

#[test]
fn free_function_call_constraints_accept_if_join_after_inference() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "free_function_call_constraints_accept_if_join_after_inference.fe".into(),
        r#"
extern {
    fn todo() -> !
}

trait Other {}

impl Other for u8 {}

fn make<T>() -> T
where
    T: Other
{
    todo()
}

fn caller(flag: bool) {
    let one: u8 = 1
    let x = if flag { make() } else { one }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn free_function_call_constraints_defer_through_array_literals() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "free_function_call_constraints_defer_through_array_literals.fe".into(),
        r#"
extern {
    fn todo() -> !
}

trait Other {}

fn make<T>() -> T
where
    T: Other
{
    todo()
}

fn caller() {
    let one: u8 = 1
    let xs = [make(), one]
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_unsatisfied_bound(&diags, "`u8` doesn't implement `Other`");
}

#[test]
fn free_function_call_constraints_accept_array_literals_after_inference() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "free_function_call_constraints_accept_array_literals_after_inference.fe".into(),
        r#"
extern {
    fn todo() -> !
}

trait Other {}

impl Other for u8 {}

fn make<T>() -> T
where
    T: Other
{
    todo()
}

fn caller() {
    let one: u8 = 1
    let xs = [make(), one]
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn free_function_call_constraints_processed_to_a_fixed_point() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "free_function_call_constraints_processed_to_a_fixed_point.fe".into(),
        r#"
extern {
    fn todo() -> !
}

trait Link<T> {}

struct Leaf {}
struct Mid {}

impl Link<Mid> for Leaf {}
impl Link<u256> for Mid {}

// `U: Link<T>` is declared before `V: Link<U>` but is ambiguous until the
// latter binds `U`, so a single pass over the bounds leaves `T` unresolved.
fn chain<T, U: Link<T>, V: Link<U>>(_ value: V) -> T {
    todo()
}

fn caller() -> u256 {
    chain(Leaf {}) + 1
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn method_call_constraints_defer_through_if_join() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "method_call_constraints_defer_through_if_join.fe".into(),
        r#"
extern {
    fn todo() -> !
}

trait Other {}

trait Make {
    fn make<T>(self) -> T
    where
        T: Other
}

struct Factory {}

impl Make for Factory {
    fn make<T>(self) -> T
    where
        T: Other
    {
        todo()
    }
}

fn caller(flag: bool) {
    let one: u8 = 1
    let x = if flag { Factory {}.make() } else { one }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_unsatisfied_bound(&diags, "`u8` doesn't implement `Other`");
}

#[test]
fn method_call_constraints_accept_if_join_after_inference() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "method_call_constraints_accept_if_join_after_inference.fe".into(),
        r#"
extern {
    fn todo() -> !
}

trait Other {}

impl Other for u8 {}

trait Make {
    fn make<T>(self) -> T
    where
        T: Other
}

struct Factory {}

impl Make for Factory {
    fn make<T>(self) -> T
    where
        T: Other
    {
        todo()
    }
}

fn caller(flag: bool) {
    let one: u8 = 1
    let x = if flag { Factory {}.make() } else { one }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn free_function_call_constraints_diagnose_generic_body_bounds() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "free_function_call_constraints_diagnose_generic_body_bounds.fe".into(),
        r#"
extern {
    fn todo() -> !
}

trait Other {}

fn make<T>() -> T
where
    T: Other
{
    todo()
}

fn caller<U>() -> U {
    make()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_unsatisfied_bound(&diags, "`U` doesn't implement `Other`");
    assert_required_by_bound(&diags, "make");
}

#[test]
fn deferred_method_call_constraints_substitute_self_assoc_bounds() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "deferred_method_call_constraints_substitute_self_assoc_bounds.fe".into(),
        r#"
trait Other {}

trait MakeFromU8 {
    type Assoc

    fn make(self, tag: u8)
    where
        Self::Assoc: Other
}

trait MakeFromBool {
    type Assoc

    fn make(self, tag: bool)
    where
        Self::Assoc: Other
}

struct Factory {}

impl MakeFromU8 for Factory {
    type Assoc = u8

    fn make(self, tag: u8)
    where
        Self::Assoc: Other
    {}
}

impl MakeFromBool for Factory {
    type Assoc = bool

    fn make(self, tag: bool)
    where
        Self::Assoc: Other
    {}
}

fn caller(tag: u8) {
    let factory = Factory {}
    factory.make(tag)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_unsatisfied_bound(&diags, "`u8` doesn't implement `Other`");
    assert_required_by_bound(&diags, "make");
}

#[test]
fn deferred_method_call_constraint_solution_disambiguates_return_type() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "deferred_method_call_constraint_solution_disambiguates_return_type.fe".into(),
        r#"
trait PickA<T> {}
trait PickB<T> {}

trait HasOutput {
    type Output
}

struct A {}
struct B {}
struct Subject {}
struct S {}

impl HasOutput for A {
    type Output = u256
}

impl HasOutput for B {
    type Output = bool
}

impl PickA<A> for Subject {}
impl PickB<B> for Subject {}

extern {
    fn todo() -> !
}

trait FooA {
    fn foo<P: HasOutput, Q: PickA<P>>(self, _ value: Q) -> P::Output
}

trait FooB {
    fn foo<P: HasOutput, Q: PickB<P>>(self, _ value: Q) -> P::Output
}

impl FooA for S {
    fn foo<P: HasOutput, Q: PickA<P>>(self, _ value: Q) -> P::Output {
        todo()
    }
}

impl FooB for S {
    fn foo<P: HasOutput, Q: PickB<P>>(self, _ value: Q) -> P::Output {
        todo()
    }
}

fn probe() {
    let s = S {}
    let value: u256 = s.foo(Subject {})
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

/// Shared declarations for the premise-normalization cases: `Sub` fixes
/// `<Self as Super>::Assoc` to `u8`, `Wide` fixes it to `u16`.
const PREMISE_NORMALIZATION: &str = r#"
trait Small {}
impl Small for u8 {}
impl Small for u16 {}
trait Super { type Assoc: Small }
trait Sub: Super<Assoc = u8> {}
trait Wide: Super<Assoc = u16> {}
trait Need<X: Small> {}

fn needs<U: Need<u8>>(_ u: own U) {}
"#;

fn premise_normalization_diags(name: &str, body: &str) -> Vec<CompleteDiagnostic> {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        format!("{name}.fe").as_str().into(),
        &format!("{PREMISE_NORMALIZATION}{body}"),
    );
    let (top_mod, _) = db.top_mod(file);
    diagnostics_for(&db, top_mod)
}

/// A premise and a goal are compared after the same normalization: with
/// `T: Sub`, the premise `U: Need<<T as Super>::Assoc>` is `U: Need<u8>`.
#[test]
fn projected_premise_satisfies_its_normalized_goal() {
    let diags = premise_normalization_diags(
        "projected_premise_satisfies_its_normalized_goal",
        r#"
fn f<T: Sub, U: Need<<T as Super>::Assoc>>(_ t: own T, _ u: own U) {
    needs(u)
}
"#,
    );
    assert!(diags.is_empty(), "{diags:#?}");
}

/// The same premise read through an enum constructor's declared bound.
#[test]
fn projected_premise_satisfies_constructor_bound() {
    let diags = premise_normalization_diags(
        "projected_premise_satisfies_constructor_bound",
        r#"
enum E<T: Sub, U: Need<<T as Super>::Assoc>> { A(T, U) }

fn f<T: Sub, U: Need<<T as Super>::Assoc>>(t: own T, u: own U) {
    let _e = E::A(t, u)
}
"#,
    );
    assert!(diags.is_empty(), "{diags:#?}");
}

/// Control: a normalized premise satisfies a projected goal.
#[test]
fn normalized_premise_satisfies_projected_goal() {
    let diags = premise_normalization_diags(
        "normalized_premise_satisfies_projected_goal",
        r#"
fn needs_projected<T: Sub, U: Need<<T as Super>::Assoc>>(_ t: own T, _ u: own U) {}

fn f<T: Sub, U: Need<u8>>(t: own T, u: own U) {
    needs_projected(t, u)
}
"#,
    );
    assert!(diags.is_empty(), "{diags:#?}");
}

/// An unmet bound still fails.
#[test]
fn missing_premise_still_fails() {
    let diags = premise_normalization_diags(
        "missing_premise_still_fails",
        r#"
fn f<T: Sub, U>(_ t: own T, u: own U) {
    needs(u)
}
"#,
    );
    assert_unsatisfied_bound(&diags, "`U` doesn't implement `Need<u8>`");
}

/// A premise whose projection normalizes to a different type does not
/// satisfy the goal.
#[test]
fn premise_projecting_to_another_type_fails() {
    let diags = premise_normalization_diags(
        "premise_projecting_to_another_type_fails",
        r#"
fn f<T: Wide, U: Need<<T as Super>::Assoc>>(_ t: own T, u: own U) {
    needs(u)
}
"#,
    );
    assert_unsatisfied_bound(&diags, "`U` doesn't implement `Need<u8>`");
}

/// A projection that no premise fixes stays a projection and does not
/// satisfy a concrete goal.
#[test]
fn unfixed_projection_premise_fails() {
    let diags = premise_normalization_diags(
        "unfixed_projection_premise_fails",
        r#"
fn f<T: Super, U: Need<<T as Super>::Assoc>>(_ t: own T, u: own U) {
    needs(u)
}
"#,
    );
    assert_unsatisfied_bound(&diags, "`U` doesn't implement `Need<u8>`");
}

/// A premise whose projection only an implementation fixes is stated the way
/// the goal is: `<S as Super>::Assoc` is `u8`.
#[test]
fn impl_fixed_projection_premise_satisfies_goal() {
    let diags = premise_normalization_diags(
        "impl_fixed_projection_premise_satisfies_goal",
        r#"
struct S {}
impl Super for S { type Assoc = u8 }

fn f<U: Need<<S as Super>::Assoc>>(u: own U) {
    needs(u)
}
"#,
    );
    assert!(diags.is_empty(), "{diags:#?}");
}

/// An implementation fixing the projection to another type does not satisfy
/// the goal.
#[test]
fn impl_fixed_projection_to_another_type_fails() {
    let diags = premise_normalization_diags(
        "impl_fixed_projection_to_another_type_fails",
        r#"
struct S {}
impl Super for S { type Assoc = u16 }

fn f<U: Need<<S as Super>::Assoc>>(u: own U) {
    needs(u)
}
"#,
    );
    assert_unsatisfied_bound(&diags, "`U` doesn't implement `Need<u8>`");
}

/// Normalizing premises terminates when an implementation's projection is
/// cyclic, and the unmet bound is still reported.
#[test]
fn cyclic_impl_projection_premise_terminates() {
    let diags = premise_normalization_diags(
        "cyclic_impl_projection_premise_terminates",
        r#"
struct C {}
impl Super for C { type Assoc = <C as Super>::Assoc }

fn f<U: Need<<C as Super>::Assoc>>(u: own U) {
    needs(u)
}
"#,
    );
    assert!(
        !diags.is_empty(),
        "a cyclic projection must not be accepted"
    );
}

/// Normalizing premises terminates when a premise's equality refers to its
/// own projection, and the unmet bound is still reported.
#[test]
fn self_referential_premise_terminates() {
    let diags = premise_normalization_diags(
        "self_referential_premise_terminates",
        r#"
fn f<T: Super<Assoc = <T as Super>::Assoc>, U: Need<<T as Super>::Assoc>>(_ t: own T, u: own U) {
    needs(u)
}
"#,
    );
    assert_unsatisfied_bound(&diags, "`U` doesn't implement `Need<u8>`");
}

/// Elaboration with premise normalization finishes on programs whose
/// associated types grow through implementations, and reports nothing.
fn assert_finishes_cleanly(name: &'static str, source: &'static str) {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(format!("{name}.fe").as_str().into(), source);
        let (top_mod, _) = db.top_mod(file);
        let messages: Vec<String> = diagnostics_for(&db, top_mod)
            .into_iter()
            .map(|diag| diag.message)
            .collect();
        sender.send(messages).expect("test thread is waiting");
    });
    match receiver.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok(messages) => assert!(messages.is_empty(), "{name}: {messages:?}"),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("{name} did not finish within 30 seconds")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("{name} panicked"),
    }
}

/// A blanket implementation whose associated type wraps its argument.
#[test]
fn growing_blanket_impl_premises_terminate() {
    assert_finishes_cleanly(
        "growing_blanket_impl_premises_terminate",
        r#"
trait Tr { type A: Tr }
struct Box<T> {}
impl<T> Tr for T { type A = Box<T> }
fn f<T: Tr>() -> u256 { 1 }
fn g() -> u256 { f<u8>() }
"#,
    );
}

/// Two traits whose associated types point at each other through wrapping
/// implementations.
#[test]
fn mutual_wrapping_impl_premises_terminate() {
    assert_finishes_cleanly(
        "mutual_wrapping_impl_premises_terminate",
        r#"
trait Ping { type P: Pong }
trait Pong { type Q: Ping }
struct A<T> {}
struct B<T> {}
impl<T> Ping for A<T> { type P = B<A<T>> }
impl<T> Pong for B<T> { type Q = A<B<T>> }
fn f<T: Ping>() -> u256 { 1 }
fn g() -> u256 { f<A<u8>>() }
"#,
    );
}

/// A doubling associated type with a bound back on the trait.
#[test]
fn doubling_family_premises_terminate() {
    assert_finishes_cleanly(
        "doubling_family_premises_terminate",
        r#"
trait Grow { type Out: Grow }
impl<T> Grow for (T, T) { type Out = ((T, T), (T, T)) }
impl Grow for u8 { type Out = (u8, u8) }
fn f<T: Grow>() -> u256 { 1 }
fn g() -> u256 { f<u8>() }
"#,
    );
}

/// Pins the known limit recorded in the pending fixture
/// `trait_resolution_conformance/pending/conditional_impl_projection_premise.fe`:
/// an implementation that needs a premise to apply does not normalize a
/// premise. When this starts passing, move the fixture out of `pending`.
#[test]
fn conditional_impl_projection_premise_is_a_known_limit() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "conditional_impl_projection_premise.fe".into(),
        include_str!(
            "../../test_files/trait_resolution_conformance/pending/conditional_impl_projection_premise.fe"
        ),
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_unsatisfied_bound(&diags, "`U` doesn't implement `Need<u8>`");
}
