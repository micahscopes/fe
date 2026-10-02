use common::InputDb;
use dir_test::{Fixture, dir_test};
use fe_driver::DriverDataBase;
use hir::analysis::{semantic::eval_body_owner_const_with_args, ty::ty_check::BodyOwner};

const QUERY_FAMILY: &str = r#"
use core::abi::AbiSize

trait Query<R> {
    const fn evaluate() -> R
}

struct AbiFacts<T> {}
impl<T: AbiSize> Query<(u256, bool)> for AbiFacts<T> {
    const fn evaluate() -> (u256, bool) {
        (<T as AbiSize>::HEAD_SIZE, <T as AbiSize>::IS_DYNAMIC)
    }
}

const fn query<F: * -> *, T, R>() -> R where F<T>: Query<R> {
    <F<T> as Query<R>>::evaluate()
}

struct Inner { first: u256, second: bool }
impl AbiSize for Inner {
    const HEAD_SIZE: u256 = 64
    const IS_DYNAMIC: bool = false
}

const fn answer() -> u256 {
    let facts = query<AbiFacts, Inner, (u256, bool)>()
    assert!(!facts.1)
    facts.0
}
"#;

#[test]
fn ordinary_query_family_proves_nominal_field_facts_before_evaluation() {
    let mut db = DriverDataBase::default();
    let file = db.workspace().update(
        &mut db,
        "file:///query_family.fe".parse().unwrap(),
        QUERY_FAMILY.to_owned(),
    );
    let module = db.top_mod(file);
    let diagnostics = db.run_on_ingot(module.ingot(&db));
    assert!(
        !diagnostics.has_errors(&db),
        "{}",
        diagnostics.format_diags(&db)
    );
    let answer = module
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|function| {
            function
                .name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "answer")
        })
        .unwrap();
    let value = eval_body_owner_const_with_args(
        &db,
        BodyOwner::Func(answer),
        hir::analysis::semantic::GenericSubst::for_body_owner(&db, BodyOwner::Func(answer), vec![]),
        vec![],
    )
    .into_ready()
    .expect("query answer evaluates");
    assert_eq!(value.pretty_print(&db), "64");
}

#[test]
fn query_family_prerequisites_remain_required_when_the_method_ignores_them() {
    let constant_body = QUERY_FAMILY.replace(
        "(<T as AbiSize>::HEAD_SIZE, <T as AbiSize>::IS_DYNAMIC)",
        "(64, false)",
    );
    let implementation = "impl AbiSize for Inner {\n    const HEAD_SIZE: u256 = 64\n    const IS_DYNAMIC: bool = false\n}";
    assert!(constant_body.contains(implementation));
    for (source, accepted) in [
        (constant_body.clone(), true),
        (constant_body.replace(implementation, ""), false),
    ] {
        let mut db = DriverDataBase::default();
        let file = db.workspace().update(
            &mut db,
            "file:///query_requirements.fe".parse().unwrap(),
            source,
        );
        let diagnostics = db.run_on_ingot(db.top_mod(file).ingot(&db));
        assert_eq!(
            !diagnostics.has_errors(&db),
            accepted,
            "{}",
            diagnostics.format_diags(&db)
        );
    }
}

fn reflected_source() -> String {
    let policy = QUERY_FAMILY
        .split("const fn answer()")
        .next()
        .unwrap()
        .replace(
            "trait Query<R> {\n    const fn evaluate() -> R\n}",
            "use core::meta::FieldQuery as Query",
        );
    format!(
        "{policy}\nuse core::meta::{{Record, field, field_type, query_field}}\n\
         const fn reflect(_ record: Record) -> (u256, bool) {{\n\
             query_field<AbiFacts, (u256, bool)>(field_type(field(record, 0)))\n\
         }}\n#[generate(reflect)]\nstruct Outer {{ nested: Inner }}"
    )
}

fn evaluate_reflected(source: String) -> Result<String, String> {
    let mut db = DriverDataBase::default();
    let file = db.workspace().update(
        &mut db,
        "file:///reflected_query.fe".parse().unwrap(),
        source,
    );
    let module = db.top_mod(file);
    let diagnostics = db.run_on_ingot(module.ingot(&db));
    if diagnostics.has_errors(&db) {
        return Err(diagnostics.format_diags(&db));
    }
    let requests = fe_driver::elaboration::invocations(&db, file).unwrap();
    let input = requests[0].record_input(&db).unwrap();
    let function = module
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|function| {
            function
                .name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "reflect")
        })
        .unwrap();
    let value = match eval_body_owner_const_with_args(
        &db,
        BodyOwner::Func(function),
        hir::analysis::semantic::GenericSubst::for_body_owner(
            &db,
            BodyOwner::Func(function),
            vec![],
        ),
        vec![input.const_argument().unwrap()],
    ) {
        hir::analysis::semantic::EvalOutcome::Ready(value) => value,
        other => return Err(format!("{other:?}")),
    };
    Ok(value.pretty_print(&db).to_string())
}

