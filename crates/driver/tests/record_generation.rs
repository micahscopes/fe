use common::InputDb;
use fe_driver::{DriverDataBase, generation::prepare_file};
use hir::{
    analysis::{
        semantic::{
            EvalOutcome, GenericSubst, SemConstScalar, SemConstValue, eval_body_owner_const,
        },
        ty::ty_check::BodyOwner,
    },
    hir_def::ItemKind,
};
use url::Url;

const SOURCE: &str = "use core::meta::{Code, Item, Record, field_count}\nmod author {\n use core::meta::{Code, Item, Record, field_count}\n const fn boost(_ n: usize) -> u256 { n as u256 + 40 }\n pub const fn make(_ record: Record) -> Code<Item> { let n = field_count(record)\n quote { const fn generated_count() -> u256 { boost(${n}) } } }\n}\nfn boost(_ n: usize) -> u256 { 999 }\n#[generate(author::make)]\nstruct Pair { first: u256, second: bool }\n";

fn input(source: &str) -> (DriverDataBase, common::file::File) {
    let mut db = DriverDataBase::default();
    let file = db.workspace().touch(
        &mut db,
        Url::parse("file:///tmp/explicit-record-provider.fe").unwrap(),
        Some(source.into()),
    );
    (db, file)
}

#[test]
fn explicit_provider_is_evaluated_then_one_output_view_is_admitted() {
    let (db, file) = input(SOURCE);
    let output = prepare_file(&db, file).unwrap().unwrap();
    let output_file = output
        .workspace()
        .get(&output, &file.url(&db).unwrap())
        .unwrap();
    assert_eq!(
        output_file.text(&output),
        file.text(&db),
        "generation must not rewrite source"
    );
    let module = output.top_mod(output_file);
    let function = module
        .all_funcs(&output)
        .into_iter()
        .find(|f| {
            f.name(&output)
                .to_opt()
                .is_some_and(|name| name.data(&output) == "generated_count")
        })
        .unwrap();
    let owner = BodyOwner::Func(*function, None);
    let result = eval_body_owner_const(
        &output,
        owner,
        GenericSubst::for_body_owner(&output, owner, vec![]),
    );
    let EvalOutcome::Ready(value) = result else {
        panic!("{result:?}")
    };
    assert!(
        matches!(value.value(&output), SemConstValue::Scalar { value: SemConstScalar::Int { value }, .. } if value.to_string() == "42")
    );
    assert!(
        prepare_file(&output, output_file)
            .err()
            .expect("output view must refuse a second generation pass")
            .cause
            .contains("authored input view")
    );
    assert!(!db.top_mod(file).all_items(&db).iter().any(|item| matches!(item, ItemKind::Func(f) if f.name(&db).to_opt().is_some_and(|n| n.data(&db) == "generated_count"))));
}

#[test]
fn invalid_output_private_provider_and_fault_keep_their_causes() {
    for (source, cause) in [
        (SOURCE.replace("boost(${n})", "true"), "type mismatch"),
        (
            SOURCE.replace("pub const fn make", "const fn make"),
            "provider is not visible",
        ),
        (
            SOURCE.replace(
                "let n = field_count(record)",
                "let zero: u256 = 0\n let failed = 1 / zero\n let n = field_count(record)",
            ),
            "division by zero",
        ),
    ] {
        let (db, file) = input(&source);
        let error = prepare_file(&db, file).err().expect("intended refusal");
        assert!(error.cause.contains(cause), "{}", error.cause);
        assert!(db.generation_inputs().functions(&db).is_empty());
    }
}

#[test]
fn metadata_provider_keeps_ordinary_move_and_mut_alias_checks() {
    let source = include_str!(
        "../../../oracles/checked-record-generation/review-regressions/repro/borrow_bypass.fe"
    );
    let (db, file) = input(source);
    let error = prepare_file(&db, file)
        .err()
        .expect("invalid move must refuse before provider execution");
    assert!(
        error.cause.contains("move conflict") && error.cause.contains("16-0002"),
        "{error}"
    );
    assert!(error.range.start() > 0.into());

    let alias_only = source.replace(
        "let second = consume(x)",
        "let second = consume(NonCopy { v: 20 })",
    );
    let (db, file) = input(&alias_only);
    let error = prepare_file(&db, file)
        .err()
        .expect("invalid mut alias must refuse before provider execution");
    assert!(
        error.cause.contains("access conflict") && error.cause.contains("16-0001"),
        "{error}"
    );
    assert!(error.range.start() > 0.into());

    let (db, file) = input(include_str!(
        "../../../oracles/checked-record-generation/review-regressions/owner-valid-twins/borrow_bypass.fe"
    ));
    assert!(prepare_file(&db, file).unwrap().is_some());
}

#[test]
fn helper_type_error_retains_original_cause_and_span() {
    let (db, file) = input(include_str!(
        "../../../oracles/checked-record-generation/review-regressions/repro/helper_body_diag_masked.fe"
    ));
    let error = prepare_file(&db, file)
        .err()
        .expect("invalid helper must refuse");
    assert!(
        error.cause.contains("type mismatch")
            && error.cause.contains("usize")
            && error.cause.contains("bool"),
        "{error}"
    );
    assert!(
        !error.cause.contains("internal borrow checking error") && !error.cause.contains("Ctfe("),
        "{error}"
    );
    assert!(error.range.start() > 0.into());
}

#[test]
fn nested_hole_refuses_with_original_constructor_cause() {
    let (db, file) = input(include_str!(
        "../../../oracles/checked-record-generation/review-regressions/repro/nested_hole.fe"
    ));
    let error = prepare_file(&db, file)
        .err()
        .expect("nested holes remain unsupported");
    assert!(
        error.cause.contains("nested quotation holes") && !error.cause.contains("Ctfe("),
        "{error}"
    );
    assert!(error.range.start() > 0.into());
}

