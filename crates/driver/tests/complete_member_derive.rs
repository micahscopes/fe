//! An explicit derive header `impl Tr for T with (P)` takes every member from
//! its provider: methods, constants and associated types, admitted by the
//! same conformance checks as a handwritten implementation body.
use common::InputDb;
use fe_driver::{DriverDataBase, generation::prepare_file};
use hir::analysis::{
    semantic::{EvalOutcome, GenericSubst, SemConstScalar, SemConstValue, eval_body_owner_const},
    ty::ty_check::BodyOwner,
};
use url::Url;

const URL: &str = "file:///tmp/complete-member-derive.fe";

fn prepare(source: &str) -> Result<DriverDataBase, String> {
    let mut db = DriverDataBase::default();
    let file = db
        .workspace()
        .touch(&mut db, Url::parse(URL).unwrap(), Some(source.into()));
    let result = prepare_file(&db, file).map_err(|error| error.cause);
    // Nothing is published into the authored view, on success or failure.
    assert!(db.generation_inputs().functions(&db).is_empty());
    assert!(db.generation_inputs().requests(&db).is_empty());
    assert_eq!(file.text(&db).as_str(), source);
    let output = result?.ok_or("expected a prepared view")?;
    let output_file = output
        .workspace()
        .get(&output, &Url::parse(URL).unwrap())
        .unwrap();
    let diagnostics = output.run_on_top_mod(output.top_mod(output_file));
    assert!(
        !diagnostics.has_errors(&output),
        "{}",
        diagnostics.format_diags(&output)
    );
    Ok(output)
}

fn prepare_err(source: &str) -> String {
    match prepare(source) {
        Ok(_) => panic!("expected preparation to refuse"),
        Err(cause) => cause,
    }
}

fn eval(db: &DriverDataBase, name: &str) -> String {
    let file = db.workspace().get(db, &Url::parse(URL).unwrap()).unwrap();
    let function = db
        .top_mod(file)
        .all_funcs(db)
        .iter()
        .copied()
        .find(|function| {
            function
                .name(db)
                .to_opt()
                .is_some_and(|ident| ident.data(db) == name)
        })
        .unwrap_or_else(|| panic!("missing function `{name}`"));
    let owner = BodyOwner::Func(function, None);
    let outcome = eval_body_owner_const(db, owner, GenericSubst::for_body_owner(db, owner, vec![]));
    let EvalOutcome::Ready(value) = outcome else {
        panic!("`{name}` did not evaluate: {outcome:?}")
    };
    let SemConstValue::Scalar {
        value: SemConstScalar::Int { value },
        ..
    } = value.value(db)
    else {
        panic!("`{name}` did not return an integer")
    };
    value.to_string()
}

/// A trait with a constant, an associated type, two required methods and a
/// default method; the provider supplies all but the default. `Square` has
/// three fields, so every expected value below follows from SIDES = 3.
const SHAPE: &str = r#"use core::derive::Derive
use core::meta::{Code, Item, Record, field_count}

trait Shape {
    const SIDES: usize
    type Unit
    const fn area() -> usize
    const fn perimeter() -> usize
    const fn described() -> usize { Self::SIDES * 10 }
}

struct Square { a: u8, b: u8, c: u8 }

struct StableShape {}

impl Derive<Shape> for StableShape {
    const fn derive(_ record: Record) -> Code<Item> {
        let n = field_count(record)
        quote {
            const SIDES: usize = ${n}
            type Unit = u256
            const fn area() -> usize { Self::SIDES * Self::SIDES }
            const fn perimeter() -> usize { 4 * Self::SIDES }
        }
    }
}

impl Shape for Square with (StableShape)

const fn sides() -> usize { Square::SIDES }
const fn area() -> usize { Square::area() }
const fn perimeter() -> usize { Square::perimeter() }
const fn described() -> usize { Square::described() }
const fn unit() -> u256 {
    let unit: <Square as Shape>::Unit = 9
    unit
}
const fn rows() -> u256 {
    let rows: [u256; <Square as Shape>::SIDES] = [7; 3]
    rows[0] + rows[1] + rows[2]
}
"#;

#[test]
fn explicit_derive_supplies_every_member_of_its_header() {
    let output = prepare(SHAPE).unwrap();
    assert_eq!(eval(&output, "sides"), "3");
    assert_eq!(eval(&output, "area"), "9");
    assert_eq!(eval(&output, "perimeter"), "12");
    // The trait's default method stays available and reads the generated const.
    assert_eq!(eval(&output, "described"), "30");
    // The generated associated type and const are usable in types.
    assert_eq!(eval(&output, "unit"), "9");
    assert_eq!(eval(&output, "rows"), "21");
}

