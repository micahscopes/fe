use common::InputDb;
use fe_driver::{DriverDataBase, generation::prepare_file};
use hir::analysis::{
    semantic::{EvalOutcome, GenericSubst, SemConstScalar, SemConstValue, eval_body_owner_const},
    ty::ty_check::BodyOwner,
};
use url::Url;

fn check_name(source: &str) {
    let mut db = DriverDataBase::default();
    let file = db.workspace().touch(
        &mut db,
        Url::parse("file:///tmp/record-name.fe").unwrap(),
        Some(source.into()),
    );
    let output = prepare_file(&db, file).unwrap().unwrap();
    let output_file = output
        .workspace()
        .get(&output, &file.url(&db).unwrap())
        .unwrap();
    let module = output.top_mod(output_file);
    assert!(!output.run_on_top_mod(module).has_errors(&output));
    let function = module
        .all_funcs(&output)
        .iter()
        .find(|function| {
            function
                .name(&output)
                .to_opt()
                .is_some_and(|name| name.data(&output) == "check_name")
        })
        .unwrap();
    let owner = BodyOwner::Func(*function, None);
    let value = eval_body_owner_const(
        &output,
        owner,
        GenericSubst::for_body_owner(&output, owner, vec![]),
    );
    let EvalOutcome::Ready(value) = value else {
        panic!("{value:?}")
    };
    assert!(matches!(
        value.value(&output),
        SemConstValue::Scalar {
            value: SemConstScalar::Bool(true),
            ..
        }
    ));
    assert_eq!(
        file.text(&db),
        output_file.text(&output),
        "authored source must stay frozen"
    );
}

#[test]
fn record_name_is_exact_across_inline_and_nested_fragment_boundaries() {
    for name in [
        "Transfer".to_string(),
        format!("Prefix{}Suffix", "abcdefghij".repeat(60)),
    ] {
        // Oracle fragments use 20 bytes and tuple groups of eight, independently
        // of the implementation's inline-string boundary and arity grouping.
        let mut pieces = name
            .as_bytes()
            .chunks(20)
            .map(|chunk| format!("\"{}\"", std::str::from_utf8(chunk).unwrap()))
            .collect::<Vec<_>>();
        while pieces.len() > 8 {
            pieces = pieces
                .chunks(8)
                .map(|group| format!("({})", group.join(",")))
                .collect();
        }
        let oracle = if pieces.len() == 1 {
            pieces.pop().unwrap()
        } else {
            format!("({})", pieces.join(","))
        };
        let source = format!(
            r#"
use core::meta::{{Code, Item, Record, record_name}}
const fn inspect(_ record: Record) -> Code<Item> {{
    let name = record_name(record)
    quote {{ const fn check_name() -> bool {{ std::abi::sol(${{name}}) == std::abi::sol({oracle}) }} }}
}}
#[generate(inspect)]
struct {name} {{}}
"#
        );
        check_name(&source);
    }
}

#[test]
fn record_name_distinguishes_enum_owner_and_selected_variant() {
    check_name(
        r#"
use core::meta::{Code, Item, Record, record_name, variant}
const fn inspect(_ record: Record) -> Code<Item> {
    let owner = record_name(record)
    let selected = record_name(variant(record, 0))
    quote {
        const fn check_name() -> bool {
            std::abi::sol(${owner}) == std::abi::sol("Door")
                && std::abi::sol(${selected}) == std::abi::sol("Open")
        }
    }
}
#[generate(inspect)]
enum Door { Open, Shut }
"#,
    );
}

#[test]
fn record_name_does_not_restore_implicit_record_value_splices() {
    let source = r#"
use core::meta::{Code, Item, Record}
const fn inspect(_ record: Record) -> Code<Item> {
    quote { const fn wrong() -> u32 { std::abi::sol(${record}) } }
}
#[generate(inspect)]
struct Secret {}
"#;
    let mut db = DriverDataBase::default();
    let file = db.workspace().touch(
        &mut db,
        Url::parse("file:///tmp/record-name-negative.fe").unwrap(),
        Some(source.into()),
    );
    assert!(
        prepare_file(&db, file).is_err(),
        "Record is metadata, not an implicit name value"
    );
    assert!(db.generation_inputs().requests(&db).is_empty());
}
