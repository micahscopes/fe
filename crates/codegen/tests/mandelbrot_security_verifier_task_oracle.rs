//! Independent structural gate for the production verifier task plan.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{layout_for, BackendKind, OptLevel};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;

const SECURITY_QUERY_COUNT: u32 = 114;
const SHARED_TASKS: u32 = 6;
const TASK_COUNT: u32 = SHARED_TASKS + SECURITY_QUERY_COUNT;

fn fixture_url() -> Url {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mandelbrot_security_verifier_task_oracle_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn compile_wasm() -> Vec<u8> {
    let url = fixture_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "security verifier task fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("security verifier task fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected security verifier task diagnostics:\n{diagnostics}",
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("security verifier task fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm backend should emit bytes");
    wasmparser::validate(&bytes).expect("security verifier task Wasm should validate");
    bytes
}

#[test]
fn production_security_verifier_tasks_are_derived_and_replay_authenticated() {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, compile_wasm())
        .expect("security verifier task module should load");
    assert_eq!(module.imports().len(), 0, "fixture must remain zero-import");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("security verifier task module should instantiate");

    let descriptor = instance
        .get_typed_func::<i32, (i32, i32)>(
            &mut store,
            "production_security_verifier_task_descriptor",
        )
        .expect("security verifier task descriptor export");
    for index in 0..TASK_COUNT {
        let (tag, payload) = descriptor
            .call(&mut store, index as i32)
            .expect("task descriptor should execute");
        let expected = if index < SHARED_TASKS {
            ((index + 1) as i32, 0)
        } else {
            (7, (index - SHARED_TASKS) as i32)
        };
        assert_eq!((tag, payload), expected, "semantic task {index}");
    }
    assert_eq!(
        descriptor
            .call(&mut store, TASK_COUNT as i32)
            .expect("invalid task descriptor should execute"),
        (0, 0),
    );

    let audit = instance
        .get_typed_func::<i32, (i32, i32, i32, i32, i32)>(
            &mut store,
            "production_security_verifier_task_trace_audit",
        )
        .expect("security verifier trace audit export");
    assert_eq!(
        audit.call(&mut store, 0).expect("clean trace audit"),
        (1, 1, 0, 0, 0),
    );
    assert_eq!(
        audit
            .call(&mut store, 1)
            .expect("coherent task mutation audit"),
        (1, 1, 1, 0, 0),
    );
    assert_eq!(
        audit.call(&mut store, 2).expect("result mutation audit"),
        (1, 1, 0, 1, 0),
    );
    assert_eq!(
        audit
            .call(&mut store, 3)
            .expect("query payload mutation audit"),
        (1, 1, 1, 0, 0),
    );
}
