use fe_hir::{
    analysis::ty::{corelib::resolve_lib_type_path, ty_def::TyId},
    core::meta::{RecordInput, RecordInputError},
    test_db::HirAnalysisTestDb,
};

#[test]
fn concrete_record_keeps_field_order_type_and_nominal_identity() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "record-input.fe".into(),
        "struct Pair { first: u256, second: bool }\nstruct Twin { first: u256, second: bool }\n",
    );
    let (module, _) = db.top_mod(file);
    db.assert_no_diags(module);
    let pair_ty = resolve_lib_type_path(&db, module.scope(), "Pair").unwrap();
    let twin_ty = resolve_lib_type_path(&db, module.scope(), "Twin").unwrap();
    let pair = RecordInput::for_authored_type(&db, pair_ty).unwrap();
    let twin = RecordInput::for_authored_type(&db, twin_ty).unwrap();
    assert_ne!(pair, twin);
    assert_eq!(pair.field_count(&db), 2);
    let first = pair.field(&db, 0).unwrap();
    let second = pair.field(&db, 1).unwrap();
    assert_eq!(first.name(&db).unwrap().data(&db), "first");
    assert_eq!(second.name(&db).unwrap().data(&db), "second");
    assert_eq!(first.ty(&db), TyId::u256(&db));
    assert_eq!(second.ty(&db), TyId::bool(&db));
    assert_eq!(first.record(), pair);
    assert_ne!(first, twin.field(&db, 0).unwrap());
    assert_eq!(
        pair.field(&db, 2),
        Err(RecordInputError::FieldOutsideRecord)
    );
}

#[test]
fn primitive_and_unsaturated_record_constructor_do_not_issue_views() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone("generic-input.fe".into(), "struct Open<T> { value: T }\n");
    let (module, _) = db.top_mod(file);
    db.assert_no_diags(module);
    assert_eq!(
        RecordInput::for_authored_type(&db, TyId::u256(&db)),
        Err(RecordInputError::NotRecord)
    );
    let constructor = resolve_lib_type_path(&db, module.scope(), "Open").unwrap();
    assert_eq!(
        RecordInput::for_authored_type(&db, constructor),
        Err(RecordInputError::NotConcrete)
    );
}

#[test]
fn authored_field_count_provider_uses_existing_checked_ctfe() {
    use fe_hir::analysis::{
        semantic::{
            EvalOutcome, GenericSubst, SemConstId, SemConstScalar, SemConstValue,
            eval_body_owner_const_with_args,
        },
        ty::ty_check::BodyOwner,
    };
    use fe_hir::test_db::find_func;

    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "record-provider.fe".into(),
        "use core::meta::{Record, field_count}\nstruct Pair { first: u256, second: bool }\nconst fn count(_ record: Record) -> usize { field_count(record) }\n",
    );
    let (module, _) = db.top_mod(file);
    db.assert_no_diags(module);
    let provider = find_func(&db, module, "count");
    let carrier = resolve_lib_type_path(&db, provider.scope(), "core::meta::Record").unwrap();
    let target = resolve_lib_type_path(&db, provider.scope(), "Pair").unwrap();
    let input = RecordInput::for_authored_type(&db, target).unwrap();
    let value = SemConstId::new(
        &db,
        SemConstValue::Meta {
            ty: carrier,
            value: fe_hir::core::meta::MetaValue::Record(input),
        },
    );
    let owner = BodyOwner::Func(provider, None);
    let outcome = eval_body_owner_const_with_args(
        &db,
        owner,
        GenericSubst::for_body_owner(&db, owner, vec![]),
        vec![value],
    );
    let EvalOutcome::Ready(result) = outcome else {
        panic!("{outcome:?}")
    };
    assert!(
        matches!(result.value(&db), SemConstValue::Scalar { value: SemConstScalar::Int { value }, .. } if value.to_string() == "2")
    );

    // A normal constant with the same struct shape is not a compiler-issued
    // structural view, even though it has the right ordinary Fe carrier type.
    let seal = fe_hir::analysis::semantic::bool_const(&db, false);
    let forged = SemConstId::new(
        &db,
        SemConstValue::Struct {
            ty: carrier,
            fields: vec![seal].into_boxed_slice(),
        },
    );
    let refusal = eval_body_owner_const_with_args(
        &db,
        owner,
        GenericSubst::for_body_owner(&db, owner, vec![]),
        vec![forged],
    );
    assert!(
        format!("{refusal:?}").contains("field_count requires a compiler-issued Record"),
        "{refusal:?}"
    );
}

#[test]
fn compiler_library_identity_ignores_local_core_module() {
    use common::ingot::IngotKind;
    use fe_hir::analysis::ty::ty_def::{TyBase, TyData};
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone("local-core-identity.fe".into(),
        "mod core { pub mod meta { pub struct Record { pub v: u256 } } }\nstruct Pair { v: u256 }\n");
    let (module, _) = db.top_mod(file);
    let record = resolve_lib_type_path(&db, module.scope(), "core::meta::Record").unwrap();
    let TyData::TyBase(TyBase::Adt(adt)) = record.data(&db) else {
        panic!("not nominal Record")
    };
    assert_eq!(
        adt.scope(&db).top_mod(&db).ingot(&db).kind(&db),
        IngotKind::Core
    );
    // Non-library target paths keep ordinary authored lookup semantics.
    let pair = resolve_lib_type_path(&db, module.scope(), "Pair").unwrap();
    assert!(RecordInput::for_authored_type(&db, pair).is_ok());
}

#[test]
fn forged_field_and_field_type_carriers_are_refused_by_reflection_intrinsics() {
    use fe_hir::analysis::{
        semantic::{GenericSubst, SemConstId, SemConstValue, eval_body_owner_const_with_args},
        ty::ty_check::BodyOwner,
    };
    use fe_hir::test_db::find_func;

    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "forged-field-carriers.fe".into(),
        r#"use core::meta::{Field, FieldQuery, FieldType, type_is, query_field, field_type}
struct Ask<T> {}
impl<T> FieldQuery<usize> for Ask<T> {
    const fn evaluate() -> usize { 1 }
}
const fn check_ft(_ f: Field) -> FieldType { field_type(f) }
const fn check_ti(_ ft: FieldType) -> bool { type_is<u256>(ft) }
const fn check_qf(_ ft: FieldType) -> usize { query_field<Ask, usize>(ft) }
"#,
    );
    let (module, _) = db.top_mod(file);
    db.assert_no_diags(module);
    let seal = fe_hir::analysis::semantic::bool_const(&db, false);
    for (fn_name, carrier_path, expected) in [
        (
            "check_ft",
            "core::meta::Field",
            "field_type requires a compiler-issued Field",
        ),
        (
            "check_ti",
            "core::meta::FieldType",
            "type_is requires a compiler-issued FieldType",
        ),
        (
            "check_qf",
            "core::meta::FieldType",
            "query_field requires a compiler-issued FieldType",
        ),
    ] {
        let func = find_func(&db, module, fn_name);
        let carrier = resolve_lib_type_path(&db, func.scope(), carrier_path).unwrap();
        let forged = SemConstId::new(
            &db,
            SemConstValue::Struct {
                ty: carrier,
                fields: vec![seal].into_boxed_slice(),
            },
        );
        let owner = BodyOwner::Func(func, None);
        let refusal = eval_body_owner_const_with_args(
            &db,
            owner,
            GenericSubst::for_body_owner(&db, owner, vec![]),
            vec![forged],
        );
        assert!(
            format!("{refusal:?}").contains(expected),
            "expected `{expected}`, got `{refusal:?}`"
        );
    }
}
