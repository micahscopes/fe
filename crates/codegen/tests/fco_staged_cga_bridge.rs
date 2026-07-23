use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

fn compile(name: &str, source: &str) -> Result<Vec<u8>, String> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}")).unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .map_err(|err| err.to_string())?
        .into_bytecode()
        .ok_or_else(|| "backend output was not bytecode".to_string())
}

fn source_without_projection_root() -> String {
    include_str!("../../hir/tests/fixtures/fco_cl41_schedule_probe.fe")
        .replace(
            "type CanonicalSchedule = StagedSchedule<32>",
            "type CanonicalSchedule = StagedSchedule<4>",
        )
        .replace(
            "pub fn fco_published_schedule_runtime()",
            "fn fco_published_schedule_runtime()",
        )
}

#[test]
fn direct_fe_computed_ground_schedule_executes() {
    let mut source = source_without_projection_root();
    source.push_str(
        "\npub fn direct_fe_computed_schedule_runtime() -> i32 {\n\
             <CanonicalSchedule as Eval>::eval()\n\
         }\n",
    );
    let wasm = compile("fco_staged_cga_direct.fe", &source)
        .expect("the direct Fe-computed Schedule<4> should compile");
    wasmparser::validate(&wasm).unwrap();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let run = instance
        .get_typed_func::<(), i32>(&mut store, "direct_fe_computed_schedule_runtime")
        .unwrap();
    assert_eq!(run.call(&mut store, ()).unwrap(), 4);
}

#[test]
fn fco_published_projection_executes_the_same_ground_tree() {
    let source = include_str!("../../hir/tests/fixtures/fco_cl41_schedule_probe.fe").replace(
        "type CanonicalSchedule = StagedSchedule<32>",
        "type CanonicalSchedule = StagedSchedule<4>",
    );
    let wasm = compile("fco_staged_cga_projection.fe", &source)
        .expect("FCO-published projection should compile as the same Schedule<4> tree");
    wasmparser::validate(&wasm).unwrap();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let run = instance
        .get_typed_func::<(), i32>(&mut store, "fco_published_schedule_runtime")
        .unwrap();
    assert_eq!(run.call(&mut store, ()).unwrap(), 4);
}
