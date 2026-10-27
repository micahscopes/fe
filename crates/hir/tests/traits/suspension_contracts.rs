use camino::Utf8PathBuf;
use common::stdlib::HasBuiltinCore;
use fe_hir::{
    analysis::semantic::{
        SemanticInstance, check_runtime_semantic_accesses, check_semantic_accesses,
        identity_semantic_instance_key,
        suspension::{SuspensionContractOrigin, suspension_contract},
    },
    analysis::ty::{
        corelib::{SuspensionFootprint, effect_key_suspension, suspension_capability_method},
        trait_resolution::PredicateListId,
        ty_check::{BodyOwner, EffectParamSite},
    },
    semantic::effect_requirements_for_site,
    test_db::{HirAnalysisTestDb, find_func},
};

#[test]
fn suspension_is_a_nominal_contract_not_a_method_name() {
    let mut db = HirAnalysisTestDb::default();
    db.initialize_builtin_core();
    let file = db.new_stand_alone(
        Utf8PathBuf::from("suspension_methods.fe"),
        r#"
use core::task::Suspend
trait Extended: Suspend { fn transfer() }
trait Unrelated { fn suspend() }
fn ordinary() {}
"#,
    );
    let (module, _) = db.top_mod(file);
    db.assert_no_diags(module);
    let mut checked = 0;
    for func in module.all_funcs(&db) {
        let Some(name) = func.name(&db).to_opt() else {
            continue;
        };
        let expected = match name.data(&db).as_str() {
            "transfer" => true,
            "suspend" | "ordinary" => false,
            _ => continue,
        };
        assert_eq!(suspension_capability_method(&db, *func), expected);
        checked += 1;
    }
    assert_eq!(checked, 3);
}

#[test]
fn abstract_and_nested_rows_retain_their_contract_origins() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        Utf8PathBuf::from("suspension_rows.fe"),
        r#"
trait Effects { uses E }
struct Wrapped<C> {}
impl<C: Effects> Effects for Wrapped<C> { uses E = C::E }
fn direct<C: Effects>() uses C::E {}
fn nested<W: Effects>() uses W::E {}
fn invoke<C: Effects>() uses C::E { nested<Wrapped<C>>() }
"#,
    );
    let (module, _) = db.top_mod(file);
    db.assert_no_diags(module);
    for name in ["direct", "nested"] {
        let func = find_func(&db, module, if name == "nested" { "invoke" } else { name });
        let key = identity_semantic_instance_key(&db, BodyOwner::Func(func, None));
        let key = if name == "nested" {
            SemanticInstance::new(&db, key)
                .call_sites(&db)
                .iter()
                .flatten()
                .find_map(|site| site.callee)
                .expect("nested row call")
                .key
        } else {
            key
        };
        let contract = suspension_contract(&db, key);
        assert!(!contract.entries.is_empty(), "{name}");
        assert!(
            contract
                .entries
                .iter()
                .all(|entry| entry.footprint == SuspensionFootprint::Unresolved)
        );
        // The selected generic instance may retain the outer row as abstract
        // rather than selecting Wrapped<C>'s implementation. Neither that
        // representation nor a partially expanded inner row may become empty.
        assert!(
            contract.entries.iter().any(|entry| match &entry.origin {
                SuspensionContractOrigin::AbstractRow { .. } => true,
                SuspensionContractOrigin::Effect { path: Some(_), .. } => name == "nested",
                _ => false,
            }),
            "{name}: {contract:?}"
        );
    }
}

#[test]
fn suspension_checks_live_accesses_inputs_and_hidden_helpers() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        Utf8PathBuf::from("suspension_accesses.fe"),
        r#"
