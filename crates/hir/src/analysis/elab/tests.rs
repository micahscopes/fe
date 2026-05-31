use super::*;
use crate::{
    analysis::ty::{
        constraint::ConstraintId,
        generated::{
            GeneratedExprId, GeneratedExprKind, GeneratedMethodBodyKind, GeneratedStructFieldInit,
            GeneratedStructFieldInitListId,
        },
        trait_def::TraitInstId,
        ty_def::TyId,
    },
    hir_def::{IdentId, ItemKind},
    span::LazySpan,
    test_db::HirAnalysisTestDb,
};

fn find_trait<'db>(
    db: &'db HirAnalysisTestDb,
    top_mod: TopLevelMod<'db>,
    name: &str,
) -> Trait<'db> {
    top_mod
        .all_items(db)
        .iter()
        .find_map(|item| match item {
            ItemKind::Trait(trait_)
                if trait_
                    .name(db)
                    .to_opt()
                    .is_some_and(|ident| ident.data(db) == name) =>
            {
                Some(*trait_)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing `{name}` trait"))
}

fn first_builder_context<'db>(
    db: &'db HirAnalysisTestDb,
    top_mod: TopLevelMod<'db>,
) -> ElaborationCtfeContextId<'db> {
    let request = *elaboration_requests_for_top_mod(db, top_mod)
        .first()
        .expect("missing elaboration request");
    *elaboration_ctfe_contexts_for_request(db, request)
        .first()
        .expect("missing elaboration context")
}

fn skipped_output_span_text<'db>(
    db: &'db HirAnalysisTestDb,
    output: ProviderOutputId<'db>,
) -> String {
    let ProviderOutputStatus::Skipped { span, .. } = output.status(db) else {
        panic!("expected skipped provider output");
    };
    let resolved = span.resolve(db).expect("skip span should resolve");
    let text = resolved.file.text(db);
    text[resolved.range.start().into()..resolved.range.end().into()].to_string()
}

fn provider_output_source_for_commands<'db>(
    db: &'db HirAnalysisTestDb,
    context: ElaborationCtfeContextId<'db>,
    commands: BuilderCommandListId<'db>,
) -> GeneratedImplSource<'db> {
    GeneratedImplSource::ProviderOutput(ProviderOutputId::new(
        db,
        context.request(db),
        context.provider(db),
        context,
        ProviderOutputStatus::Succeeded { commands },
    ))
}

#[test]
fn raw_impl_builder_records_required_obligations() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "raw_impl_builder_records_required_obligations.fe".into(),
        r#"
trait Eq {}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    let eq = find_trait(&db, top_mod, "Eq");
    let field_obligation =
        ConstraintId::from_trait(&db, TraitInstId::new_simple(&db, eq, vec![TyId::u256(&db)]));
    let context = first_builder_context(&db, top_mod);
    let mut builder = ImplBuilderSession::new(&db, context);
    builder.require(field_obligation).unwrap();

    let generated = builder.finish(&db).unwrap();
    assert_eq!(generated.obligations.list(&db), &[field_obligation]);
}

#[test]
fn raw_impl_builder_rejects_wrong_target() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "raw_impl_builder_rejects_wrong_target.fe".into(),
        r#"
trait Eq {}
trait Default {}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    let default = find_trait(&db, top_mod, "Default");
    let target_ty = first_builder_context(&db, top_mod)
        .request(&db)
        .target(&db)
        .ty(&db);
    let wrong_goal =
        ConstraintId::from_trait(&db, TraitInstId::new_simple(&db, default, vec![target_ty]));

    let context = first_builder_context(&db, top_mod);
    let mut builder = ImplBuilderSession::new(&db, context);
    let err = builder.emit_impl(&db, wrong_goal).unwrap_err();
    assert!(matches!(err, BuilderError::WrongTarget { .. }));
}

#[test]
fn builder_command_validation_requires_finish() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "builder_command_validation_requires_finish.fe".into(),
        r#"