#[test]
fn reflected_nominal_field_consumes_checked_trait_facts() {
    assert_eq!(
        evaluate_reflected(reflected_source()).unwrap(),
        "(64, false)"
    );
}

#[test]
fn reflected_query_rejects_missing_unused_prerequisites_and_execution_failure() {
    let implementation = "impl AbiSize for Inner {\n    const HEAD_SIZE: u256 = 64\n    const IS_DYNAMIC: bool = false\n}";
    let constant_body = reflected_source().replace(
        "(<T as AbiSize>::HEAD_SIZE, <T as AbiSize>::IS_DYNAMIC)",
        "(64, false)",
    );
    assert_eq!(
        evaluate_reflected(constant_body.clone()).unwrap(),
        "(64, false)"
    );
    let missing = evaluate_reflected(constant_body.replace(implementation, "")).unwrap_err();
    assert!(missing.contains("no unique proven FieldQuery"), "{missing}");
    let failed = evaluate_reflected(reflected_source().replace(
        "(<T as AbiSize>::HEAD_SIZE, <T as AbiSize>::IS_DYNAMIC)",
        "assert!(false)\n        (64, false)",
    ))
    .unwrap_err();
    assert!(failed.contains("AssertionFailed"), "{failed}");
}

fn nested_source() -> String {
    let primitive = include_str!("../../fe/tests/fixtures/structural_generation/abi.fe");
    let authors = primitive.split("\nmod empty {").next().unwrap();
    let nested = include_str!("../../fe/tests/fixtures/structural_generation/nested_abi.fe")
        .split("\nuse core::abi::{AbiSize, ByteInput, encode_alloc}")
        .next()
        .unwrap();
    format!("{authors}\n{nested}")
}

#[test]
fn unused_nested_encoder_requires_supporting_impls_after_edits_and_restoration() {
    let source = nested_source();
    let encoder = "impl Encode<Sol> for Inner {\n        fn encode(own self, _ ptr: *u8) {\n            encode_field_at<Sol, u256>(self.nonce, ptr, head_pos: 0, 64)\n            encode_field_at<Sol, bool>(self.ready, ptr, head_pos: 32, 64)\n        }\n    }";
    let size = "impl AbiSize for Inner {\n        const HEAD_SIZE: u256 = 64\n        const IS_DYNAMIC: bool = false\n    }";
    assert!(source.contains(encoder));
    assert!(source.contains(size));
    let mut warm = DriverDataBase::default();
    for (edited, failure) in [
        (source.clone(), None),
        (source.replace(encoder, ""), Some("Encode")),
        (source.clone(), None),
        (source.replace(size, ""), Some("FieldQuery")),
        (source, None),
    ] {
        let mut fresh = DriverDataBase::default();
        for db in [&mut warm, &mut fresh] {
            db.workspace()
                .update(db, "file:///nested.fe".parse().unwrap(), edited.clone());
            match (fe_driver::elaboration::prepare(db), failure) {
                (Ok(Some(_)), None) => {}
                (Err(error), Some(expected)) => {
                    assert!(error.message.contains(expected), "{error:?}");
                }
                (Err(error), None) => panic!("unexpected failure: {error:?}"),
                _ => panic!("supporting implementation requirement was lost"),
            }
        }
    }
}