#[test]
fn runtime_metadata_refuses_and_compile_time_quotes_stay_valid() {
    for source in [
        include_str!(
            "../../../oracles/checked-record-generation/review-regressions/phase/runtime-quote.fe"
        ),
        include_str!(
            "../../../oracles/checked-record-generation/review-regressions/phase/runtime-tuple.fe"
        ),
        include_str!(
            "../../../oracles/checked-record-generation/review-regressions/phase/runtime-wrapper.fe"
        ),
    ] {
        let (db, file) = input(source);
        let diagnostics = db.run_on_top_mod(db.top_mod(file)).format_diags(&db);
        assert!(
            diagnostics.contains("8-0122") && diagnostics.contains("compile-time-only"),
            "{diagnostics}"
        );
        assert!(!diagnostics.contains("16-0004"), "{diagnostics}");
    }
    for source in [
        include_str!(
            "../../../oracles/checked-record-generation/review-regressions/phase/const-local-quote.fe"
        ),
        include_str!(
            "../../../oracles/checked-record-generation/review-regressions/phase/const-tuple.fe"
        ),
        include_str!(
            "../../../oracles/checked-record-generation/review-regressions/phase/const-quote-value.fe"
        ),
    ] {
        let (db, file) = input(source);
        let diagnostics = db.run_on_top_mod(db.top_mod(file)).format_diags(&db);
        assert!(diagnostics.is_empty(), "{diagnostics}");
    }
}

#[test]
fn checked_metadata_tuple_and_compile_time_wrapper_declaration_stay_valid() {
    let (db, file) = input(include_str!(
        "../../../oracles/checked-record-generation/review-regressions/independent-probes/probe_const_item_tuple_meta.fe"
    ));
    let diagnostics = db.run_on_top_mod(db.top_mod(file)).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let (db, file) = input(include_str!(
        "../../../oracles/checked-record-generation/review-regressions/independent-probes/probe_unused_runtime_struct_with_meta.fe"
    ));
    let diagnostics = db.run_on_top_mod(db.top_mod(file)).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unused declaration may be consumed only at compile time: {diagnostics}"
    );
}

#[test]
fn function_local_static_helper_retains_producer_origin() {
    let (db, file) = input(include_str!(
        "../../../oracles/checked-record-generation/review-regressions/independent-probes/probe_func_local_item_hygiene.fe"
    ));
    let output = prepare_file(&db, file).unwrap().unwrap();
    let output_file = output
        .workspace()
        .get(&output, &file.url(&db).unwrap())
        .unwrap();
    assert!(
        output
            .run_on_top_mod(output.top_mod(output_file))
            .is_empty()
    );
}

#[test]
fn unspliced_producer_local_is_not_a_runtime_capture() {
    let (db, file) = input(include_str!(
        "../../../oracles/checked-record-generation/review-regressions/independent-probes/probe_func_local_capture_refuses.fe"
    ));
    let errors = prepare_file(&db, file)
        .err()
        .expect("unspliced local must refuse");
    let rendered = format!("{errors:?}");
    assert!(
        rendered.contains("undefined variable") && rendered.contains("n"),
        "{rendered}"
    );
}

#[test]
fn provider_with_named_trait_choice_and_evidence_splices_into_generated_item() {
    let source = r#"
use core::meta::{Code, Item, Record, field_count}

trait Base {
    const B: usize
    const fn base(self) -> usize
}

impl Base for u32 {
    const B: usize = 30
    const fn base(self) -> usize { 40 }
}

trait Rate {
    const fn rate(self) -> usize
}

impl Rate for u32
where
    u32: Base
{
    const fn rate(self) -> usize {
        <u32 as Base>::B + self.base()
    }
}

impl Rate for u32 as FlatRate {
    const fn rate(self) -> usize { 10 }
}

const fn rate_with<T>(_ x: T, _ ev: core::obligations::Evidence<Rate<T>>) -> usize {
    with (ev) {
        x.rate()
    }
}

pub const fn make(_ record: Record) -> Code<Item> {
    let fields = field_count(record)
    let zero: u32 = 0
    let proved = rate_with(zero, core::obligations::prove<Rate<u32>>())
    let flat = with (FlatRate) { zero.rate() }
    let total = fields + proved + flat
    quote {
        const fn generated_quote_total() -> usize {
            ${total}
        }
    }
}

#[generate(make)]
struct Pair {
    first: u256,
    second: bool,
}
"#;
    let (db, file) = input(source);
    let output = prepare_file(&db, file).unwrap().unwrap();
    let output_file = output
        .workspace()
        .get(&output, &file.url(&db).unwrap())
        .unwrap();
    let module = output.top_mod(output_file);
    assert!(output.run_on_top_mod(module).is_empty());
    let function = module
        .all_funcs(&output)
        .into_iter()
        .find(|f| {
            f.name(&output)
                .to_opt()
                .is_some_and(|name| name.data(&output) == "generated_quote_total")
        })
        .unwrap();
    let owner = BodyOwner::Func(*function, None);
    let result = eval_body_owner_const(
        &output,
        owner,
        GenericSubst::for_body_owner(&output, owner, vec![]),
    );
    let EvalOutcome::Ready(value) = result else {
        panic!("{result:?}")
    };
    assert!(
        matches!(
            value.value(&output),
            SemConstValue::Scalar {
                value: SemConstScalar::Int { value },
                ..
            } if value.to_string() == "82"
        ),
        "expected 2 + (30 + 40) + 10 = 82, got {value:?}"
    );
}
