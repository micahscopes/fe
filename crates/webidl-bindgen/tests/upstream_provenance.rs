use fe_compiler_protocol::{InterfaceFunction, InterfaceManifest};
use fe_webidl_bindgen::{
    WEBGPU_QUEUE_IDLE_PROVENANCE, WEBGPU_QUEUE_IDLE_WEBIDL, WEBGPU_WEBIDL_MODULE,
    adapter_operation_metadata, build_adapter_plan, emit_fe_flat_host_imports,
    emit_js_canonical_adapter, parse, select_adapter_operations,
};
use sha2::{Digest, Sha256};

const SELECTED: &str = include_str!("fixtures/upstream/webref-3.81.3-dom-selection.webidl");
const PROVENANCE: &str = include_str!("fixtures/upstream/provenance.json");
const UNSUPPORTED: &str = include_str!("fixtures/upstream/expected-unsupported.json");

fn import(name: &str) -> InterfaceFunction {
    InterfaceFunction {
        module: "fe:web".to_owned(),
        name: name.to_owned(),
        signature_complete: false,
        params: Vec::new(),
        results: Vec::new(),
    }
}

#[test]
fn pinned_webgpu_queue_idle_selection_has_exact_provenance_and_generated_names() {
    let provenance: serde_json::Value = serde_json::from_str(WEBGPU_QUEUE_IDLE_PROVENANCE).unwrap();
    assert_eq!(
        provenance["revision"],
        "f3b81966c45f34f62df20e7f8d6f66d5b5ba9279"
    );
    assert_eq!(provenance["license"], "MIT");
    assert_eq!(
        provenance["sources"][0]["git_blob"],
        "3360beca8efc5c6a3dc46dcd7822f1e4a2bb46f6"
    );
    assert_eq!(
        provenance["selection"]["sha256"],
        format!("{:x}", Sha256::digest(WEBGPU_QUEUE_IDLE_WEBIDL.as_bytes()))
    );

    let world = parse(WEBGPU_QUEUE_IDLE_WEBIDL).unwrap();
    assert_eq!(
        world
            .interfaces
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["GPUQueue"]
    );
    let plan = build_adapter_plan(&world, "webgpu-queue-idle", WEBGPU_WEBIDL_MODULE).unwrap();
    let functions = plan.resources[0]
        .functions
        .iter()
        .map(|function| function.import_name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        functions,
        [
            "gpu_queue_on_submitted_work_done",
            "gpu_queue_resource_drop",
        ]
    );
    let raw = emit_fe_flat_host_imports(&world, WEBGPU_WEBIDL_MODULE).unwrap();
    assert!(
        raw.contains(
            "gpu_queue_on_submitted_work_done(self_: GPUQueue) -> Pending<WasmBackend, ()>"
        )
    );
    assert!(!raw.contains("g_p_u_queue"), "{raw}");
}

