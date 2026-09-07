//! Const-indexed task families must share authored functions, not source wrappers.
use common::InputDb;
use driver::DriverDataBase;
use hir::analysis::{
    semantic::{
        GenericSubst, SemanticInstanceKey, get_or_build_semantic_instance,
        identity_semantic_instance_key,
    },
    ty::{
        const_ty::{ConstTyData, ConstTyId, EvaluatedConstTy},
        ty_check::BodyOwner,
        ty_def::{TyData, TyId},
    },
};
use hir::hir_def::{CallableDef, IntegerId};
use url::Url;

#[test]
fn exact_const_specializations_are_distinct_executable_internal_roots() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///specialized_roots.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(
        "fn indexed<const I: u32>(_ value: u32) -> u32 { value + I }\nfn policy(_ value: u32) -> u32 { value * 2 }".to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top = db.top_mod(file);
    let func = top
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|func| {
            func.name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "indexed")
        })
        .unwrap();
    let identity = identity_semantic_instance_key(&db, BodyOwner::Func(func));
    let const_ty = CallableDef::Func(func).params(&db)[0]
        .const_ty_ty(&db)
        .unwrap();
    let instances = [0_u32, 3].map(|index| {
        let arg = TyId::new(
            &db,
            TyData::ConstTy(ConstTyId::new(
                &db,
                ConstTyData::Evaluated(
                    EvaluatedConstTy::LitInt(IntegerId::new(&db, num_bigint::BigUint::from(index))),
                    const_ty,
                ),
            )),
        );
        let key = SemanticInstanceKey::new(
            &db,
            identity.owner(&db),
            GenericSubst::new(&db, vec![arg]),
            identity.effect_providers(&db),
            identity.impl_env(&db).clone(),
        );
        get_or_build_semantic_instance(&db, key)
    });
    let policy = top
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|func| {
            func.name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "policy")
        })
        .unwrap();
    for include_policy in [false, true] {
        let package = if include_policy {
            mir::build_wasm_runtime_package_for_entries_with_internal_roots(
                &db,
                top,
                &[],
                &[policy],
                &instances,
            )
            .unwrap()
        } else {
            mir::build_wasm_runtime_package_for_entries_with_internal_instances(
                &db,
                top,
                &[],
                &instances,
            )
            .unwrap()
        };
        let mut options = fe_codegen::WasmCompileOptions::default();
        for (index, semantic) in instances.iter().enumerate() {
            let function = package
                .functions(&db)
                .iter()
                .copied()
                .find(|function| function.instance(&db).key(&db).semantic(&db) == Some(*semantic))
                .unwrap();
            options = options.with_export_alias(function.symbol(&db), format!("slot{index}"));
        }
        if include_policy {
            let symbol = mir::runtime_package_symbol_for_func(&db, package, policy).unwrap();
            options = options.with_export_alias(symbol, "policy_call");
        }
        let artifact =
            fe_codegen::compile_runtime_package_wasm_with_options(&db, &package, options).unwrap();
        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &artifact.bytes).unwrap();
        let mut store = wasmtime::Store::new(&engine, ());
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        for (index, expected) in [(0, 11), (1, 14)] {
            let call = instance
                .get_typed_func::<i32, i32>(&mut store, &format!("slot{index}"))
                .unwrap();
            assert_eq!(call.call(&mut store, 11).unwrap(), expected);
        }
        if include_policy {
            let call = instance
                .get_typed_func::<i32, i32>(&mut store, "policy_call")
                .unwrap();
            assert_eq!(call.call(&mut store, 11).unwrap(), 22);
        }
    }
    assert!(
        mir::build_wasm_runtime_package_for_entries_with_internal_instances(
            &db,
            top,
            &[],
            &[instances[0], instances[0]]
        )
        .is_err()
    );
    assert!(
        mir::build_wasm_runtime_package_for_entries_with_internal_roots(
            &db,
            top,
            &[],
            &[policy, policy],
            &instances
        )
        .is_err()
    );
    assert!(
        mir::build_wasm_runtime_package_for_entries_with_internal_roots(
            &db,
            top,
            &[],
            &[func],
            &instances
        )
        .is_err()
    );
    assert!(
        mir::build_wasm_runtime_package_for_entries_with_internal_roots(&db, top, &[], &[], &[])
            .is_err()
    );
}
