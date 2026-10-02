use common::{InputDb, config::ArithmeticMode, dependencies::DependencyArguments};
use fe_driver::DriverDataBase;
use hir::analysis::{semantic::eval_body_owner_const_with_args, ty::ty_check::BodyOwner};
use salsa::Setter;
use url::Url;

fn put(db: &mut DriverDataBase, url: &str, text: &str) {
    db.workspace().update(db, url.parse().unwrap(), text.into());
}

fn answer(db: &DriverDataBase) -> String {
    let file = db
        .workspace()
        .get(db, &"file:///project/src/lib.fe".parse().unwrap())
        .unwrap();
    let module = db.top_mod(file);
    let diagnostics = db.run_on_ingot(module.ingot(db));
    assert!(
        !diagnostics.has_errors(db),
        "{}",
        diagnostics.format_diags(db)
    );
    let function = module
        .all_funcs(db)
        .iter()
        .copied()
        .find(|f| {
            f.name(db)
                .to_opt()
                .is_some_and(|name| name.data(db) == "answer")
        })
        .unwrap();
    (match eval_body_owner_const_with_args(
        db,
        BodyOwner::Func(function),
        hir::analysis::semantic::GenericSubst::for_body_owner(
            db,
            BodyOwner::Func(function),
            vec![],
        ),
        vec![],
    ) {
        hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
        outcome => Err(outcome),
    })
    .unwrap()
    .pretty_print(db)
}

#[test]
fn elaboration_input_views_preserve_resolution_and_isolate_real_edits() {
    let mut db = DriverDataBase::default();
    put(
        &mut db,
        "file:///project/fe.toml",
        "name = \"project\"\nversion = \"0.1.0\"\n[dependencies]\npolicy = { path = \"../policy\" }\n",
    );
    put(
        &mut db,
        "file:///policy/fe.toml",
        "name = \"policy\"\nversion = \"0.1.0\"\n",
    );
    put(
        &mut db,
        "file:///policy/src/lib.fe",
        "pub const fn number() -> u256 { 11 }",
    );
    put(
        &mut db,
        "file:///project/src/lib.fe",
        "const fn answer() -> u256 { policy::number() }",
    );
    let project: Url = "file:///project/".parse().unwrap();
    let policy: Url = "file:///policy/".parse().unwrap();
    db.dependency_graph().add_dependency(
        &mut db,
        &project,
        &policy,
        "policy".into(),
        DependencyArguments::default(),
    );
    db.dependency_graph()
        .force_dependency_arithmetic(&mut db, &policy, ArithmeticMode::Unchecked);
    db.compiler_options().set_recovery_mode(&mut db).to(false);
    db.compilation_settings()
        .set_profile(&mut db)
        .to("release".into());
    assert_eq!(answer(&db), "11");

    let mut stage = db.fork_inputs();
    assert_eq!(answer(&stage), "11");
    assert!(!stage.compiler_options().recovery_mode(&stage));
    assert_eq!(stage.compilation_settings().profile(&stage), "release");
    assert_eq!(
        stage
            .dependency_graph()
            .forced_dependency_arithmetic_for(&stage, &policy),
        Some(ArithmeticMode::Unchecked)
    );

    put(
        &mut stage,
        "file:///policy/src/lib.fe",
        "pub const fn number() -> u256 { 23 }",
    );
    assert_eq!(answer(&stage), "23");
    assert_eq!(answer(&db), "11");
    put(
        &mut db,
        "file:///policy/src/lib.fe",
        "pub const fn number() -> u256 { 37 }",
    );
    assert_eq!(answer(&db), "37");
    assert_eq!(answer(&stage), "23");
    assert_eq!(answer(&db.fork_inputs()), "37");
}

#[test]
fn removed_authored_files_do_not_reappear_in_a_new_view() {
    let mut db = DriverDataBase::default();
    let url: Url = "file:///scratch.fe".parse().unwrap();
    put(&mut db, url.as_str(), "struct Gone {} ");
    let old = db.fork_inputs();
    db.workspace().remove(&mut db, &url);
    let new = db.fork_inputs();
    assert!(old.workspace().get(&old, &url).is_some());
    assert!(new.workspace().get(&new, &url).is_none());
}

#[test]
fn source_invocations_resolve_aliases_in_each_record_owner_scope() {
    use fe_driver::elaboration::invocations;
    use hir::analysis::ty::ty_def::TyId;
    let mut db = DriverDataBase::default();
    let url: Url = "file:///invocation.fe".parse().unwrap();
    put(
        &mut db,
        url.as_str(),
        r#"
mod strategies {
    pub const fn first() -> u256 { 1 }
    pub const fn second() -> u256 { 2 }
}
mod a {
    use ingot::strategies::first as author
    #[generate(author)]
    struct Packet { value: u256 }
}
mod b {
    use ingot::strategies::second as author
    #[generate(author)]
    struct Packet { flag: bool }
}
"#,
    );
    let file = db.workspace().get(&db, &url).unwrap();
    let module = db.top_mod(file);
    let requests = invocations(&db, file).unwrap();
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].owner, requests[1].owner);
    for name in ["first", "second"] {
        let func = module
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|f| f.name(&db).to_opt().is_some_and(|n| n.data(&db) == name))
            .unwrap();
        let expected = TyId::func(&db, func.as_callable(&db).unwrap());
        assert_eq!(
            requests.iter().filter(|r| r.provider == expected).count(),
            1
        );
    }
}

#[test]
fn malformed_and_inaccessible_invocations_do_not_become_grants() {
    use fe_driver::elaboration::{InvocationErrorKind as Kind, invocations};
    let mut db = DriverDataBase::default();
    let url: Url = "file:///invocation.fe".parse().unwrap();
    for (source, expected) in [
        ("#[generate]\nstruct Packet {}", Kind::Form),
        ("#[generate(author, other)]\nstruct Packet {}", Kind::Form),
        ("#[generate(author = 1)]\nstruct Packet {}", Kind::Form),
        ("#[generate(missing)]\nstruct Packet {}", Kind::Resolution),
        (
            "struct Author {}\n#[generate(Author)]\nstruct Packet {}",
            Kind::Provider,
        ),
        ("#[generate(author)]\nfn f() {}", Kind::Target),
        (
            "mod hidden { const fn author() -> u256 { 1 } }\n#[generate(hidden::author)]\nstruct Packet {}",
            Kind::Visibility,
        ),
    ] {
        put(&mut db, url.as_str(), source);
        let file = db.workspace().get(&db, &url).unwrap();
        let error = invocations(&db, file).unwrap_err();
        assert_eq!(error.kind, expected, "{source}");
    }
}

