use camino::Utf8PathBuf;
use fe_mir::{
    RuntimeInstanceKey, get_or_build_runtime_instance, indirect_host_result,
    instance::RuntimeInstanceSource,
};
use hir::{
    analysis::{
        semantic::{get_or_build_semantic_instance, identity_semantic_instance_key},
        ty::ty_check::BodyOwner,
    },
    hir_def::{Func, HostResultCodec, TopLevelMod},
    test_db::HirAnalysisTestDb,
};

const SOURCE: &str = r#"
pub enum Reply {
    Ok { value: u32 },
    Error { code: u32 }
}

#[host_import(module = "fe:host")]
extern {
    #[host_result(codec = "fe:host-wasm-codec/v1")]
    pub unsafe fn receive() -> Reply
}
"#;

fn receive<'db>(db: &'db HirAnalysisTestDb, top_mod: TopLevelMod<'db>) -> Func<'db> {
    top_mod
        .all_funcs(db)
        .iter()
        .copied()
        .find(|func| {
            func.name(db)
                .to_opt()
                .is_some_and(|name| name.data(db) == "receive")
        })
        .expect("missing generated extern function")
}

#[test]
fn generated_descriptor_is_preserved_from_hir_to_mir() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(Utf8PathBuf::from("generated_host_result.fe"), SOURCE);
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    let func = receive(&db, top_mod);
    let hir_descriptor = hir::hir_def::ItemKind::Func(func)
        .attrs(&db)
        .and_then(|attrs| attrs.indirect_host_result(&db))
        .expect("HIR must retain generated host-result metadata");
    assert_eq!(hir_descriptor.codec, HostResultCodec::FeHostWasm);
    assert_eq!(hir_descriptor.version, 1);
    assert!(hir_descriptor.requires_realloc);
    assert!(hir_descriptor.requires_post_return);

    let semantic = get_or_build_semantic_instance(
        &db,
        identity_semantic_instance_key(&db, BodyOwner::Func(func)),
    );
    let key = RuntimeInstanceKey::new(&db, RuntimeInstanceSource::Semantic(semantic), Vec::new());
    let runtime = get_or_build_runtime_instance(&db, key);
    assert_eq!(
        indirect_host_result(&db, runtime),
        Some(hir_descriptor),
        "MIR must carry the exact typed descriptor without backend reinterpretation"
    );
}