trait Eq {}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    let eq = find_trait(&db, top_mod, "Eq");
    let requirement =
        ConstraintId::from_trait(&db, TraitInstId::new_simple(&db, eq, vec![TyId::u256(&db)]));
    let context = first_builder_context(&db, top_mod);
    let commands = BuilderCommandListId::new(
        &db,
        vec![BuilderCommand::Require {
            constraint: requirement,
            origin: RequirementOrigin::Synthetic,
        }],
    );
    let err = generated_impl_from_builder_commands(
        &db,
        context,
        provider_output_source_for_commands(&db, context, commands),
        commands,
    )
    .unwrap_err();
    assert!(matches!(err, BuilderError::NotFinished));
}

#[test]
fn builder_command_validation_rejects_commands_after_finish() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "builder_command_validation_rejects_commands_after_finish.fe".into(),
        r#"
trait Eq {}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    let eq = find_trait(&db, top_mod, "Eq");
    let requirement =
        ConstraintId::from_trait(&db, TraitInstId::new_simple(&db, eq, vec![TyId::u256(&db)]));
    let context = first_builder_context(&db, top_mod);
    let commands = BuilderCommandListId::new(
        &db,
        vec![
            BuilderCommand::Finish,
            BuilderCommand::Require {
                constraint: requirement,
                origin: RequirementOrigin::Synthetic,
            },
        ],
    );
    let err = generated_impl_from_builder_commands(
        &db,
        context,
        provider_output_source_for_commands(&db, context, commands),
        commands,
    )
    .unwrap_err();
    assert!(matches!(err, BuilderError::CommandAfterFinish));
}

#[test]
fn provider_output_reports_missing_finish() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_output_reports_missing_finish.fe".into(),
        r#"
trait Eq {}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    let context = first_builder_context(&db, top_mod);
    let output = provider_output_for_context(&db, context);
    assert!(matches!(
        output.status(&db),
        ProviderOutputStatus::Skipped {
            reason: ProviderSkipReason::MissingFinish,
            ..
        }
    ));
}

#[test]
fn provider_output_reports_missing_reflect_capability() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_output_reports_missing_reflect_capability.fe".into(),
        r#"
trait Eq {}

struct Foo {}

derive Eq for Foo using StableEq

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
"#,
    );
    let (top_mod, _) = db.top_mod(file);

    let context = first_builder_context(&db, top_mod);
    let output = provider_output_for_context(&db, context);
    assert!(matches!(
        output.status(&db),
        ProviderOutputStatus::Skipped {
            reason: ProviderSkipReason::MissingReflectCapability,
            ..
        }
    ));
    assert_eq!(skipped_output_span_text(&db, output), "reflect.fields()");
}

#[test]
fn provider_output_reports_malformed_builder_finish_call_as_unsupported() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_output_reports_malformed_builder_finish_call_as_unsupported.fe".into(),
        r#"
trait Eq {}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish(1)
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let context = first_builder_context(&db, top_mod);
    let output = provider_output_for_context(&db, context);
    assert!(matches!(
        output.status(&db),
        ProviderOutputStatus::Skipped {
            reason: ProviderSkipReason::UnsupportedProviderBody,
            ..
        }
    ));
    assert_eq!(skipped_output_span_text(&db, output), "builder");
}

#[test]
fn provider_output_reports_unsupported_control_flow() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_output_reports_unsupported_control_flow.fe".into(),
        r#"
trait Eq {}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        while true {
            builder.finish()
        }
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let context = first_builder_context(&db, top_mod);
    let output = provider_output_for_context(&db, context);
    assert!(matches!(
        output.status(&db),
        ProviderOutputStatus::Skipped {
            reason: ProviderSkipReason::UnsupportedProviderBody,
            ..
        }
    ));
    assert!(skipped_output_span_text(&db, output).starts_with("while true"));
}

#[test]
fn provider_output_rejects_duplicate_finish_calls() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_output_rejects_duplicate_finish_calls.fe".into(),
        r#"
trait Eq {}