#[test]
fn record_inputs_preserve_field_types_and_reject_foreign_descriptors() {
    use fe_driver::elaboration::{RecordInputError, invocations};
    use hir::analysis::ty::ty_def::TyId;
    let mut db = DriverDataBase::default();
    let url: Url = "file:///records.fe".parse().unwrap();
    put(
        &mut db,
        url.as_str(),
        r#"
pub const fn author() -> u256 { 0 }
type Word = u256
#[generate(author)]
struct Packet { nonce: Word, ready: bool, code: u64 }
mod other {
    #[generate(ingot::author)]
    struct Packet { nonce: u256 }
}
#[generate(author)]
struct Empty {}
"#,
    );
    let file = db.workspace().get(&db, &url).unwrap();
    let requests = invocations(&db, file).unwrap();
    let records: Vec<_> = requests
        .iter()
        .map(|request| request.record_input(&db).unwrap())
        .collect();
    let packet = records
        .iter()
        .find(|r| r.descriptors().count() == 3)
        .unwrap();
    let fields: Vec<_> = packet
        .descriptors()
        .map(|descriptor| packet.field(&descriptor).unwrap())
        .collect();
    assert_eq!(fields[0].name.data(&db), "nonce");
    assert_eq!(fields[0].ty, TyId::u256(&db));
    assert_eq!(fields[1].name.data(&db), "ready");
    assert_eq!(fields[1].ty, TyId::bool(&db));
    assert_eq!(fields[2].name.data(&db), "code");
    assert_eq!(fields[2].ty.pretty_print(&db), "u64");
    let foreign = records
        .iter()
        .find(|r| r.descriptors().count() == 1)
        .unwrap()
        .descriptors()
        .next()
        .unwrap();
    assert_eq!(
        packet.field(&foreign).unwrap_err(),
        RecordInputError::WrongOwner
    );
    assert!(records.iter().any(|r| r.descriptors().count() == 0));

    // Even identical source in a different database is a different input view.
    let stage = db.fork_inputs();
    let stage_file = stage.workspace().get(&stage, &url).unwrap();
    for request in invocations(&stage, stage_file).unwrap() {
        let record = request.record_input(&stage).unwrap();
        for descriptor in record.descriptors() {
            assert_eq!(
                packet.field(&descriptor).unwrap_err(),
                RecordInputError::WrongOwner
            );
        }
    }
}

#[test]
fn field_type_edits_match_fresh_resolution_and_invalid_types_are_rejected() {
    use fe_driver::elaboration::{RecordInputError, invocations};
    let mut db = DriverDataBase::default();
    let url: Url = "file:///edits.fe".parse().unwrap();
    for field_ty in ["u256", "bool", "u64", "Missing"] {
        let source = format!(
            "const fn author() -> u256 {{ 0 }}\ntype Word = {field_ty}\n#[generate(author)]\nstruct Packet {{ value: Word }}"
        );
        put(&mut db, url.as_str(), &source);
        let inspect = |view: &DriverDataBase| {
            let file = view.workspace().get(view, &url).unwrap();
            let requests = invocations(view, file).unwrap();
            let record = requests[0].record_input(view)?;
            let descriptor = record.descriptors().next().unwrap();
            Ok::<_, RecordInputError>(record.field(&descriptor)?.ty.pretty_print(view).clone())
        };
        let mut fresh = DriverDataBase::default();
        put(&mut fresh, url.as_str(), &source);
        assert_eq!(inspect(&db), inspect(&fresh));
        if field_ty == "Missing" {
            assert_eq!(inspect(&db), Err(RecordInputError::InvalidField(0)));
        } else {
            assert_eq!(inspect(&db), Ok(field_ty.to_owned()));
        }
    }
    put(
        &mut db,
        url.as_str(),
        "const fn author() -> u256 { 0 }\n#[generate(author)]\nstruct Packet<T> { value: T }",
    );
    let file = db.workspace().get(&db, &url).unwrap();
    assert!(matches!(
        invocations(&db, file).unwrap()[0].record_input(&db),
        Err(RecordInputError::GenericRecord)
    ));
}

#[test]
fn reusable_fe_provider_observes_actual_heterogeneous_fields() {
    use fe_driver::elaboration::invocations;
    let mut db = DriverDataBase::default();
    let url: Url = "file:///reflection.fe".parse().unwrap();
    put(
        &mut db,
        url.as_str(),
        r#"
mod strategies {
    use core::meta::{Record, field_count, field, field_type, type_is}
    pub const fn inspect(_ record: Record) -> u256 {
        let mut result: u256 = 0
        let mut i: usize = 0
        while i < field_count(record) {
            let ty = field_type(field(record, i))
            let digit: u256 = if type_is<u256>(ty) { 1 }
                else if type_is<bool>(ty) { 2 }
                else if type_is<u64>(ty) { 3 }
                else { 9 }
            result = result * 10 + digit
            i += 1
        }
        result
    }
}
type Word = u256
#[generate(strategies::inspect)]
struct Packet { nonce: Word, ready: bool, code: u64 }
#[generate(strategies::inspect)]
struct Reordered { code: u64, ready: bool, nonce: u256 }
#[generate(strategies::inspect)]
struct Empty {}
mod nominal { pub struct Word { value: u256 } }
#[generate(strategies::inspect)]
struct Wrapped { nonce: nominal::Word }
"#,
    );
    let file = db.workspace().get(&db, &url).unwrap();
    let module = db.top_mod(file);
    let diagnostics = db.run_on_ingot(module.ingot(&db));
    assert!(
        !diagnostics.has_errors(&db),
        "{}",
        diagnostics.format_diags(&db)
    );
    let provider = module
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|f| {
            f.name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "inspect")
        })
        .unwrap();
    for request in invocations(&db, file).unwrap() {
        let input = request.record_input(&db).unwrap().const_argument().unwrap();
        let result = (match eval_body_owner_const_with_args(
            &db,
            BodyOwner::Func(provider),
            hir::analysis::semantic::GenericSubst::for_body_owner(
                &db,
                BodyOwner::Func(provider),
                vec![],
            ),
            vec![input],
        ) {
            hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
            outcome => Err(outcome),
        })
        .unwrap();
        let expected = match request.owner.name(&db).to_opt().unwrap().data(&db).as_str() {
            "Packet" => "123",
            "Reordered" => "321",
            "Empty" => "0",
            "Wrapped" => "9",
            _ => unreachable!(),
        };
        assert_eq!(result.pretty_print(&db), expected);
    }
}

