use fe_hir::test_db::{HirAnalysisTestDb, format_diagnostics};

const CL41_GRADE1_SOURCE: &str = include_str!("fixtures/sparse_cl41_grade1.fe");

const SOURCE: &str = r#"
struct Missing {}
struct Found<const Slot: usize> {}
struct Select<const Present: usize, const Slot: usize> {}

trait SelectOut { type Out }
impl<const Slot: usize> SelectOut for Select<0, Slot> { type Out = Missing }
impl<const Slot: usize> SelectOut for Select<1, Slot> { type Out = Found<Slot> }

const fn present(_ mask: usize, _ blade: usize) -> usize {
    (mask >> blade) & 1
}

const fn rank(_ mask: usize, _ blade: usize) -> usize {
    match blade {
        0 => 0
        1 => mask & 1
        2 => (mask & 1) + ((mask >> 1) & 1)
        3 => (mask & 1) + ((mask >> 1) & 1) + ((mask >> 2) & 1)
        4 => (mask & 1) + ((mask >> 1) & 1) + ((mask >> 2) & 1) + ((mask >> 3) & 1)
        5 => (mask & 1) + ((mask >> 1) & 1) + ((mask >> 2) & 1) + ((mask >> 3) & 1) + ((mask >> 4) & 1)
        6 => (mask & 1) + ((mask >> 1) & 1) + ((mask >> 2) & 1) + ((mask >> 3) & 1) + ((mask >> 4) & 1) + ((mask >> 5) & 1)
        _ => (mask & 1) + ((mask >> 1) & 1) + ((mask >> 2) & 1) + ((mask >> 3) & 1) + ((mask >> 4) & 1) + ((mask >> 5) & 1) + ((mask >> 6) & 1)
    }
}

// Current recursive type functions admit one const subject. Keep each known
// sparse support as its own selector; this also makes the mask ground before
// the Select payload is normalized.
recursive type fn FindA<const Blade: usize>() -> (*) {
    match Blade {
        0 => <Select<{present(146, 0)}, {rank(146, 0)}> as SelectOut>::Out
        1 => <Select<{present(146, 1)}, {rank(146, 1)}> as SelectOut>::Out
        2 => <Select<{present(146, 2)}, {rank(146, 2)}> as SelectOut>::Out
        3 => <Select<{present(146, 3)}, {rank(146, 3)}> as SelectOut>::Out
        4 => <Select<{present(146, 4)}, {rank(146, 4)}> as SelectOut>::Out
        5 => <Select<{present(146, 5)}, {rank(146, 5)}> as SelectOut>::Out
        6 => <Select<{present(146, 6)}, {rank(146, 6)}> as SelectOut>::Out
        7 => <Select<{present(146, 7)}, {rank(146, 7)}> as SelectOut>::Out
        _ => FindA<0>
    }
}
recursive type fn FindB<const Blade: usize>() -> (*) {
    match Blade {
        0 => <Select<{present(85, 0)}, {rank(85, 0)}> as SelectOut>::Out
        1 => <Select<{present(85, 1)}, {rank(85, 1)}> as SelectOut>::Out
        2 => <Select<{present(85, 2)}, {rank(85, 2)}> as SelectOut>::Out
        3 => <Select<{present(85, 3)}, {rank(85, 3)}> as SelectOut>::Out
        4 => <Select<{present(85, 4)}, {rank(85, 4)}> as SelectOut>::Out
        5 => <Select<{present(85, 5)}, {rank(85, 5)}> as SelectOut>::Out
        6 => <Select<{present(85, 6)}, {rank(85, 6)}> as SelectOut>::Out
        7 => <Select<{present(85, 7)}, {rank(85, 7)}> as SelectOut>::Out
        _ => FindB<0>
    }
}

// Mask A = blades [1, 4, 7]. Mask B = blades [0, 2, 4, 6].
fn takes_missing(_ value: Missing) {}
fn takes_found1(_ value: Found<1>) {}
fn takes_found2(_ value: Found<2>) {}
fn mask_a_present(value: FindA<4>) { takes_found1(value) }
fn mask_a_absent(value: FindA<3>) { takes_missing(value) }
fn mask_b_present(value: FindB<4>) { takes_found2(value) }
fn mask_b_absent(value: FindB<7>) { takes_missing(value) }

struct Compact4 { c0: i32, c1: i32, c2: i32, c3: i32 }
struct SparseMv<const Mask: usize> { compact: Compact4 }

trait Coefficient { fn read(compact: Compact4) -> i32 }
impl Coefficient for Missing { fn read(compact: Compact4) -> i32 { 0 } }
impl Coefficient for Found<0> { fn read(compact: Compact4) -> i32 { compact.c0 } }
impl Coefficient for Found<1> { fn read(compact: Compact4) -> i32 { compact.c1 } }
impl Coefficient for Found<2> { fn read(compact: Compact4) -> i32 { compact.c2 } }
impl Coefficient for Found<3> { fn read(compact: Compact4) -> i32 { compact.c3 } }

// Keep the runtime accessor generic only over the already-normalized lookup
// type.  This is the current-Fe ergonomic seam: callers write one ground
// support-specific lookup argument, while Missing supplies default zero.
fn coefficient_at<Lookup: Coefficient>(value: Compact4) -> i32 {
    <Lookup as Coefficient>::read(compact: value)
}

fn ground_mask_a_present(value: SparseMv<146>) -> i32 {
    coefficient_at<FindA<4>>(value: value.compact)
}
fn ground_mask_a_absent(value: SparseMv<146>) -> i32 {
    coefficient_at<FindA<3>>(value: value.compact)
}
fn ground_mask_b_present(value: SparseMv<85>) -> i32 {
    coefficient_at<FindB<4>>(value: value.compact)
}

"#;

const GENERIC_BRIDGE: &str = r#"
fn generic_coefficient<const Blade: usize>(value: SparseMv<146>) -> i32
    where FindA<Blade>: Coefficient
{
    <FindA<Blade> as Coefficient>::read(compact: value.compact)
}

fn ground_entry_through_generic(value: SparseMv<146>) -> i32 {
    generic_coefficient<4>(value: value)
}
"#;

#[test]
fn bitset_support_ground_queries_normalize_for_two_masks() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone("sparse_bitset_support.fe".into(), SOURCE);
    let (top_mod, _) = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod);
    if !diagnostics.is_empty() {
        panic!(
            "unexpected diagnostics:\n{}",
            format_diagnostics(&db, &diagnostics)
        );
    }
}

#[test]
fn cl41_32_blade_grade_pruning_has_exact_ground_support_and_ranks() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone("sparse_cl41_grade1.fe".into(), CL41_GRADE1_SOURCE);
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
#[ignore = "current HIR solver stack-overflows on symbolic FindA bound before ground call-site discharge"]
fn generic_sparse_mv_bridge_ground_call_repro() {
    let mut db = HirAnalysisTestDb::default();
    let source = format!("{SOURCE}\n{GENERIC_BRIDGE}");
    let file = db.new_stand_alone("sparse_bitset_generic_bridge.fe".into(), &source);
    let (top_mod, _) = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod);
    if !diagnostics.is_empty() {
        panic!(
            "unexpected diagnostics:\n{}",
            format_diagnostics(&db, &diagnostics)
        );
    }
}