struct Foo {}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (builder: mut ImplBuilder<Eq<T>>)
    {
        builder.finish()
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    let context = first_builder_context(&db, top_mod);
    let output = provider_output_for_context(&db, context);
    assert!(matches!(
        output.status(&db),
        ProviderOutputStatus::Skipped {
            reason: ProviderSkipReason::DuplicateFinish,
            ..
        }
    ));
}

#[test]
fn provider_output_rejects_commands_after_finish() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "provider_output_rejects_commands_after_finish.fe".into(),
        r#"
trait Eq {}

struct Foo {
    x: u256,
}

derive Eq for Foo using StableEq

impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,
            builder: mut ImplBuilder<Eq<T>>,
        )
    {
        builder.finish()
        for field in reflect.fields() {
            builder.require<Eq>(field.ty())
        }
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    let context = first_builder_context(&db, top_mod);
    let output = provider_output_for_context(&db, context);
    assert!(matches!(
        output.status(&db),
        ProviderOutputStatus::Skipped {
            reason: ProviderSkipReason::CommandAfterFinish,
            ..
        }
    ));
}

#[test]
fn generated_bool_method_body_satisfies_bool_required_method() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_bool_method_body_satisfies_bool_required_method.fe".into(),
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
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let context = first_builder_context(&db, top_mod);
    let output = provider_output_for_context(&db, context);
    let eq_trait = find_trait(&db, top_mod, "Eq");
    let method_name = *eq_trait
        .method_defs(&db)
        .keys()
        .next()
        .expect("missing required method");

    let commands = BuilderCommandListId::new(
        &db,
        vec![
            BuilderCommand::EmitMethodExpr {
                name: method_name,
                expr: GeneratedExprId::new(&db, GeneratedExprKind::BoolLiteral(true)),
            },
            BuilderCommand::Finish,
        ],
    );
    let generated = generated_impl_from_builder_commands(
        &db,
        context,
        GeneratedImplSource::ProviderOutput(output),
        commands,
    )
    .unwrap();

    assert!(generated_missing_required_methods(&db, generated).is_empty());
    assert!(generated_invalid_required_method_bodies(&db, generated).is_empty());
    let GeneratedMethodBodyKind::Expr(expr) = generated.methods.list(&db)[0].body;
    assert!(matches!(
        expr.kind(&db),
        GeneratedExprKind::BoolLiteral(true)
    ));
}

#[test]
fn generated_bool_and_method_body_satisfies_bool_required_method() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_bool_and_method_body_satisfies_bool_required_method.fe".into(),
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
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let context = first_builder_context(&db, top_mod);
    let output = provider_output_for_context(&db, context);
    let eq_trait = find_trait(&db, top_mod, "Eq");
    let method_name = *eq_trait
        .method_defs(&db)
        .keys()
        .next()
        .expect("missing required method");
    let lhs = GeneratedExprId::new(&db, GeneratedExprKind::BoolLiteral(true));
    let rhs = GeneratedExprId::new(&db, GeneratedExprKind::BoolLiteral(false));
    let expr = GeneratedExprId::new(&db, GeneratedExprKind::BoolAnd { lhs, rhs });

    let commands = BuilderCommandListId::new(
        &db,
        vec![
            BuilderCommand::EmitMethodExpr {
                name: method_name,
                expr,
            },
            BuilderCommand::Finish,
        ],
    );
    let generated = generated_impl_from_builder_commands(
        &db,
        context,
        GeneratedImplSource::ProviderOutput(output),
        commands,
    )
    .unwrap();

    assert!(generated_missing_required_methods(&db, generated).is_empty());
    assert!(generated_invalid_required_method_bodies(&db, generated).is_empty());
}

#[test]
fn generated_field_get_eq_expr_satisfies_bool_required_method() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_field_get_eq_expr_satisfies_bool_required_method.fe".into(),
        r#"
trait Eq {
    fn eq(self, other: Self) -> bool
}