#[test]
fn structural_intrinsics_reject_fabricated_values_and_out_of_bounds_fields() {
    use fe_driver::elaboration::invocations;
    use hir::analysis::semantic::{SemConstValue, bool_const, sem_const_ty, struct_const};
    let mut db = DriverDataBase::default();
    let url: Url = "file:///reflection_errors.fe".parse().unwrap();
    put(
        &mut db,
        url.as_str(),
        r#"
use core::meta::{Record, field, field_type, type_is}
const fn inspect(_ record: Record) -> bool { type_is<bool>(field_type(field(record, 0))) }
const fn unsupported(_ record: Record) -> bool { type_is<(u256, bool)>(field_type(field(record, 0))) }
#[generate(inspect)]
struct Empty {}
#[generate(inspect)]
struct One { value: bool }
"#,
    );
    let file = db.workspace().get(&db, &url).unwrap();
    let module = db.top_mod(file);
    let diagnostics = db.run_on_ingot(module.ingot(&db));
    assert!(
        !diagnostics.has_errors(&db),
        "{}",
        diagnostics.format_diags(&db)
    );
    let provider_named = |name: &str| {
        module
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|func| {
                func.name(&db)
                    .to_opt()
                    .is_some_and(|ident| ident.data(&db) == name)
            })
            .unwrap()
    };
    let provider = provider_named("inspect");
    let requests = invocations(&db, file).unwrap();
    let input = requests
        .iter()
        .find(|r| r.owner.name(&db).to_opt().unwrap().data(&db) == "Empty")
        .unwrap()
        .record_input(&db)
        .unwrap()
        .const_argument()
        .unwrap();
    assert!(matches!(
        input.value(&db).clone(),
        SemConstValue::Structural { .. }
    ));
    let fake = struct_const(
        &db,
        sem_const_ty(&db, input),
        vec![bool_const(&db, false)].into(),
    );
    for argument in [input, fake] {
        assert!(
            (match eval_body_owner_const_with_args(
                &db,
                BodyOwner::Func(provider),
                hir::analysis::semantic::GenericSubst::for_body_owner(
                    &db,
                    BodyOwner::Func(provider),
                    vec![]
                ),
                vec![argument]
            ) {
                hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
                outcome => Err(outcome),
            })
            .is_err()
        );
    }
    let one = requests
        .iter()
        .find(|r| r.owner.name(&db).to_opt().unwrap().data(&db) == "One")
        .unwrap()
        .record_input(&db)
        .unwrap()
        .const_argument()
        .unwrap();
    let error = (match eval_body_owner_const_with_args(
        &db,
        BodyOwner::Func(provider_named("unsupported")),
        hir::analysis::semantic::GenericSubst::for_body_owner(
            &db,
            BodyOwner::Func(provider_named("unsupported")),
            vec![],
        ),
        vec![one],
    ) {
        hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
        outcome => Err(outcome),
    })
    .unwrap_err();
    assert!(
        matches!(error, hir::analysis::semantic::EvalOutcome::Failed(hir::analysis::semantic::EvalFailure::Ctfe(hir::analysis::semantic::CtfeError::InvalidOperation { message, .. }))
        if message == "type_is currently requires bool, u64 or u256")
    );
}

#[test]
fn untaken_source_cannot_construct_a_reflection_grant() {
    let mut db = DriverDataBase::default();
    let url: Url = "file:///forged_reflection.fe".parse().unwrap();
    put(
        &mut db,
        url.as_str(),
        r#"
use core::meta::{Record, field_count}
const fn bad() -> usize {
    if false { field_count(Record { seal: false }) } else { 0 }
}
"#,
    );
    let file = db.workspace().get(&db, &url).unwrap();
    let diagnostics = db.run_on_ingot(db.top_mod(file).ingot(&db));
    assert!(diagnostics.has_errors(&db));
    let rendered = diagnostics.format_diags(&db);
    assert!(rendered.contains("`seal` is not visible"), "{rendered}");
}

#[test]
fn reflection_evaluation_tracks_field_type_edits_and_restoration() {
    use fe_driver::elaboration::invocations;
    let mut db = DriverDataBase::default();
    let url: Url = "file:///reflection_edits.fe".parse().unwrap();
    let evaluate = |view: &DriverDataBase| {
        let file = view.workspace().get(view, &url).unwrap();
        let module = view.top_mod(file);
        let diagnostics = view.run_on_ingot(module.ingot(view));
        assert!(
            !diagnostics.has_errors(view),
            "{}",
            diagnostics.format_diags(view)
        );
        let request = invocations(view, file).unwrap().pop().unwrap();
        let argument = request
            .record_input(view)
            .unwrap()
            .const_argument()
            .unwrap();
        (match eval_body_owner_const_with_args(
            view,
            BodyOwner::Func(module.all_funcs(view)[0]),
            hir::analysis::semantic::GenericSubst::for_body_owner(
                view,
                BodyOwner::Func(module.all_funcs(view)[0]),
                vec![],
            ),
            vec![argument],
        ) {
            hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
            outcome => Err(outcome),
        })
        .unwrap()
        .pretty_print(view)
    };
    for (ty, expected) in [("u256", "false"), ("bool", "true"), ("u256", "false")] {
        let source = format!(
            r#"
use core::meta::{{Record, field, field_type, type_is}}
const fn inspect(_ record: Record) -> bool {{ type_is<bool>(field_type(field(record, 0))) }}
type Word = {ty}
#[generate(inspect)]
struct Packet {{ value: Word }}
"#
        );
        put(&mut db, url.as_str(), &source);
        let mut fresh = DriverDataBase::default();
        put(&mut fresh, url.as_str(), &source);
        assert_eq!(evaluate(&db), expected);
        assert_eq!(evaluate(&fresh), expected);
    }
}

const ACCESSOR_SOURCE: &str = r#"
mod authors {
    use core::meta::{Record, field, field_type, DeclarationName, DeclarationType, BodyExpr, FunctionDefinition}
    pub const fn accessor(_ record: Record) -> FunctionDefinition<1, 2> {
        let f = field(record, 2)
        FunctionDefinition {
            name: DeclarationName::FieldPrefix("get_", f),
            public: true,
            const_function: true,
            parameters: [DeclarationType::Record(record)],
            return_type: DeclarationType::Field(field_type(f)),
            expressions: [BodyExpr::Parameter(0), BodyExpr::Field(0, f)],
            result: 1,
        }
    }
}
mod owner {
    #[generate(ingot::authors::accessor)]
    pub struct Packet { nonce: u256, ready: bool, code: u64 }
    pub const fn sample() -> Packet { Packet { nonce: 42, ready: true, code: 258 } }
}
mod other {
    #[generate(ingot::authors::accessor)]
    pub struct Packet { nonce: u256, ready: bool, code: u64 }
}
"#;