use core::task::{Native, Pending, Suspend, TaskOutcome}
fn cut() uses (cap: mut Native) {}
fn live() uses (cap: mut Native) {
    var value: u64 = 5
    let held = mut value
    cut()
    held = 9
}
fn ended() uses (cap: mut Native) {
    var value: u64 = 5
    let held = mut value
    held = 9
    cut()
}
fn input(_ value: mut u64) uses (cap: mut Native) {
    cut()
    value = 9
}
fn hidden(_ pending: own Pending<Native, u64>) -> TaskOutcome<u64, u64> {
    var native = Native {}
    native.suspend(pending)
}
struct Owned { value: u64 }
struct Pair { first: u64, second: Owned }
impl Pair {
    fn part(mut self) -> mut u64 {
        let saved = self.second
        yield mut self.first
        self.second = saved
    }
}
fn projection_live() uses (cap: mut Native) {
    var pair = Pair { first: 5, second: Owned { value: 1 } }
    let held = mut pair.part()
    cut()
    held = 9
}
fn projection_ended() uses (cap: mut Native) {
    var pair = Pair { first: 5, second: Owned { value: 1 } }
    let held = mut pair.part()
    held = 9
    cut()
}
fn hole(_ pair: mut Pair) uses (cap: mut Native) {
    let saved = pair.second
    cut()
    pair.second = saved
}
"#,
    );
    let (module, _) = db.top_mod(file);
    db.assert_no_diags(module);
    for (name, expected) in [
        ("live", false),
        ("ended", true),
        ("input", false),
        ("hidden", false),
        ("projection_live", false),
        ("projection_ended", true),
        ("hole", false),
    ] {
        let func = find_func(&db, module, name);
        let key = identity_semantic_instance_key(&db, BodyOwner::Func(func, None));
        let result = check_runtime_semantic_accesses(&db, SemanticInstance::new(&db, key));
        assert_eq!(result.is_ok(), expected, "{name}: {result:?}");
        if let Err(error) = result {
            let diagnostic = error
                .diagnostic()
                .expect("access diagnostic, not upstream blockage");
            assert!(
                diagnostic.sub_diagnostics.iter().any(|label| {
                    label.message.contains(match name {
                        "hidden" => "declares no suspension",
                        "hole" => "unrestored move",
                        _ => "borrowed data remains live",
                    })
                }),
                "{name}: {diagnostic:?}"
            );
        }
    }
}

#[test]
fn generic_provider_custody_is_deferred_then_checked_concretely() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        Utf8PathBuf::from("suspension_custody.fe"),
        r#"
use core::task::{Native, Pending, Suspend, TaskOutcome}
struct Stateful { value: u64 }
impl Suspend for Stateful {
    type Backend = Stateful
    type Error = u64
    fn suspend<T>(mut self, _ pending: own Pending<Stateful, T>) -> TaskOutcome<u64, T> {
        TaskOutcome::Cancelled
    }
}
fn generic<C: Suspend>(_ cap: mut C, _ pending: own Pending<C::Backend, u64>) -> TaskOutcome<C::Error, u64> {
    cap.suspend(pending)
}
fn zero(_ pending: own Pending<Native, u64>) -> TaskOutcome<u64, u64> uses (cap: mut Native) {
    generic(mut cap, pending)
}
fn stateful(_ cap: mut Stateful, _ pending: own Pending<Stateful, u64>) -> TaskOutcome<u64, u64> {
    generic(mut cap, pending)
}
"#,
    );
    let (module, _) = db.top_mod(file);
    db.assert_no_diags(module);
    let func = find_func(&db, module, "generic");
    let key = identity_semantic_instance_key(&db, BodyOwner::Func(func, None));
    let generic = SemanticInstance::new(&db, key);
    check_semantic_accesses(&db, generic).expect("generic custody is a deferred obligation");
    assert!(check_runtime_semantic_accesses(&db, generic).is_err());
    for (name, expected) in [("zero", true), ("stateful", false)] {
        let func = find_func(&db, module, name);
        let key = identity_semantic_instance_key(&db, BodyOwner::Func(func, None));
        let caller = SemanticInstance::new(&db, key);
        let callee = caller
            .call_sites(&db)
            .iter()
            .flatten()
            .find_map(|site| site.callee)
            .expect("generic helper call")
            .key;
        let instance = SemanticInstance::new(&db, callee);
        let result = check_runtime_semantic_accesses(&db, instance);
        assert_eq!(result.is_ok(), expected, "{name}: {result:?}");
    }
}

