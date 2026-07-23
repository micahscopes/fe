use fe_hir::test_db::{HirAnalysisTestDb, format_diagnostics};

const SOURCE: &str = r#"
struct Zero {}
struct Term<const I: usize> {}
struct Add<L, R> {}
struct Select<const Keep: usize, H, T> {}

trait SelectOut { type Out }
impl<H, T> SelectOut for Select<0, H, T> { type Out = T }
impl<H, T> SelectOut for Select<1, H, T> { type Out = Add<H, T> }

const fn wanted(_ want: usize, _ actual: usize) -> usize {
    if want == actual { 1 } else { 0 }
}

recursive type fn Filter<const Want: usize, const N: usize>() -> (*) {
    match N {
        0 => Zero
        _ => <Select<
            {wanted(Want, N - 1)},
            Term<{N - 1}>,
            Filter<Want, {N - 1}>,
        > as SelectOut>::Out
    }
}

struct Probe { value: Filter<1, 3> }
fn takes_expected(_ value: Add<Term<1>, Zero>) {}
fn exact_filter(value: Filter<1, 3>) { takes_expected(value) }
"#;

#[test]
fn probe_invariant_const_selector_inside_recursive_type_fn() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone("invariant_const_selector_probe.fe".into(), SOURCE);
    let (top_mod, _) = db.top_mod(file);
    let rendered = format_diagnostics(&db, &db.run_on_top_mod(top_mod));
    assert!(rendered.is_empty(), "unexpected diagnostics:\n{rendered}");
}
