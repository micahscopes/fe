use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WasmCompileOptions, compile_runtime_package_wasm_with_options};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;

const SOURCE: &str = include_str!("fixtures/recursive_clifford_support_plan_ingot/src/lib.fe");

#[test]
fn authored_recurrence_derives_sparse_plan_through_an_ordinary_ingot() {
    assert!(SOURCE.contains("sphere.clifford_gp("));
    assert!(SOURCE.contains("first.clifford_gp("));
    assert_eq!(
        SOURCE.matches(".clifford_gp(").count(),
        2,
        "the dependent ingot must derive S*P*S through the public recurrence"
    );
    assert!(SOURCE.contains("{authored_vector_keep0()}"));
    assert!(SOURCE.contains("{authored_vector_support_count()}"));
    for forbidden in [
        "support_gp(",
        "schedule_keep_word",
        "SCHEDULE_KEEP",
        "gp_sign",
        "raw_",
        "triple",
        "Term<1>",
        "python",
        "ImplBuilder",
    ] {
        assert!(
            !SOURCE.contains(forbidden),
            "dependent ingot must not reconstruct support or plan through `{forbidden}`"
        );
    }

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/recursive_clifford_support_plan_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "support-plan ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("support-plan ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected support-plan diagnostics:\n{diagnostics}"
    );
    let package =
        mir::build_wasm_runtime_package_for_entry(&db, top_mod, "authored_support_plan_probe")
            .expect("CTFE-derived support plan runtime package");
    let wasm =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .expect("CTFE-derived support plan Wasm")
            .bytes;
    wasmparser::validate(&wasm).expect("support-plan Wasm validates");

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let probe = instance
        .get_typed_func::<(), i32>(&mut store, "authored_support_plan_probe")
        .unwrap();
    assert_eq!(probe.call(&mut store, ()).unwrap(), 19_266_655);
}
