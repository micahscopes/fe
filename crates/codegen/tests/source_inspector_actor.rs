//! Independent semantic acceptance for the resident Fe SourceInspector.
//! Wasmtime executes authored Fe over lifecycle, selection, stale completion,
//! text/binary success, error, and cancellation events. A separate Rust model
//! derives state and decoded effects; artifact byte equality is not a behavior
//! oracle.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    HOST_COMPLETION_RUNTIME_JS, MATERIALIZED_TASK_RUNTIME_JS, RESIDENT_ACTOR_INITIALIZE_EXPORT,
    RESIDENT_ACTOR_PROJECT_EXPORT, RESIDENT_ACTOR_TRANSITION_EXPORT, compile_resident_actor,
    emit_materialized_task_adapter_js,
};
use fe_compiler_protocol::{InterfaceFunction, InterfaceManifest};
use fe_webidl_bindgen::{
    BROWSER_FETCH_WEBIDL, adapter_operation_metadata, build_adapter_plan,
    emit_js_selected_core_adapter, parse, select_adapter_operations,
};
use hir::hir_def::HirIngot;
use url::Url;

const STATE_LEAVES: usize = 24;

fn i32_results(values: &[wasmtime::Val]) -> Vec<i32> {
    values
        .iter()
        .map(|value| match value {
            wasmtime::Val::I32(value) => *value,
            other => panic!("resident actor returned non-i32 state leaf {other:?}"),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Model {
    url: String,
    route: String,
    scope: u32,
    destination: u32,
    anchor: u32,
    push_route: bool,
    selected: u32,
    pending: u32,
    status: u32,
    byte_len: u32,
    content: String,
    connected: bool,
    open: bool,
    prevent_default: bool,
    focus_close: bool,
    revision: u32,
    surfaces_settled: u32,
    surface_sequence_complete: bool,
    surface_sequence_failed: bool,
    control_revision: u32,
    control_request: u32,
    control_url_len: u32,
    control_binary: bool,
    control_active: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            url: String::new(),
            route: String::new(),
            scope: 0,
            destination: 0,
            anchor: 0,
            push_route: false,
            selected: 0,
            pending: 0,
            status: 0,
            byte_len: 0,
            content: String::new(),
            connected: false,
            open: false,
            prevent_default: false,
            focus_close: false,
            revision: 0,
            surfaces_settled: 0,
            surface_sequence_complete: false,
            surface_sequence_failed: false,
            control_revision: 0,
            control_request: 0,
            control_url_len: 0,
            control_binary: false,
            control_active: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Event<'a> {
    kind: u32,
    target: u32,
    request: u32,
    key: u32,
    detail: u32,
    text: &'a str,
}

fn browser_event(kind: u32, target: u32, detail: u32, text: &str) -> Event<'_> {
    Event {
        kind,
        target,
        request: 0,
        key: 0,
        detail,
        text,
    }
}

fn task_event(
    target: u32,
    request: u32,
    status: u32,
    byte_len: u32,
    content: &str,
    surface_settled: u32,
) -> Event<'_> {
    Event {
        kind: 13,
        target,
        request,
        key: status,
        detail: if surface_settled == 0 {
            byte_len
        } else {
            surface_settled
        },
        text: content,
    }
}

fn reduce(model: &mut Model, event: Event<'_>) {
    model.prevent_default = false;
    model.focus_close = false;
    model.push_route = false;

    match event.kind {
        0 => {
            model.connected = true;
            model.surfaces_settled = 0;
            model.surface_sequence_complete = false;
            model.surface_sequence_failed = false;
            model.revision += 1;
        }
        1 => {
            model.connected = false;
            model.open = false;
            model.revision += 1;
            model.control_revision = model.revision;
            model.control_request = model.pending;
            model.control_url_len = model.url.len() as u32;
            model.control_binary = model.selected == 2;
            model.control_active = false;
        }
        3 if model.connected && (1..=4).contains(&event.target) && !event.text.is_empty() => {
            model.selected = event.target - 1;
            model.url = event.text.chars().take(2048).collect();
            model.revision += 1;
            model.pending = model.revision.max(1);
            model.status = 0;
            model.byte_len = 0;
            model.content.clear();
            model.open = true;
            model.prevent_default = true;
            model.focus_close = true;
            model.control_revision = model.revision;
            model.control_request = model.pending;
            model.control_url_len = model.url.len() as u32;
            model.control_binary = model.selected == 2;
            model.control_active = true;
        }
        3 if model.connected && event.target == 11 && !event.text.is_empty() => {
            if let Some(destination) = route_destination(event.text) {
                let anchor = if destination.3 == "Selection" { 1 } else { 0 };
                let changed = model.scope != destination.0
                    || model.destination != destination.1
                    || model.anchor != anchor;
                model.scope = destination.0;
                model.destination = destination.1;
                model.anchor = anchor;
                model.route = if model.scope == 0 {
                    format!("#{}", destination.3)
                } else {
                    format!("?demo={}#{}", destination.2, destination.3)
                };
                model.push_route = changed;
                model.prevent_default = true;
            }
        }
        3 if model.connected && event.target == 5 && model.open => {
            model.open = false;
            model.prevent_default = true;
            model.revision += 1;
            model.control_revision = model.revision;
            model.control_request = model.pending;
            model.control_url_len = model.url.len() as u32;
            model.control_binary = model.selected == 2;
            model.control_active = false;
        }
        7 if model.connected && model.open && event.detail == 27 => {
            model.open = false;
            model.prevent_default = true;
            model.revision += 1;
            model.control_revision = model.revision;
            model.control_request = model.pending;
            model.control_url_len = model.url.len() as u32;
            model.control_binary = model.selected == 2;
            model.control_active = false;
        }
        13 if model.connected && event.target == 6 && event.detail > model.surfaces_settled => {
            model.surfaces_settled = event.detail;
            model.revision += 1;
        }
        13 if model.connected && event.target == 7 => {
            model.surfaces_settled = event.detail;
            model.surface_sequence_complete = true;
            model.surface_sequence_failed = false;
            model.revision += 1;
        }
        13 if model.connected && event.target == 8 => {
            model.surfaces_settled = event.detail;
            model.surface_sequence_complete = true;
            model.surface_sequence_failed = true;
            model.revision += 1;
        }
        13 if model.connected
            && (event.target == 9 || event.target == 10)
            && model.open
            && event.request == model.pending =>
        {
            model.byte_len = event.detail;
            model.content = event.text.to_owned();
            model.status = if event.target == 9 && (200..300).contains(&event.key) {
                1
            } else {
                2
            };
            model.revision += 1;
        }
        14 if model.connected && !event.text.is_empty() => {
            if let Some(destination) = route_destination(event.text) {
                model.scope = destination.0;
                model.destination = destination.1;
                model.anchor = if destination.3 == "Selection" { 1 } else { 0 };
            } else {
                model.scope = 0;
                model.destination = 0;
                model.anchor = 0;
            }
            model.route.clear();
        }
        _ => {}
    }
}

fn route_destination(input: &str) -> Option<(u32, u32, &'static str, &'static str)> {
    let url = Url::parse(input).ok()?;
    let anchor = match url.fragment().unwrap_or("") {
        "" | "Top" => "Top",
        "Selection" => "Selection",
        _ => return None,
    };
    let query = url.query().unwrap_or("");
    if query.is_empty() {
        return Some((0, 0, "", anchor));
    }
    if query.contains('&') {
        return None;
    }
    let (key, value) = query.split_once('=')?;
    if key != "demo" {
        return None;
    }
    let destination = match value {
        "Gradient" => (0, "Gradient"),
        "ClassicQuilting" => (1, "ClassicQuilting"),
        "QuiltingLod" => (2, "QuiltingLod"),
        "TodoMvc" => (3, "TodoMvc"),
        "EventStudio" => (4, "EventStudio"),
        "Cga3d" => (5, "Cga3d"),
        "Qcga" => (6, "Qcga"),
        "QcgaPencil" => (7, "QcgaPencil"),
        "Desargues" => (8, "Desargues"),
        "Plasma" => (9, "Plasma"),
        "Raymarch" => (10, "Raymarch"),
        "Mandelbrot" => (11, "Mandelbrot"),
        "PerturbationMandelbrot" => (12, "PerturbationMandelbrot"),
        "MandelbrotProof" => (13, "MandelbrotProof"),
        "Dec" => (14, "Dec"),
        "KnownColor" => (15, "KnownColor"),
        "Rollcall" => (16, "Rollcall"),
        _ => return None,
    };
    Some((1, destination.0, destination.1, anchor))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Operation {
    Text(u32, String),
    Href(u32, String),
    Push(String),
}

fn decode(bytes: &[u8]) -> Vec<Operation> {
    fn byte(bytes: &[u8], cursor: &mut usize) -> u8 {
        let value = bytes[*cursor];
        *cursor += 1;
        value
    }
    fn word(bytes: &[u8], cursor: &mut usize) -> u32 {
        let end = *cursor + 4;
        let value = u32::from_le_bytes(bytes[*cursor..end].try_into().unwrap());
        *cursor = end;
        value
    }
    fn text(bytes: &[u8], cursor: &mut usize) -> String {
        let len = word(bytes, cursor) as usize;
        let end = *cursor + len;
        let value = std::str::from_utf8(&bytes[*cursor..end])
            .unwrap()
            .to_owned();
        *cursor = end;
        value
    }
    let mut cursor = 0;
    let mut operations = Vec::new();
    while cursor < bytes.len() {
        let opcode = byte(bytes, &mut cursor);
        match opcode {
            4 | 11 => {
                let id = word(bytes, &mut cursor);
                let value = text(bytes, &mut cursor);
                operations.push(match opcode {
                    4 => Operation::Text(id, value),
                    11 => Operation::Href(id, value),
                    _ => unreachable!(),
                });
            }
            15 => operations.push(Operation::Push(text(bytes, &mut cursor))),
            _ => panic!("unexpected SourceInspector opcode {opcode}"),
        }
    }
    operations
}

fn expected_projection(model: &Model) -> (u32, u32, u32, Vec<Operation>) {
    let mut mask = if model.scope == 0 {
        ((1u32 << 17) - 1) << 13
    } else {
        1u32 << (13 + model.destination)
    };
    if model.connected && model.open {
        mask |= 1 << 0;
        mask |= 1 << (model.selected + 5);
        mask |= match model.status {
            0 => 1 << 3,
            2 => 1 << 4,
            _ if model.selected == 2 => 1 << 2,
            _ => 1 << 1,
        };
    }
    let mut operations = Vec::new();
    if model.open && !model.url.is_empty() {
        operations.push(Operation::Href(11, model.url.clone()));
    }
    if model.push_route && !model.route.is_empty() {
        operations.push(Operation::Push(model.route.clone()));
    }
    if model.status == 1 {
        if model.selected == 2 {
            operations.push(Operation::Text(10, model.byte_len.to_string()));
        } else {
            operations.push(Operation::Text(9, model.content.clone()));
        }
    } else if model.status == 2 {
        operations.push(Operation::Text(12, model.content.clone()));
    }
    (
        mask,
        u32::from(model.focus_close) * 5,
        u32::from(model.prevent_default),
        operations,
    )
}

#[test]
fn source_inspector_owns_selection_loading_stale_response_and_presentation_policy() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../demos/sketches/source_inspector")
        .canonicalize()
        .expect("SourceInspector ingot path");
    let source = std::fs::read_to_string(path.join("src/lib.fe"))
        .expect("SourceInspector authored Fe source");
    assert!(
        source.contains("Stream<SurfaceToken>")
            && source.contains("EventSource<SurfaceToken> = BrowserSurfaceEvents {}")
            && source.contains("Stream<AnimationFrame>")
            && source.contains("EventSource<AnimationFrame> = BrowserAnimationFrames::new()")
            && source.contains("Stream<DocumentVisibility>")
            && source.contains("EventSource<DocumentVisibility> = BrowserVisibilityEvents::new()")
            && source.contains(
                "ActorSink<WasmBackend, ComponentEvent<InspectorAction, InspectorTask>> = BrowserActorSink {}"
            )
            && source.contains("ComponentEventKind::TaskMessage")
            && source.contains("derive RouteSegment for GalleryDestination using RouteSegmentProvider")
            && source.contains("ComponentEventKind::Navigation")
            && source.contains("writer.push_location(value: route_location)"),
        "gallery lifecycle and routing must remain typed Fe programs"
    );
    assert!(
        !source.contains("loader.next_begin"),
        "surface discovery must not bypass EventSource through the loader authority"
    );
    let url = Url::from_directory_path(path).expect("SourceInspector URL");
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "SourceInspector diagnostics"
    );
    let ingot = db.workspace().containing_ingot(&db, url).unwrap();
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "SourceInspector diagnostics:\n{diagnostics}"
    );
    let artifact = compile_resident_actor(&db, top_mod)
        .expect("SourceInspector contract")
        .expect("SourceInspector actor");
    assert_eq!(artifact.contract.actor, "SourceInspector");
    assert_eq!(artifact.contract.event_leaf_count, 14);
    assert_eq!(artifact.contract.state_leaf_count, STATE_LEAVES);
    assert_eq!(
        artifact.contract.scoped_task_source_entries,
        ["activate_surfaces", "load_resources"]
    );
    let [surface_task, resource_task] = artifact.scoped_tasks.as_slice() else {
        panic!("SourceInspector must materialize its surface and resource tasks")
    };
    assert!(surface_task.input.is_empty());
    assert_eq!(resource_task.input.len(), STATE_LEAVES);

    if std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        let directory = tempfile::tempdir().unwrap();
        let wasm_path = directory.path().join("source-inspector.wasm");
        let task_runtime_path = directory.path().join("materialized-task.js");
        let host_runtime_path = directory.path().join("host-completion.js");
        let adapter_path = directory.path().join("task-adapter.mjs");
        let fetch_adapter_path = directory.path().join("fetch-adapter.mjs");
        let test_path = directory.path().join("surface-task.mjs");
        let adapter =
            emit_materialized_task_adapter_js(&artifact.scoped_tasks, "./materialized-task.js")
                .unwrap()
                .expect("SourceInspector scoped task adapter");
        std::fs::write(&wasm_path, &artifact.wasm).unwrap();
        std::fs::write(&task_runtime_path, MATERIALIZED_TASK_RUNTIME_JS).unwrap();
        std::fs::write(&host_runtime_path, HOST_COMPLETION_RUNTIME_JS).unwrap();
        std::fs::write(&adapter_path, adapter).unwrap();
        let world = parse(BROWSER_FETCH_WEBIDL).unwrap();
        let fetch_plan = build_adapter_plan(&world, "browser-fetch", "fe:web-fetch").unwrap();
        let fetch_metadata = adapter_operation_metadata(&fetch_plan, "generated-browser-fetch");
        let fetch_imports = [
            "response_array_buffer",
            "response_get_ok",
            "response_get_status",
            "response_get_url",
            "response_resource_drop",
            "response_text",
            "window_fetch",
        ];
        let interface = InterfaceManifest {
            imports: fetch_imports
                .into_iter()
                .map(|name| InterfaceFunction {
                    module: "fe:web-fetch".to_owned(),
                    name: name.to_owned(),
                    signature_complete: false,
                    params: Vec::new(),
                    results: Vec::new(),
                })
                .collect(),
            ..InterfaceManifest::default()
        };
        let fetch_selection = select_adapter_operations(&interface, &fetch_metadata).unwrap();
        let fetch_adapter = emit_js_selected_core_adapter(
            &world,
            &fetch_plan,
            "generated-browser-fetch",
            &fetch_selection,
        )
        .unwrap();
        std::fs::write(&fetch_adapter_path, fetch_adapter).unwrap();
        let script = format!(
            r#"
import {{ createMaterializedTaskRegistry }} from {adapter_url:?};
import {{ createHostCompletionBroker }} from {host_runtime_url:?};
import {{ createFeBrowserCoreAdapter }} from {fetch_adapter_url:?};
const tokens = [11n, 22n, 0n];
const loads = [];
const trace = [];
const visibilityCalls = [];
const visibilityStates = [1, 0];
const frameTimes = [16.0, 32.0];
const actorEvents = [];
let residentState = [];
let instance;
const broker = createHostCompletionBroker({{
  actorEvents: {{
    send(event, signal) {{
      if (signal.aborted) throw new DOMException("cancelled", "AbortError");
      actorEvents.push(event);
      trace.push(`actor:${{event[1]}}:${{event[13]}}`);
      residentState = instance.exports.fe_actor_transition_v1(...event);
    }},
  }},
  documentEvents: {{
    visibility: async (seen, previousHidden, signal) => {{
      if (signal.aborted) throw new DOMException("cancelled", "AbortError");
      visibilityCalls.push([seen, previousHidden]);
      const state = visibilityStates.shift();
      trace.push(`visibility:${{state}}`);
      return state;
    }},
  }},
  windowEvents: {{
    animationFrame: async signal => {{
      if (signal.aborted) throw new DOMException("cancelled", "AbortError");
      const timestamp = frameTimes.shift();
      trace.push(`frame:${{timestamp}}`);
      return timestamp;
    }},
    viewport: async () => ({{ width: 800, height: 600, devicePixelRatio: 2 }}),
  }},
  surface: {{
    next: async signal => {{
      if (signal.aborted) throw new DOMException("cancelled", "AbortError");
      if (visibilityStates.length !== 0) throw new Error("surface pull began while hidden");
      const token = tokens.shift();
      trace.push(`next:${{token}}`);
      return token;
    }},
    load: async (token, signal) => {{
      if (signal.aborted) throw new DOMException("cancelled", "AbortError");
      loads.push(token);
      trace.push(`load:${{token}}`);
      if (token === 22n) throw new Error("synthetic surface failure");
      return token;
    }},
  }},
}});
const fetchCalls = [];
let resolveSlow;
const slowFetch = new Promise(resolve => {{ resolveSlow = resolve; }});
const response = (url, status, text, bytes) => ({{
  url,
  status,
  ok: status >= 200 && status < 300,
  text: () => Promise.resolve(text),
  arrayBuffer: () => Promise.resolve(Uint8Array.from(bytes).buffer),
}});
const fetchAdapter = createFeBrowserCoreAdapter(broker.completions, {{
  fetch(input) {{
    fetchCalls.push(input);
    if (input.endsWith("/slow.fe")) return slowFetch;
    if (input.endsWith("/fresh.fe")) return Promise.resolve(response(input, 200, "fresh Fe", []));
    if (input.endsWith("/module.wasm")) return Promise.resolve(response(input, 200, "", [1, 2, 3, 4, 5]));
    if (input.endsWith("/missing.wgsl")) return Promise.resolve(response(input, 404, "not found", []));
    if (input.endsWith("/broken.fe")) return Promise.reject(new Error("synthetic fetch failure"));
    return Promise.reject(new Error(`unexpected fetch ${{input}}`));
  }},
}});
const bytes = await Bun.file({wasm_path:?}).arrayBuffer();
({{ instance }} = await WebAssembly.instantiate(bytes, {{
  ...broker.imports,
  ...fetchAdapter.imports,
}}));
fetchAdapter.attach(instance);
residentState = instance.exports.fe_actor_initialize_v1();
residentState = instance.exports.fe_actor_transition_v1(
  0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
);
const tasks = createMaterializedTaskRegistry(instance.exports);
if (Object.keys(tasks).join() !== "activate_surfaces,load_resources") throw new Error("task registry drift");
const output = await broker.run(tasks.activate_surfaces, []);
if (output.length !== 1 || output[0] !== 2) throw new Error(`Fe surface policy returned ${{output}}`);
if (visibilityCalls.length !== 2 || visibilityCalls[0][0] !== false || visibilityCalls[0][1] !== false
    || visibilityCalls[1][0] !== true || visibilityCalls[1][1] !== true) {{
  throw new Error("Fe did not retain typed visibility state while gating surface work");
}}
if (trace.join() !== "visibility:1,visibility:0,next:11,load:11,actor:6:1,frame:16,next:22,load:22,actor:6:2,frame:32,next:0,actor:7:2") {{
  throw new Error(`Fe did not pace surface activation by animation frame: ${{trace}}`);
}}
if (actorEvents.length !== 3
    || actorEvents[0].join() !== "13,6,0,0,0,0,0,0,0,0,0,0,0,1"
    || actorEvents[1].join() !== "13,6,0,0,0,0,0,0,0,0,0,0,0,2"
    || actorEvents[2].join() !== "13,7,0,0,0,0,0,0,0,0,0,0,0,2") {{
  throw new Error(`scoped task did not deliver typed resident events: ${{JSON.stringify(actorEvents)}}`);
}}
if (residentState[21] !== 2 || residentState[22] !== 1 || residentState[23] !== 0) {{
  throw new Error(`resident Fe state did not retain surface completion: ${{residentState}}`);
}}
if (loads.length !== 2 || loads[0] !== 11n || loads[1] !== 22n) throw new Error("host reordered surface tokens");
if (tokens.length !== 0) throw new Error("Fe did not pull the end sentinel");
if (broker.activeCount() !== 0 || broker.cancelAll() !== 0) throw new Error("surface task leaked pending work");

actorEvents.length = 0;
const waitFor = async (predicate, message) => {{
  for (let attempt = 0; attempt < 200; attempt += 1) {{
    if (predicate()) return;
    await new Promise(resolve => setTimeout(resolve, 0));
  }}
  throw new Error(message);
}};
const scratch = instance.exports.cabi_realloc(0, 0, 1, 4096);
const encode = new TextEncoder();
const decode = new TextDecoder();
const activate = (target, url) => {{
  const encoded = encode.encode(url);
  new Uint8Array(instance.exports.memory.buffer, scratch, encoded.length).set(encoded);
  residentState = instance.exports.fe_actor_transition_v1(
    3, target, 0, 0, 1, 0, 0, scratch, encoded.length,
    0, 0, 0, 0, 0,
  );
}};
const close = () => {{
  residentState = instance.exports.fe_actor_transition_v1(
    3, 5, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0,
  );
}};
const content = () => decode.decode(new Uint8Array(
  instance.exports.memory.buffer,
  residentState[14],
  residentState[15],
));

const resourceLifetime = new AbortController();
const resourceRun = broker.run(
  tasks.load_resources,
  tasks.load_resources.liftInput(residentState),
  {{ signal: resourceLifetime.signal }},
);
await Promise.resolve();
activate(1, "https://example.test/slow.fe");
await waitFor(() => fetchCalls.length === 1, "Fe did not begin the first generated fetch");
activate(1, "https://example.test/fresh.fe");
await waitFor(
  () => residentState[12] === 1 && content() === "fresh Fe",
  "Fe did not select and commit the newer text request",
);
if (fetchCalls.join() !== "https://example.test/slow.fe,https://example.test/fresh.fe") {{
  throw new Error(`Fe request order drifted: ${{fetchCalls}}`);
}}
if (actorEvents.length !== 1 || actorEvents[0][1] !== 9 || actorEvents[0][9] !== 6
    || actorEvents[0][10] !== 200 || actorEvents[0][12] !== 8) {{
  throw new Error(`fresh text completion did not use the typed Fe payload: ${{JSON.stringify(actorEvents)}}`);
}}
resolveSlow(response("https://example.test/slow.fe", 200, "stale", []));
await new Promise(resolve => setTimeout(resolve, 0));
await new Promise(resolve => setTimeout(resolve, 0));
if (actorEvents.length !== 1 || content() !== "fresh Fe") {{
  throw new Error("a stale generated fetch completion reached resident Fe state");
}}

activate(3, "https://example.test/module.wasm");
await waitFor(
  () => residentState[12] === 1 && residentState[13] === 5,
  "Fe did not commit the generated binary response",
);
activate(2, "https://example.test/missing.wgsl");
await waitFor(
  () => residentState[12] === 2 && content() === "not found",
  "Fe did not classify a non-success HTTP response",
);
activate(1, "https://example.test/broken.fe");
await waitFor(
  () => residentState[12] === 2 && content() === "Resource request failed",
  "Fe did not own generated fetch failure presentation",
);
if (actorEvents.map(event => event[1]).join() !== "9,9,9,10"
    || actorEvents.map(event => event[9]).join() !== "6,8,10,12") {{
  throw new Error(`resource task correlation drifted: ${{JSON.stringify(actorEvents)}}`);
}}
if (fetchAdapter.runtime.inventory().resources !== 0) {{
  throw new Error("SourceInspector leaked a generated Response authority");
}}
close();
const control = new DataView(instance.exports.memory.buffer, residentState[1], 20);
if (control.getUint32(16, true) !== 0) throw new Error("close did not publish inactive Fe load state");
resourceLifetime.abort();
let cancelled = false;
try {{ await resourceRun; }}
catch (error) {{ cancelled = error?.name === "AbortError"; }}
if (!cancelled) {{
  throw new Error("resource scope did not retain cancellation as its terminal verdict");
}}
if (broker.activeCount() !== 0 || broker.cancelAll() !== 0) {{
  throw new Error("resource task leaked pending completion work");
}}
"#,
            adapter_url = format!("file://{}", adapter_path.display()),
            host_runtime_url = format!("file://{}", host_runtime_path.display()),
            fetch_adapter_url = format!("file://{}", fetch_adapter_path.display()),
            wasm_path = wasm_path.display().to_string(),
        );
        std::fs::write(&test_path, script).unwrap();
        let output = std::process::Command::new("bun")
            .arg("run")
            .arg(&test_path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Fe-authored surface task failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &artifact.wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    linker
        .func_wrap("fe:web-surface", "next_begin", || -> i32 { 0 })
        .unwrap();
    linker
        .func_wrap("fe:web-surface", "load_begin", |_surface: i64| -> i32 { 0 })
        .unwrap();
    linker
        .func_wrap(
            "fe:web-document",
            "visibility_begin",
            |_seen: i32, _previous: i32| -> i32 { 0 },
        )
        .unwrap();
    linker
        .func_wrap("fe:web-window", "animation_frame_begin", || -> i32 { 0 })
        .unwrap();
    linker
        .func_wrap("fe:host", "sleep_begin", |_delay: i64| -> i32 { 0 })
        .unwrap();
    linker
        .func_wrap("fe:host", "race_begin", |_left: i32, _right: i32| -> i32 {
            0
        })
        .unwrap();
    linker
        .func_wrap(
            "fe:host",
            "select_begin",
            |_left: i32, _right: i32| -> i32 { 0 },
        )
        .unwrap();
    linker
        .func_wrap("fe:host", "cancel_pending", |_pending: i32| -> i32 { 0 })
        .unwrap();
    linker
        .func_wrap("fe:actor-notification", "notify", || {})
        .unwrap();
    linker
        .func_wrap("fe:actor-notification", "wait_begin", || -> i32 { 0 })
        .unwrap();
    linker
        .func_wrap(
            "fe:web-fetch",
            "window_fetch",
            |_ptr: i32, _len: i32| -> i32 { 0 },
        )
        .unwrap();
    linker
        .func_wrap(
            "fe:web-fetch",
            "response_get_status",
            |_response: i32| -> i32 { 200 },
        )
        .unwrap();
    linker
        .func_wrap("fe:web-fetch", "response_text", |_response: i32| -> i32 {
            0
        })
        .unwrap();
    linker
        .func_wrap(
            "fe:web-fetch",
            "response_array_buffer",
            |_response: i32| -> i32 { 0 },
        )
        .unwrap();
    linker
        .func_wrap(
            "fe:web-fetch",
            "response_resource_drop",
            |_response: i32| {},
        )
        .unwrap();
    linker
        .func_wrap(
            "fe:actor",
            "send_begin",
            |_kind: i32,
             _target: i32,
             _request: i32,
             _key: i32,
             _detail: i32,
             _value: f32,
             _timestamp: f32,
             _text_ptr: i32,
             _text_len: i32,
             _task_request: i32,
             _task_status: i32,
             _task_byte_len: i32,
             _task_content_len: i32,
             _task_surface_settled: i32|
             -> i32 { 0 },
        )
        .unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    let initialize = instance
        .get_func(&mut store, RESIDENT_ACTOR_INITIALIZE_EXPORT)
        .unwrap();
    let transition = instance
        .get_func(&mut store, RESIDENT_ACTOR_TRANSITION_EXPORT)
        .unwrap();
    let project = instance
        .get_typed_func::<(), (i32, i32, i32, i32, i32)>(&mut store, RESIDENT_ACTOR_PROJECT_EXPORT)
        .unwrap();
    let realloc = instance
        .get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "cabi_realloc")
        .unwrap();
    let mut initial_results = vec![wasmtime::Val::I32(0); STATE_LEAVES];
    initialize
        .call(&mut store, &[], &mut initial_results)
        .unwrap();
    let initial = i32_results(&initial_results);
    let pointers = (initial[0], initial[1], initial[2], initial[4], initial[14]);
    let scratch = realloc.call(&mut store, (0, 0, 1, 4096)).unwrap();
    let mut model = Model::default();

    let mut tape = vec![
        Event {
            kind: 3,
            target: 1,
            request: 0,
            key: 0,
            detail: 1,
            text: "https://example.test/a.fe",
        },
        Event {
            kind: 0,
            target: 0,
            request: 0,
            key: 0,
            detail: 0,
            text: "",
        },
        Event {
            kind: 14,
            target: 0,
            request: 0,
            key: 0,
            detail: 0,
            text: "https://example.test/gallery.html?demo=Mandelbrot#Selection",
        },
        Event {
            kind: 3,
            target: 11,
            request: 0,
            key: 0,
            detail: 1,
            text: "https://example.test/gallery.html?demo=QcgaPencil#Selection",
        },
        Event {
            kind: 14,
            target: 0,
            request: 0,
            key: 0,
            detail: 0,
            text: "https://example.test/gallery.html?demo=%41ll#Selection",
        },
        Event {
            kind: 13,
            target: 6,
            request: 0,
            key: 0,
            detail: 1,
            text: "",
        },
        Event {
            kind: 13,
            target: 7,
            request: 0,
            key: 0,
            detail: 2,
            text: "",
        },
        // Legacy surface lifecycle events are valid transport facts, but the
        // canonical actor no longer stores or interprets them. Its scoped Fe
        // task owns sequencing through EventSource<SurfaceToken>, Stream, and
        // the shared typed runtime-control Pending/TaskOutcome rail.
        Event {
            kind: 9,
            target: 0,
            request: 2,
            key: 0,
            detail: 0,
            text: "",
        },
        Event {
            kind: 9,
            target: 0,
            request: 1,
            key: 0,
            detail: 0,
            text: "",
        },
        Event {
            kind: 10,
            target: 0,
            request: 1,
            key: 0,
            detail: 0,
            text: "",
        },
        Event {
            kind: 11,
            target: 0,
            request: 2,
            key: 0,
            detail: 0,
            text: "",
        },
        Event {
            kind: 9,
            target: 0,
            request: 3,
            key: 0,
            detail: 0,
            text: "",
        },
        Event {
            kind: 12,
            target: 0,
            request: 3,
            key: 0,
            detail: 0,
            text: "",
        },
        Event {
            kind: 3,
            target: 1,
            request: 0,
            key: 0,
            detail: 1,
            text: "https://example.test/a.fe",
        },
        Event {
            kind: 3,
            target: 0,
            request: 0,
            key: 7,
            detail: 1,
            text: "",
        },
        Event {
            kind: 8,
            target: 0,
            request: 99,
            key: 200,
            detail: 5,
            text: "stale",
        },
        Event {
            kind: 8,
            target: 0,
            request: 2,
            key: 200,
            detail: 8,
            text: "actor A",
        },
        Event {
            kind: 3,
            target: 3,
            request: 0,
            key: 0,
            detail: 1,
            text: "https://example.test/a.wasm",
        },
        Event {
            kind: 8,
            target: 0,
            request: 4,
            key: 200,
            detail: 4294967295,
            text: "",
        },
        Event {
            kind: 7,
            target: 0,
            request: 0,
            key: 0,
            detail: 27,
            text: "",
        },
        Event {
            kind: 3,
            target: 2,
            request: 0,
            key: 0,
            detail: 1,
            text: "https://example.test/missing.wgsl",
        },
        Event {
            kind: 8,
            target: 0,
            request: 7,
            key: 404,
            detail: 9,
            text: "not found",
        },
        Event {
            kind: 1,
            target: 0,
            request: 0,
            key: 0,
            detail: 0,
            text: "",
        },
        // Reconnection restarts the actor-scoped loader task; lifecycle events
        // remain irrelevant to the resident reducer.
        Event {
            kind: 0,
            target: 0,
            request: 0,
            key: 0,
            detail: 0,
            text: "",
        },
        Event {
            kind: 9,
            target: 0,
            request: 4,
            key: 0,
            detail: 0,
            text: "",
        },
        Event {
            kind: 10,
            target: 0,
            request: 4,
            key: 0,
            detail: 0,
            text: "",
        },
        Event {
            kind: 1,
            target: 0,
            request: 0,
            key: 0,
            detail: 0,
            text: "",
        },
        Event {
            kind: 8,
            target: 0,
            request: 7,
            key: 200,
            detail: 2,
            text: "ok",
        },
        browser_event(0, 0, 0, ""),
        browser_event(3, 1, 1, "https://example.test/b.fe"),
        task_event(9, 99, 200, 5, "stale", 0),
        task_event(9, 12, 200, 8, "actor B", 0),
        browser_event(3, 2, 1, "https://example.test/missing-b.wgsl"),
        task_event(10, 14, 404, 9, "not found", 0),
        browser_event(3, 5, 0, ""),
    ];
    let route_urls = [
        "Gradient",
        "ClassicQuilting",
        "QuiltingLod",
        "TodoMvc",
        "EventStudio",
        "Cga3d",
        "Qcga",
        "QcgaPencil",
        "Desargues",
        "Plasma",
        "Raymarch",
        "Mandelbrot",
        "PerturbationMandelbrot",
        "MandelbrotProof",
        "Dec",
        "KnownColor",
        "Rollcall",
    ]
    .map(|name| format!("https://example.test/gallery.html?demo={name}#Selection"));
    for (index, url) in route_urls.iter().enumerate() {
        tape.push(browser_event(3, 11, 1, url));
        tape.push(browser_event(14, 0, 0, url));
        if index == 0 {
            // The Fe route state consumes the click but does not ask the fixed
            // History adapter to create a duplicate entry.
            tape.push(browser_event(3, 11, 1, url));
        }
    }
    let all_url = "https://example.test/gallery.html#Top";
    tape.push(browser_event(3, 11, 1, all_url));
    tape.push(browser_event(14, 0, 0, all_url));
    tape.push(browser_event(3, 11, 1, all_url));
    for (index, event) in tape.into_iter().enumerate() {
        let task_message = event.kind == 13;
        let resource_message = task_message && (event.target == 9 || event.target == 10);
        if resource_message {
            memory
                .write(&mut store, initial[14] as usize, event.text.as_bytes())
                .unwrap();
        } else {
            memory
                .write(&mut store, scratch as usize, event.text.as_bytes())
                .unwrap();
        }
        reduce(&mut model, event);
        let params = [
            wasmtime::Val::I32(event.kind as i32),
            wasmtime::Val::I32(event.target as i32),
            wasmtime::Val::I32(if task_message {
                0
            } else {
                event.request as i32
            }),
            wasmtime::Val::I32(if task_message { 0 } else { event.key as i32 }),
            wasmtime::Val::I32(if task_message { 0 } else { event.detail as i32 }),
            wasmtime::Val::F32(0.0f32.to_bits()),
            wasmtime::Val::F32((index as f32).to_bits()),
            wasmtime::Val::I32(if task_message || event.text.is_empty() {
                0
            } else {
                scratch
            }),
            wasmtime::Val::I32(if task_message {
                0
            } else {
                event.text.len() as i32
            }),
            wasmtime::Val::I32(if resource_message {
                event.request as i32
            } else {
                0
            }),
            wasmtime::Val::I32(if resource_message {
                event.key as i32
            } else {
                0
            }),
            wasmtime::Val::I32(if resource_message {
                event.detail as i32
            } else {
                0
            }),
            wasmtime::Val::I32(if resource_message {
                event.text.len() as i32
            } else {
                0
            }),
            wasmtime::Val::I32(if task_message && !resource_message {
                event.detail as i32
            } else {
                0
            }),
        ];
        let mut actual_results = vec![wasmtime::Val::I32(0); STATE_LEAVES];
        transition
            .call(&mut store, &params, &mut actual_results)
            .unwrap_or_else(|error| panic!("SourceInspector event {index}: {error}"));
        let actual = i32_results(&actual_results);
        assert_eq!(
            (actual[0], actual[1], actual[2], actual[4], actual[14]),
            pointers,
            "persistent pointers at {index}"
        );
        assert_eq!(
            actual[3] as u32,
            model.url.len() as u32,
            "url length at {index}"
        );
        assert_eq!(
            actual[5] as usize,
            model.route.len(),
            "route length at {index}"
        );
        assert_eq!(actual[6] as u32, model.scope, "route scope at {index}");
        assert_eq!(actual[7] as u32, model.destination, "route at {index}");
        assert_eq!(actual[8] as u32, model.anchor, "route anchor at {index}");
        assert_eq!(actual[9] != 0, model.push_route, "route push at {index}");
        assert_eq!(actual[10] as u32, model.selected, "selection at {index}");
        assert_eq!(
            actual[11] as u32, model.pending,
            "pending request at {index}"
        );
        assert_eq!(actual[12] as u32, model.status, "status at {index}");
        assert_eq!(actual[13] as u32, model.byte_len, "byte length at {index}");
        assert_eq!(
            actual[15] as u32,
            model.content.len() as u32,
            "content length at {index}"
        );
        assert_eq!(actual[16] != 0, model.connected, "connected at {index}");
        assert_eq!(actual[17] != 0, model.open, "open at {index}");
        assert_eq!(
            actual[18] != 0,
            model.prevent_default,
            "prevent default at {index}"
        );
        assert_eq!(actual[19] != 0, model.focus_close, "focus at {index}");
        assert_eq!(actual[20] as u32, model.revision, "revision at {index}");
        let mut control = [0u8; 20];
        memory
            .read(&store, initial[1] as usize, &mut control)
            .unwrap();
        let control_words = control
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            control_words,
            [
                model.control_revision,
                model.control_request,
                model.control_url_len,
                u32::from(model.control_binary),
                u32::from(model.control_active),
            ],
            "Fe-owned load command at {index}"
        );
        assert_eq!(
            actual[21] as u32, model.surfaces_settled,
            "settled surfaces at {index}"
        );
        assert_eq!(
            actual[22] != 0,
            model.surface_sequence_complete,
            "surface completion at {index}"
        );
        assert_eq!(
            actual[23] != 0,
            model.surface_sequence_failed,
            "surface failure at {index}"
        );
        let patch = project.call(&mut store, ()).unwrap();
        let expected = expected_projection(&model);
        assert_eq!(
            (patch.0 as u32, patch.1 as u32, patch.2 as u32),
            (expected.0, expected.1, expected.2)
        );
        let mut commands = vec![0; patch.4 as usize];
        memory
            .read(&store, patch.3 as usize, &mut commands)
            .unwrap();
        assert_eq!(decode(&commands), expected.3, "effects at event {index}");
    }

    assert!(
        {
            let params = [
                wasmtime::Val::I32(3),
                wasmtime::Val::I32(99),
                wasmtime::Val::I32(0),
                wasmtime::Val::I32(0),
                wasmtime::Val::I32(0),
                wasmtime::Val::F32(0),
                wasmtime::Val::F32(0),
                wasmtime::Val::I32(0),
                wasmtime::Val::I32(0),
                wasmtime::Val::I32(0),
                wasmtime::Val::I32(0),
                wasmtime::Val::I32(0),
                wasmtime::Val::I32(0),
                wasmtime::Val::I32(0),
            ];
            let mut results = vec![wasmtime::Val::I32(0); STATE_LEAVES];
            transition.call(&mut store, &params, &mut results).is_err()
        },
        "invalid FCO-derived InspectorAction must trap before reaching Fe"
    );
}
