//! Executed semantic gate for Fe-authored vertex + fragment raster lowering.
//!
//! A 2x2 target observes a varying emitted at one triangle vertex. Exact color
//! counts prove that the generated GPU pipeline executes both Fe bodies,
//! interpolates their typed interface, shares actor state, and honors the
//! Fe-derived draw count. WGSL text alone cannot establish those properties.

use std::path::Path;

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WasmCompileOptions, WebBindingAccess, WebBuildOptions, WebBundle,
    compile_runtime_package_wasm_with_options,
};
use hir::hir_def::HirIngot;
use url::Url;

fn compile_bundle(bounded_increment: bool) -> WebBundle {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/actor_raster_typed");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    assert!(!driver::init_ingot(&mut db, &url));
    if bounded_increment {
        let source = include_str!("fixtures/actor_raster_typed/src/lib.fe")
            .replace("if vertex_index == 1", "if vertex_index + 1 == 2");
        db.workspace().touch(&mut db, url.join("src/lib.fe").unwrap(), Some(source));
    }
    let top_mod = db
        .workspace()
        .containing_ingot(&db, url)
        .unwrap()
        .root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let bundle = WebBundle::compile(&db, top_mod, WebBuildOptions::render("shade", None)).unwrap();
    assert!(
        bundle.wgsl.contains("fn tinted_heat")
            && bundle.wgsl.contains("fn shade_color")
            && bundle.wgsl.matches("shade_color").count() >= 2,
        "paired authored raster stages must retain the closed scalar helper graph:\n{}",
        bundle.wgsl,
    );
    bundle
}

#[test]
fn raster_constant_divisor_guards_normalize_but_real_traps_remain() {
    let source = include_str!("fixtures/actor_raster_typed/src/lib.fe");
    for (name, divisor, succeeds) in [("constant", "3", true), ("dynamic", "vertex_index", false)] {
        let source = source.replace("if vertex_index ==", &format!("if vertex_index % {divisor} =="));
        let mut db = DriverDataBase::default();
        let url = Url::parse(&format!("file:///raster_divisor_{name}.fe")).unwrap();
        db.workspace().touch(&mut db, url.clone(), Some(source));
        let file = db.workspace().get(&db, &url).unwrap();
        let top = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top).format_diags(&db);
        assert!(diagnostics.is_empty(), "{diagnostics}");
        let result = WebBundle::compile(&db, top, WebBuildOptions::render("shade", None));
        if succeeds {
            let bundle = result.expect("statically nonzero divisor must not retain a dead trap");
            assert!(!bundle.wgsl.is_empty());
        } else {
            let error = result.err().expect("possible zero divisor must still fail closed").to_string();
            assert!(error.contains("trap") || error.contains("nonzero"), "{error}");
        }
    }
}

