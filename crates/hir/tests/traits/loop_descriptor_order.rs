use fe_hir::test_db::HirAnalysisTestDb;

#[test]
fn ambiguous_loop_descriptors_do_not_choose_by_impl_order() {
    let mut rejected = Vec::new();
    for source in [
        r#"// P4 follow-up: which impl does the loop pick for an unconstrained integer
// receiver? The impls return different values so execution shows the choice.
// Expected by the law: rejected (ambiguous, or u256 by default with no Seq impl).
// Item types differ (u8 / u16) as in l1; the body only casts the element.
use core::seq::Seq
struct Wrap<T> { x: T }
impl Seq for Wrap<u8> { type Item = u8
    fn len(self) -> usize { 1 }
    fn get(self, _ i: usize) -> u8 { 8 } }
impl Seq for Wrap<u16> { type Item = u16
    fn len(self) -> usize { 1 }
    fn get(self, _ i: usize) -> u16 { 16 } }
fn pick<T>(_ x: T) -> Wrap<T> { Wrap { x } }
fn sum() -> u256 {
    let mut s: u256 = 0
    for e in pick(7) { s = s + (e as u256) }
    s
}
#[test]
fn which() { assert(sum() == 8) }
#[test]
fn which16() { assert(sum() == 16) }
"#,
        r#"// P4 follow-up: which impl does the loop pick for an unconstrained integer
// receiver? The impls return different values so execution shows the choice.
// Expected by the law: rejected (ambiguous, or u256 by default with no Seq impl).
// Same as l1c with the two impls declared in the opposite order.
// Item types differ (u8 / u16) as in l1; the body only casts the element.
use core::seq::Seq
struct Wrap<T> { x: T }
impl Seq for Wrap<u16> { type Item = u16
    fn len(self) -> usize { 1 }
    fn get(self, _ i: usize) -> u16 { 16 } }
impl Seq for Wrap<u8> { type Item = u8
    fn len(self) -> usize { 1 }
    fn get(self, _ i: usize) -> u8 { 8 } }
fn pick<T>(_ x: T) -> Wrap<T> { Wrap { x } }
fn sum() -> u256 {
    let mut s: u256 = 0
    for e in pick(7) { s = s + (e as u256) }
    s
}
#[test]
fn which() { assert(sum() == 8) }
#[test]
fn which16() { assert(sum() == 16) }
"#,
    ] {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("loop_descriptor_order.fe".into(), source);
        let (module, _) = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(module);
        let mut errors = diagnostics
            .iter()
            .map(|diagnostic| {
                let complete = diagnostic.to_complete(&db);
                (complete.error_code.to_string(), complete.message)
            })
            .collect::<Vec<_>>();
        errors.sort();
        assert!(
            !errors.is_empty(),
            "multiple matching Seq impls must not select a descriptor"
        );
        rejected.push(errors);
        let explicit = source.replace("pick(7)", "pick<u8>(7)");
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("loop_descriptor_explicit.fe".into(), &explicit);
        let (module, _) = db.top_mod(file);
        db.assert_no_diags(module);
    }
    assert_eq!(rejected[0], rejected[1]);
}
