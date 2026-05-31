use common::{
    InputDb,
    diagnostics::{CompleteDiagnostic, cmp_complete_diagnostics},
};
use fe_hir::{
    analysis::{
        elab::{
            elaboration_ctfe_context_summaries_for_top_mod,
            elaboration_request_summaries_for_top_mod, evidence_provider_summaries_for_top_mod,
            generated_impl_summaries_for_top_mod,
            generated_requirement_artifact_summaries_for_top_mod,
            generated_trace_summaries_for_top_mod, reflected_field_summaries_for_top_mod,
        },
        initialize_analysis_pass,
    },
    hir_def::TopLevelMod,
    test_db::HirAnalysisTestDb,
};
use url::Url;

fn project_file(
    db: &mut HirAnalysisTestDb,
    root: &str,
    config: &str,
    source: &str,
) -> common::file::File {
    let config_url = Url::parse(&format!("file:///{root}/fe.toml")).unwrap();
    db.workspace()
        .touch(db, config_url, Some(config.to_string()));
    let source_url = Url::parse(&format!("file:///{root}/src/lib.fe")).unwrap();
    db.workspace()
        .touch(db, source_url, Some(source.to_string()))
}

fn diagnostics_for<'db>(
    db: &'db HirAnalysisTestDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<CompleteDiagnostic> {
    let mut manager = initialize_analysis_pass();
    let mut diags: Vec<_> = manager
        .run_on_module(db, top_mod)
        .into_iter()
        .map(|diag| diag.to_complete(db))
        .collect();
    diags.sort_by(cmp_complete_diagnostics);
    diags
}

fn assert_unsatisfied_bound(diags: &[CompleteDiagnostic], expected: &str) {
    assert!(
        diags.iter().any(|diag| {
            diag.message == "trait bound is not satisfied"
                && diag
                    .sub_diagnostics
                    .iter()
                    .any(|sub| sub.message.contains(expected))
        }),
        "expected unsatisfied bound containing `{expected}`, got diagnostics: {diags:#?}"
    );
}

fn assert_generated_requirement_note(diags: &[CompleteDiagnostic], expected: &str) {
    assert!(
        diags.iter().any(|diag| {
            diag.sub_diagnostics
                .iter()
                .any(|sub| sub.message.contains(expected))
        }),
        "expected generated requirement note containing `{expected}`, got diagnostics: {diags:#?}"
    );
}

fn assert_diag_message(diags: &[CompleteDiagnostic], expected: &str) {
    assert!(
        diags.iter().any(|diag| {
            diag.message.contains(expected)
                || diag
                    .sub_diagnostics
                    .iter()
                    .any(|sub| sub.message.contains(expected))
                || diag.notes.iter().any(|note| note.contains(expected))
        }),
        "expected diagnostic message containing `{expected}`, got diagnostics: {diags:#?}"
    );
}

#[test]
fn named_derive_provider_signature_validation_accepts_const_evidence_return() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "named_derive_provider_signature_validation_accepts_const_evidence_return.fe".into(),
        r#"
trait Eq {}

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

// Compatibility coverage for staged `#[evidence_provider(...)]` syntax. The
// preferred provider declaration surface is `impl Provider: Derive for Head`.
#[test]
fn evidence_provider_summaries_include_provider_identity() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "evidence_provider_summaries_include_provider_identity.fe".into(),
        r#"
trait Eq {}

#[evidence_provider(Eq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (builder: mut ImplBuilder<Eq<T>>)
{
    builder.finish()
    ev
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        evidence_provider_summaries_for_top_mod(&db, top_mod),
        vec!["provider derive_eq for Eq via Derive<Eq> -> T: Eq".to_string()]
    );
}

#[test]
fn evidence_provider_can_declare_named_identity() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "evidence_provider_can_declare_named_identity.fe".into(),
        r#"
trait Eq {}

#[evidence_provider(Eq, name = StableEq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (builder: mut ImplBuilder<Eq<T>>)
{
    builder.finish()
    ev
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        evidence_provider_summaries_for_top_mod(&db, top_mod),
        vec!["provider StableEq for Eq via Derive<Eq> -> T: Eq".to_string()]
    );
}

#[test]
fn named_derive_provider_summaries_include_provider_identity() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "named_derive_provider_summaries_include_provider_identity.fe".into(),
        r#"
trait Eq {}

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        evidence_provider_summaries_for_top_mod(&db, top_mod),
        vec!["provider StableEq for Eq via Derive<Eq> -> T: Eq".to_string()]
    );
}

#[test]
fn named_derive_provider_must_use_derive_marker() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "named_derive_provider_must_use_derive_marker.fe".into(),
        r#"
trait Eq {}
trait NotDerive {}

impl StableEq: NotDerive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "invalid evidence provider");
    assert_diag_message(
        &diags,
        "derive provider declarations must use `Derive` after `:`",
    );
}

#[test]
fn named_derive_provider_head_must_resolve_to_trait() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "named_derive_provider_head_must_resolve_to_trait.fe".into(),
        r#"
trait Eq {}
struct NotATrait {}

impl StableEq: Derive for NotATrait {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "invalid evidence provider");
    assert_diag_message(&diags, "derive provider head must resolve to a trait");
}

#[test]
fn named_derive_provider_must_contain_derive_function() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "named_derive_provider_must_contain_derive_function.fe".into(),
        r#"
trait Eq {}

impl StableEq: Derive for Eq {
    const fn build<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "invalid evidence provider");
    assert_diag_message(
        &diags,
        "derive provider declarations must contain one `derive` function",
    );
}

#[test]
fn named_derive_provider_rejects_multiple_derive_functions() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "named_derive_provider_rejects_multiple_derive_functions.fe".into(),
        r#"
trait Eq {}

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
        ev
    }

    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "invalid evidence provider");
    assert_diag_message(
        &diags,
        "derive provider declarations may contain only one `derive` function",
    );
}