fn function_definitions<'db>(
    db: &'db DriverDataBase,
    url: &Url,
) -> Vec<(
    fe_driver::elaboration::RecordInput<'db>,
    hir::analysis::semantic::SemConstId<'db>,
)> {
    use hir::{
        analysis::ty::ty_def::{TyBase, TyData},
        hir_def::CallableDef,
    };
    let file = db.workspace().get(db, url).unwrap();
    let diagnostics = db.run_on_ingot(db.top_mod(file).ingot(db));
    assert!(
        !diagnostics.has_errors(db),
        "{}",
        diagnostics.format_diags(db)
    );
    fe_driver::elaboration::invocations(db, file)
        .unwrap()
        .into_iter()
        .map(|request| {
            let TyData::TyBase(TyBase::Func(CallableDef::Func(provider))) =
                request.provider.data(db)
            else {
                panic!("expected provider function")
            };
            let input = request.record_input(db).unwrap();
            let value = (match eval_body_owner_const_with_args(
                db,
                BodyOwner::Func(*provider),
                hir::analysis::semantic::GenericSubst::for_body_owner(
                    db,
                    BodyOwner::Func(*provider),
                    vec![],
                ),
                vec![input.const_argument().unwrap()],
            ) {
                hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
                outcome => Err(outcome),
            })
            .unwrap();
            (input, value)
        })
        .collect()
}

#[test]
fn fe_authored_function_is_checked_in_owner_scope_and_consumed() {
    use fe_driver::elaboration::construction::admit_function;
    let mut db = DriverDataBase::default();
    let url: Url = "file:///accessor.fe".parse().unwrap();
    put(&mut db, url.as_str(), ACCESSOR_SOURCE);
    let definitions = function_definitions(&db, &url);
    let (input, value) = &definitions[0];
    let mut output = admit_function(input, *value).unwrap();
    let file = output.workspace().get(&output, &url).unwrap();
    let source = format!(
        "{}\nconst fn answer() -> u64 {{ owner::get_code(owner::sample()) }}",
        file.text(&output)
    );
    put(&mut output, url.as_str(), &source);
    let diagnostics = output.run_on_ingot(output.top_mod(file).ingot(&output));
    assert!(
        !diagnostics.has_errors(&output),
        "{}",
        diagnostics.format_diags(&output)
    );
    let answer = output
        .top_mod(file)
        .all_funcs(&output)
        .iter()
        .copied()
        .find(|f| {
            f.name(&output)
                .to_opt()
                .is_some_and(|n| n.data(&output) == "answer")
        })
        .unwrap();
    assert_eq!(
        (match eval_body_owner_const_with_args(
            &output,
            BodyOwner::Func(answer),
            hir::analysis::semantic::GenericSubst::for_body_owner(
                &output,
                BodyOwner::Func(answer),
                vec![]
            ),
            vec![]
        ) {
            hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
            outcome => Err(outcome),
        })
        .unwrap()
        .pretty_print(&output),
        "258"
    );
    assert!(
        !db.workspace()
            .get(&db, &url)
            .unwrap()
            .text(&db)
            .contains("fn get_code")
    );
}

#[test]
fn construction_rejects_wrong_owner_and_checks_even_unused_body_nodes() {
    use fe_driver::elaboration::construction::{ConstructionError, admit_function};
    let mut db = DriverDataBase::default();
    let url: Url = "file:///accessor_errors.fe".parse().unwrap();
    put(&mut db, url.as_str(), ACCESSOR_SOURCE);
    let definitions = function_definitions(&db, &url);
    assert!(matches!(
        admit_function(&definitions[0].0, definitions[1].1),
        Err(ConstructionError::Authority)
    ));
    for source in [
        ACCESSOR_SOURCE.replace("return_type: DeclarationType::Field(field_type(f))", "return_type: DeclarationType::Bool"),
        ACCESSOR_SOURCE.replace("FunctionDefinition<1, 2>", "FunctionDefinition<1, 5>").replace(
            "[BodyExpr::Parameter(0), BodyExpr::Field(0, f)]",
            "[BodyExpr::Parameter(0), BodyExpr::Field(0, f), BodyExpr::Bool(true), BodyExpr::U256(3), BodyExpr::Add(2, 3)]"),
    ] {
        put(&mut db, url.as_str(), &source);
        let definitions = function_definitions(&db, &url);
        assert!(matches!(admit_function(&definitions[0].0, definitions[0].1), Err(ConstructionError::Output(_))));
    }
}

const ACCESSOR_POLICIES: &str = r#"
mod authors {
    use core::meta::{Record, FieldType, field_count, field, field_type, type_is, DeclarationName, DeclarationType, BodyExpr, FunctionDefinition}
    trait Select: Copy { const fn include(self, _ ty: FieldType) -> bool }
    struct All {}
    struct Flags {}
    impl Copy for All {}
    impl Copy for Flags {}
    impl Select for All { const fn include(self, _ ty: FieldType) -> bool { true } }
    impl Select for Flags { const fn include(self, _ ty: FieldType) -> bool { type_is<bool>(ty) } }
    const fn generate(_ record: Record) -> [Option<FunctionDefinition<1, 2>>; 32] uses (policy: Select) {
        let mut definitions: [Option<FunctionDefinition<1, 2>>; 32] = [Option::None; 32]
        let mut i: usize = 0
        while i < field_count(record) {
            let f = field(record, i)
            if policy.include(field_type(f)) {
                definitions[i] = Option::Some(FunctionDefinition {
                    name: DeclarationName::FieldPrefix("get_", f),
                    public: true,
                    const_function: true,
                    parameters: [DeclarationType::Record(record)],
                    return_type: DeclarationType::Field(field_type(f)),
                    expressions: [BodyExpr::Parameter(0), BodyExpr::Field(0, f)],
                    result: 1,
                })
            }
            i += 1
        }
        definitions
    }
    pub const fn all(_ record: Record) -> [Option<FunctionDefinition<1, 2>>; 32] {
        with (Select = All {}) { generate(record) }
    }
    pub const fn flags(_ record: Record) -> [Option<FunctionDefinition<1, 2>>; 32] {
        with (Select = Flags {}) { generate(record) }
    }
}
mod owner {
    #[generate(ingot::authors::all)]
    pub struct Packet { nonce: u256, ready: bool, code: u64 }
    pub const fn sample() -> Packet { Packet { nonce: 42, ready: true, code: 258 } }
}
"#;

