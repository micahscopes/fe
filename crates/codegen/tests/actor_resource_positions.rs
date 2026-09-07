//! Resource positions must follow the lowered ABI, not source field ordinals.
use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::compile_actor_shader_stage;
use url::Url;

#[test]
fn resources_after_structured_actor_state_use_flattened_argument_positions() {
    for resource_first in [true,false] {
        let fields=if resource_first {
            "buffer: StorageBuffer<u32, 4>, settings: Settings,"
        } else {"settings: Settings, buffer: StorageBuffer<u32, 4>,"};
        let source=format!(r#"
use std::webgpu::{{GpuProgram,WebGpuBackend,StorageBuffer,ComputeSurface,Workgroup,FixedDispatch,FragmentSurface}}
struct Pair {{left:u32,right:u32}}
struct Settings {{pair:Pair,gain:u32}}
actor Example uses (GpuProgram<WebGpuBackend>) {{
    {fields}
    fn prepare(self) uses (ComputeSurface<Workgroup<1,1,1>,FixedDispatch<1,1,1>>) {{
        self.buffer.store(index:0,value:self.settings.pair.left)
    }}
    fn paint(self,x:i32,y:i32)->i32 uses (FragmentSurface) {{0}}
}}
"#);
        let mut db=DriverDataBase::default();
        let url=Url::parse(&format!("file:///actor_resource_order_{resource_first}.fe")).unwrap();
        db.workspace().touch(&mut db,url.clone(),Some(source));
        let file=db.workspace().get(&db,&url).unwrap();
        let top=db.top_mod(file);
        let diagnostics=db.run_on_top_mod(top).format_diags(&db);
        assert!(diagnostics.is_empty(),"{diagnostics}");
        let shader=compile_actor_shader_stage(&db,top,"prepare")
            .unwrap_or_else(|error|panic!("resource_first={resource_first}: {error}"));
        assert!(!shader.wgsl.expect("WGSL emission").is_empty());
    }
}
