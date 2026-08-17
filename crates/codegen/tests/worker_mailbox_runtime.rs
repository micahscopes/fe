//! Executable gate for compiler-derived typed Worker mailbox mechanics.
//!
//! Fe and the compiler own the child type, request/response relation, and
//! opaque lane identity. This gate exercises only the fixed JavaScript
//! transport: readiness, cancellation, scalar canonical lifting, dispatch,
//! and response lowering.

use fe_codegen::{
    CanonicalExecution, CanonicalField, CanonicalInterfaceManifest, CanonicalLaneDecl,
    CanonicalLaneIntent, CanonicalPlacement, CanonicalType, browser_actor_runtime_files,
    emit_canonical_interface_js,
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