#[test]
fn scoped_fe_policy_selects_a_checked_accessor_set() {
    use fe_driver::elaboration::construction::admit_functions;
    let mut db = DriverDataBase::default();
    let url: Url = "file:///accessor_policy.fe".parse().unwrap();
    for (provider, expected_names) in [
        ("all", vec!["get_nonce", "get_ready", "get_code"]),
        ("flags", vec!["get_ready"]),
    ] {
        let source = ACCESSOR_POLICIES.replace(
            "#[generate(ingot::authors::all)]",
            &format!("#[generate(ingot::authors::{provider})]"),
        );
        put(&mut db, url.as_str(), &source);
        let definitions = function_definitions(&db, &url);
        let mut output = admit_functions(&definitions[0].0, definitions[0].1).unwrap();
        let file = output.workspace().get(&output, &url).unwrap();
        let mut fresh = DriverDataBase::default();
        put(&mut fresh, url.as_str(), &source);
        let fresh_definitions = function_definitions(&fresh, &url);
        let fresh_output =
            admit_functions(&fresh_definitions[0].0, fresh_definitions[0].1).unwrap();
        let fresh_file = fresh_output.workspace().get(&fresh_output, &url).unwrap();
        assert_eq!(file.text(&output), fresh_file.text(&fresh_output));
        for name in ["get_nonce", "get_ready", "get_code"] {
            assert_eq!(
                file.text(&output).contains(&format!("fn {name}(")),
                expected_names.contains(&name)
            );
        }
        let caller = format!(
            "{}\nconst fn answer() -> bool {{ owner::get_ready(owner::sample()) }}",
            file.text(&output)
        );
        put(&mut output, url.as_str(), &caller);
        let diagnostics = output.run_on_ingot(output.top_mod(file).ingot(&output));
        assert!(
            !diagnostics.has_errors(&output),
            "{}",
            diagnostics.format_diags(&output)
        );
        let answer = output
            .top_mod(file)
            .all_funcs(&output)
            .iter()
            .copied()
            .find(|f| {
                f.name(&output)
                    .to_opt()
                    .is_some_and(|n| n.data(&output) == "answer")
            })
            .unwrap();
        assert_eq!(
            (match eval_body_owner_const_with_args(
                &output,
                BodyOwner::Func(answer),
                hir::analysis::semantic::GenericSubst::for_body_owner(
                    &output,
                    BodyOwner::Func(answer),
                    vec![]
                ),
                vec![]
            ) {
                hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
                outcome => Err(outcome),
            })
            .unwrap()
            .pretty_print(&output),
            "true"
        );
        drop(diagnostics);
        if provider == "flags" {
            let missing = format!(
                "{}\nfn missing() -> u64 {{ owner::get_code(owner::sample()) }}",
                file.text(&output)
            );
            put(&mut output, url.as_str(), &missing);
            let diagnostics = output.run_on_ingot(output.top_mod(file).ingot(&output));
            assert!(diagnostics.has_errors(&output));
            assert!(diagnostics.format_diags(&output).contains("get_code"));
        }
    }
}

#[test]
fn optional_definition_repetition_does_not_make_noncopy_payloads_copyable() {
    let mut db = DriverDataBase::default();
    let url: Url = "file:///optional_copy.fe".parse().unwrap();
    let good = r#"
struct Unique { value: u256 }
fn require_copy<T: Copy>(_ value: T) {}
fn good() {
    let values: [Option<u256>; 2] = [Option::None; 2]
    require_copy(values)
}

"#;
    put(&mut db, url.as_str(), good);
    let file = db.workspace().get(&db, &url).unwrap();
    let diagnostics = db.run_on_ingot(db.top_mod(file).ingot(&db));
    assert!(
        !diagnostics.has_errors(&db),
        "{}",
        diagnostics.format_diags(&db)
    );
    drop(diagnostics);
    put(
        &mut db,
        url.as_str(),
        &format!("{good}\nfn bad() {{ require_copy(Option::Some(Unique {{ value: 1 }})) }}"),
    );
    let diagnostics = db.run_on_ingot(db.top_mod(file).ingot(&db));
    assert!(diagnostics.has_errors(&db));
    assert!(diagnostics.format_diags(&db).contains("Copy"));
}

#[test]
fn project_preparation_admits_all_invocations_before_checking_authored_consumers() {
    let mut db = DriverDataBase::default();
    let url: Url = "file:///integrated_accessors.fe".parse().unwrap();
    let source = format!(
        "{ACCESSOR_SOURCE}\nconst fn answer() -> u64 {{ owner::get_code(owner::sample()) }}\nfn other_answer(_ value: other::Packet) -> u64 {{ other::get_code(value) }}"
    );
    put(&mut db, url.as_str(), &source);
    let output = fe_driver::elaboration::prepare(&db).unwrap().unwrap();
    let file = output.workspace().get(&output, &url).unwrap();
    assert_eq!(file.text(&output).matches("fn get_code(").count(), 2);
    let answer = output
        .top_mod(file)
        .all_funcs(&output)
        .iter()
        .copied()
        .find(|f| {
            f.name(&output)
                .to_opt()
                .is_some_and(|n| n.data(&output) == "answer")
        })
        .unwrap();
    assert_eq!(
        (match eval_body_owner_const_with_args(
            &output,
            BodyOwner::Func(answer),
            hir::analysis::semantic::GenericSubst::for_body_owner(
                &output,
                BodyOwner::Func(answer),
                vec![]
            ),
            vec![]
        ) {
            hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
            outcome => Err(outcome),
        })
        .unwrap()
        .pretty_print(&output),
        "258"
    );
    assert_eq!(db.workspace().get(&db, &url).unwrap().text(&db), &source);
    put(
        &mut db,
        url.as_str(),
        &source.replace(
            "return_type: DeclarationType::Field(field_type(f))",
            "return_type: DeclarationType::Bool",
        ),
    );
    assert!(fe_driver::elaboration::prepare(&db).is_err());
    assert_eq!(
        (match eval_body_owner_const_with_args(
            &output,
            BodyOwner::Func(answer),
            hir::analysis::semantic::GenericSubst::for_body_owner(
                &output,
                BodyOwner::Func(answer),
                vec![]
            ),
            vec![]
        ) {
            hir::analysis::semantic::EvalOutcome::Ready(value) => Ok(value),
            outcome => Err(outcome),
        })
        .unwrap()
        .pretty_print(&output),
        "258"
    );
}