#[test]
fn reflected_query_exhaustion_is_an_error() {
    let source = reflected_source().replace(
        "(<T as AbiSize>::HEAD_SIZE, <T as AbiSize>::IS_DYNAMIC)",
        "while true {}\n        (64, false)",
    );
    let error = evaluate_reflected(source).unwrap_err();
    assert!(error.contains("StepLimitExceeded"), "{error}");
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/reflected_nested_observation", glob: "*.fe")]
fn nested_edits_and_restoration_match_fresh_facts_and_generated_calls(fixture: Fixture<&str>) {
    let base = format!("{}\n{}", nested_source(), fixture.content());
    let alias = base
        .replace(
            "type PairAlias = ingot::left::Inner",
            "type PairAlias = ingot::right::Inner",
        )
        .replace(
            "inner: ingot::left::sample()",
            "inner: ingot::right::sample()",
        );
    let field = base
        .replace("inner: PairAlias", "inner: ingot::right::Inner")
        .replace(
            "inner: ingot::left::sample()",
            "inner: ingot::right::sample()",
        );
    let record = base
        .replace(
            "inner: PairAlias, active: bool",
            "inner: PairAlias, active: bool, extra: u256",
        )
        .replace(
            "inner: ingot::left::sample(), active: false",
            "inner: ingot::left::sample(), active: false, extra: 7",
        );
    // These two edits deliberately change policy facts only. They check dependency
    // invalidation, not the runtime correctness of the edited codec's byte layout.
    // The quoted size implementation reads the same Fe-authored layout as offsets,
    // so changing the head policy changes both reported values.
    let provider = base.replace("abi_field_head_size<T>()", "abi_field_head_size<T>() + 32");
    let supporting = base.replace("const HEAD_SIZE: u256 = 64", "const HEAD_SIZE: u256 = 96");
    let mut warm = DriverDataBase::default();
    let url = url::Url::from_file_path(fixture.path()).expect("fixture path is absolute");
    for (edited, expected) in [
        (base.clone(), "(128, 96)"),
        (alias, "(96, 64)"),
        (base.clone(), "(128, 96)"),
        (field, "(96, 64)"),
        (base.clone(), "(128, 96)"),
        (record, "(160, 96)"),
        (base.clone(), "(128, 96)"),
        (provider, "(224, 160)"),
        (base.clone(), "(128, 96)"),
        (supporting, "(160, 128)"),
        (base, "(128, 96)"),
    ] {
        let mut fresh = DriverDataBase::default();
        let mut texts = Vec::new();
        let mut values = Vec::new();
        for db in [&mut warm, &mut fresh] {
            db.workspace().update(db, url.clone(), edited.clone());
            let output = fe_driver::elaboration::prepare(db).unwrap().unwrap();
            let file = output.workspace().get(&output, &url).unwrap();
            let function = output
                .top_mod(file)
                .all_funcs(&output)
                .iter()
                .copied()
                .find(|function| {
                    function
                        .name(&output)
                        .to_opt()
                        .is_some_and(|name| name.data(&output) == "observed")
                })
                .unwrap();
            let value = eval_body_owner_const_with_args(
                &output,
                BodyOwner::Func(function),
                hir::analysis::semantic::GenericSubst::for_body_owner(
                    &output,
                    BodyOwner::Func(function),
                    vec![],
                ),
                vec![],
            )
            .into_ready()
            .expect("reflected observation evaluates");
            values.push(value.pretty_print(&output));
            texts.push(file.text(&output).to_owned());
        }
        assert_eq!(values[0], values[1], "warm and fresh evaluation differ");
        assert_eq!(values[0], expected);
        assert_eq!(texts[0], texts[1]);
    }
}

#[test]
fn reflected_dynamic_fact_does_not_evaluate_head_size() {
    let source = reflected_source()
        .replace(
            "const HEAD_SIZE: u256 = 64",
            "const HEAD_SIZE: u256 = unavailable()",
        )
        .replace(
            "(<T as AbiSize>::HEAD_SIZE, <T as AbiSize>::IS_DYNAMIC)",
            "(0, <T as AbiSize>::IS_DYNAMIC)",
        );
    let source = format!("{source}\nconst fn unavailable() -> u256 {{ while true {{}}\n0 }}");
    assert_eq!(evaluate_reflected(source.clone()).unwrap(), "(0, false)");
    let demanded = source.replace(
        "(0, <T as AbiSize>::IS_DYNAMIC)",
        "(<T as AbiSize>::HEAD_SIZE, <T as AbiSize>::IS_DYNAMIC)",
    );
    let error = evaluate_reflected(demanded).unwrap_err();
    assert!(error.contains("StepLimitExceeded"), "{error}");
}

#[test]
fn repeated_reflected_queries_share_the_provider_step_budget() {
    let one = reflected_source().replace(
        "(<T as AbiSize>::HEAD_SIZE, <T as AbiSize>::IS_DYNAMIC)",
        "let mut i: u256 = 0\nwhile i < 5000 { i += 1 }\n(64, false)",
    );
    assert_eq!(evaluate_reflected(one.clone()).unwrap(), "(64, false)");
    let repeated = one.replace(
        "query_field<AbiFacts, (u256, bool)>(field_type(field(record, 0)))",
        "let mut i: usize = 0\nwhile i < 100 {\nlet _ = query_field<AbiFacts, (u256, bool)>(field_type(field(record, 0)))\ni += 1\n}\n(64, false)",
    );
    let error = evaluate_reflected(repeated).unwrap_err();
    assert!(error.contains("StepLimitExceeded"), "{error}");
}

#[test]
fn unsupported_query_execution_and_dynamic_codec_are_rejected() {
    let source = reflected_source().replace(
        "(<T as AbiSize>::HEAD_SIZE, <T as AbiSize>::IS_DYNAMIC)",
        "(unavailable(), false)",
    );
    let error = evaluate_reflected(format!(
        "{source}\nextern {{ const fn unavailable() -> u256 }}"
    ))
    .unwrap_err();
    assert!(
        error.contains("query method is not const evaluable"),
        "{error}"
    );

    let base = nested_source();
    let mut warm = DriverDataBase::default();
    for (source, accepted) in [
        (base.clone(), true),
        (
            base.replace(
                "const IS_DYNAMIC: bool = false",
                "const IS_DYNAMIC: bool = true",
            ),
            false,
        ),
        (base, true),
    ] {
        let mut fresh = DriverDataBase::default();
        for db in [&mut warm, &mut fresh] {
            db.workspace().update(
                db,
                "file:///dynamic_nested.fe".parse().unwrap(),
                source.clone(),
            );
            let result = fe_driver::elaboration::prepare(db);
            if accepted {
                assert!(result.unwrap().is_some());
            } else {
                let error = result.err().expect("dynamic codec must be rejected");
                assert!(
                    error.message.contains("assertion") || error.message.contains("Assertion"),
                    "{error:?}"
                );
            }
        }
    }
}
