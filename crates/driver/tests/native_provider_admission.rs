use common::InputDb;
use fe_driver::{DriverDataBase, generation::prepare_file};
use url::Url;

const PROVIDER_CONDITION_FALSE: &str = "use core::derive::Derive\nuse core::meta::{Record, Code, Item}\nuse core::permits::PermitAuthority\n#[canonical]\ntrait Value { const fn value() -> u256 }\npub struct Key {}\n#[permit_root]\nconst ROOT: PermitAuthority<Value<Key>> = PermitAuthority {}\nstruct Provider {}\nimpl Derive<Value> for Provider where false {\n    const fn derive(_ record: Record) -> Code<Item> {\n        quote { const fn value() -> u256 { 7 } }\n    }\n}\n#[permit(ROOT)]\nimpl Value for Key with (Provider)\n#[test]\nfn independent_expected_seven_and_repeat_use() {\n    assert!(Key::value() == 7)\n    assert!(Key::value() + Key::value() == 14)\n}\n";

const PROVIDER_CONDITION_FALSE_FAULT: &str = "use core::derive::Derive\nuse core::meta::{Record, Code, Item}\nuse core::permits::PermitAuthority\n#[canonical]\ntrait Value { const fn value() -> u256 }\npub struct Key {}\n#[permit_root]\nconst ROOT: PermitAuthority<Value<Key>> = PermitAuthority {}\nstruct Provider {}\nimpl Derive<Value> for Provider where false {\n    const fn derive(_ record: Record) -> Code<Item> {\n        let zero: u256 = 0\n        let fault = 1 / zero\n        quote { const fn value() -> u256 { 7 } }\n    }\n}\n#[permit(ROOT)]\nimpl Value for Key with (Provider)\n#[test]\nfn independent_expected_seven_and_repeat_use() {\n    assert!(Key::value() == 7)\n    assert!(Key::value() + Key::value() == 14)\n}\n";

fn prepare(source: &str) -> Result<(), String> {
    let mut db = DriverDataBase::default();
    let file = db.workspace().touch(
        &mut db,
        Url::parse("file:///tmp/native-provider-admission.fe").unwrap(),
        Some(source.into()),
    );
    let result = prepare_file(&db, file).map(|_| ()).map_err(|e| e.cause);
    assert!(db.generation_inputs().functions(&db).is_empty());
    assert_eq!(file.text(&db).as_str(), source);
    result
}

#[test]
fn false_provider_condition_refuses_before_body_execution() {
    for source in [PROVIDER_CONDITION_FALSE, PROVIDER_CONDITION_FALSE_FAULT] {
        let error = prepare(source).expect_err("false declaration cannot enter CTFE");
        assert!(
            error.contains("provider application not admitted"),
            "{error}"
        );
        assert!(error.contains("ConditionsNotAdmitted"), "{error}");
        assert!(!error.contains("division by zero"), "{error}");
    }
}

#[test]
fn true_provider_condition_does_not_hide_body_fault() {
    let source = PROVIDER_CONDITION_FALSE_FAULT.replace("where false", "where true");
    let error = prepare(&source).expect_err("lawful provider must execute its fault");
    assert!(error.contains("division by zero"), "{error}");
    assert!(!error.contains("not admitted"), "{error}");
}

#[test]
fn true_provider_condition_publishes_normally() {
    let source = PROVIDER_CONDITION_FALSE.replace("where false", "where true");
    prepare(&source).expect("ground true provider must be admitted");
}