#[test]
fn missing_and_extra_generated_members_are_ordinary_conformance_errors() {
    for (from, to, expected) in [
        (
            "            const fn perimeter() -> usize { 4 * Self::SIDES }\n",
            "",
            "not all trait methods are implemented",
        ),
        (
            "            const SIDES: usize = ${n}\n",
            "",
            "missing associated const `SIDES`",
        ),
        (
            "            type Unit = u256\n",
            "",
            "missing associated type `Unit`",
        ),
        (
            "            type Unit = u256\n",
            "            type Unit = u256\n            const fn volume() -> usize { 0 }\n",
            "method `volume` is not defined in trait `Shape`",
        ),
        (
            "            type Unit = u256\n",
            "            type Unit = u256\n            const EXTRA: usize = 1\n",
            "associated const `EXTRA` is not defined in trait",
        ),
    ] {
        assert!(SHAPE.contains(from), "{from:?}");
        let cause = prepare_err(&SHAPE.replace(from, to));
        assert!(cause.contains(expected), "expected {expected:?} in {cause}");
    }
}

#[test]
fn generic_associated_type_members_are_refused() {
    let source = SHAPE.replace("type Unit = u256", "type Unit<T> = T");
    let cause = prepare_err(&source);
    assert!(
        cause.contains("a generated associated type cannot take generic parameters"),
        "{cause}"
    );
}

/// Members spliced from another producer keep their place and resolve their
/// free names where that producer wrote them: the private `four` in `helpers`
/// (4), not the root `four` (99), which would give 297.
#[test]
fn members_from_another_producer_splice_in_order_with_their_own_hygiene() {
    let source = SHAPE
        .replace(
            "struct StableShape {}\n",
            "struct StableShape {}\n\nconst fn four() -> usize { 99 }\n\nmod helpers {\n    use core::meta::{Code, Item}\n    const fn four() -> usize { 4 }\n    pub const fn perimeter_member() -> Code<Item> {\n        quote { const fn perimeter() -> usize { four() * Self::SIDES } }\n    }\n}\n",
        )
        .replace(
            "        let n = field_count(record)\n",
            "        let n = field_count(record)\n        let perimeter = helpers::perimeter_member()\n",
        )
        .replace(
            "            const fn perimeter() -> usize { 4 * Self::SIDES }\n",
            "            ${perimeter}\n",
        );
    let output = prepare(&source).unwrap();
    assert_eq!(eval(&output, "perimeter"), "12");
    assert_eq!(eval(&output, "area"), "9");
}

/// A provider opening written inside a generated member has no authored
/// identity to prepare against. Methods and constants under an explicit
/// header are both generated owners, so either is refused with the
/// generated-owner diagnostic instead of being keyed by quote syntax.
#[test]
fn provider_openings_inside_generated_members_are_refused() {
    let with_counter = SHAPE.replace(
        "struct StableShape {}\n",
        "struct StableShape {}\n\ntrait Score { const fn score() -> usize }\n\nstruct Counter {}\n\nimpl Derive<Score> for Counter {\n    const fn derive(_ record: Record) -> Code<Item> {\n        quote { const fn score() -> usize { 1 } }\n    }\n}\n",
    );
    for (from, to) in [
        (
            "            const SIDES: usize = ${n}\n",
            "            const SIDES: usize = with (Counter) { Square::score() }\n",
        ),
        (
            "            const fn area() -> usize { Self::SIDES * Self::SIDES }\n",
            "            const fn area() -> usize { with (Counter) { Square::score() } }\n",
        ),
    ] {
        assert!(with_counter.contains(from), "{from:?}");
        let cause = prepare_err(&with_counter.replace(from, to));
        assert!(
            cause.contains("generated openings need a portable generated-owner identity"),
            "{cause}"
        );
    }
}

/// RD108: named implementations with associated outputs need an ordinary
/// representative and must agree with it, generated or handwritten.
#[test]
fn named_generated_outputs_need_and_agree_with_an_ordinary_implementation() {
    let named = SHAPE.replace(
        "impl Shape for Square with (StableShape)",
        "impl Shape for Square as Generated with (StableShape)",
    );
    let named = named[..named.find("const fn sides()").unwrap()].to_string()
        + "const fn area() -> usize { with (Generated) { Square::area() } }\n";
    let cause = prepare_err(&named);
    assert!(
        cause.contains("named associated outputs require an ordinary implementation"),
        "{cause}"
    );

    let ordinary = |sides: &str| {
        format!(
            "{named}impl Shape for Square {{\n    const SIDES: usize = {sides}\n    type Unit = u256\n    const fn area() -> usize {{ 100 }}\n    const fn perimeter() -> usize {{ 100 }}\n}}\nconst fn ordinary_area() -> usize {{ Square::area() }}\n"
        )
    };
    // Equal outputs: both coexist and each opening selects its own methods.
    let output = prepare(&ordinary("3")).unwrap();
    assert_eq!(eval(&output, "area"), "9");
    assert_eq!(eval(&output, "ordinary_area"), "100");
    // A different constant is refused at the named declaration.
    let cause = prepare_err(&ordinary("5"));
    assert!(
        cause.contains("named implementation changes a type-readable associated constant"),
        "{cause}"
    );
}
