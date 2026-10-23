use camino::Utf8PathBuf;
use common::{InputDb, stdlib::HasBuiltinCore};
use fe_hir::{
    analysis::ty::{
        corelib::{effect_key_may_suspend, suspension_capability_method},
        trait_resolution::PredicateListId,
        ty_check::EffectParamSite,
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
fn suspension_effect_footprint_includes_supertraits() {
    let mut db = HirAnalysisTestDb::default();
    db.initialize_builtin_core();
    let file = db.new_stand_alone(
        Utf8PathBuf::from("suspension_effects.fe"),
        r#"
use core::task::{Pending, Suspend, TaskOutcome}
trait Extended: Suspend {}
trait Unrelated {}
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
"#,
    );
    let (module, _) = db.top_mod(file);
    db.assert_no_diags(module);
    for (name, expected) in [
        ("direct", true),
        ("extended", true),
        ("concrete", true),
        ("unrelated", false),
    ] {
        let func = find_func(&db, module, name);
        let requirements = effect_requirements_for_site(&db, EffectParamSite::Func(func, None));
        assert_eq!(requirements.len(), 1);
        assert_eq!(
            effect_key_may_suspend(
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