#[test]
fn raster_enum_match_preserves_closed_return_domain_in_root_and_helper() {
    // Both placements must compile. Proved callee return bounds eliminate the
    // invalid-tag path without requiring the author to relocate the match.
    for at_root in [false, true] {
        let source = include_str!("fixtures/actor_raster_typed/src/lib.fe");
        let source = if at_root {
            source.replace("        RasterVertex {", "        let slot = match checked_slot(Edge {low:vertex_index,high:vertex_index.wrapping_add(1)}) { Some(v) => v, None => 0 }\n        RasterVertex {")
            .replace("if vertex_index == 1", "if slot == 1")
        } else {
            source.replace("if vertex_index == 1", "if resolved(vertex_index) == 1")
        };
        let source = source
            .replace(
                "if vertex_index == 2",
                "if resolved(vertex_index.wrapping_add(1)) == 2",
            )
            .replace(
                "if vertex_index == 0",
                "if resolved(vertex_index.wrapping_add(2)) == 0",
            );
        let source = format!(
            r#"{source}
struct Edge {{low:u32,high:u32}}
impl Copy for Edge {{}}
fn checked_slot(_ edge:Edge)->Option<u32> {{
    if edge.high==4 && edge.low<4 {{return Some(edge.low.wrapping_add(6))}}
    if edge.low==0 && edge.high==1 {{Some(0)}}
    else if edge.low==1 && edge.high==2 {{Some(1)}}
    else if edge.low==2 && edge.high==3 {{Some(2)}}
    else if edge.low==0 && edge.high==3 {{Some(3)}}
    else if edge.low==0 && edge.high==2 {{Some(4)}}
    else if edge.low==1 && edge.high==3 {{Some(5)}} else {{None}}
}}
fn resolved(_ n:u32)->u32 {{match checked_slot(Edge {{low:n,high:n.wrapping_add(1)}}) {{Some(v)=>v,None=>0}}}}
"#
        );
        let mut db = DriverDataBase::default();
        let url = Url::parse(&format!("file:///raster_retained_enum_{at_root}.fe")).unwrap();
        db.workspace().touch(&mut db, url.clone(), Some(source));
        let file = db.workspace().get(&db, &url).unwrap();
        let top = db.top_mod(file);
        let diagnostics = db.run_on_top_mod(top).format_diags(&db);
        assert!(diagnostics.is_empty(), "{diagnostics}");
        let result = WebBundle::compile(&db, top, WebBuildOptions::render("shade", None));
        assert!(!result.expect("both exhaustive-match placements must compile").wgsl.is_empty());
    }
}