struct Foo {
    x: u256,
}

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
    let context = first_builder_context(&db, top_mod);
    let output = provider_output_for_context(&db, context);
    let eq_trait = find_trait(&db, top_mod, "Eq");
    let method_name = *eq_trait
        .method_defs(&db)
        .keys()
        .next()
        .expect("missing required method");
    let target_ty = context.request(&db).target(&db).ty(&db);
    let field = reflect_struct_fields(&db, target_ty)
        .into_iter()
        .next()
        .expect("missing reflected field");
    let self_ref = GeneratedExprId::new(&db, GeneratedExprKind::SelfRef { ty: target_ty });
    let method_arg_ref = GeneratedExprId::new(
        &db,
        GeneratedExprKind::MethodArgRef {
            name: IdentId::new(&db, "other".to_string()),
            ty: target_ty,
        },
    );
    let lhs = GeneratedExprId::new(
        &db,
        GeneratedExprKind::FieldGet {
            base: self_ref,
            field,
        },
    );
    let rhs = GeneratedExprId::new(
        &db,
        GeneratedExprKind::FieldGet {
            base: method_arg_ref,
            field,
        },
    );
    let expr = GeneratedExprId::new(&db, GeneratedExprKind::EqExpr { lhs, rhs });

    let commands = BuilderCommandListId::new(
        &db,
        vec![
            BuilderCommand::EmitMethodExpr {
                name: method_name,
                expr,
            },
            BuilderCommand::Finish,
        ],
    );
    let generated = generated_impl_from_builder_commands(
        &db,
        context,
        GeneratedImplSource::ProviderOutput(output),
        commands,
    )
    .unwrap();

    assert!(generated_missing_required_methods(&db, generated).is_empty());
    assert!(generated_invalid_required_method_bodies(&db, generated).is_empty());
}

#[test]
fn generated_struct_init_body_satisfies_self_returning_method() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_struct_init_body_satisfies_self_returning_method.fe".into(),
        r#"
trait Default {
    fn default() -> Self
}

struct Foo {
    value: u256,
}

derive Default for Foo using StableDefault

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
    let context = first_builder_context(&db, top_mod);
    let output = provider_output_for_context(&db, context);
    let default_trait = find_trait(&db, top_mod, "Default");
    let method_name = *default_trait
        .method_defs(&db)
        .keys()
        .next()
        .expect("missing required method");
    let target_ty = context.request(&db).target(&db).ty(&db);
    let field = reflect_struct_fields(&db, target_ty)
        .into_iter()
        .next()
        .expect("missing reflected field");
    let value = GeneratedExprId::new(&db, GeneratedExprKind::DefaultCall { ty: field.ty });
    let fields =
        GeneratedStructFieldInitListId::new(&db, vec![GeneratedStructFieldInit { field, value }]);
    let expr = GeneratedExprId::new(
        &db,
        GeneratedExprKind::StructInit {
            target: target_ty,
            fields,
        },
    );

    let commands = BuilderCommandListId::new(
        &db,
        vec![
            BuilderCommand::EmitMethodExpr {
                name: method_name,
                expr,
            },
            BuilderCommand::Finish,
        ],
    );
    let generated = generated_impl_from_builder_commands(
        &db,
        context,
        GeneratedImplSource::ProviderOutput(output),
        commands,
    )
    .unwrap();

    assert!(generated_missing_required_methods(&db, generated).is_empty());
    assert!(generated_invalid_required_method_bodies(&db, generated).is_empty());
}

#[test]
fn generated_struct_init_body_rejects_missing_fields() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_struct_init_body_rejects_missing_fields.fe".into(),
        r#"
trait Default {
    fn default() -> Self
}

struct Foo {
    value: u256,
}

