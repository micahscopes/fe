use fe_hir::{
    analysis::ty::{
        const_expr::{ConstExpr, ConstExprId},
        const_ty::{
            ConstCanonEnv, ConstTyData, ConstTyId, EvaluatedConstTy,
            evaluate_type_level_const_expr, evaluate_type_level_int_const_expr,
        },
        trait_resolution::PredicateListId,
        ty_def::{TyData, TyId},
    },
    hir_def::IntegerId,
    test_db::HirAnalysisTestDb,
};

#[test]
fn scalar_intrinsic_type_level_arithmetic_keeps_its_fast_path() {
    for (op, expected) in [
        ("add", 10u32),
        ("sub", 4),
        ("mul", 21),
        ("div", 2),
        ("rem", 1),
        ("pow", 343),
        ("shl", 56),
        ("shr", 0),
        ("bitand", 3),
        ("bitor", 7),
        ("bitxor", 4),
    ] {
        let mut db = HirAnalysisTestDb::default();
        let source = format!("extern {{ const fn __{op}_u256(_: u256, _: u256) -> u256 }}");
        let file = db.new_stand_alone("scalar_const_identity.fe".into(), &source);
        let (top_mod, _) = db.top_mod(file);
        let func = top_mod.all_funcs(&db)[0];
        let ty = TyId::u256(&db);
        let args = [7u32, 3]
            .into_iter()
            .map(|value| {
                TyId::new(
                    &db,
                    TyData::ConstTy(ConstTyId::new(
                        &db,
                        ConstTyData::Evaluated(
                            EvaluatedConstTy::LitInt(IntegerId::new(
                                &db,
                                num_bigint::BigUint::from(value),
                            )),
                            ty,
                        ),
                    )),
                )
            })
            .collect();
        let expr = ConstExprId::new(
            &db,
            ConstExpr::ExternConstFnCall {
                func,
                generic_args: vec![],
                args,
            },
        );
        let result = evaluate_type_level_int_const_expr(&db, expr, ty).unwrap();
        match result.data(&db) {
            ConstTyData::Evaluated(EvaluatedConstTy::LitInt(value), _) => assert_eq!(
                value.data(&db),
                &num_bigint::BigUint::from(expected),
                "{op}"
            ),
            other => panic!("expected scalar result for {op}, got {other:?}"),
        }
    }
}

#[test]
fn authored_numeric_names_do_not_select_type_level_arithmetic() {
    for name in [
        "add",
        "sub",
        "mul",
        "__add_u256",
        "__sub_u256",
        "__checked_add_u256",
    ] {
        let mut db = HirAnalysisTestDb::default();
        let source = format!("const fn {name}(_ a: u256, _ b: u256) -> u256 {{ a ^ b }}");
        let file = db.new_stand_alone("const_numeric_identity.fe".into(), &source);
        let (top_mod, _) = db.top_mod(file);
        let func = top_mod.all_funcs(&db)[0];
        let ty = TyId::u256(&db);
        let args = [7u32, 3]
            .into_iter()
            .map(|value| {
                TyId::new(
                    &db,
                    TyData::ConstTy(ConstTyId::new(
                        &db,
                        ConstTyData::Evaluated(
                            EvaluatedConstTy::LitInt(IntegerId::new(
                                &db,
                                num_bigint::BigUint::from(value),
                            )),
                            ty,
                        ),
                    )),
                )
            })
            .collect();
        let expr = ConstExprId::new(
            &db,
            ConstExpr::UserConstFnCall {
                func,
                generic_args: vec![],
                args,
            },
        );
        assert!(
            evaluate_type_level_int_const_expr(&db, expr, ty).is_none(),
            "authored {name} must not be evaluated by spelling"
        );
        let env = ConstCanonEnv::new(func.scope(), PredicateListId::empty_list(&db), None);
        let result = evaluate_type_level_const_expr(&db, expr, ty, env).unwrap();
        match result.data(&db) {
            ConstTyData::Evaluated(EvaluatedConstTy::LitInt(value), _) => {
                assert_eq!(value.data(&db), &4u32.into(), "{name}")
            }
            other => panic!("expected authored XOR result for {name}, got {other:?}"),
        }
    }
}