#[test]
fn derive_declaration_using_selects_named_derive_provider() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_declaration_using_selects_named_derive_provider.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        elaboration_ctfe_context_summaries_for_top_mod(&db, top_mod),
        vec!["Foo: Eq requested for Foo using StableEq using Derive<Eq> evidence from StableEq with [mut capability ImplBuilder<Foo: Eq>]".to_string()]
    );
    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Foo: Eq with obligations {}".to_string()]
    );
}

#[test]
fn implicit_derive_is_ambiguous_with_two_named_derive_providers() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "implicit_derive_is_ambiguous_with_two_named_derive_providers.fe".into(),
        r#"
trait Eq {}

struct Foo {}

derive Eq for Foo

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}

impl FastEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);

    assert_diag_message(
        &diags,
        "implicit derive request for `Derive<Eq>` is ambiguous",
    );
}

#[test]
fn named_derive_provider_must_be_const_fn() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "named_derive_provider_must_be_const_fn.fe".into(),
        r#"
trait Eq {}

impl StableEq: Derive for Eq {
    fn derive<T>() -> bool {
        true
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "invalid evidence provider");
    assert_diag_message(&diags, "must be `const fn`");
}

#[test]
fn named_derive_provider_must_return_evidence() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "named_derive_provider_must_return_evidence.fe".into(),
        r#"
trait Eq {}

impl StableEq: Derive for Eq {
    const fn derive<T>() -> bool {
        true
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "invalid evidence provider");
    assert_diag_message(&diags, "must return `Evidence<C>`");
}

#[test]
fn named_derive_provider_head_must_match_returned_constraint() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "named_derive_provider_head_must_match_returned_constraint.fe".into(),
        r#"
trait Eq {}
trait Default {}

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Default<T>>) -> Evidence<Default<T>> {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "invalid evidence provider");
    assert_diag_message(&diags, "does not match the provider head");
}

#[test]
fn named_derive_provider_body_must_declare_forwarded_capabilities() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "named_derive_provider_body_must_declare_forwarded_capabilities.fe".into(),
        r#"
trait Eq {}

const fn helper<T>() uses (reflect: Reflect<T>) {}

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
        helper<T>()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "compiler capability is not available");
}

#[test]
fn derive_attrs_collect_elaboration_requests() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_attrs_collect_elaboration_requests.fe".into(),
        r#"
trait Eq {}
trait Default {}

#[derive(Eq, Default)]
struct Pair<A, B> {
    a: A,
    b: B,
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    let summaries = elaboration_request_summaries_for_top_mod(&db, top_mod);
    assert_eq!(
        summaries,
        vec![
            "Pair<A, B>: Eq requested for Pair<A, B>".to_string(),
            "Pair<A, B>: Default requested for Pair<A, B>".to_string(),
        ]
    );
}

#[test]
fn derive_declarations_collect_elaboration_requests() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_declarations_collect_elaboration_requests.fe".into(),
        r#"
trait Eq {}
trait Default {}

struct Pair<A, B> {
    a: A,
    b: B,
}

derive Eq for Pair
derive Default for Pair
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    let summaries = elaboration_request_summaries_for_top_mod(&db, top_mod);
    assert_eq!(
        summaries,
        vec![
            "Pair<A, B>: Eq requested for Pair<A, B>".to_string(),
            "Pair<A, B>: Default requested for Pair<A, B>".to_string(),
        ]
    );
}

#[test]
fn derive_attr_head_must_resolve_to_trait() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_attr_head_must_resolve_to_trait.fe".into(),
        r#"
struct NotATrait {}

#[derive(NotATrait)]
struct Foo {}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "invalid elaboration request");
    assert_diag_message(&diags, "derive head must resolve to a trait");
}

#[test]
fn derive_declaration_head_must_resolve_to_trait() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_declaration_head_must_resolve_to_trait.fe".into(),
        r#"
struct NotATrait {}
struct Foo {}

derive NotATrait for Foo
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "invalid elaboration request");
    assert_diag_message(&diags, "derive head must resolve to a trait");
}

#[test]
fn derive_declaration_target_must_resolve_to_adt() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_declaration_target_must_resolve_to_adt.fe".into(),
        r#"
trait Eq {}
const VALUE: u256 = 1

derive Eq for VALUE
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "invalid elaboration request");
    assert_diag_message(&diags, "derive target must resolve to a struct or enum");
}

#[test]
fn derive_declaration_rejects_type_alias_target() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_declaration_rejects_type_alias_target.fe".into(),
        r#"
trait Eq {}
struct Foo {}
type Alias = Foo

derive Eq for Alias
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "invalid elaboration request");
    assert_diag_message(
        &diags,
        "derive target must be a nominal struct or enum, not a type alias",
    );
}

#[test]
fn elaboration_ctfe_context_seeds_declared_provider_capabilities() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "elaboration_ctfe_context_seeds_declared_provider_capabilities.fe".into(),
        r#"
trait Eq {}

struct Foo {
    x: u256,
}

derive Eq for Foo

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    let summaries = elaboration_ctfe_context_summaries_for_top_mod(&db, top_mod);
    assert_eq!(
        summaries,
        vec![
            "Foo: Eq requested for Foo using Derive<Eq> evidence from StableEq with [read capability Reflect<Foo>, mut capability ImplBuilder<Foo: Eq>]".to_string(),
        ]
    );
}

#[test]
fn elaboration_ctfe_context_only_seeds_declared_provider_capabilities() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "elaboration_ctfe_context_only_seeds_declared_provider_capabilities.fe".into(),
        r#"
trait Eq {}

struct Foo {}

derive Eq for Foo

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    let summaries = elaboration_ctfe_context_summaries_for_top_mod(&db, top_mod);
    assert_eq!(
        summaries,
        vec![
            "Foo: Eq requested for Foo using Derive<Eq> evidence from StableEq with []".to_string()
        ]
    );
}

#[test]
fn typed_reflection_exposes_struct_fields_from_reflect_capability() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "typed_reflection_exposes_struct_fields_from_reflect_capability.fe".into(),
        r#"