#[test]
fn raster_bundles_execute_const_indexed_scoped_task_families() {
    let source = include_str!("fixtures/actor_raster_typed/src/lib.fe");
    let source = format!(
        "{}\n{}",
        r#"
use core::actor::{ScopedTask, ScopedTaskFamily}
use core::pending::{Suspend, TaskOutcome, Timer}
use std::host::{HostTimer, Resumable, sleep}
use std::wasm::WasmBackend
"#,
        source.replace(
            "actor TypedMesh uses (GpuProgram<WebGpuBackend>) {",
            r#"
actor TypedMesh uses (GpuProgram<WebGpuBackend>) {
    fn observe(self) -> u32 uses (ScopedTask) {
        with (Timer<WasmBackend> = HostTimer {}, Suspend<WasmBackend, u32> = Resumable {}) {
            match sleep(0) {
                TaskOutcome::Success(_) => if self.tint > 0.0 { 13 } else { 99 },
                _ => 99,
            }
        }
    }
    fn heartbeat<const I: u32>() -> u32 uses (ScopedTaskFamily<2>) {
        with (Timer<WasmBackend> = HostTimer {}, Suspend<WasmBackend, u32> = Resumable {}) {
            match sleep(0) {
                TaskOutcome::Success(_) => I + 11,
                _ => 99,
            }
        }
    }
"#
        )
    );
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///raster_scoped_families.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source));
    let file = db.workspace().get(&db, &url).unwrap();
    let top = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top).format_diags(&db);
    assert!(diagnostics.is_empty(), "{diagnostics}");
    let bundle = WebBundle::compile(&db, top, WebBuildOptions::render("shade", None)).unwrap();
    assert_eq!(
        bundle.scoped_tasks.len(),
        3,
        "raster compilation must not discard Fe tasks"
    );
    assert_eq!(bundle.manifest.passes.len(), 1);
    assert_ne!(
        bundle.scoped_tasks[0].start_export,
        bundle.scoped_tasks[1].start_export
    );
    let directory = tempfile::tempdir().unwrap();
    let site = directory.path().join("site");
    bundle.write_atomic(&site).unwrap();
    let wasm = bundle
        .manifest
        .artifacts
        .wasm
        .as_ref()
        .expect("task Wasm artifact");
    let script = format!(
        r#"
import {{createMaterializedTaskRegistry}} from './tasks/tasks.js';
import {{createHostCompletionBroker}} from './tasks/host-completion.js';
const broker=createHostCompletionBroker();
const {{instance}}=await WebAssembly.instantiate(await Bun.file({wasm:?}).arrayBuffer(),broker.imports);
const tasks=Object.values(createMaterializedTaskRegistry(instance.exports));
if(tasks.filter(task=>task.inputWidth===1).length!==1)throw Error('missing state-taking task');
const results=await Promise.all(tasks.map(task=>broker.run(task,task.inputWidth ? task.liftInput([0.4]) : [])));
if(JSON.stringify(results.map(r=>r[0]).sort())!==JSON.stringify([11,12,13]))throw Error(JSON.stringify(results));
if(broker.activeCount()!==0)throw Error('leaked task operations');
"#
    );
    std::fs::write(site.join("run.mjs"), script).unwrap();
    let result = std::process::Command::new("bun")
        .arg("run.mjs")
        .current_dir(&site)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn raster_tasks_deliver_nominal_messages_into_resident_render_state() {
    let source = include_str!("fixtures/actor_raster_typed/src/lib.fe");
    let source = format!("{}\n{}", r#"
use core::actor::{ActorSink, InitialState, ResidentTransition, ScopedTaskFamily}
use core::pending::{Suspend, TaskOutcome}
use std::actor::{ActorMessage, BrowserActorSink}
use std::host::Resumable
use std::wasm::WasmBackend
struct Message { amount: f32 }
struct WrongMessage { amount: f32 }
struct State { tint: f32 }
use std::webgpu::StorageBuffer
use std::webgpu::{SurfaceTransition, SurfaceScheduling, LatestPerFrame}
use std::web::SurfaceEvent
"#, source.replace("    tint: f32,", r#"
    payload: StorageBuffer<u32, 4>,
    tint: f32,
    fn initial() -> State uses (InitialState) { State { tint: 10.0 } }
    fn receive(self, event: Message) -> State uses (ResidentTransition) {
        State { tint: self.tint + event.amount }
    }
    fn control(self, event: own SurfaceEvent) -> State
        uses (SurfaceTransition, SurfaceScheduling<LatestPerFrame>) {
        State { tint: self.tint + event.delta_x }
    }
    fn notify<const I: u32>(state: State) -> u32 uses (ScopedTaskFamily<2>) {
        with (ActorSink<WasmBackend, Message> = BrowserActorSink {},
            Suspend<WasmBackend, u32> = Resumable {}) {
            match ActorMessage::new(Message { amount: if I == 0 { 1.0 } else { 2.0 } }).send() {
                TaskOutcome::Success(_) => if state.tint > 0.0 { I + 1 } else { 99 },
                _ => 99,
            }
        }
    }
"#));
    let compile = |source: String| {
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///raster_task_notifications.fe").unwrap();
        db.workspace().touch(&mut db, url.clone(), Some(source));
        let top = db.top_mod(db.workspace().get(&db, &url).unwrap());
        let diagnostics = db.run_on_top_mod(top).format_diags(&db);
        assert!(diagnostics.is_empty(), "{diagnostics}");
        WebBundle::compile(&db, top, WebBuildOptions::render("shade", None))
    };
    let bundle = compile(source.clone()).unwrap();
    assert_eq!(bundle.scoped_tasks.len(), 2);
    let directory = tempfile::tempdir().unwrap();
    let site = directory.path().join("site");
    bundle.write_atomic(&site).unwrap();
    let wasm = bundle.manifest.artifacts.wasm.as_ref().unwrap();
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/render-runtime/fe-render-runtime.js");
    let script = format!(r#"
import {{createMaterializedTaskRegistry}} from './tasks/tasks.js';
import {{createHostCompletionBroker}} from './tasks/host-completion.js';
globalThis.HTMLElement=class {{}};
globalThis.customElements={{define(){{}}}};
const {{FeSurfaceElement}}=await import({runtime:?});
const surface=Object.create(FeSurfaceElement.prototype);
surface._resources={resources};
surface._members=[{{name:'tint'}}];
surface._fsm='hidden';
surface._refreshControlValues=()=>{{}};
const broker=createHostCompletionBroker({{actorEvents:{{send:(event,signal)=>surface._deliverActorNotification(event,signal)}}}});
const {{instance}}=await WebAssembly.instantiate(await Bun.file({wasm:?}).arrayBuffer(),broker.imports);
const e=instance.exports;
surface._wasmArenaCheckpoint=e.fe_cabi_checkpoint;
surface._wasmArenaRewind=e.fe_cabi_rewind;
surface._actorNotificationKernel=e.fe_surface_actor_message_v1;
surface._surfaceStateReplaceKernel=e.fe_surface_state_replace_v1;
const initial=surface._runWasmInitialization(()=>e.fe_surface_initialize_v1());
surface._replaceSurfaceState(Array.isArray(initial)?initial:[initial]);
const tasks=Object.values(createMaterializedTaskRegistry(e));
const results=await Promise.all(tasks.map(task=>broker.run(task,task.liftInput([10]))));
if(JSON.stringify(results.map(r=>r[0]).sort())!=='[1,2]')throw Error(JSON.stringify(results));
if(surface._uniforms[0]!==13)throw Error('messages failed to share resident state: '+surface._uniforms);
// A real scheduled surface transition must see those same globals.
surface._attachSurfaceByteTransport(e);
surface._surfaceTransitionKernel=e.fe_surface_transition_scheduled_v1;
surface._surfaceTransitionStateResident=true;
const afterControl=surface._runSurfaceFrame([{{mx:0,my:0,dx:4,dy:0,wheelDelta:0,wheelMode:0,
    buttons:0,timestamp:0,width:2,height:2,eventKind:0,paramIndex:0,paramValue:0}}]);
if(afterControl[0]!==17)throw Error('surface and task states diverged');
surface._deliverActorNotification([2]);
if(surface._uniforms[0]!==19)throw Error('notification missed surface state');
surface._replaceSurfaceState([20]);
surface._deliverActorNotification([2]);
if(surface._uniforms[0]!==22)throw Error('replacement and message state diverged');
const cancelled=new AbortController();cancelled.abort();
let rejected=false;
try{{surface._deliverActorNotification([100],cancelled.signal)}}catch(error){{rejected=error.name==='AbortError'}}
if(!rejected||surface._uniforms[0]!==22)throw Error('cancelled notification mutated state');
if(broker.activeCount()!==0)throw Error('leaked task operations');
"#, runtime=runtime.to_string_lossy(), resources=serde_json::to_string(&bundle.manifest.resources).unwrap());
    std::fs::write(site.join("run.mjs"), script).unwrap();
    let result = std::process::Command::new("bun").arg("run.mjs").current_dir(&site).output().unwrap();
    assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    let wrong = source.replace("ActorSink<WasmBackend, Message>", "ActorSink<WasmBackend, WrongMessage>")
        .replace("ActorMessage::new(Message", "ActorMessage::new(WrongMessage");
    assert!(compile(wrong).unwrap_err().to_string().contains("typed sink event differs"));
    let missing = source.replace(" uses (ResidentTransition)", "");
    assert!(compile(missing).unwrap_err().to_string().contains("no ResidentTransition target"));
    let opaque_self = source.replace("fn notify<const I: u32>(state: State)", "fn notify<const I: u32>(self)")
        .replace("if state.tint", "if self.tint");
    assert!(compile(opaque_self).unwrap_err().to_string().contains("opaque resource custody"));
}

fn device() -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let allow_skip = std::env::var_os("MB2_ALLOW_GPU_SKIP").is_some();
    let instance = wgpu::Instance::default();
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        ..Default::default()
    })) {
        Ok(adapter) => adapter,
        Err(error) if allow_skip => {
            eprintln!("authored raster GPU gate skipped (MB2_ALLOW_GPU_SKIP): {error:?}");
            return None;
        }
        Err(error) => panic!("authored raster GPU gate has no adapter: {error:?}"),
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::empty(),
        ..Default::default()
    }))
    .unwrap();
    Some((adapter, device, queue))
}

