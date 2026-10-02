use common::InputDb;
use fe_driver::DriverDataBase;
use hir::analysis::ty::corelib::{intrinsic_contract, resolve_lib_func_path};

#[test]
fn reflection_contracts_follow_core_declarations_and_reject_impostors() {
    let mut db = DriverDataBase::default();
    let url = url::Url::parse("file:///reflection_contracts.fe").unwrap();
    db.workspace().update(
        &mut db,
        url.clone(),
        include_str!("fixtures/read_back_view_rejected/impostor_intrinsic.fe").to_owned(),
    );
    let file = db.workspace().get(&db, &url).unwrap();
    let module = db.top_mod(file);
    let impostor = module
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|function| {
            function
                .name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "node_kind")
        })
        .unwrap();
    assert!(intrinsic_contract(&db, impostor).is_none());
    let core = resolve_lib_func_path(&db, impostor.scope(), "core::meta::node_kind").unwrap();
    let declarations = core
        .top_mod(&db)
        .all_funcs(&db)
        .iter()
        .copied()
        .filter(|function| function.is_extern(&db))
        .collect::<Vec<_>>();
    assert_eq!(declarations.len(), 23, "core reflection extern surface");
    for declaration in declarations {
        let contract = intrinsic_contract(&db, declaration)
            .unwrap_or_else(|| panic!("missing contract: {:?}", declaration.name(&db)));
        assert_eq!(contract.memory, Some([].as_slice()));
        assert_eq!(contract.pointer_return, None);
    }
}