trait Eq {}

struct Box<T> {
    value: T,
}

derive Eq for Box<T>

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (reflect: Reflect<T>)
    {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        reflected_field_summaries_for_top_mod(&db, top_mod),
        vec!["Box<T>.value: Field<Box<T>, T>".to_string()]
    );
}

#[test]
fn typed_reflection_requires_reflect_capability() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "typed_reflection_requires_reflect_capability.fe".into(),
        r#"
trait Eq {}

struct Foo {
    x: u256,
}

derive Eq for Foo

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        reflected_field_summaries_for_top_mod(&db, top_mod),
        Vec::<String>::new()
    );
}

#[test]
fn field_descriptor_methods_return_type_info() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "field_descriptor_methods_return_type_info.fe".into(),
        r#"
struct Foo {
    x: u256,
}

const fn field_ty<V>(field: Field<Foo, V>) -> TypeInfo<V> {
    field.ty()
}

const fn field_parent<V>(field: Field<Foo, V>) -> TypeInfo<Foo> {
    field.parent()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn reflect_type_info_method_returns_type_info() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "reflect_type_info_method_returns_type_info.fe".into(),
        r#"
const fn reflected_type<T>(reflect: Reflect<T>) -> TypeInfo<T> {
    reflect.type_info()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn generated_derive_evidence_is_visible_to_trait_solver() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_derive_evidence_is_visible_to_trait_solver.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Foo: Eq with obligations {}".to_string()]
    );
}

#[test]
fn generated_derive_evidence_requires_builder_capability() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_derive_evidence_requires_builder_capability.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {}

derive Eq for Foo

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (reflect: Reflect<T>)
    {
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(
        &diags,
        "provider does not declare mutable ImplBuilder capability",
    );
    assert_unsatisfied_bound(&diags, "`Foo` doesn't implement `Eq`");
}

#[test]
fn provider_field_builder_method_requires_declared_reflect_receiver() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_field_builder_method_requires_declared_reflect_receiver.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {
    x: u256,
}

derive Eq for Foo

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        for field in reflect.fields() {
            builder.require<Eq>(field.ty())
        }
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "undefined variable `reflect`");
    assert_diag_message(&diags, "provider body requires Reflect capability");
    assert_unsatisfied_bound(&diags, "`Foo` doesn't implement `Eq`");
    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        Vec::<String>::new()
    );
}

#[test]
fn generated_derive_records_field_obligations() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_derive_records_field_obligations.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct FieldTy {}

impl Eq for FieldTy {}

struct Box {
    value: FieldTy,
}

derive Eq for Box using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.require<Eq>(field.ty())
        }
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Box>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Box: Eq with obligations {FieldTy: Eq}".to_string()]
    );
}

#[test]
fn provider_body_controls_generated_field_obligations() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_body_controls_generated_field_obligations.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct FieldTy {}

struct Box {
    value: FieldTy,
}

derive Eq for Box using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Box>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Box: Eq with obligations {}".to_string()]
    );
}

#[test]
fn provider_body_can_emit_field_obligations_from_reflect_fields_loop() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_body_can_emit_field_obligations_from_reflect_fields_loop.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct FieldTy {}

impl Eq for FieldTy {}

struct Pair {
    a: FieldTy,
    b: FieldTy,
}

derive Eq for Pair using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.require<Eq>(field.ty())
        }
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Pair>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Pair: Eq with obligations {FieldTy: Eq, FieldTy: Eq}".to_string()]
    );
    assert_eq!(
        generated_requirement_artifact_summaries_for_top_mod(&db, top_mod),
        vec![
            "Pair: Eq requirement #0 requires FieldTy: Eq from field Pair.a".to_string(),
            "Pair: Eq requirement #1 requires FieldTy: Eq from field Pair.b".to_string(),
        ]
    );
}

#[test]
fn builder_require_records_arbitrary_field_constraints() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "builder_require_records_arbitrary_field_constraints.fe".into(),
        r#"
trait Eq {}
trait Default {}

fn require_eq<T>()
where
    T: Eq
{}

struct FieldTy {}

impl Default for FieldTy {}

struct Box {
    value: FieldTy,
}

derive Eq for Box using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.require<Default>(field.ty())
        }
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Box>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Box: Eq with obligations {FieldTy: Default}".to_string()]
    );
}

#[test]
fn builder_require_accepts_generic_constraint_heads() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "builder_require_accepts_generic_constraint_heads.fe".into(),
        r#"
trait Eq {}
trait Marker {}

struct FieldTy {}

struct Box {
    value: FieldTy,
}

derive Eq for Box using StableEq

impl StableEq: Derive for Eq {
    const fn derive<P, T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        ) where P<T>
    {
        for field in reflect.fields() {
            builder.require<P>(field.ty())
        }
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Box: Eq with obligations {P<FieldTy>}".to_string()]
    );
}

#[test]
fn provider_body_must_finish_generated_impl() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_body_must_finish_generated_impl.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "provider did not call builder.finish()");
    assert_unsatisfied_bound(&diags, "`Foo` doesn't implement `Eq`");
    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        Vec::<String>::new()
    );
}

#[test]
fn generated_trace_summaries_include_request_provider_and_field_obligations() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_trace_summaries_include_request_provider_and_field_obligations.fe".into(),
        r#"
trait Eq {}

struct FieldTy {}

impl Eq for FieldTy {}

struct Box {
    value: FieldTy,
}

derive Eq for Box using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.require<Eq>(field.ty())
        }
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_trace_summaries_for_top_mod(&db, top_mod),
        vec![
            "Box: Eq requested by derive declaration".to_string(),
            "Box: Eq generated by StableEq".to_string(),
            "Box: Eq consumes Derive<Eq>".to_string(),
            "Box: Eq generated output source provider".to_string(),
            "Box: Eq provides Box: Eq".to_string(),
            "Box: Eq requires FieldTy: Eq from field Box.value".to_string(),
        ]
    );
}