fn readback(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(30)),
        })
        .unwrap();
    rx.recv().unwrap().unwrap();
    let bytes = slice.get_mapped_range().to_vec();
    buffer.unmap();
    bytes
}

#[test]
fn fe_vertex_varying_and_fragment_execute_as_one_gpu_pipeline() {
    execute_raster(compile_bundle(false));
}

#[test]
fn proved_vertex_increment_preserves_executed_pixels() {
    execute_raster(compile_bundle(true));
}

fn execute_raster(bundle: WebBundle) {
    let pass = &bundle.manifest.passes[0];
    assert_eq!(pass.draw_vertices, Some(3));
    let Some((adapter, device, queue)) = device() else {
        return;
    };
    eprintln!("authored raster adapter: {}", adapter.get_info().name);

    let binding = &pass.layout.bindings[0];
    assert_eq!(binding.members[0].name, "tint");
    assert_eq!(binding.access, WebBindingAccess::Read);
    let state = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Fe authored raster state"),
        size: 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&state, 0, &0.4f32.to_le_bytes());
    let group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Fe authored raster group layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: binding.binding,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Fe authored raster group"),
        layout: &group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: binding.binding,
            resource: state.as_entire_binding(),
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Fe authored raster pipeline layout"),
        bind_group_layouts: &[Some(&group_layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Fe authored raster WGSL"),
        source: wgpu::ShaderSource::Wgsl(bundle.wgsl.as_str().into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Fe authored raster pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: pass.layout.vertex_entry.as_deref(),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: pass.layout.fragment_entry.as_deref(),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Fe authored raster target"),
        size: wgpu::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Fe authored raster readback"),
        size: 512,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let view = target.create_view(&Default::default());
        let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Fe authored raster draw"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        render.set_pipeline(&pipeline);
        render.set_bind_group(0, &group, &[]);
        render.draw(0..pass.draw_vertices.unwrap(), 0..1);
    }
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(2),
            },
        },
        wgpu::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let bytes = readback(&device, &staging);
    let pixels = [
        &bytes[0..4],
        &bytes[4..8],
        &bytes[256..260],
        &bytes[260..264],
    ];
    let hot = pixels
        .iter()
        .filter(|pixel| **pixel == [0, 0, 255, 255])
        .count();
    let cool = pixels
        .iter()
        .filter(|pixel| **pixel == [255, 0, 0, 255])
        .count();
    assert_eq!(
        (hot, cool),
        (1, 3),
        "interpolated Fe varying pixels: {pixels:?}"
    );
}