#[test]
fn suspension_effect_footprint_includes_supertraits() {
    let mut db = HirAnalysisTestDb::default();
    db.initialize_builtin_core();
    let file = db.new_stand_alone(
        Utf8PathBuf::from("suspension_effects.fe"),
        r#"
use core::task::{Pending, Suspend, TaskOutcome}
trait Extended: Suspend {}
trait Unrelated {}
trait Effects { uses E }
struct Native {}
impl Suspend for Native {
    type Backend = Native
    type Error = u64
    fn suspend<T>(mut self, _ pending: own Pending<Native, T>) -> TaskOutcome<u64, T> {
        TaskOutcome::Cancelled
    }
}
fn direct() uses (cap: mut Suspend) {}
fn extended() uses (cap: mut Extended) {}
fn concrete() uses (cap: mut Native) {}
fn unrelated() uses (cap: mut Unrelated) {}
fn abstract_row<C: Effects>() uses C::E {}
"#,
    );
    let (module, _) = db.top_mod(file);
    db.assert_no_diags(module);
    for (name, expected) in [
        ("direct", SuspensionFootprint::MaySuspend),
        ("extended", SuspensionFootprint::MaySuspend),
        ("concrete", SuspensionFootprint::MaySuspend),
        ("unrelated", SuspensionFootprint::Absent),
        ("abstract_row", SuspensionFootprint::Unresolved),
    ] {
        let func = find_func(&db, module, name);
        let requirements = effect_requirements_for_site(&db, EffectParamSite::Func(func, None));
        assert_eq!(requirements.len(), 1);
        assert_eq!(
            effect_key_suspension(
                &db,
                func.scope(),
                PredicateListId::empty_list(&db),
                requirements[0].key.clone(),
            ),
            expected,
            "{name}",
        );
    }
}

#[test]
fn a_timer_is_a_suspension_capability_and_sleeping_is_checked_like_a_cut() {
    let mut db = HirAnalysisTestDb::default();
    db.initialize_builtin_core();
    let file = db.new_stand_alone(
        Utf8PathBuf::from("suspension_timer.fe"),
        r#"
use core::task::{Native, TaskOutcome, Timer}
fn declared() uses (cap: mut Timer) {}
fn generic<C: Timer>(_ cap: mut C) -> TaskOutcome<C::Error, u64> {
    let pending = cap.sleep_begin(5)
    cap.suspend(pending)
}
fn live() uses (cap: mut Native) {
    var value: u64 = 5
    let held = mut value
    let pending = cap.sleep_begin(5)
    let _ = cap.suspend(pending)
    held = 9
}
fn ended() uses (cap: mut Native) {
    var value: u64 = 5
    let held = mut value
    held = 9
    let pending = cap.sleep_begin(5)
    let _ = cap.suspend(pending)
}
fn hidden() {
    var native = Native {}
    let pending = native.sleep_begin(5)
    let _ = native.suspend(pending)
}
"#,
    );
    let (module, _) = db.top_mod(file);
    db.assert_no_diags(module);
    let func = find_func(&db, module, "declared");
    let requirements = effect_requirements_for_site(&db, EffectParamSite::Func(func, None));
    assert_eq!(requirements.len(), 1);
    assert_eq!(
        effect_key_suspension(
            &db,
            func.scope(),
            PredicateListId::empty_list(&db),
            requirements[0].key.clone(),
        ),
        SuspensionFootprint::MaySuspend
    );
    for (name, expected) in [("live", false), ("ended", true), ("hidden", false)] {
        let func = find_func(&db, module, name);
        let key = identity_semantic_instance_key(&db, BodyOwner::Func(func, None));
        let result = check_runtime_semantic_accesses(&db, SemanticInstance::new(&db, key));
        assert_eq!(result.is_ok(), expected, "{name}: {result:?}");
        if let Err(error) = result {
            let diagnostic = error
                .diagnostic()
                .expect("access diagnostic, not upstream blockage");
            assert!(
                diagnostic.sub_diagnostics.iter().any(|label| {
                    label.message.contains(match name {
                        "hidden" => "declares no suspension",
                        _ => "borrowed data remains live",
                    })
                }),
                "{name}: {diagnostic:?}"
            );
        }
    }
}
