use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WasmCompileOptions, compile_runtime_package_wasm_with_options};
use url::Url;

#[test]
fn parameter_edits_follow_named_declarations_not_state_field_order() {
    let source = r#"
use core::marker::Copy
use std::web::view::{Param,ParamReadout,SurfaceState,SurfaceEvent,SurfaceEventKind,
    ApplyParamBindings,ParamBindingsProvider}
struct Controls {first:f32,second:f32}
impl Copy for Controls {}
struct Params {observed:Param,second:Param,first:Param}
impl SurfaceState for Controls {type Params=Params}
derive ApplyParamBindings for Controls using ParamBindingsProvider<Params>
const fn params()->Params {
    Params {observed:Param::readout(ParamReadout::Scalar),
        second:Param::range(min:0.0,max:10.0,init:2.0),
        first:Param::range(min:0.0,max:10.0,init:1.0)}
}
pub fn edit(index:u32)->bool {
    let before=Controls {first:1.0,second:2.0}
    let event=SurfaceEvent {pointer_x:0.0,pointer_y:0.0,delta_x:0.0,delta_y:0.0,
        wheel_delta:0.0,wheel_mode:0,buttons:0,timestamp:0.0,width:32.0,height:32.0,
        event_kind:SurfaceEventKind::ParamEdit,param_index:index,param_value:7.0}
    let after=before.apply_param_bindings(params(),event)
    let first=if index==2 {7.0} else {1.0}
    let second=if index==1 {7.0} else {2.0}
    after.first==first && after.second==second
}
"#;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///named_param_binding_order.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let package = mir::build_wasm_runtime_package_for_entry(&db, top, "edit").unwrap();
    let artifact =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .unwrap();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &artifact.bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let edit = instance
        .get_typed_func::<i32, i32>(&mut store, "edit")
        .unwrap();
    for index in [0, 1, 2, 3, 255] {
        assert_eq!(
            edit.call(&mut store, index).unwrap(),
            1,
            "parameter index {index}"
        );
    }
}