#[test]
fn generated_trace_preserves_duplicate_field_origins() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_trace_preserves_duplicate_field_origins.fe".into(),
        r#"
trait Eq {}

struct FieldTy {}

impl Eq for FieldTy {}

struct Pair {
    a: FieldTy,
    b: FieldTy,
}

derive Eq for Pair using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.require<Eq>(field.ty())
        }
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_trace_summaries_for_top_mod(&db, top_mod),
        vec![
            "Pair: Eq requested by derive declaration".to_string(),
            "Pair: Eq generated by StableEq".to_string(),
            "Pair: Eq consumes Derive<Eq>".to_string(),
            "Pair: Eq generated output source provider".to_string(),
            "Pair: Eq provides Pair: Eq".to_string(),
            "Pair: Eq requires FieldTy: Eq from field Pair.a".to_string(),
            "Pair: Eq requires FieldTy: Eq from field Pair.b".to_string(),
        ]
    );
    assert_eq!(
        generated_requirement_artifact_summaries_for_top_mod(&db, top_mod),
        vec![
            "Pair: Eq requirement #0 requires FieldTy: Eq from field Pair.a".to_string(),
            "Pair: Eq requirement #1 requires FieldTy: Eq from field Pair.b".to_string(),
        ]
    );
}

#[test]
fn generated_generic_derive_forwards_field_obligations() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_generic_derive_forwards_field_obligations.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct Pair<A, B> {
    a: A,
    b: B,
}

derive Eq for Pair using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.require<Eq>(field.ty())
        }
        builder.finish()
        ev
    }
}

fn caller<A, B>()
where
    A: Eq,
    B: Eq
{
    require_eq<Pair<A, B>>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Pair<A, B>: Eq with obligations {A: Eq, B: Eq}".to_string()]
    );
}

#[test]
fn provider_outside_core_can_derive_core_eq() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_outside_core_can_derive_core_eq.fe".into(),
        r#"
use core::ops::Eq

fn require_eq<T>()
where
    T: Eq
{}

struct Pair<A, B> {
    a: A,
    b: B,
}

derive Eq for Pair using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        let mut acc = builder.bool(true)
        for field in reflect.fields() {
            builder.require<Eq>(field.ty())
            acc = builder.and(acc, builder.eq(
                builder.field_get(builder.self_ref(), field),
                builder.field_get(builder.arg_ref("other"), field),
            ))
        }
        builder.emit_method(acc)
        builder.finish()
        ev
    }
}

fn caller<A, B>()
where
    A: Eq,
    B: Eq
{
    require_eq<Pair<A, B>>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Pair<A, B>: Eq with obligations {A: Eq, B: Eq}".to_string()]
    );
    assert_eq!(
        evidence_provider_summaries_for_top_mod(&db, top_mod),
        vec!["provider StableEq for Eq via Derive<Eq> -> T: Eq<T>".to_string()]
    );
}

#[test]
fn provider_outside_core_derives_concrete_core_eq_struct() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_outside_core_derives_concrete_core_eq_struct.fe".into(),
        r#"
use core::ops::Eq

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {
    value: u256,
}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        let mut acc = builder.bool(true)
        for field in reflect.fields() {
            builder.require<Eq>(field.ty())
            acc = builder.and(acc, builder.eq(
                builder.field_get(builder.self_ref(), field),
                builder.field_get(builder.arg_ref("other"), field),
            ))
        }
        builder.emit_method(acc)
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Foo: Eq with obligations {u256: Eq}".to_string()]
    );
}