#[test]
fn project_preparation_rejects_unchecked_entry_premises_and_untaken_invalid_source() {
    let mut db = DriverDataBase::default();
    let url: Url = "file:///invalid_entry.fe".parse().unwrap();
    let constrained = ACCESSOR_SOURCE
        .replace(
            "pub const fn accessor",
            "trait Need {}\npub const fn accessor",
        )
        .replace(
            "-> FunctionDefinition<1, 2> {",
            "-> FunctionDefinition<1, 2> where u256: Need {",
        );
    put(&mut db, url.as_str(), &constrained);
    let error = match fe_driver::elaboration::prepare(&db) {
        Err(error) => error,
        Ok(_) => panic!("unchecked entry premise admitted"),
    };
    assert!(error.message.contains("without entry requirements"));
    let invalid = ACCESSOR_SOURCE.replace(
        "let f = field(record, 2)",
        "if false { let invalid: bool = 1 }\nlet f = field(record, 2)",
    );
    put(&mut db, url.as_str(), &invalid);
    assert!(fe_driver::elaboration::prepare(&db).is_err());
}

#[test]
fn selected_generation_skips_unrelated_ingots_but_includes_dependencies() {
    let mut db = DriverDataBase::default();
    let selected: Url = "file:///selected/".parse().unwrap();
    let other: Url = "file:///other/".parse().unwrap();
    put(
        &mut db,
        "file:///selected/fe.toml",
        "name = \"selected\"\nversion = \"0.1.0\"\n",
    );
    put(
        &mut db,
        "file:///other/fe.toml",
        "name = \"other\"\nversion = \"0.1.0\"\n",
    );
    put(&mut db, "file:///selected/src/lib.fe", ACCESSOR_SOURCE);
    put(
        &mut db,
        "file:///other/src/lib.fe",
        "#[generate(missing)]\nstruct Other {}",
    );
    assert!(
        fe_driver::elaboration::prepare_target(&db, &selected)
            .unwrap()
            .is_some()
    );
    assert!(fe_driver::elaboration::prepare(&db).is_err());
    db.dependency_graph().add_dependency(
        &mut db,
        &selected,
        &other,
        "other".into(),
        DependencyArguments::default(),
    );
    let error = match fe_driver::elaboration::prepare_target(&db, &selected) {
        Err(error) => error,
        Ok(_) => panic!("dependency invocation was skipped"),
    };
    assert_eq!(error.url.as_str(), "file:///other/src/lib.fe");
}

fn calling_accessor_source() -> String {
    let source = ACCESSOR_SOURCE
        .replace("BodyExpr, FunctionDefinition", "BodyExpr, FunctionDefinition, FunctionCall, CallArgument")
        .replace("FunctionDefinition<1, 2>", "FunctionDefinition<1, 3>")
        .replace(
            "expressions: [BodyExpr::Parameter(0), BodyExpr::Field(0, f)]",
            r#"expressions: [BodyExpr::Parameter(0), BodyExpr::Field(0, f),
                BodyExpr::Call(FunctionCall {
                    path: "ingot::support::keep",
                    types: [Option::Some(DeclarationType::Field(field_type(f))), Option::None],
                    arguments: [Option::Some(CallArgument { label: Option::Some("value"), expression: 1 }), Option::None, Option::None, Option::None],
                })]"#,
        )
        .replace("result: 1", "result: 2");
    format!(
        r#"{source}
mod support {{
    pub trait Allowed {{}}
    impl Allowed for u64 {{}}
    pub const fn keep<T: Allowed + Copy>(value: T) -> T {{ value }}
}}
const fn answer() -> u64 {{ owner::get_code(owner::sample()) }}
"#
    )
}

#[test]
fn generated_calls_check_complete_requirements_and_track_supporting_impl_edits() {
    let mut db = DriverDataBase::default();
    let url = "file:///project/src/lib.fe";
    let source = calling_accessor_source();
    for admitted in [true, false, true] {
        let source = if admitted {
            source.clone()
        } else {
            source.replace("impl Allowed for u64 {}", "").replace(
                "const fn answer() -> u64 { owner::get_code(owner::sample()) }",
                "",
            )
        };
        put(&mut db, url, &source);
        let mut fresh = DriverDataBase::default();
        put(&mut fresh, url, &source);
        for view in [&db, &fresh] {
            match fe_driver::elaboration::prepare(view) {
                Ok(Some(output)) if admitted => assert_eq!(answer(&output), "258"),
                Err(error) if !admitted => assert!(error.message.contains("Allowed"), "{error:?}"),
                Err(error) => panic!("unexpected rejection: {error:?}"),
                _ => panic!("unexpected admission state"),
            }
        }
    }
}

#[test]
fn generated_call_syntax_rejects_injection_holes_and_forward_references() {
    let source = calling_accessor_source();
    for invalid in [
        source.replace("ingot::support::keep", "ingot::keep() {}"),
        source.replace("expression: 1", "expression: 2"),
        source.replace(
            "Option::Some(\"value\")",
            "Option::Some(\"value: 0, injected\")",
        ),
        source.replace(
            "types: [Option::Some(DeclarationType::Field(field_type(f))), Option::None]",
            "types: [Option::None, Option::Some(DeclarationType::Field(field_type(f)))]",
        ),
    ] {
        let mut db = DriverDataBase::default();
        put(&mut db, "file:///project/src/lib.fe", &invalid);
        assert!(fe_driver::elaboration::prepare(&db).is_err());
    }
}

#[test]
fn generated_calls_admit_existing_abi_codec_with_named_types_and_pointer_arguments() {
    let source = ACCESSOR_SOURCE
        .replace("let f = field(record, 2)", "let f = field(record, 2)\nlet no_label: Option<String<31>> = Option::None")
        .replace("BodyExpr, FunctionDefinition", "BodyExpr, FunctionDefinition, FunctionCall, CallArgument")
        .replace("FunctionDefinition<1, 2>", "FunctionDefinition<2, 6>")
        .replace("const_function: true", "const_function: false")
        .replace("parameters: [DeclarationType::Record(record)]", "parameters: [DeclarationType::Record(record), DeclarationType::BytePointer]")
        .replace("return_type: DeclarationType::Field(field_type(f))", "return_type: DeclarationType::U256")
        .replace(
            "expressions: [BodyExpr::Parameter(0), BodyExpr::Field(0, f)]",
            r#"expressions: [
                BodyExpr::Parameter(0), BodyExpr::Parameter(1), BodyExpr::Field(0, f),
                BodyExpr::U256(0), BodyExpr::U256(32),
                BodyExpr::Call(FunctionCall {
                    path: "core::abi::encode_field_at",
                    types: [Option::Some(DeclarationType::Named("std::abi::Sol")), Option::Some(DeclarationType::Field(field_type(f)))],
                    arguments: [
                        Option::Some(CallArgument { label: no_label, expression: 2 }),
                        Option::Some(CallArgument { label: no_label, expression: 1 }),
                        Option::Some(CallArgument { label: Option::Some("head_pos"), expression: 3 }),
                        Option::Some(CallArgument { label: no_label, expression: 4 }),
                    ],
                }),
            ]"#,
        )
        .replace("result: 1", "result: 5");
    let mut db = DriverDataBase::default();
    put(&mut db, "file:///codec.fe", &source);
    let output = fe_driver::elaboration::prepare(&db).unwrap().unwrap();
    let file = output
        .workspace()
        .get(&output, &"file:///codec.fe".parse().unwrap())
        .unwrap();
    assert!(
        file.text(&output)
            .contains("core::abi::encode_field_at<std::abi::Sol, u64>")
    );
}