#[test]
fn fe_vertex_and_fragment_bodies_match_the_source_oracle_in_wasmtime() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/actor_raster_typed");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = db
        .workspace()
        .containing_ingot(&db, url)
        .unwrap()
        .root_mod(&db);
    let package = mir::build_wasm_runtime_package_for_entries(
        &db,
        top_mod,
        &["vertices".to_string(), "shade".to_string()],
    )
    .unwrap();
    let wasm = compile_runtime_package_wasm_with_options(
        &db,
        &package,
        WasmCompileOptions::default().with_optimization(),
    )
    .unwrap()
    .bytes;
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let vertices = instance
        .get_typed_func::<(i32, f32), (f32, f32, f32, f32, f32, f32, f32, f32)>(
            &mut store, "vertices",
        )
        .unwrap();
    let shade = instance
        .get_typed_func::<(f32, f32, f32, f32, f32), i32>(&mut store, "shade")
        .unwrap();

    let expected = [
        (-1.0, -1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0),
        (3.0, -1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0),
        (-1.0, 3.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0),
    ];
    for (index, oracle) in expected.into_iter().enumerate() {
        assert_eq!(
            vertices.call(&mut store, (index as i32, 0.4)).unwrap(),
            oracle
        );
    }
    assert_eq!(
        shade.call(&mut store, (0.0, 0.0, 1.0, 0.75, 0.4)).unwrap() as u32,
        0xffff_0000
    );
    assert_eq!(
        shade.call(&mut store, (0.0, 0.0, 1.0, 0.25, 0.4)).unwrap() as u32,
        0xff00_00ff
    );
}