#[test]
fn core_derives_dependency_activates_canonical_eq_and_default_providers() {
    let mut db = HirAnalysisTestDb::default();
    let file = project_file(
        &mut db,
        "core_derives_dependency_activates_canonical_eq_and_default_providers",
        r#"
[ingot]
name = "core_derives_dependency_activates_canonical_eq_and_default_providers"
version = "0.1.0"

[dependencies]
core_derives = true
"#,
        r#"
use core::Default
use core::ops::Eq

fn require_eq<T>()
where
    T: Eq
{}

fn require_default<T>()
where
    T: Default
{}

struct Foo {
    value: u256,
}

derive Eq for Foo using StableEq
derive Default for Foo using StableDefault

fn caller() {
    require_eq<Foo>()
    require_default<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec![
            "generated provider Foo: Eq with obligations {u256: Eq}".to_string(),
            "generated provider Foo: Default with obligations {u256: Default}".to_string(),
        ]
    );
    assert_eq!(
        elaboration_ctfe_context_summaries_for_top_mod(&db, top_mod),
        vec![
            "Foo: Eq requested for Foo using StableEq using Derive<Eq> evidence from StableEq with [read capability Reflect<Foo>, mut capability ImplBuilder<Foo: Eq<Foo>>]".to_string(),
            "Foo: Default requested for Foo using StableDefault using Derive<Default> evidence from StableDefault with [read capability Reflect<Foo>, mut capability ImplBuilder<Foo: Default>]".to_string(),
        ]
    );
}

#[test]
fn core_derives_dependency_allows_implicit_canonical_provider_selection() {
    let mut db = HirAnalysisTestDb::default();
    let file = project_file(
        &mut db,
        "core_derives_dependency_allows_implicit_canonical_provider_selection",
        r#"
[ingot]
name = "core_derives_dependency_allows_implicit_canonical_provider_selection"
version = "0.1.0"

[dependencies]
core_derives = true
"#,
        r#"
use core::Default
use core::ops::Eq

fn require_eq<T>()
where
    T: Eq
{}

fn require_default<T>()
where
    T: Default
{}

struct Foo {
    value: u256,
}

derive Eq for Foo
derive Default for Foo

fn caller() {
    require_eq<Foo>()
    require_default<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        elaboration_ctfe_context_summaries_for_top_mod(&db, top_mod),
        vec![
            "Foo: Eq requested for Foo using Derive<Eq> evidence from StableEq with [read capability Reflect<Foo>, mut capability ImplBuilder<Foo: Eq<Foo>>]".to_string(),
            "Foo: Default requested for Foo using Derive<Default> evidence from StableDefault with [read capability Reflect<Foo>, mut capability ImplBuilder<Foo: Default>]".to_string(),
        ]
    );
    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec![
            "generated provider Foo: Eq with obligations {u256: Eq}".to_string(),
            "generated provider Foo: Default with obligations {u256: Default}".to_string(),
        ]
    );
}

#[test]
fn core_derives_provider_is_not_visible_without_dependency_activation() {
    let mut db = HirAnalysisTestDb::default();
    let file = project_file(
        &mut db,
        "core_derives_provider_is_not_visible_without_dependency_activation",
        r#"
[ingot]
name = "core_derives_provider_is_not_visible_without_dependency_activation"
version = "0.1.0"
"#,
        r#"
use core::ops::Eq

struct Foo {
    value: u256,
}

derive Eq for Foo using StableEq
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(
        &diags,
        "selected evidence provider `StableEq` for `Derive<Eq>` was not found",
    );
    assert!(generated_impl_summaries_for_top_mod(&db, top_mod).is_empty());
}

#[test]
fn core_derives_dependency_makes_implicit_derive_ambiguous_with_local_provider() {
    let mut db = HirAnalysisTestDb::default();
    let file = project_file(
        &mut db,
        "core_derives_dependency_makes_implicit_derive_ambiguous_with_local_provider",
        r#"
[ingot]
name = "core_derives_dependency_makes_implicit_derive_ambiguous_with_local_provider"
version = "0.1.0"

[dependencies]
core_derives = true
"#,
        r#"
use core::ops::Eq

struct Foo {}

derive Eq for Foo

impl FastEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.emit_method(builder.bool(true))
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(
        &diags,
        "implicit derive request for `Derive<Eq>` is ambiguous",
    );
    assert!(generated_impl_summaries_for_top_mod(&db, top_mod).is_empty());
}

#[test]
fn core_derives_dependency_allows_explicit_local_provider_selection() {
    let mut db = HirAnalysisTestDb::default();
    let file = project_file(
        &mut db,
        "core_derives_dependency_allows_explicit_local_provider_selection",
        r#"
[ingot]
name = "core_derives_dependency_allows_explicit_local_provider_selection"
version = "0.1.0"

[dependencies]
core_derives = true
"#,
        r#"
use core::ops::Eq

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {}

derive Eq for Foo using FastEq

impl FastEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.emit_method(builder.bool(true))
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        elaboration_ctfe_context_summaries_for_top_mod(&db, top_mod),
        vec!["Foo: Eq requested for Foo using FastEq using Derive<Eq> evidence from FastEq with [mut capability ImplBuilder<Foo: Eq<Foo>>]".to_string()]
    );
    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Foo: Eq with obligations {}".to_string()]
    );
}

#[test]
fn provider_generated_marker_derive_end_to_end() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_generated_marker_derive_end_to_end.fe".into(),
        r#"
trait DeriveMarker {}

fn require_marker<T>()
where
    T: DeriveMarker
{}

struct Pair<A, B> {
    a: A,
    b: B,
}

derive DeriveMarker for Pair using StableMarker

impl StableMarker: Derive for DeriveMarker {
    const fn derive<T>(ev: own Evidence<DeriveMarker<T>>) -> Evidence<DeriveMarker<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<DeriveMarker<T>>,
        )
    {
        for field in reflect.fields() {
            builder.require<DeriveMarker>(field.ty())
        }
        builder.finish()
        ev
    }
}

fn caller<A, B>()
where
    A: DeriveMarker,
    B: DeriveMarker
{
    require_marker<Pair<A, B>>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec![
            "generated provider Pair<A, B>: DeriveMarker with obligations {A: DeriveMarker, B: DeriveMarker}"
                .to_string()
        ]
    );
    assert_eq!(
        generated_trace_summaries_for_top_mod(&db, top_mod),
        vec![
            "Pair<A, B>: DeriveMarker requested by derive declaration".to_string(),
            "Pair<A, B>: DeriveMarker generated by StableMarker".to_string(),
            "Pair<A, B>: DeriveMarker consumes Derive<DeriveMarker>".to_string(),
            "Pair<A, B>: DeriveMarker generated output source provider".to_string(),
            "Pair<A, B>: DeriveMarker provides Pair<A, B>: DeriveMarker".to_string(),
            "Pair<A, B>: DeriveMarker requires A: DeriveMarker from field Pair<A, B>.a".to_string(),
            "Pair<A, B>: DeriveMarker requires B: DeriveMarker from field Pair<A, B>.b".to_string(),
        ]
    );
}

#[test]
fn generated_default_derive_records_field_obligations() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_default_derive_records_field_obligations.fe".into(),
        r#"
trait Default {}

fn require_default<T>()
where
    T: Default
{}

struct FieldTy {}

impl Default for FieldTy {}

struct Box {
    value: FieldTy,
}

derive Default for Box using StableDefault

impl StableDefault: Derive for Default {
    const fn derive<T>(ev: own Evidence<Default<T>>) -> Evidence<Default<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Default<T>>,
        )
    {
        for field in reflect.fields() {
            builder.require<Default>(field.ty())
        }
        builder.finish()
        ev
    }
}

fn caller() {
    require_default<Box>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Box: Default with obligations {FieldTy: Default}".to_string()]
    );
}

#[test]
fn implicit_attr_provider_ambiguity_is_diagnosed() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "implicit_attr_provider_ambiguity_is_diagnosed.fe".into(),
        r#"
trait Eq {}

#[derive(Eq)]
struct Foo {}

#[evidence_provider(Eq)]
const fn derive_eq1<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (
        reflect: Reflect<T>,
        builder: mut ImplBuilder<Eq<T>>,
    )
{
    ev
}