const ABI_SOURCE: &str = include_str!("../../fe/tests/fixtures/structural_generation/abi.fe");

#[test]
fn fe_authored_abi_implementations_admit_fixed_records_and_consumers() {
    let mut db = DriverDataBase::default();
    let url: Url = "file:///abi.fe".parse().unwrap();
    put(&mut db, url.as_str(), ABI_SOURCE);
    let output = fe_driver::elaboration::prepare(&db).unwrap().unwrap();
    let file = output.workspace().get(&output, &url).unwrap();
    let generated = file.text(&output);
    assert_eq!(generated.matches("impl core::abi::AbiSize for").count(), 4);
    assert_eq!(
        generated
            .matches("impl core::abi::Encode<std::abi::Sol> for")
            .count(),
        4
    );
    assert_eq!(generated.matches("fn encode(own self").count(), 4);
    assert_eq!(
        generated
            .matches("core::abi::encode_field_at<std::abi::Sol,")
            .count(),
        7
    );
    assert_eq!(db.workspace().get(&db, &url).unwrap().text(&db), ABI_SOURCE);
}

#[test]
fn generated_implementations_check_trait_contracts_and_coherence() {
    for (case, source) in [
        ABI_SOURCE.replace(
            "DeclarationName::Literal(\"IS_DYNAMIC\")",
            "DeclarationName::Literal(\"MISSING\")",
        ),
        ABI_SOURCE.replace(
            "#[generate(ingot::authors::size)]",
            "#[generate(ingot::authors::size)]\n#[generate(ingot::authors::size)]",
        ),
        ABI_SOURCE.replace(
            "parameters: [DeclarationType::Record(record), DeclarationType::BytePointer]",
            "parameters: [DeclarationType::Bool, DeclarationType::BytePointer]",
        ),
        ABI_SOURCE.replace(
            "return_type: DeclarationType::Unit",
            "return_type: DeclarationType::U256",
        ),
        ABI_SOURCE.replace(
            "expressions[3] = BodyExpr::Unit",
            "expressions[3] = BodyExpr::End",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut db = DriverDataBase::default();
        put(&mut db, "file:///abi.fe", &source);
        assert!(
            fe_driver::elaboration::prepare(&db).is_err(),
            "case {case} was admitted"
        );
    }
}

#[test]
fn generated_implementation_cannot_be_transferred_to_another_owner() {
    use fe_driver::elaboration::construction::{ConstructionError, render_implementation};
    let source = ABI_SOURCE
        .split("\nuse core::abi::{AbiSize, ByteInput")
        .next()
        .unwrap();
    let mut db = DriverDataBase::default();
    let url: Url = "file:///abi.fe".parse().unwrap();
    put(&mut db, url.as_str(), source);
    let definitions = function_definitions(&db, &url);
    let first = &definitions[0];
    let other = definitions
        .iter()
        .find(|(input, _)| input.const_argument() != first.0.const_argument());
    let other = other.expect("distinct owner inputs");
    assert!(render_implementation(&first.0, first.1).is_ok());
    assert_eq!(
        render_implementation(&other.0, first.1),
        Err(ConstructionError::Authority)
    );
}

#[test]
fn unused_generated_encoder_requires_each_codec_after_supporting_impl_edits() {
    let source = ABI_SOURCE
        .split("\nuse core::abi::{AbiSize, ByteInput")
        .next()
        .unwrap();
    let mut db = DriverDataBase::default();
    let (core_url, original) = db
        .workspace()
        .all_files(&db)
        .iter()
        .find(|(_, file)| file.text(&db).contains("impl<A: Abi> Encode<A> for u64 {"))
        .map(|(url, file)| (url.clone(), file.text(&db).clone()))
        .unwrap();
    let codec = "impl<A: Abi> Encode<A> for u64 {\n    fn encode(own self, _ ptr: *u8) {\n        store_word(ptr: ptr, value: self as u256)\n    }\n}";
    assert!(original.contains(codec));
    let removed = original.replacen(codec, "", 1);
    for (support, admitted) in [(&original, true), (&removed, false), (&original, true)] {
        put(&mut db, core_url.as_str(), support);
        put(&mut db, "file:///abi.fe", source);
        let mut fresh = DriverDataBase::default();
        put(&mut fresh, core_url.as_str(), support);
        put(&mut fresh, "file:///abi.fe", source);
        for view in [&db, &fresh] {
            match fe_driver::elaboration::prepare(view) {
                Ok(Some(_)) if admitted => {}
                Err(error) if !admitted => assert!(error.message.contains("Encode"), "{error:?}"),
                Err(error) => panic!("unexpected rejection: {error:?}"),
                _ => panic!("unused encoder admitted without its field codec"),
            }
        }
    }
}

fn scaled_abi_source(fields: usize) -> String {
    let author = ABI_SOURCE.split("\nmod empty {").next().unwrap();
    let fields = (0..fields)
        .map(|i| format!("f{i}: u256"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{author}\n#[generate(ingot::authors::size)]\n#[generate(ingot::authors::encoder)]\n#[generate(ingot::authors::offsets)]\nstruct Scale {{ {fields} }}"
    )
}

#[test]
fn structural_abi_fixed_scaling_series_and_declared_field_limit() {
    let mut previous_bytes = 0;
    for count in [0, 1, 3, 8, 32] {
        let mut db = DriverDataBase::default();
        let url: Url = "file:///scale.fe".parse().unwrap();
        let source = scaled_abi_source(count);
        put(&mut db, url.as_str(), &source);
        let start = std::time::Instant::now();
        let output = fe_driver::elaboration::prepare(&db).unwrap().unwrap();
        let elapsed = start.elapsed();
        let file = output.workspace().get(&output, &url).unwrap();
        let generated = file.text(&output);
        let added_bytes = generated.len() - source.len();
        assert!(added_bytes > previous_bytes);
        previous_bytes = added_bytes;
        assert_eq!(
            generated
                .matches("core::abi::encode_field_at<std::abi::Sol,")
                .count(),
            count
        );
        assert_eq!(generated.matches("pub const fn offset_f").count(), count);
        assert_eq!(
            generated
                .matches("fn encode(own self, _ __meta_arg_1: *u8)")
                .count(),
            1
        );
        eprintln!(
            "ABI scaling: fields={count} added_bytes={added_bytes} prepare_ms={}",
            elapsed.as_millis()
        );
    }
    let mut db = DriverDataBase::default();
    put(&mut db, "file:///scale.fe", &scaled_abi_source(33));
    let error = match fe_driver::elaboration::prepare(&db) {
        Err(error) => error,
        Ok(_) => panic!("provider field limit silently admitted"),
    };
    assert!(
        error.message.contains("generation evaluation failed"),
        "{error:?}"
    );
}

#[test]
fn structural_generation_reports_evaluation_exhaustion_and_declared_output_limits() {
    for (source, expected) in [
        (
            ACCESSOR_SOURCE.replace(
                "let f = field(record, 2)",
                "let _again = accessor(record)\nlet f = field(record, 2)",
            ),
            "RecursionLimitExceeded",
        ),
        (
            ACCESSOR_SOURCE.replace(
                "let f = field(record, 2)",
                "while true {}\nlet f = field(record, 2)",
            ),
            "StepLimitExceeded",
        ),
        (
            ACCESSOR_SOURCE.replace("FunctionDefinition<1, 2>", "FunctionDefinition<1, 513>"),
            "exceeds its declared allocation limits",
        ),
    ] {
        let mut db = DriverDataBase::default();
        put(&mut db, "file:///limits.fe", &source);
        let error = match fe_driver::elaboration::prepare(&db) {
            Err(error) => error,
            Ok(_) => panic!("exhaustion silently produced a result"),
        };
        assert!(
            error.message.contains(expected),
            "expected {expected}: {error:?}"
        );
    }
}

#[test]
fn abi_record_field_type_and_provider_edits_match_fresh_generated_output() {
    let mut db = DriverDataBase::default();
    let url: Url = "file:///edits.fe".parse().unwrap();
    let base = scaled_abi_source(3);
    for (source, fields, marker) in [
        (base.clone(), 3, "self.f1"),
        (scaled_abi_source(1), 1, "self.f0"),
        (
            base.replace("f1: u256", "renamed: bool"),
            3,
            "encode_field_at<std::abi::Sol, bool>",
        ),
        (
            base.replace(
                "BodyExpr::U256(plan.offsets[i])",
                "BodyExpr::U256(plan.offsets[i] + 1)",
            ),
            3,
            "let __meta_node_5 = (1 as u256)",
        ),
        (base, 3, "self.f1"),
    ] {
        put(&mut db, url.as_str(), &source);
        let mut fresh = DriverDataBase::default();
        put(&mut fresh, url.as_str(), &source);
        let output = fe_driver::elaboration::prepare(&db).unwrap().unwrap();
        let fresh_output = fe_driver::elaboration::prepare(&fresh).unwrap().unwrap();
        let text = output.workspace().get(&output, &url).unwrap().text(&output);
        let fresh_text = fresh_output
            .workspace()
            .get(&fresh_output, &url)
            .unwrap()
            .text(&fresh_output);
        assert_eq!(text, fresh_text);
        assert!(text.contains(marker), "missing {marker}");
        assert_eq!(text.matches("pub const fn offset_").count(), fields);
        assert_eq!(
            text.matches("core::abi::encode_field_at<std::abi::Sol,")
                .count(),
            fields
        );
    }
}

#[test]
fn generation_rejects_output_collisions_and_observation_of_its_own_output() {
    for source in [
        ACCESSOR_SOURCE.replace(
            "pub const fn sample()",
            "pub fn get_code(_ value: Packet) -> u64 { 0 }\npub const fn sample()",
        ),
        ACCESSOR_SOURCE.replace("let f = field(record, 2)", "let _future = ingot::owner::get_code(ingot::owner::sample())\nlet f = field(record, 2)"),
    ] {
        let mut db = DriverDataBase::default();
        put(&mut db, "file:///collision.fe", &source);
        let error = match fe_driver::elaboration::prepare(&db) {
            Err(error) => error,
            Ok(_) => panic!("invalid visibility or collision admitted"),
        };
        assert!(error.message.contains("get_code"), "{error:?}");
    }
}

#[test]
fn generated_source_byte_limit_rejects_oversized_well_typed_call_syntax() {
    let source = r#"
use core::meta::{Record, field, field_type, DeclarationName, DeclarationType, BodyExpr, FunctionDefinition, FunctionCall, CallArgument}
mod support {
    pub const fn quad<T: Copy>(first_argument_abcdefghij: T, second_argument_abcdefghij: T, third_argument_abcdefghij: T, fourth_argument_abcdefghij: T) -> T {
        first_argument_abcdefghij
    }
}
const fn generate(_ record: Record) -> FunctionDefinition<1, 512> {
    let f = field(record, 0)
    let call = BodyExpr::Call(FunctionCall {
        path: "ingot::support::quad",
        types: [Option::Some(DeclarationType::Field(field_type(f))), Option::None],
        arguments: [
            Option::Some(CallArgument { label: Option::Some("first_argument_abcdefghij"), expression: 1 }),
            Option::Some(CallArgument { label: Option::Some("second_argument_abcdefghij"), expression: 1 }),
            Option::Some(CallArgument { label: Option::Some("third_argument_abcdefghij"), expression: 1 }),
            Option::Some(CallArgument { label: Option::Some("fourth_argument_abcdefghij"), expression: 1 }),
        ],
    })
    let mut expressions: [BodyExpr; 512] = [call; 512]
    expressions[0] = BodyExpr::Parameter(0)
    expressions[1] = BodyExpr::Field(0, f)
    FunctionDefinition {
        name: DeclarationName::Literal("oversized"), public: true, const_function: true,
        parameters: [DeclarationType::Record(record)], return_type: DeclarationType::Field(field_type(f)),
        expressions, result: 511,
    }
}
#[generate(generate)]
struct RecordValue { value: u64 }
"#;
    let mut db = DriverDataBase::default();
    put(&mut db, "file:///oversized.fe", source);
    let error = match fe_driver::elaboration::prepare(&db) {
        Err(error) => error,
        Ok(_) => panic!("generated byte limit was ignored"),
    };
    assert!(
        error
            .message
            .contains("invalid generated declaration: Limit"),
        "{error:?}"
    );
}