#[test]
fn generated_webgpu_queue_idle_adapter_scopes_the_borrow_through_settlement() {
    if !std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }

    let world = parse(WEBGPU_QUEUE_IDLE_WEBIDL).unwrap();
    let plan = build_adapter_plan(&world, "webgpu-queue-idle", WEBGPU_WEBIDL_MODULE).unwrap();
    let adapter = emit_js_canonical_adapter(&world, &plan).unwrap();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "fe-webidl-webgpu-queue-idle-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let adapter_path = directory.join("adapter.mjs");
    let test_path = directory.join("test.mjs");
    std::fs::write(&adapter_path, adapter).unwrap();
    let runtime_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/browser-runtime/host-runtime.js")
        .canonicalize()
        .unwrap();
    let script = format!(
        r#"
import {{ createFeHostAdapter }} from {adapter_url:?};
import {{ createFeHostRuntime }} from {runtime_url:?};

const runtime = createFeHostRuntime();
const adapter = createFeHostAdapter({{}}, runtime);
const queueOps = adapter.imports[{module:?}];
let calls = 0;
let resolveIdle;
let borrowedHandle;
const queue = {{
  onSubmittedWorkDone() {{
    calls += 1;
    return new Promise(resolve => {{ resolveIdle = resolve; }});
  }},
}};
const pending = runtime.resources.withBorrowed(queue, handle => {{
  borrowedHandle = handle;
  return queueOps.gpu_queue_on_submitted_work_done(handle);
}});
if (calls !== 1 || runtime.inventory().resources !== 1)
  throw new Error("queue borrow was not retained through the pending promise");
resolveIdle();
await pending;
if (runtime.inventory().resources !== 0)
  throw new Error("resolved queue borrow was not retired");
let stale = false;
try {{ runtime.resources.borrow(borrowedHandle); }} catch (error) {{ stale = error.code === "stale_handle"; }}
if (!stale) throw new Error("retired queue handle remained usable");

const rejection = new Error("queue rejected");
let observed;
try {{
  await runtime.resources.withBorrowed(
    {{ onSubmittedWorkDone() {{ return Promise.reject(rejection); }} }},
    handle => queueOps.gpu_queue_on_submitted_work_done(handle),
  );
}} catch (error) {{ observed = error; }}
if (observed !== rejection || runtime.inventory().resources !== 0)
  throw new Error("rejected queue promise did not preserve error and retire its borrow");
"#,
        adapter_url = format!("file://{}", adapter_path.display()),
        runtime_url = format!("file://{}", runtime_path.display()),
        module = WEBGPU_WEBIDL_MODULE,
    );
    std::fs::write(&test_path, script).unwrap();
    let output = std::process::Command::new("node")
        .arg(&test_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generated WebGPU queue-idle adapter failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn property_policy_metadata_validates_and_executes_in_semantic_adapter() {
    let source = r#"
interface ForwardTarget { attribute DOMString value; };
interface PolicyHost {
  [SameObject] readonly attribute ForwardTarget stable;
  [LegacyUnforgeable] readonly attribute DOMString identifier;
  [PutForwards=value] readonly attribute ForwardTarget forwarded;
};
"#;
    let world = parse(source).unwrap();
    let host = &world.interfaces["PolicyHost"];
    let attributes = host
        .members
        .iter()
        .filter_map(|member| match member {
            fe_webidl_bindgen::Member::Attribute(attribute) => Some(attribute),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(attributes[0].attributes.same_object);
    assert!(attributes[1].attributes.legacy_unforgeable);
    assert_eq!(
        attributes[2].attributes.put_forwards.as_deref(),
        Some("value")
    );
    assert!(
        parse("interface Bad { [SameObject] attribute DOMString value; };")
            .unwrap_err()
            .detail
            .contains("readonly")
    );
    assert!(
        parse(
            "interface Target { readonly attribute DOMString value; }; \
         interface Bad { [PutForwards=value] readonly attribute Target target; };"
        )
        .unwrap_err()
        .detail
        .contains("writable")
    );

    if !std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }
    let plan = build_adapter_plan(&world, "property-policy", "fe:web").unwrap();
    let adapter = emit_js_canonical_adapter(&world, &plan).unwrap();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "fe-webidl-property-policy-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let adapter_path = directory.join("adapter.mjs");
    let test_path = directory.join("test.mjs");
    std::fs::write(&adapter_path, adapter).unwrap();
    let runtime_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../demos/shared/host-runtime.js")
        .canonicalize()
        .unwrap();
    let script = format!(
        r#"
import {{ createFeHostAdapter }} from {adapter_url:?};
import {{ createFeHostRuntime }} from {runtime_url:?};
const stable = {{ value: "stable" }};
const forwarded = {{ value: "before" }};
const host = {{ stable, forwarded }};
Object.defineProperty(host, "identifier", {{
  value: "fixed", enumerable: true, configurable: false,
}});
const runtime = createFeHostRuntime();
const handle = runtime.resources.insert(host);
const imports = createFeHostAdapter({{ interfaces: {{}} }}, runtime).imports["fe:web"];
const first = imports.policy_host_get_stable(handle);
const second = imports.policy_host_get_stable(handle);
if (first !== second) throw new Error("SameObject did not preserve handle identity");
if (imports.policy_host_get_identifier(handle) !== "fixed")
  throw new Error("LegacyUnforgeable getter failed");
imports.policy_host_set_forwarded(handle, "after");
if (forwarded.value !== "after") throw new Error("PutForwards setter did not forward");
"#,
        adapter_url = format!("file://{}", adapter_path.display()),
        runtime_url = format!("file://{}", runtime_path.display()),
    );
    std::fs::write(&test_path, script).unwrap();
    let output = std::process::Command::new("bun")
        .arg("run")
        .arg(&test_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "property-policy adapter failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rich_event_members_preserve_ownership_union_defaults_and_backend_gating() {
    let world = parse(SELECTED).unwrap();
    let plan = build_adapter_plan(&world, "pinned-dom", "fe:web").unwrap();
    let event = plan
        .resources
        .iter()
        .find(|resource| resource.name == "Event")
        .unwrap();
    let composed_path = event
        .functions
        .iter()
        .find(|function| function.member_name == "composedPath")
        .unwrap();
    assert_eq!(
        composed_path.result,
        fe_webidl_bindgen::TypeRef::Sequence(Box::new(fe_webidl_bindgen::TypeRef::Named(
            "EventTarget".to_owned()
        )))
    );
    let trusted = event
        .functions
        .iter()
        .find(|function| function.member_name == "isTrusted")
        .unwrap();
    assert!(trusted.attributes.legacy_unforgeable);
    let add = plan
        .resources
        .iter()
        .find(|resource| resource.name == "EventTarget")
        .unwrap()
        .functions
        .iter()
        .find(|function| function.member_name == "addEventListener")
        .unwrap();
    assert_eq!(
        add.params[2].default_,
        Some(fe_webidl_bindgen::DefaultValueDef::EmptyDictionary)
    );
    let options = &add.params[2].type_;
    let fe_webidl_bindgen::TypeRef::Union(cases) = options else {
        panic!("listener options should remain a union");
    };
    assert_eq!(
        cases,
        &[
            fe_webidl_bindgen::TypeRef::Named("AddEventListenerOptions".to_owned()),
            fe_webidl_bindgen::TypeRef::Bool,
        ]
    );
    let create_element = plan
        .resources
        .iter()
        .find(|resource| resource.name == "Document")
        .unwrap()
        .functions
        .iter()
        .find(|function| function.member_name == "createElement")
        .unwrap();
    assert_eq!(
        create_element.params[1].default_,
        Some(fe_webidl_bindgen::DefaultValueDef::EmptyDictionary)
    );
    assert_eq!(
        create_element.params[1].type_,
        fe_webidl_bindgen::TypeRef::Union(vec![
            fe_webidl_bindgen::TypeRef::String(fe_webidl_bindgen::StringKind::Dom),
            fe_webidl_bindgen::TypeRef::Named("ElementCreationOptions".to_owned()),
        ])
    );

    let support_error = fe_host_abi::SupportProfile::current_fe_wasm_imports()
        .check(&plan.host_abi)
        .unwrap_err();
    assert!(
        support_error
            .missing
            .contains(&fe_host_abi::AbiFeature::List)
    );
    assert!(
        support_error
            .missing
            .contains(&fe_host_abi::AbiFeature::Variant)
    );
    assert!(
        support_error
            .missing
            .contains(&fe_host_abi::AbiFeature::Callback)
    );

    let raw = emit_fe_flat_host_imports(
        &parse(
            "interface EventTarget {}; \
             interface Event { sequence<EventTarget> composedPath(); };",
        )
        .unwrap(),
        "fe:web",
    )
    .unwrap();
    assert!(
        raw.contains("#[host_result(codec = \"fe:host-wasm-codec/v1\")]"),
        "{raw}"
    );
    assert!(
        raw.contains(
            "pub unsafe fn event_composed_path(self_: Event) -> BrowserList<EventTarget, 0>"
        ),
        "{raw}"
    );

    let adapter = emit_js_canonical_adapter(&world, &plan).unwrap();
    assert!(adapter.contains("Array.from("), "{adapter}");
    assert!(
        adapter.contains("runtime.resources.insert(value)"),
        "{adapter}"
    );
    assert!(
        adapter.contains("case: \"named-addeventlisteneroptions\", value: {}"),
        "{adapter}"
    );
    assert!(adapter.contains("invalid Web IDL union case"), "{adapter}");
    if !std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "fe-webidl-rich-event-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let adapter_path = directory.join("adapter.mjs");
    let test_path = directory.join("test.mjs");
    std::fs::write(&adapter_path, adapter).unwrap();
    let runtime_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../demos/shared/host-runtime.js")
        .canonicalize()
        .unwrap();
    let script = format!(
        r#"
import {{ createFeHostAdapter }} from {adapter_url:?};
import {{ createFeHostRuntime }} from {runtime_url:?};
const firstTarget = {{ name: "first" }};
const secondTarget = {{ name: "second" }};
const callbackEvent = {{ type: "probe" }};
const observedOptions = [];
const hostEvent = {{ composedPath() {{ return [firstTarget, secondTarget]; }} }};
Object.defineProperty(hostEvent, "isTrusted", {{
  value: true, enumerable: true, configurable: false,
}});
const created = [];
const hostDocument = {{
  createElement(localName, options) {{
    const value = {{ localName, options }};
    created.push(value);
    return value;
  }},
}};
const hostTarget = {{
  addEventListener(type, callback, options) {{
    observedOptions.push(options);
    if (callback !== null) callback(callbackEvent);
  }},
}};
const runtime = createFeHostRuntime();
const eventHandle = runtime.resources.insert(hostEvent);
const targetHandle = runtime.resources.insert(hostTarget);
const documentHandle = runtime.resources.insert(hostDocument);
const adapter = createFeHostAdapter({{ interfaces: {{}} }}, runtime);
const imports = adapter.imports["fe:web"];
const path = imports.event_composed_path(eventHandle);
if (path.length !== 2 || runtime.resources.borrow(path[0]) !== firstTarget ||
    runtime.resources.borrow(path[1]) !== secondTarget)
  throw new Error("composedPath resource conversion failed");
if (runtime.resources.liveCount !== 5)
  throw new Error("owned path handles were not retained");
runtime.resources.drop(path[0]);
runtime.resources.drop(path[1]);
if (imports.event_get_is_trusted(eventHandle) !== true)
  throw new Error("real LegacyUnforgeable getter failed");
const forgedEvent = runtime.resources.insert({{ isTrusted: true }});
let rejectedDescriptor = false;
try {{ imports.event_get_is_trusted(forgedEvent); }} catch {{ rejectedDescriptor = true; }}
if (!rejectedDescriptor) throw new Error("configurable isTrusted descriptor was accepted");
runtime.resources.drop(forgedEvent);

const defaultElement = imports.document_create_element(documentHandle, "x-default", undefined);
const stringElement = imports.document_create_element(documentHandle, "x-string", {{
  case: "string-dom", value: "custom-built-in",
}});
const dictionaryElement = imports.document_create_element(documentHandle, "x-dictionary", {{
  case: "named-elementcreationoptions", value: {{ is: "other-built-in" }},
}});
if (runtime.resources.borrow(defaultElement) !== created[0] ||
    runtime.resources.borrow(stringElement) !== created[1] ||
    runtime.resources.borrow(dictionaryElement) !== created[2])
  throw new Error("createElement result ownership failed");
if (JSON.stringify(created.map(value => value.options)) !==
    JSON.stringify([{{}}, "custom-built-in", {{ is: "other-built-in" }}]))
  throw new Error(`createElement union/default conversion failed: ${{JSON.stringify(created)}}`);
runtime.resources.drop(defaultElement);
runtime.resources.drop(stringElement);
runtime.resources.drop(dictionaryElement);

let borrowedEvent;
const callback = adapter.registerCallback("EventListener", event => {{
  borrowedEvent = event;
  if (runtime.resources.borrow(event) !== callbackEvent)
    throw new Error("callback resource was not live during invocation");
}});
imports.event_target_add_event_listener(targetHandle, "default", callback, undefined);
let stale = false;
try {{ runtime.resources.borrow(borrowedEvent); }} catch {{ stale = true; }}
if (!stale) throw new Error("callback resource borrow escaped its invocation");
imports.event_target_add_event_listener(targetHandle, "boolean", null, {{
  case: "bool", value: true,
}});
imports.event_target_add_event_listener(targetHandle, "dictionary", null, {{
  case: "named-addeventlisteneroptions",
  value: {{ capture: true, passive: true, once: true }},
}});
if (JSON.stringify(observedOptions[0]) !==
    JSON.stringify({{ capture: false, once: false }}))
  throw new Error(`dictionary default was not applied: ${{JSON.stringify(observedOptions[0])}}`);
if (observedOptions[1] !== true) throw new Error("boolean union arm failed");
if (JSON.stringify(observedOptions[2]) !==
    JSON.stringify({{ capture: true, passive: true, once: true }}))
  throw new Error("dictionary union arm failed");
adapter.releaseCallback(callback);
"#,
        adapter_url = format!("file://{}", adapter_path.display()),
        runtime_url = format!("file://{}", runtime_path.display()),
    );
    std::fs::write(&test_path, script).unwrap();
    let output = std::process::Command::new("bun")
        .arg("run")
        .arg(&test_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "rich event adapter failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn pinned_webref_selection_has_verified_provenance_and_inventory() {
    let provenance: serde_json::Value = serde_json::from_str(PROVENANCE).unwrap();
    assert_eq!(provenance["package"], "@webref/raw-idl");
    assert_eq!(provenance["version"], "3.81.3");
    assert_eq!(
        provenance["revision"],
        "512c0b511d3c13b2421450ea7e33b80c6c8ef26c"
    );
    assert_eq!(provenance["license"], "MIT");
    assert_eq!(provenance["sources"].as_array().unwrap().len(), 2);
    let digest = format!("{:x}", Sha256::digest(SELECTED.as_bytes()));
    assert_eq!(provenance["selection"]["sha256"], digest);

    let unsupported: serde_json::Value = serde_json::from_str(UNSUPPORTED).unwrap();
    assert_eq!(unsupported["revision"], provenance["revision"]);
    let items = unsupported["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|item| {
        item["source"]
            .as_str()
            .is_some_and(|source| source.contains(".idl:"))
            && item["reason"]
                .as_str()
                .is_some_and(|reason| !reason.is_empty())
    }));
}

#[test]
fn pinned_webref_subset_links_and_selects_deterministically() {
    let first = parse(SELECTED).unwrap();
    let second = parse(SELECTED).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first
            .interfaces
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "DOMImplementation",
            "Document",
            "Element",
            "Event",
            "EventTarget",
            "Node",
            "Window"
        ]
    );
    assert_eq!(
        first.interfaces["Node"].inherits.as_deref(),
        Some("EventTarget")
    );
    assert_eq!(
        first.interfaces["Document"].inherits.as_deref(),
        Some("Node")
    );
    assert_eq!(
        first.interfaces["Element"].inherits.as_deref(),
        Some("Node")
    );

    let plan = build_adapter_plan(&first, "pinned-dom", "fe:web").unwrap();
    let metadata = adapter_operation_metadata(&plan, "webref-3.81.3");
    let interface = InterfaceManifest {
        imports: vec![
            import("document_create_element"),
            import("document_create_event"),
            import("event_composed_path"),
            import("event_target_add_event_listener"),
            import("event_target_dispatch_event"),
            import("window_close"),
        ],
        ..InterfaceManifest::default()
    };
    let selected = select_adapter_operations(&interface, &metadata).unwrap();
    assert_eq!(selected.version, 1);
    assert_eq!(selected.providers, ["webref-3.81.3"]);
    assert_eq!(
        selected
            .operations
            .iter()
            .map(|operation| operation.name.as_str())
            .collect::<Vec<_>>(),
        [
            "document_create_element",
            "document_create_event",
            "event_composed_path",
            "event_target_add_event_listener",
            "event_target_dispatch_event",
            "window_close"
        ]
    );
    assert_eq!(
        selected.resources,
        ["Document", "Element", "Event", "EventTarget", "Window"]
    );
}
