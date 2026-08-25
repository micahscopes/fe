//! Executable gate for compiler-derived typed Worker mailbox mechanics.
//!
//! Fe and the compiler own the child type, request/response relation, and
//! opaque lane identity. This gate exercises only the fixed JavaScript
//! transport: readiness, cancellation, scalar canonical lifting, dispatch,
//! and response lowering.

use fe_codegen::{
    CanonicalExecution, CanonicalField, CanonicalInterfaceManifest, CanonicalLaneDecl,
    CanonicalLaneIntent, CanonicalListElement, CanonicalPlacement, CanonicalType,
    browser_actor_runtime_files, emit_canonical_interface_js,
};

#[test]
fn compiler_derived_worker_mailbox_dispatches_and_cancels_without_host_policy() {
    if !std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }

    let lane = "request_0123456789abcdef";
    let record = CanonicalType::Record(vec![CanonicalField::new("value", CanonicalType::U32)]);
    let interface = CanonicalInterfaceManifest::build(vec![CanonicalLaneDecl {
        name: lane.to_owned(),
        export: Some("fe_cabi_double".to_owned()),
        request: record.clone(),
        response: record,
        intent: CanonicalLaneIntent {
            execution: CanonicalExecution::Wasm,
            placement: CanonicalPlacement::Worker,
            capabilities: Vec::new(),
        },
    }])
    .expect("scalar Worker interface");

    let directory = tempfile::tempdir().unwrap();
    for (relative, source) in browser_actor_runtime_files() {
        let path = directory.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    std::fs::write(
        directory.path().join("interface.js"),
        emit_canonical_interface_js(&interface).unwrap(),
    )
    .unwrap();
    let test_path = directory.path().join("worker-mailbox.mjs");
    let script = format!(
        r#"
import {{ createModuleWorkerScope }} from "./runtime/module-worker-actor.js";
import {{ createCanonicalWorkerMailboxImports }} from "./runtime/actor-client.js";
import {{ compileActorMailbox }} from "./interface.js";

const lane = {lane:?};
const tape = [];
const actor = {{
  request(actualLane, payload, requestId, {{ signal }}) {{
    tape.push(["request", actualLane, payload.value, requestId, signal?.aborted ?? false]);
    return Promise.resolve({{ value: payload.value * 2 }});
  }},
  async restart() {{ throw new Error("restart was not selected by Fe"); }},
  observeFailure() {{ return new Promise(() => {{}}); }},
  close() {{ tape.push(["close"]); }},
  epoch() {{ return 0; }},
  status() {{ return {{ state: "ready" }}; }},
}};
const scope = createModuleWorkerScope({{
  async createActor({{ initialEpoch }}) {{
    tape.push(["spawn", initialEpoch]);
    return actor;
  }},
}});

// A request may wait for Fe to select spawn, but aborting its owning
// continuation removes it. A later spawn must not revive or dispatch it.
const cancelled = new AbortController();
const waiting = scope.request(lane, {{ value: 7 }}, cancelled.signal);
cancelled.abort();
let cancellationError;
try {{ await waiting; }} catch (error) {{ cancellationError = error; }}
if (cancellationError?.code !== "FE_ACTOR_ABORTED") {{
  throw new Error("pre-spawn mailbox cancellation was not terminal");
}}
await scope.spawn(0);
if (tape.some(entry => entry[0] === "request")) {{
  throw new Error("an aborted readiness waiter was revived by spawn");
}}

let begun;
const completions = {{
  protocol: "fe:generated-completion/v1",
  begin(kind, operation, successWidth, lower, cancel) {{
    begun = {{ kind, operation, successWidth, lower, cancel }};
    return 17;
  }},
}};
const imports = createCanonicalWorkerMailboxImports({{
  scope,
  completions,
  mailbox: compileActorMailbox(),
}});
imports.attach({{}});
const token = imports["fe:worker-mailbox"][lane](21);
if (token !== 17 || begun.kind !== `worker-mailbox/${{lane}}`
    || begun.successWidth !== 1) {{
  throw new Error("generated completion registration drifted");
}}
const response = await begun.operation(new AbortController().signal);
const carriers = begun.lower(response);
if (JSON.stringify(carriers) !== "[42]") {{
  throw new Error(`canonical response lowering drifted: ${{JSON.stringify(carriers)}}`);
}}
begun.cancel(true);
if (JSON.stringify(tape) !== JSON.stringify([
  ["spawn", 0], ["request", lane, 21, 0, false],
])) {{
  throw new Error(`host selected or rewrote mailbox semantics: ${{JSON.stringify(tape)}}`);
}}

// Closing an idle scope rejects every readiness waiter without dispatching.
const idle = createModuleWorkerScope({{
  async createActor() {{ throw new Error("idle scope unexpectedly spawned"); }},
}});
const closedRequest = idle.request(lane, {{ value: 9 }});
idle.close(0);
let closeError;
try {{ await closedRequest; }} catch (error) {{ closeError = error; }}
if (closeError?.code !== "FE_ACTOR_CLOSED") {{
  throw new Error("scope close did not reject its mailbox readiness waiter");
}}
"#,
    );
    std::fs::write(&test_path, script).unwrap();
    let output = std::process::Command::new("bun")
        .arg("run")
        .arg(&test_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "typed Worker mailbox runtime failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn compiler_derived_worker_mailbox_copies_and_releases_rich_owned_values() {
    if !std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }

    let lane = "request_fedcba9876543210";
    let request = CanonicalType::Record(vec![
        CanonicalField::new("text", CanonicalType::String),
        CanonicalField::new("payload", CanonicalType::Bytes),
        CanonicalField::new(
            "values",
            CanonicalType::List {
                element: CanonicalListElement::U32,
                max: 4,
            },
        ),
        CanonicalField::new("seed", CanonicalType::U32),
    ]);
    let response = CanonicalType::Record(vec![
        CanonicalField::new("text", CanonicalType::String),
        CanonicalField::new("payload", CanonicalType::Bytes),
        CanonicalField::new(
            "values",
            CanonicalType::List {
                element: CanonicalListElement::U32,
                max: 4,
            },
        ),
        CanonicalField::new("receipt", CanonicalType::U32),
    ]);
    let interface = CanonicalInterfaceManifest::build(vec![CanonicalLaneDecl {
        name: lane.to_owned(),
        export: Some("fe_cabi_rich".to_owned()),
        request,
        response,
        intent: CanonicalLaneIntent {
            execution: CanonicalExecution::Wasm,
            placement: CanonicalPlacement::Worker,
            capabilities: Vec::new(),
        },
    }])
    .expect("rich Worker interface");

    let directory = tempfile::tempdir().unwrap();
    for (relative, source) in browser_actor_runtime_files() {
        let path = directory.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    std::fs::write(
        directory.path().join("interface.js"),
        emit_canonical_interface_js(&interface).unwrap(),
    )
    .unwrap();
    let test_path = directory.path().join("rich-worker-mailbox.mjs");
    let script = format!(
        r#"
import {{ createCanonicalWorkerMailboxImports }} from "./runtime/actor-client.js";
import {{ compileActorMailbox }} from "./interface.js";

const lane = {lane:?};
const memory = new WebAssembly.Memory({{ initial: 1 }});
let cursor = 1024;
const allocations = [];
const exports = {{
  memory,
  cabi_realloc(oldPointer, oldSize, align, size) {{
    if (oldPointer !== 0 || oldSize !== 0 || ![1, 4].includes(align) || size <= 0) {{
      throw new Error("mailbox used a non-canonical allocation request");
    }}
    cursor = Math.ceil(cursor / align) * align;
    const pointer = cursor;
    cursor += size;
    allocations.push([pointer, size, align]);
    return pointer;
  }},
  fe_cabi_post_return(pointer, size, align) {{
    const expected = allocations.pop();
    if (JSON.stringify(expected) !== JSON.stringify([pointer, size, align])) {{
      throw new Error("mailbox response allocations were not released in LIFO order");
    }}
    cursor = pointer;
  }},
}};
const bytes = new Uint8Array(memory.buffer);
const view = new DataView(memory.buffer);
const requestText = new TextEncoder().encode("hé");
bytes.set(requestText, 64);
bytes.set([3, 1, 4], 96);
view.setUint32(128, 7, true);
view.setUint32(132, 11, true);
view.setUint32(136, 13, true);

let captured;
const completions = {{
  protocol: "fe:generated-completion/v1",
  begin(identity, operation, successWidth, lower, release) {{
    captured = {{ identity, operation, successWidth, lower, release }};
    return 23;
  }},
}};
let requestCount = 0;
const scope = {{
  request(actualLane, request) {{
    if (actualLane !== lane) throw new Error("rich mailbox routed to the wrong lane");
    requestCount += 1;
    if (request.text !== "hé"
        || JSON.stringify(Array.from(request.payload)) !== "[3,1,4]"
        || !(request.values instanceof Uint32Array)
        || JSON.stringify(Array.from(request.values)) !== "[7,11,13]") {{
      throw new Error("rich mailbox request did not preserve canonical ownership");
    }}
    if (request.seed === 99) {{
      return Promise.resolve({{
        text: "bad",
        payload: new Uint8Array([1]),
        values: new Uint32Array([1, 2, 3, 4, 5]),
        receipt: 0,
      }});
    }}
    return Promise.resolve({{
      text: `${{request.text}}!`,
      payload: new Uint8Array([...request.payload, 9]),
      values: new Uint32Array(Array.from(request.values, value => value + request.seed)),
      receipt: request.seed + request.values[2],
    }});
  }},
}};
const bridge = createCanonicalWorkerMailboxImports({{
  scope,
  completions,
  mailbox: compileActorMailbox(),
}});
bridge.attach(exports);
const begin = bridge["fe:worker-mailbox"][lane];
if (begin(64, requestText.length, 96, 3, 128, 3, 5) !== 23
    || captured.successWidth !== 7) {{
  throw new Error("rich mailbox carrier width drifted");
}}

// Lifting must sever the request from mutable guest memory before the
// asynchronous Worker edge starts.
bytes.fill(0, 64, 64 + requestText.length);
bytes.fill(0, 96, 99);
view.setUint32(128, 0, true);
const response = await captured.operation(new AbortController().signal);
const carriers = captured.lower(response);
if (carriers.length !== 7 || carriers[6] !== 18) {{
  throw new Error(`rich mailbox response carriers drifted: ${{String(carriers)}}`);
}}
const responseText = new TextDecoder("utf-8", {{ fatal: true }}).decode(
  bytes.slice(carriers[0], carriers[0] + carriers[1]),
);
if (responseText !== "hé!"
    || JSON.stringify(Array.from(bytes.slice(carriers[2], carriers[2] + carriers[3])))
      !== "[3,1,4,9]") {{
  throw new Error("rich mailbox response bytes drifted");
}}
const responseValues = [];
for (let index = 0; index < carriers[5]; index += 1) {{
  responseValues.push(view.getUint32(carriers[4] + index * 4, true));
}}
if (JSON.stringify(responseValues) !== "[12,16,18]") {{
  throw new Error(`rich mailbox response list drifted: ${{JSON.stringify(responseValues)}}`);
}}
captured.release(true);
if (cursor !== 1024 || allocations.length !== 0) {{
  throw new Error("rich mailbox leaked successful response allocations");
}}

// A response that fails after earlier descriptors were lowered must roll
// those allocations back through the same checked stack.
bytes.set(requestText, 64);
bytes.set([3, 1, 4], 96);
view.setUint32(128, 7, true);
view.setUint32(132, 11, true);
view.setUint32(136, 13, true);
begin(64, requestText.length, 96, 3, 128, 3, 99);
const invalid = await captured.operation(new AbortController().signal);
let invalidError;
try {{ captured.lower(invalid); }} catch (error) {{ invalidError = error; }}
captured.release(false);
if (!(invalidError instanceof RangeError)
    || cursor !== 1024 || allocations.length !== 0 || requestCount !== 2) {{
  throw new Error("invalid rich response did not fail and reclaim its partial allocation");
}}
"#,
    );
    std::fs::write(&test_path, script).unwrap();
    let output = std::process::Command::new("bun")
        .arg("run")
        .arg(&test_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "rich typed Worker mailbox runtime failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