#[evidence_provider(Eq)]
const fn derive_eq2<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (
        reflect: Reflect<T>,
        builder: mut ImplBuilder<Eq<T>>,
    )
{
    ev
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(
        &diags,
        "implicit derive request for `Derive<Eq>` is ambiguous",
    );
}

#[test]
fn implicit_attr_provider_ambiguity_does_not_generate_impls() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "implicit_attr_provider_ambiguity_does_not_generate_impls.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

#[derive(Eq)]
struct Foo {}

#[evidence_provider(Eq)]
const fn derive_eq1<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (builder: mut ImplBuilder<Eq<T>>)
{
    builder.finish()
    ev
}

#[evidence_provider(Eq)]
const fn derive_eq2<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (builder: mut ImplBuilder<Eq<T>>)
{
    builder.finish()
    ev
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(
        &diags,
        "implicit derive request for `Derive<Eq>` is ambiguous",
    );
    assert_unsatisfied_bound(&diags, "`Foo` doesn't implement `Eq`");
    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        Vec::<String>::new()
    );
}

#[test]
fn derive_attr_using_selects_named_provider_when_multiple_match() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_attr_using_selects_named_provider_when_multiple_match.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

#[derive(Eq, using = FastEq)]
struct Foo {}

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}

impl FastEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        elaboration_ctfe_context_summaries_for_top_mod(&db, top_mod),
        vec!["Foo: Eq requested for Foo using FastEq using Derive<Eq> evidence from FastEq with [mut capability ImplBuilder<Foo: Eq>]".to_string()]
    );
    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Foo: Eq with obligations {}".to_string()]
    );
}

#[test]
fn derive_declaration_using_selects_named_provider_when_multiple_match() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_declaration_using_selects_named_provider_when_multiple_match.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {}

derive Eq for Foo using FastEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}

impl FastEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        elaboration_ctfe_context_summaries_for_top_mod(&db, top_mod),
        vec!["Foo: Eq requested for Foo using FastEq using Derive<Eq> evidence from FastEq with [mut capability ImplBuilder<Foo: Eq>]".to_string()]
    );
    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Foo: Eq with obligations {}".to_string()]
    );
}

#[test]
fn derive_declaration_using_selects_attr_provider_identity() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_declaration_using_selects_attr_provider_identity.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {}

derive Eq for Foo using StableEq

#[evidence_provider(Eq, name = StableEq)]
const fn derive_eq<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
    uses (builder: mut ImplBuilder<Eq<T>>)
{
    builder.finish()
    ev
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        elaboration_ctfe_context_summaries_for_top_mod(&db, top_mod),
        vec!["Foo: Eq requested for Foo using StableEq using Derive<Eq> evidence from StableEq with [mut capability ImplBuilder<Foo: Eq>]".to_string()]
    );
    assert_eq!(
        generated_trace_summaries_for_top_mod(&db, top_mod)
            .into_iter()
            .filter(|summary| summary.ends_with("generated by StableEq"))
            .collect::<Vec<_>>(),
        vec!["Foo: Eq generated by StableEq".to_string()]
    );
}

#[test]
fn derive_attr_using_reports_missing_provider() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_attr_using_reports_missing_provider.fe".into(),
        r#"
trait Eq {}

#[derive(Eq, using = missing_provider)]
struct Foo {}

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(
        &diags,
        "selected evidence provider `missing_provider` for `Derive<Eq>` was not found",
    );
}

#[test]
fn derive_attr_using_reports_wrong_provider_head() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_attr_using_reports_wrong_provider_head.fe".into(),
        r#"
trait Eq {}
trait Default {}

#[derive(Eq, using = StableDefault)]
struct Foo {}

impl StableDefault: Derive for Default {
    const fn derive<T>(ev: own Evidence<Default<T>>) -> Evidence<Default<T>>
        uses (builder: mut ImplBuilder<Default<T>>)
    {
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(
        &diags,
        "selected evidence provider `StableDefault` does not provide `Derive<Eq>` evidence",
    );
}

#[test]
fn derive_declaration_using_reports_missing_provider() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_declaration_using_reports_missing_provider.fe".into(),
        r#"
trait Eq {}

struct Foo {}

derive Eq for Foo using missing_provider

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(
        &diags,
        "selected evidence provider `missing_provider` for `Derive<Eq>` was not found",
    );
}

#[test]
fn derive_declaration_using_reports_wrong_provider_head() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_declaration_using_reports_wrong_provider_head.fe".into(),
        r#"
trait Eq {}
trait Default {}

struct Foo {}

derive Eq for Foo using StableDefault

impl StableDefault: Derive for Default {
    const fn derive<T>(ev: own Evidence<Default<T>>) -> Evidence<Default<T>>
        uses (builder: mut ImplBuilder<Default<T>>)
    {
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(
        &diags,
        "selected evidence provider `StableDefault` does not provide `Derive<Eq>` evidence",
    );
}

#[test]
fn generated_derive_conflicts_with_authored_impl() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_derive_conflicts_with_authored_impl.fe".into(),
        r#"
trait Eq {}

struct Foo {}

derive Eq for Foo using StableEq

impl Eq for Foo {}

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "conflicts with an authored implementation");
}

#[test]
fn duplicate_derive_attr_generated_derives_conflict() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "duplicate_derive_attr_generated_derives_conflict.fe".into(),
        r#"
trait Eq {}

#[derive(Eq, Eq)]
struct Foo {}

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "conflicts with another generated implementation");
    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        Vec::<String>::new()
    );
}

#[test]
fn explicitly_selected_duplicate_generated_derives_conflict() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "explicitly_selected_duplicate_generated_derives_conflict.fe".into(),
        r#"
trait Eq {}

struct Foo {}

derive Eq for Foo using StableEq
derive Eq for Foo using FastEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}

impl FastEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "conflicts with another generated implementation");
    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        Vec::<String>::new()
    );
}

#[test]
fn selected_named_providers_for_same_global_target_conflict() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "selected_named_providers_for_same_global_target_conflict.fe".into(),
        r#"
trait Eq {}

struct Foo {}