derive Default for Foo using StableDefault

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
    let context = first_builder_context(&db, top_mod);
    let output = provider_output_for_context(&db, context);
    let default_trait = find_trait(&db, top_mod, "Default");
    let method_name = *default_trait
        .method_defs(&db)
        .keys()
        .next()
        .expect("missing required method");
    let target_ty = context.request(&db).target(&db).ty(&db);
    let fields = GeneratedStructFieldInitListId::new(&db, Vec::new());
    let expr = GeneratedExprId::new(
        &db,
        GeneratedExprKind::StructInit {
            target: target_ty,
            fields,
        },
    );

    let commands = BuilderCommandListId::new(
        &db,
        vec![
            BuilderCommand::EmitMethodExpr {
                name: method_name,
                expr,
            },
            BuilderCommand::Finish,
        ],
    );
    let generated = generated_impl_from_builder_commands(
        &db,
        context,
        GeneratedImplSource::ProviderOutput(output),
        commands,
    )
    .unwrap();

    assert!(generated_missing_required_methods(&db, generated).is_empty());
    assert_eq!(
        generated_invalid_required_method_bodies(&db, generated),
        vec![method_name]
    );
}

#[test]
fn generated_struct_init_body_rejects_wrong_field_type() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_struct_init_body_rejects_wrong_field_type.fe".into(),
        r#"
trait Default {
    fn default() -> Self
}

struct Foo {
    value: u256,
}

derive Default for Foo using StableDefault

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
    let context = first_builder_context(&db, top_mod);
    let output = provider_output_for_context(&db, context);
    let default_trait = find_trait(&db, top_mod, "Default");
    let method_name = *default_trait
        .method_defs(&db)
        .keys()
        .next()
        .expect("missing required method");
    let target_ty = context.request(&db).target(&db).ty(&db);
    let field = reflect_struct_fields(&db, target_ty)
        .into_iter()
        .next()
        .expect("missing reflected field");
    let value = GeneratedExprId::new(&db, GeneratedExprKind::BoolLiteral(false));
    let fields =
        GeneratedStructFieldInitListId::new(&db, vec![GeneratedStructFieldInit { field, value }]);
    let expr = GeneratedExprId::new(
        &db,
        GeneratedExprKind::StructInit {
            target: target_ty,
            fields,
        },
    );

    let commands = BuilderCommandListId::new(
        &db,
        vec![
            BuilderCommand::EmitMethodExpr {
                name: method_name,
                expr,
            },
            BuilderCommand::Finish,
        ],
    );
    let generated = generated_impl_from_builder_commands(
        &db,
        context,
        GeneratedImplSource::ProviderOutput(output),
        commands,
    )
    .unwrap();

    assert!(generated_missing_required_methods(&db, generated).is_empty());
    assert_eq!(
        generated_invalid_required_method_bodies(&db, generated),
        vec![method_name]
    );
}

#[test]
fn generated_bool_method_body_rejects_non_bool_required_method() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generated_bool_method_body_rejects_non_bool_required_method.fe".into(),
        r#"
trait Count {
    fn count(self) -> u256
}

struct Foo {}

derive Count for Foo using StableCount

impl StableCount: Derive for Count {
    const fn derive<T>(ev: own Evidence<Count<T>>) -> Evidence<Count<T>>
        uses (builder: mut ImplBuilder<Count<T>>)
    {
        builder.finish()
        ev
    }
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let context = first_builder_context(&db, top_mod);
    let output = provider_output_for_context(&db, context);
    let count_trait = find_trait(&db, top_mod, "Count");
    let method_name = *count_trait
        .method_defs(&db)
        .keys()
        .next()
        .expect("missing required method");

    let commands = BuilderCommandListId::new(
        &db,
        vec![
            BuilderCommand::EmitMethodExpr {
                name: method_name,
                expr: GeneratedExprId::new(&db, GeneratedExprKind::BoolLiteral(true)),
            },
            BuilderCommand::Finish,
        ],
    );
    let generated = generated_impl_from_builder_commands(
        &db,
        context,
        GeneratedImplSource::ProviderOutput(output),
        commands,
    )
    .unwrap();

    assert!(generated_missing_required_methods(&db, generated).is_empty());
    assert_eq!(
        generated_invalid_required_method_bodies(&db, generated),
        vec![method_name]
    );
}