derive Eq for Foo using StableEq
derive Eq for Foo using FastEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}

impl FastEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "conflicts with another generated implementation");
    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        Vec::<String>::new()
    );
}

#[test]
fn selected_named_provider_conflicts_with_authored_impl() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "selected_named_provider_conflicts_with_authored_impl.fe".into(),
        r#"
trait Eq {}

struct Foo {}

impl Eq for Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "conflicts with an authored implementation");
    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        Vec::<String>::new()
    );
}

#[test]
fn provider_outputs_reject_traits_with_required_methods_without_generated_methods() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_outputs_reject_traits_with_required_methods_without_generated_methods.fe".into(),
        r#"
trait Eq {
    fn eq(self, other: Self) -> bool
}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(
        &diags,
        "provider output for `Eq` does not generate required methods yet",
    );
}

#[test]
fn provider_generated_bool_method_satisfies_required_method_trait() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_generated_bool_method_satisfies_required_method_trait.fe".into(),
        r#"
trait Eq {
    fn eq(self, other: Self) -> bool
}

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.emit_method(builder.bool(true))
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Foo: Eq with obligations {}".to_string()]
    );
}

#[test]
fn provider_generated_bool_and_method_satisfies_required_method_trait() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_generated_bool_and_method_satisfies_required_method_trait.fe".into(),
        r#"
trait Eq {
    fn eq(self, other: Self) -> bool
}

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.emit_method(builder.and(builder.bool(true), builder.bool(true)))
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Foo: Eq with obligations {}".to_string()]
    );
}

#[test]
fn provider_generated_duplicate_method_emission_is_rejected() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_generated_duplicate_method_emission_is_rejected.fe".into(),
        r#"
trait Eq {
    fn eq(self, other: Self) -> bool
}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.emit_method(builder.bool(true))
        builder.emit_method(builder.bool(false))
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "provider output for `Foo: Eq` is invalid");
    assert_diag_message(&diags, "duplicate generated method `eq`");
}

#[test]
fn provider_generated_field_get_eq_method_satisfies_required_method_trait() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_generated_field_get_eq_method_satisfies_required_method_trait.fe".into(),
        r#"
trait Eq {
    fn eq(self, other: Self) -> bool
}

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {
    value: u256,
}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.emit_method(builder.eq(
                builder.field_get(builder.self_ref(), field),
                builder.field_get(builder.arg_ref("other"), field),
            ))
        }
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Foo: Eq with obligations {}".to_string()]
    );
}

#[test]
fn provider_generated_method_arg_ref_uses_required_param_name() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_generated_method_arg_ref_uses_required_param_name.fe".into(),
        r#"
trait Eq {
    fn eq(self, rhs: Self) -> bool
}

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {
    value: u256,
}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.emit_method(builder.eq(
                builder.field_get(builder.self_ref(), field),
                builder.field_get(builder.arg_ref("rhs"), field),
            ))
        }
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn provider_generated_method_arg_ref_uses_defaulted_self_arg() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_generated_method_arg_ref_uses_defaulted_self_arg.fe".into(),
        r#"
trait Eq<Other = Self> {
    fn eq(self, other: Other) -> bool
}

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {
    value: u256,
}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.emit_method(builder.eq(
                builder.field_get(builder.self_ref(), field),
                builder.field_get(builder.arg_ref("other"), field),
            ))
        }
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn provider_generated_field_get_normalizes_ref_receiver() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_generated_field_get_normalizes_ref_receiver.fe".into(),
        r#"
trait Eq {
    fn eq(self, other: ref Self) -> bool
}

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {
    value: u256,
}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.emit_method(builder.eq(
                builder.field_get(builder.self_ref(), field),
                builder.field_get(builder.arg_ref("other"), field),
            ))
        }
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn provider_generated_field_get_rejects_unrelated_receiver() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_generated_field_get_rejects_unrelated_receiver.fe".into(),
        r#"
trait Eq {
    fn eq(self, other: u256) -> bool
}

struct Foo {
    value: u256,
}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.emit_method(builder.eq(
                builder.field_get(builder.self_ref(), field),
                builder.field_get(builder.arg_ref("other"), field),
            ))
        }
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(
        &diags,
        "provider output for `Eq` does not generate required methods yet",
    );
    assert_diag_message(&diags, "unsupported eq");
    assert_diag_message(
        &diags,
        "generated by StableEq for `Foo: Eq requested for Foo using StableEq`",
    );
}

#[test]
fn provider_generated_method_rejects_unavailable_method_arg() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_generated_method_rejects_unavailable_method_arg.fe".into(),
        r#"
trait Eq {
    fn eq(self) -> bool
}

struct Foo {
    value: u256,
}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.emit_method(builder.eq(
                builder.field_get(builder.self_ref(), field),
                builder.field_get(builder.arg_ref("other"), field),
            ))
        }
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(
        &diags,
        "provider output for `Eq` does not generate required methods yet",
    );
    assert_diag_message(&diags, "unsupported eq");
    assert_diag_message(
        &diags,
        "generated by StableEq for `Foo: Eq requested for Foo using StableEq`",
    );
}

#[test]
fn provider_generated_eq_like_derive_accumulates_field_method_body() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_generated_eq_like_derive_accumulates_field_method_body.fe".into(),
        r#"
trait TestEq {
    fn eq(self, other: Self) -> bool
}

fn require_test_eq<T>()
where
    T: TestEq
{}

struct Pair<A, B> {
    a: A,
    b: B,
}

derive TestEq for Pair using StableTestEq

impl StableTestEq: Derive for TestEq {
    const fn derive<T>(ev: own Evidence<TestEq<T>>) -> Evidence<TestEq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<TestEq<T>>,
        )
    {
        let mut acc = builder.bool(true)
        for field in reflect.fields() {
            builder.require<TestEq>(field.ty())
            acc = builder.and(acc, builder.eq(
                builder.field_get(builder.self_ref(), field),
                builder.field_get(builder.arg_ref("other"), field),
            ))
        }
        builder.emit_method(acc)
        builder.finish()
        ev
    }
}

fn caller<A, B>()
where
    A: TestEq,
    B: TestEq
{
    require_test_eq<Pair<A, B>>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec![
            "generated provider Pair<A, B>: TestEq with obligations {A: TestEq, B: TestEq}"
                .to_string()
        ]
    );
}

#[test]
fn generated_default_derive_with_required_method_waits_for_body_ir() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_default_derive_with_required_method_waits_for_body_ir.fe".into(),
        r#"
trait Default {
    fn default() -> Self
}

struct Foo {}

derive Default for Foo using StableDefault

impl StableDefault: Derive for Default {
    const fn derive<T>(ev: own Evidence<Default<T>>) -> Evidence<Default<T>>
        uses (
            builder: mut ImplBuilder<Default<T>>,
        )
    {
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(
        &diags,
        "provider output for `Default` does not generate required methods yet",
    );
    assert_diag_message(&diags, "missing default");
    assert_diag_message(
        &diags,
        "generated by StableDefault for `Foo: Default requested for Foo using StableDefault`",
    );
}

#[test]
fn provider_generated_default_like_derive_builds_struct_init_body() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_generated_default_like_derive_builds_struct_init_body.fe".into(),
        r#"
trait Default {
    fn default() -> Self
}

fn require_default<T>()
where
    T: Default
{}

struct Pair<A, B> {
    a: A,
    b: B,
}

derive Default for Pair using StableDefault

impl StableDefault: Derive for Default {
    const fn derive<T>(ev: own Evidence<Default<T>>) -> Evidence<Default<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Default<T>>,
        )
    {
        let mut init = builder.struct_init()
        for field in reflect.fields() {
            builder.require<Default>(field.ty())
            init = builder.with_field(init, field, builder.default(field.ty()))
        }
        builder.emit_method(init)
        builder.finish()
        ev
    }
}

fn caller<A, B>()
where
    A: Default,
    B: Default
{
    require_default<Pair<A, B>>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec![
            "generated provider Pair<A, B>: Default with obligations {A: Default, B: Default}"
                .to_string()
        ]
    );
}

#[test]
fn provider_outside_core_derives_concrete_core_default_struct() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_outside_core_derives_concrete_core_default_struct.fe".into(),
        r#"
use core::Default

fn require_default<T>()
where
    T: Default
{}

struct Foo {
    value: u256,
}

derive Default for Foo using StableDefault

impl StableDefault: Derive for Default {
    const fn derive<T>(ev: own Evidence<Default<T>>) -> Evidence<Default<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Default<T>>,
        )
    {
        let mut init = builder.struct_init()
        for field in reflect.fields() {
            builder.require<Default>(field.ty())
            init = builder.with_field(init, field, builder.default(field.ty()))
        }
        builder.emit_method(init)
        builder.finish()
        ev
    }
}

fn caller() {
    require_default<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec!["generated provider Foo: Default with obligations {u256: Default}".to_string()]
    );
}

#[test]
fn generated_derive_reports_missing_field_obligation() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_derive_reports_missing_field_obligation.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct FieldTy {}

struct Box {
    value: FieldTy,
}

derive Eq for Box using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.require<Eq>(field.ty())
        }
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Box>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_unsatisfied_bound(&diags, "FieldTy: Eq");
    assert_generated_requirement_note(
        &diags,
        "required by generated obligation FieldTy: Eq from field Box.value",
    );
    assert_generated_requirement_note(
        &diags,
        "generated by StableEq for `Box: Eq requested for Box using StableEq`",
    );
}

#[test]
fn recursive_generated_derive_does_not_panic() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "recursive_generated_derive_does_not_panic.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct Loop {
    next: Loop,
}

derive Eq for Loop using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.require<Eq>(field.ty())
        }
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Loop>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "recursive generated evidence request");
    assert_diag_message(&diags, "Loop: Eq -> Loop: Eq");
    assert_diag_message(
        &diags,
        "requests: Loop: Eq requested for Loop using StableEq generated by StableEq",
    );
    assert_diag_message(&diags, "Loop: Eq from field Loop.next");
    assert_unsatisfied_bound(&diags, "Loop: Eq");
}

#[test]
fn mutually_recursive_generated_derives_do_not_panic() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "mutually_recursive_generated_derives_do_not_panic.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct Left {
    right: Right,
}

struct Right {
    left: Left,
}

derive Eq for Left using StableEq
derive Eq for Right using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        for field in reflect.fields() {
            builder.require<Eq>(field.ty())
        }
        builder.finish()
        ev
    }
}

fn caller() {
    require_eq<Left>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_diag_message(&diags, "recursive generated evidence request");
    assert_diag_message(&diags, "Left: Eq -> Right: Eq -> Left: Eq");
    assert_diag_message(
        &diags,
        "Left: Eq requested for Left using StableEq generated by StableEq",
    );
    assert_diag_message(
        &diags,
        "Right: Eq requested for Right using StableEq generated by StableEq",
    );
    assert_diag_message(&diags, "Right: Eq from field Left.right");
    assert_diag_message(&diags, "Left: Eq from field Right.left");
    assert_unsatisfied_bound(&diags, "Right: Eq");
    assert_eq!(
        generated_impl_summaries_for_top_mod(&db, top_mod),
        vec![
            "generated provider Left: Eq with obligations {Right: Eq}".to_string(),
            "generated provider Right: Eq with obligations {Left: Eq}".to_string(),
        ]
    );
}

#[test]
fn derive_without_provider_does_not_satisfy_trait_solver() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "derive_without_provider_does_not_satisfy_trait_solver.fe".into(),
        r#"
trait Eq {}

fn require_eq<T>()
where
    T: Eq
{}

struct Foo {}

derive Eq for Foo

fn caller() {
    require_eq<Foo>()
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);
    assert_unsatisfied_bound(&diags, "`Foo` doesn't implement `Eq`");
}
