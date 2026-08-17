//! Acceptance for the target-neutral resident actor contract and its first Fe
//! web-component consumer. Correctness comes from executing authored Fe over a
//! lifecycle/input tape and comparing every state to an independent reducer;
//! source or artifact byte equality is not used as behavioral evidence.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    CanonicalType, RESIDENT_ACTOR_INITIALIZE_EXPORT, RESIDENT_ACTOR_PROJECT_EXPORT,
    RESIDENT_ACTOR_STATE_REPLACE_EXPORT, RESIDENT_ACTOR_TRANSITION_EXPORT,
    browser_actor_runtime_files, compile_resident_actor, emit_canonical_interface_js,
    emit_materialized_task_adapter_js, materialize_scoped_task_package,
};
use hir::hir_def::HirIngot;
use url::Url;

fn fixture() -> (DriverDataBase, Url) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/web_component_actor")
        .canonicalize()
        .expect("component fixture path");
    let url = Url::from_directory_path(path).expect("component fixture URL");
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "component fixture ingot initialization diagnostics"
    );
    (db, url)
}

fn scoped_task_fixture() -> (DriverDataBase, common::file::File) {
    const SOURCE: &str = r#"
use core::actor::{InitialState, ProjectState, ResidentTransition, ScopedTask}
use core::pending::{Suspend, TaskOutcome, Timer}
use std::host::{HostTimer, Resumable}
use std::wasm::WasmBackend

pub struct Tick { pub value: u32 }
pub struct TickState { pub value: u32 }
pub struct TickProjection { pub value: u32 }

fn sleep_once(_ ms: u64) -> u64
    uses (timer: mut Timer<WasmBackend>, suspend: Suspend<WasmBackend, u32>)
{
    let pending = timer.sleep_begin(ms)
    let result: TaskOutcome<u32, u64> = suspend.suspend(pending)
    match result {
        TaskOutcome::Success(woke_at) => woke_at
        TaskOutcome::Failure(_) => 4001
        TaskOutcome::Cancelled => 4002
    }
}

actor Clock {
    value: u32,

    fn initial() -> TickState uses (InitialState) {
        TickState { value: 0 }
    }

    fn receive(self, event: Tick) -> TickState uses (ResidentTransition) {
        TickState { value: self.value + event.value }
    }

    fn project(self) -> TickProjection uses (ProjectState) {
        TickProjection { value: self.value }
    }

    fn heartbeat() -> u64 uses (ScopedTask) {
        with (Timer<WasmBackend> = HostTimer {}, Suspend<WasmBackend, u32> = Resumable {}) {
            sleep_once(1)
        }
    }
}
"#;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///web_component_scoped_task.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    (db, file)
}

fn structured_child_fixture() -> (DriverDataBase, common::file::File) {
    const SOURCE: &str = r#"
use core::Worker
use core::actor::{
    ActorMailbox, ActorSink, InitialState, ProjectState, ResidentTransition, ScopedTask,
}
use core::pending::{Pending, Suspend, TaskOutcome}
use std::actor::{ActorMessage, BrowserActorSink}
use std::host::Resumable
use std::runtime::{BrowserActorMailbox, ChildScopeExit, supervise_browser_child}
use std::wasm::WasmBackend

pub struct Request { pub value: u32 }
pub struct Response { pub value: u32 }
pub struct ParentEvent { pub value: u32 }
pub struct ParentState { pub value: u32 }
pub struct ParentProjection { pub value: u32 }

actor ArithmeticChild {
    fn double(request: Request) -> Response uses (Worker) {
        Response { value: request.value * 2 }
    }
}

fn request_double(_ request: own Request) -> TaskOutcome<u32, Response>
uses (
    mailbox: mut ActorMailbox<WasmBackend, ArithmeticChild>,
    suspend: Suspend<WasmBackend, u32>,
)
{
    let pending: Pending<WasmBackend, Response> = mailbox.ask(request)
    suspend.suspend(pending)
}

actor Parent {
    value: u32,

    fn initial() -> ParentState uses (InitialState) {
        ParentState { value: 21 }
    }

    fn receive(self, event: ParentEvent) -> ParentState uses (ResidentTransition) {
        ParentState { value: self.value + event.value }
    }

    fn project(self) -> ParentProjection uses (ProjectState) {
        ParentProjection { value: self.value }
    }

    fn supervise() -> u32 uses (ScopedTask) {
        match supervise_browser_child(
            child: ArithmeticChild {},
            max_restarts: 2,
            window_ms: 1000,
            backoff_ms: 3,
            startup_timeout_ms: 1000,
        ) {
            ChildScopeExit::Cancelled(epoch) => epoch
            ChildScopeExit::Exhausted(epoch, _) => epoch
            ChildScopeExit::TransportFailure(epoch, _) => epoch
            ChildScopeExit::InvariantViolation(epoch, _) => epoch
        }
    }

    fn calculate(self) -> u32 uses (ScopedTask) {
        let value = with (
            ActorMailbox<WasmBackend, ArithmeticChild> = BrowserActorMailbox<ArithmeticChild> {},
            Suspend<WasmBackend, u32> = Resumable {},
        ) {
            match request_double(Request { value: self.value }) {
                TaskOutcome::Success(response,) => response.value
                TaskOutcome::Failure(error,) => 100 + error
                TaskOutcome::Cancelled => 200
            }
        }
        with (
            ActorSink<WasmBackend, ParentEvent> = BrowserActorSink {},
            Suspend<WasmBackend, u32> = Resumable {},
        ) {
            match ActorMessage::new(ParentEvent { value: value }).send() {
                TaskOutcome::Success(_,) => value
                TaskOutcome::Failure(error,) => 300 + error
                TaskOutcome::Cancelled => 400
            }
        }
    }
}
"#;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///web_component_structured_child.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    (db, file)
}

fn two_structured_children_fixture() -> (DriverDataBase, common::file::File) {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///web_component_two_structured_children.fe").unwrap();
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(include_str!("fixtures/two_structured_children.fe").to_owned()),
    );
    let file = db.workspace().get(&db, &url).unwrap();
    (db, file)
}

fn invalid_mailbox_fixture(
    supervised: bool,
    forged_response: bool,
) -> (DriverDataBase, common::file::File) {
    let claimed_response = if forged_response {
        "WrongResponse"
    } else {
        "Response"
    };
    let forged_relation = if forged_response {
        "impl Handles<Request, WrongResponse> for ArithmeticChild {}"
    } else {
        ""
    };
    let supervisor = if supervised {
        r#"
    fn supervise() -> u32 uses (ScopedTask) {
        match supervise_browser_child(
            child: ArithmeticChild {},
            max_restarts: 1,
            window_ms: 1000,
            backoff_ms: 1,
            startup_timeout_ms: 1000,
        ) {
            ChildScopeExit::Cancelled(epoch) => epoch
            ChildScopeExit::Exhausted(epoch, _) => epoch
            ChildScopeExit::TransportFailure(epoch, _) => epoch
            ChildScopeExit::InvariantViolation(epoch, _) => epoch
        }
    }
"#
    } else {
        ""
    };
    let source = format!(
        r#"
use core::Worker
use core::actor::{{
    ActorMailbox, Handles, InitialState, ProjectState, ResidentTransition, ScopedTask,
}}
use core::pending::{{Pending, Suspend, TaskOutcome}}
use std::host::Resumable
use std::runtime::{{BrowserActorMailbox, ChildScopeExit, supervise_browser_child}}
use std::wasm::WasmBackend

pub struct Request {{ pub value: u32 }}
pub struct Response {{ pub value: u32 }}
pub struct WrongResponse {{ pub value: u32 }}
pub struct ParentEvent {{ pub value: u32 }}
pub struct ParentState {{ pub value: u32 }}
pub struct ParentProjection {{ pub value: u32 }}

actor ArithmeticChild {{
    fn double(request: Request) -> Response uses (Worker) {{
        Response {{ value: request.value * 2 }}
    }}
}}

{forged_relation}

fn request_child(_ request: own Request) -> TaskOutcome<u32, {claimed_response}>
uses (
    mailbox: mut ActorMailbox<WasmBackend, ArithmeticChild>,
    suspend: Suspend<WasmBackend, u32>,
)
{{
    let pending: Pending<WasmBackend, {claimed_response}> = mailbox.ask(request)
    suspend.suspend(pending)
}}

actor Parent {{
    value: u32,

    fn initial() -> ParentState uses (InitialState) {{
        ParentState {{ value: 21 }}
    }}

    fn receive(self, event: ParentEvent) -> ParentState uses (ResidentTransition) {{
        ParentState {{ value: self.value + event.value }}
    }}

    fn project(self) -> ParentProjection uses (ProjectState) {{
        ParentProjection {{ value: self.value }}
    }}

{supervisor}

    fn calculate(self) -> u32 uses (ScopedTask) {{
        with (
            ActorMailbox<WasmBackend, ArithmeticChild> = BrowserActorMailbox<ArithmeticChild> {{}},
            Suspend<WasmBackend, u32> = Resumable {{}},
        ) {{
            match request_child(Request {{ value: self.value }}) {{
                TaskOutcome::Success(response,) => response.value
                TaskOutcome::Failure(error,) => 100 + error
                TaskOutcome::Cancelled => 200
            }}
        }}
    }}
}}
"#,
    );
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///web_component_invalid_mailbox.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source));
    let file = db.workspace().get(&db, &url).unwrap();
    (db, file)
}

fn partial_state_scoped_task_fixture() -> (DriverDataBase, common::file::File) {
    const SOURCE: &str = r#"
use core::actor::{InitialState, ProjectState, ResidentTransition, ScopedTask}

pub struct Tick { pub value: u32 }
pub struct TickState { pub value: u32, pub revision: u32 }
pub struct TickProjection { pub value: u32 }

actor Clock {
    value: u32,
    revision: u32,

    fn initial() -> TickState uses (InitialState) {
        TickState { value: 0, revision: 0 }
    }

    fn receive(self, event: Tick) -> TickState uses (ResidentTransition) {
        TickState { value: self.value + event.value, revision: self.revision + 1 }
    }

    fn project(self) -> TickProjection uses (ProjectState) {
        TickProjection { value: self.value }
    }

    fn heartbeat(value: u32) -> u32 uses (ScopedTask) { value }
}
"#;
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///partial_state_scoped_task.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    (db, file)
}

fn actor_sink_fixture(event_ty: &str) -> (DriverDataBase, common::file::File) {
    let source = format!(
        r#"
use core::actor::{{ActorSink, InitialState, ProjectState, ResidentTransition, ScopedTask}}
use core::pending::{{Suspend, TaskOutcome}}
use std::actor::{{ActorMessage, BrowserActorSink}}
use std::host::Resumable
use std::wasm::WasmBackend

pub enum TickKind {{ Add, Reset }}
impl Copy for TickKind {{}}
pub struct Tick {{ pub kind: TickKind, pub value: u32 }}
// Deliberately layout-identical to `Tick`: the resident contract must compare
// semantic Fe identity rather than accepting an equal flattened lane shape.
pub struct Other {{ pub kind: TickKind, pub value: u32 }}
pub struct TickState {{ pub value: u32, pub messages: u32 }}
pub struct TickProjection {{ pub value: u32, pub messages: u32 }}

actor Clock {{
    value: u32,
    messages: u32,

    fn initial() -> TickState uses (InitialState) {{
        TickState {{ value: 2, messages: 0 }}
    }}

    fn receive(self, event: Tick) -> TickState uses (ResidentTransition) {{
        match event.kind {{
            TickKind::Add => TickState {{
                value: self.value + event.value,
                messages: self.messages + 1,
            }}
            TickKind::Reset => TickState {{ value: 0, messages: self.messages + 1 }}
        }}
    }}

    fn project(self) -> TickProjection uses (ProjectState) {{
        TickProjection {{ value: self.value, messages: self.messages }}
    }}

    fn heartbeat() -> u32 uses (ScopedTask) {{
        with (
            ActorSink<WasmBackend, {event_ty}> = BrowserActorSink {{}},
            Suspend<WasmBackend, u32> = Resumable {{}},
        ) {{
            let message: ActorMessage<{event_ty}> = ActorMessage::new(
                {event_ty} {{ kind: TickKind::Add, value: 7 }}
            )
            let outcome: TaskOutcome<u32, ()> = message.send()
            match outcome {{
                TaskOutcome::Success(_) => 71
                TaskOutcome::Failure(error) => 100 + error
                TaskOutcome::Cancelled => 200
            }}
        }}
    }}
}}
"#,
    );
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///web_component_actor_sink.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source));
    let file = db.workspace().get(&db, &url).unwrap();
    (db, file)
}

fn function_exports(wasm: &[u8]) -> Vec<String> {
    let mut exports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::ExportSection(reader) = payload.expect("valid Wasm") {
            for export in reader {
                let export = export.expect("valid export");
                if export.kind == wasmparser::ExternalKind::Func {
                    exports.push(export.name.to_owned());
                }
            }
        }
    }
    exports
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InspectorState {
    selected: u32,
    connected: bool,
    revision: u32,
}

#[derive(Debug, Clone, Copy)]
struct Event {
    kind: u32,
    target: u32,
    key: u32,
    detail: u32,
    value: f32,
    timestamp: f32,
    text_ptr: u32,
    text_len: u32,
}

fn oracle(mut state: InspectorState, event: Event) -> InspectorState {
    match event.kind {
        0 => {
            state.connected = true;
            state.revision += 1;
        }
        1 => {
            state.connected = false;
            state.revision += 1;
        }
        3 if state.connected => {
            state.selected = event.target;
            state.revision += 1;
        }
        7 if state.connected => {
            match event.detail {
                37 if state.selected > 0 => state.selected -= 1,
                39 => state.selected += 1,
                _ => {}
            }
            state.revision += 1;
        }
        _ => {}
    }
    state
}

#[test]
fn fe_component_actor_owns_lifecycle_selection_and_resident_state() {
    let (db, url) = fixture();
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("component fixture ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "component fixture diagnostics:\n{diagnostics}"
    );

    let artifact = compile_resident_actor(&db, top_mod)
        .expect("resident actor contract")
        .expect("role-selected resident actor");
    assert_eq!(artifact.contract.actor, "SourceInspector");
    assert_eq!(artifact.contract.init_source_entry, "initial");
    assert_eq!(artifact.contract.projection_source_entry, "project");
    assert_eq!(artifact.contract.source_entry, "receive");
    assert_eq!(artifact.contract.event_leaf_count, 9);
    assert_eq!(artifact.contract.state_leaf_count, 3);
    assert_eq!(artifact.contract.projection_leaf_count, 5);
    assert!(matches!(artifact.contract.event, CanonicalType::Record(_)));
    assert!(matches!(artifact.contract.state, CanonicalType::Record(_)));
    assert!(matches!(
        artifact.contract.projection,
        CanonicalType::Record(_)
    ));

    let exports = function_exports(&artifact.wasm);
    assert!(
        exports
            .iter()
            .any(|name| name == RESIDENT_ACTOR_TRANSITION_EXPORT),
        "fixed resident transition export missing: {exports:?}"
    );
    assert!(
        exports
            .iter()
            .any(|name| name == RESIDENT_ACTOR_STATE_REPLACE_EXPORT),
        "fixed state seed export missing: {exports:?}"
    );
    assert!(
        exports
            .iter()
            .any(|name| name == RESIDENT_ACTOR_INITIALIZE_EXPORT),
        "fixed Fe-owned initializer export missing: {exports:?}"
    );
    assert!(
        exports
            .iter()
            .any(|name| name == RESIDENT_ACTOR_PROJECT_EXPORT),
        "fixed Fe-owned projection export missing: {exports:?}"
    );
    assert!(
        !exports
            .iter()
            .any(|name| name == "receive" || name == "initial" || name == "project"),
        "ordinary Fe behavior names must not become host discovery policy"
    );

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &artifact.wasm).expect("resident module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("resident instance");
    let transition = instance
        .get_typed_func::<(i32, i32, i32, i32, i32, f32, f32, i32, i32), (i32, i32, i32)>(
            &mut store,
            RESIDENT_ACTOR_TRANSITION_EXPORT,
        )
        .expect("component transition signature");
    let replace = instance
        .get_typed_func::<(i32, i32, i32), ()>(&mut store, RESIDENT_ACTOR_STATE_REPLACE_EXPORT)
        .expect("component state seed signature");
    let initialize = instance
        .get_typed_func::<(), (i32, i32, i32)>(&mut store, RESIDENT_ACTOR_INITIALIZE_EXPORT)
        .expect("Fe-owned component initializer signature");
    let project = instance
        .get_typed_func::<(), (i32, i32, i32, i32, i32)>(&mut store, RESIDENT_ACTOR_PROJECT_EXPORT)
        .expect("Fe-owned component projection signature");

    assert!(
        transition
            .call(&mut store, (0, 0, 0, 0, 0, 0.0, 0.0, 0, 0))
            .is_err(),
        "component transition must reject events before a complete state seed"
    );
    assert!(
        project.call(&mut store, ()).is_err(),
        "component projection must reject reads before Fe initialization"
    );
    let initial = initialize
        .call(&mut store, ())
        .expect("initialize component from authored Fe");
    assert_eq!(initial, (0, 0, 0));
    assert_eq!(
        project.call(&mut store, ()).expect("initial patch"),
        (0, 0, 0, 0, 0)
    );

    // The replacement ABI remains an explicit restoration boundary, not the
    // ordinary startup path. Exercise it once, then return to Fe's initializer.
    replace
        .call(&mut store, (99, 1, 7))
        .expect("explicit restoration state replacement");
    assert_eq!(
        initialize.call(&mut store, ()).expect("reinitialize in Fe"),
        (0, 0, 0)
    );

    let tape = [
        Event {
            kind: 3,
            target: 7,
            key: 0,
            detail: 0,
            value: 11.0,
            timestamp: 1.0,
            text_ptr: 0,
            text_len: 0,
        },
        Event {
            kind: 0,
            target: 0,
            key: 0,
            detail: 0,
            value: 12.0,
            timestamp: 2.0,
            text_ptr: 0,
            text_len: 0,
        },
        Event {
            kind: 3,
            target: 2,
            key: 0,
            detail: 0,
            value: 13.0,
            timestamp: 3.0,
            text_ptr: 0,
            text_len: 0,
        },
        Event {
            kind: 7,
            target: 0,
            key: 0,
            detail: 39,
            value: 14.0,
            timestamp: 4.0,
            text_ptr: 0,
            text_len: 0,
        },
        Event {
            kind: 7,
            target: 0,
            key: 0,
            detail: 37,
            value: 15.0,
            timestamp: 5.0,
            text_ptr: 0,
            text_len: 0,
        },
        Event {
            kind: 1,
            target: 0,
            key: 0,
            detail: 0,
            value: 16.0,
            timestamp: 6.0,
            text_ptr: 0,
            text_len: 0,
        },
        Event {
            kind: 3,
            target: 9,
            key: 0,
            detail: 0,
            value: 17.0,
            timestamp: 7.0,
            text_ptr: 0,
            text_len: 0,
        },
        Event {
            kind: 0,
            target: 0,
            key: 0,
            detail: 0,
            value: 18.0,
            timestamp: 8.0,
            text_ptr: 0,
            text_len: 0,
        },
        Event {
            kind: 7,
            target: 0,
            key: 0,
            detail: 37,
            value: 19.0,
            timestamp: 9.0,
            text_ptr: 0,
            text_len: 0,
        },
        Event {
            kind: 4,
            target: 99,
            key: 0,
            detail: 12,
            value: 20.0,
            timestamp: 10.0,
            text_ptr: 0,
            text_len: 0,
        },
    ];
    let mut expected = InspectorState {
        selected: 0,
        connected: false,
        revision: 0,
    };
    for (index, event) in tape.into_iter().enumerate() {
        expected = oracle(expected, event);
        let got = transition
            .call(
                &mut store,
                (
                    event.kind as i32,
                    event.target as i32,
                    0,
                    event.key as i32,
                    event.detail as i32,
                    event.value,
                    event.timestamp,
                    event.text_ptr as i32,
                    event.text_len as i32,
                ),
            )
            .unwrap_or_else(|error| panic!("component event {index} trapped: {error}"));
        assert_eq!(
            (got.0 as u32, got.1 != 0, got.2 as u32),
            (expected.selected, expected.connected, expected.revision),
            "component state differs after event {index}"
        );
        let expected_mask = if expected.connected && expected.selected < 32 {
            1u32 << expected.selected
        } else {
            0
        };
        let patch = project
            .call(&mut store, ())
            .unwrap_or_else(|error| panic!("component projection {index} trapped: {error}"));
        assert_eq!(
            (
                patch.0 as u32,
                patch.1 as u32,
                patch.2 as u32,
                patch.3 as u32,
                patch.4 as u32,
            ),
            (expected_mask, 0, 0, 0, 0),
            "Fe component patch differs after event {index}"
        );
    }

    assert!(
        transition
            .call(&mut store, (99, 0, 0, 0, 0, 0.0, 11.0, 0, 0))
            .is_err(),
        "an invalid fieldless-enum tag must trap before Fe can interpret it"
    );
}

#[test]
fn resident_actor_scoped_task_is_role_selected_and_materialized_without_a_task_table() {
    let (db, file) = scoped_task_fixture();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "component scoped-task fixture diagnostics:\n{diagnostics}"
    );

    let artifact = compile_resident_actor(&db, top_mod)
        .expect("resident actor plus scoped task contract")
        .expect("role-selected resident actor");
    assert_eq!(artifact.contract.actor, "Clock");
    assert_eq!(artifact.contract.scoped_task_source_entries, ["heartbeat"]);
    let [task] = artifact.scoped_tasks.as_slice() else {
        panic!("expected exactly one compiler-materialized scoped task")
    };
    assert!(
        task.input.is_empty(),
        "scoped actor tasks start without host arguments"
    );
    assert_eq!(task.continuations.len(), 1);
    assert_eq!(task.continuations[0].state, 1);

    let exports = function_exports(&artifact.wasm);
    assert!(exports.iter().any(|name| name == &task.start_export));
    assert!(
        exports
            .iter()
            .any(|name| name == &task.continuations[0].export)
    );
    assert!(
        !exports.iter().any(|name| name == "heartbeat"),
        "the authored behavior name must not become a Wasm discovery ABI"
    );
}

#[test]
fn resident_actor_derives_a_separate_zero_import_child_from_the_fe_nominal_type() {
    let (db, file) = structured_child_fixture();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "structured-child fixture diagnostics:\n{diagnostics}"
    );

    let artifact = compile_resident_actor(&db, top_mod)
        .expect("resident actor plus structured child contract")
        .expect("role-selected resident actor");
    assert_eq!(artifact.scoped_tasks.len(), 2);
    let parent_imports = wasmparser::Parser::new(0)
        .parse_all(&artifact.wasm)
        .filter_map(|payload| match payload.expect("valid parent Wasm") {
            wasmparser::Payload::ImportSection(reader) => Some(
                reader
                    .into_imports()
                    .map(|import| {
                        let import = import.expect("valid parent import");
                        (import.module.to_owned(), import.name.to_owned())
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    let mailbox_imports = parent_imports
        .iter()
        .filter(|(module, _)| module == "fe:worker-mailbox")
        .collect::<Vec<_>>();
    let [mailbox_import] = mailbox_imports.as_slice() else {
        panic!("expected exactly one typed Worker mailbox import: {parent_imports:?}")
    };
    assert!(mailbox_import.1.starts_with("request_"));
    assert_ne!(mailbox_import.1, "ask_begin");
    assert!(!mailbox_import.1.contains("double"));
    let [child] = artifact.structured_children.as_slice() else {
        panic!("nominal child type should select one actor artifact")
    };
    assert_eq!(child.actor, "ArithmeticChild");
    let [lane] = child.interface.lanes.as_slice() else {
        panic!("expected exactly one canonical child lane")
    };
    assert!(lane.name.starts_with("request_"));
    assert!(!lane.name.contains("double"));
    assert_eq!(lane.name, mailbox_import.1);
    let expected_export = format!("fe_cabi_{}", lane.name);
    assert_eq!(lane.export.as_deref(), Some(expected_export.as_str()));
    assert_eq!(
        lane.intent.placement,
        fe_codegen::CanonicalPlacement::Worker
    );
    assert_eq!(lane.request.size, 4);
    assert_eq!(lane.response.size, 4);

    let child_imports = wasmparser::Parser::new(0)
        .parse_all(&child.wasm)
        .filter_map(|payload| match payload.expect("valid child Wasm") {
            wasmparser::Payload::ImportSection(reader) => Some(reader.count()),
            _ => None,
        })
        .sum::<u32>();
    assert_eq!(
        child_imports, 0,
        "child actor must be independently instantiable"
    );

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &child.wasm).expect("child actor module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("zero-import child instance");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("child memory");
    let alloc = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .expect("child canonical allocator");
    let request = alloc.call(&mut store, (4, 4)).expect("request allocation");
    memory
        .write(&mut store, request as usize, &21_u32.to_le_bytes())
        .expect("write child request");
    let call = instance
        .get_typed_func::<i32, i32>(
            &mut store,
            lane.export.as_deref().expect("child lane export"),
        )
        .expect("typed child lane");
    let response = call
        .call(&mut store, request)
        .expect("execute Fe child lane");
    let mut value = [0_u8; 4];
    memory
        .read(&store, response as usize, &mut value)
        .expect("read child response");
    assert_eq!(u32::from_le_bytes(value), 42);
}

#[test]
fn resident_actor_typed_mailbox_round_trips_through_the_compiled_child() {
    if !std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }

    let (db, file) = structured_child_fixture();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "structured-child round-trip diagnostics:\n{diagnostics}"
    );
    let artifact = compile_resident_actor(&db, top_mod)
        .expect("resident actor plus structured child contract")
        .expect("role-selected resident actor");
    let [child] = artifact.structured_children.as_slice() else {
        panic!("expected one compiler-derived child artifact")
    };
    let [lane] = child.interface.lanes.as_slice() else {
        panic!("expected exactly one canonical child lane")
    };

    let directory = tempfile::tempdir().expect("typed mailbox execution directory");
    let parent_wasm = directory.path().join("parent.wasm");
    let child_wasm = directory.path().join("child.wasm");
    let task_adapter = directory.path().join("tasks.mjs");
    let task_runtime = directory.path().join("materialized-task.js");
    let host_runtime = directory.path().join("host-completion.js");
    let interface = directory.path().join("interface.js");
    std::fs::write(&parent_wasm, &artifact.wasm).unwrap();
    std::fs::write(&child_wasm, &child.wasm).unwrap();
    std::fs::write(
        &task_adapter,
        emit_materialized_task_adapter_js(&artifact.scoped_tasks, "./materialized-task.js")
            .expect("emit typed mailbox tasks")
            .expect("typed mailbox tasks exist"),
    )
    .unwrap();
    std::fs::write(&task_runtime, fe_codegen::MATERIALIZED_TASK_RUNTIME_JS).unwrap();
    std::fs::write(&host_runtime, fe_codegen::HOST_COMPLETION_RUNTIME_JS).unwrap();
    std::fs::write(
        &interface,
        emit_canonical_interface_js(&child.interface).expect("emit child interface"),
    )
    .unwrap();
    for (relative, source) in browser_actor_runtime_files() {
        let path = directory.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }

    let runner = directory.path().join("run-mailbox.mjs");
    std::fs::write(
        &runner,
        format!(
            r#"
import {{ createMaterializedTaskRegistry }} from {tasks:?};
import {{ createHostCompletionBroker }} from {host:?};
import {{ createModuleWorkerScope }} from "./runtime/module-worker-actor.js";
import {{ createCanonicalWorkerMailboxImports }} from "./runtime/actor-client.js";
import {{ createInterfaceCaller, compileActorMailbox }} from "./interface.js";

const parentBytes = await Bun.file({parent_wasm:?}).arrayBuffer();
const childBytes = await Bun.file({child_wasm:?}).arrayBuffer();
const child = await WebAssembly.instantiate(childBytes, {{}});
const caller = createInterfaceCaller(child.instance.exports);
const mailbox = compileActorMailbox();
const lane = {lane:?};
if (Object.keys(mailbox).length !== 1 || !Object.hasOwn(mailbox, lane)) {{
  throw new Error("compiler-derived mailbox lane drifted");
}}

const tape = [];
const scope = createModuleWorkerScope({{
  async createActor({{ initialEpoch }}) {{
    tape.push(["spawn", initialEpoch]);
    let epoch = initialEpoch;
    return {{
      request(actualLane, payload, requestId, {{ signal }}) {{
        if (signal?.aborted) throw new DOMException("cancelled", "AbortError");
        tape.push(["request", actualLane, payload.value, requestId]);
        return caller.call(actualLane, payload);
      }},
      async restart() {{ epoch += 1; return epoch; }},
      observeFailure() {{ return new Promise(() => {{}}); }},
      close() {{ tape.push(["close", epoch]); }},
      epoch() {{ return epoch; }},
      status() {{ return {{ state: "ready" }}; }},
    }};
  }},
}});
await scope.spawn(0);

let instance;
let residentValue;
const accepted = [];
const broker = createHostCompletionBroker({{
  workerScopes: [{{
    scope,
    spawn: {scope_spawn:?},
    failure: {scope_failure:?},
    close: {scope_close:?},
  }}],
  actorEvents: {{
    send(event, signal) {{
      if (signal.aborted) throw new DOMException("cancelled", "AbortError");
      accepted.push(event);
      residentValue = instance.exports.fe_actor_transition_v1(...event);
    }},
  }},
}});
const mailboxImports = createCanonicalWorkerMailboxImports({{
  scope,
  completions: broker.completions,
  mailbox,
}});
const imports = Object.assign({{}}, broker.imports, mailboxImports);
({{ instance }} = await WebAssembly.instantiate(parentBytes, imports));
const initialized = instance.exports.fe_actor_initialize_v1();
const initialValue = Array.isArray(initialized) ? initialized[0] : initialized;
if (initialValue !== 21) throw new Error(`bad resident initial state ${{initialized}}`);
const tasks = createMaterializedTaskRegistry(instance.exports);
const output = await broker.run(tasks.calculate, [initialValue]);
if (output.length !== 1 || output[0] !== 42) {{
  throw new Error(`typed mailbox task returned ${{output}}`);
}}
if (residentValue !== 63 || accepted.length !== 1) {{
  throw new Error(`typed response did not reach resident Fe: ${{residentValue}}`);
}}
if (JSON.stringify(tape) !== JSON.stringify([
  ["spawn", 0], ["request", lane, 21, 0],
])) {{
  throw new Error(`mailbox route or payload drifted: ${{JSON.stringify(tape)}}`);
}}
if (broker.activeCount() !== 0) throw new Error("typed mailbox leaked completion tokens");
scope.close(0);
"#,
            tasks = Url::from_file_path(&task_adapter).unwrap().to_string(),
            host = Url::from_file_path(&host_runtime).unwrap().to_string(),
            parent_wasm = parent_wasm.display().to_string(),
            child_wasm = child_wasm.display().to_string(),
            lane = lane.name,
            scope_spawn = child.scope.spawn,
            scope_failure = child.scope.failure,
            scope_close = child.scope.close,
        ),
    )
    .unwrap();
    let output = std::process::Command::new("bun")
        .arg("run")
        .arg(&runner)
        .output()
        .expect("run typed mailbox round trip under Bun");
    assert!(
        output.status.success(),
        "typed mailbox round trip failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn resident_actor_two_typed_children_share_one_completion_rail_without_identity_shims() {
    if !std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }

    let (db, file) = two_structured_children_fixture();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "two-child fixture diagnostics:\n{diagnostics}"
    );
    let artifact = compile_resident_actor(&db, top_mod)
        .expect("compile two nominal children")
        .expect("role-selected two-child resident actor");
    let [child0, child1] = artifact.structured_children.as_slice() else {
        panic!(
            "expected two compiler-derived child artifacts, found {}",
            artifact.structured_children.len()
        )
    };
    assert_ne!(child0.scope.key, child1.scope.key);
    assert_eq!(artifact.scoped_tasks.len(), 3);

    let parent_imports = wasmparser::Parser::new(0)
        .parse_all(&artifact.wasm)
        .filter_map(
            |payload| match payload.expect("valid two-child parent Wasm") {
                wasmparser::Payload::ImportSection(reader) => Some(
                    reader
                        .into_imports()
                        .map(|import| {
                            let import = import.expect("valid two-child parent import");
                            (import.module.to_owned(), import.name.to_owned())
                        })
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            },
        )
        .flatten()
        .collect::<Vec<_>>();
    let lifecycle_imports = parent_imports
        .iter()
        .filter(|(module, _)| module == "fe:worker-scope")
        .map(|(_, name)| name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let expected_lifecycle = artifact
        .structured_children
        .iter()
        .flat_map(|child| {
            [
                child.scope.spawn.as_str(),
                child.scope.failure.as_str(),
                child.scope.close.as_str(),
            ]
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(lifecycle_imports, expected_lifecycle);
    assert_eq!(lifecycle_imports.len(), 6);
    assert!(
        lifecycle_imports
            .iter()
            .all(|name| !matches!(*name, "spawn_begin" | "failure_begin" | "close"))
    );

    let package =
        materialize_scoped_task_package(&artifact.scoped_tasks, &artifact.structured_children)
            .expect("materialize two-child task package")
            .expect("two-child tasks require a package");
    let entry = package
        .files
        .iter()
        .find(|file| file.path == package.entry_path)
        .expect("two-child task entry");
    let entry = std::str::from_utf8(&entry.bytes).expect("UTF-8 task entry");
    assert!(entry.contains("createStructuredWorkerScopes"));
    assert!(entry.contains("createStructuredWorkerMailboxes"));
    assert!(!entry.contains("ScaleChild"));
    assert!(!entry.contains("OffsetChild"));
    assert!(!entry.contains("workerScope:"));

    let directory = tempfile::tempdir().expect("two-child execution directory");
    for file in &package.files {
        let path = directory.path().join(&file.path);
        std::fs::create_dir_all(path.parent().expect("package file parent")).unwrap();
        std::fs::write(path, &file.bytes).unwrap();
    }
    let parent_wasm = directory.path().join("parent.wasm");
    std::fs::write(&parent_wasm, &artifact.wasm).unwrap();
    let scale_index = artifact
        .structured_children
        .iter()
        .position(|child| child.actor == "ScaleChild")
        .expect("ScaleChild artifact");
    let offset_index = artifact
        .structured_children
        .iter()
        .position(|child| child.actor == "OffsetChild")
        .expect("OffsetChild artifact");

    let runner = directory.path().join("run-two-children.mjs");
    std::fs::write(
        &runner,
        format!(
            r#"
import {{
  createHostCompletionBroker,
  createMaterializedTaskRegistry,
  createStructuredWorkerMailboxes,
}} from "./tasks.js";
import {{ createInterfaceCaller as createCaller0 }} from "./children/{key0}/interface.js";
import {{ createInterfaceCaller as createCaller1 }} from "./children/{key1}/interface.js";

const parentBytes = await Bun.file({parent_wasm:?}).arrayBuffer();
const child0Bytes = await Bun.file(new URL("./children/{key0}/child.wasm", import.meta.url)).arrayBuffer();
const child1Bytes = await Bun.file(new URL("./children/{key1}/child.wasm", import.meta.url)).arrayBuffer();
const child0 = await WebAssembly.instantiate(child0Bytes, {{}});
const child1 = await WebAssembly.instantiate(child1Bytes, {{}});
const callers = [
  createCaller0(child0.instance.exports),
  createCaller1(child1.instance.exports),
];
const lifecycle = [[], []];
const requests = [];
const makeScope = (index) => Object.freeze({{
  async spawn(epoch) {{ lifecycle[index].push(["spawn", epoch]); }},
  failure(epoch, signal) {{
    lifecycle[index].push(["failure", epoch]);
    return new Promise((_, reject) => signal.addEventListener(
      "abort",
      () => reject(new DOMException("cancelled", "AbortError")),
      {{ once: true }},
    ));
  }},
  close(epoch) {{ lifecycle[index].push(["close", epoch]); }},
  request(lane, payload, signal) {{
    if (signal?.aborted) throw new DOMException("cancelled", "AbortError");
    requests.push([index, lane, payload.value]);
    return callers[index].call(lane, payload);
  }},
}});
const scopes = Object.freeze([
  Object.freeze({{ scope: makeScope(0), spawn: {spawn0:?}, failure: {failure0:?}, close: {close0:?} }}),
  Object.freeze({{ scope: makeScope(1), spawn: {spawn1:?}, failure: {failure1:?}, close: {close1:?} }}),
]);

let instance;
let residentValue;
const broker = createHostCompletionBroker({{
  workerScopes: scopes,
  actorEvents: {{
    send(event, signal) {{
      if (signal.aborted) throw new DOMException("cancelled", "AbortError");
      residentValue = instance.exports.fe_actor_transition_v1(...event);
    }},
  }},
}});
const mailboxImports = createStructuredWorkerMailboxes(scopes, broker.completions);
const imports = Object.assign({{}}, broker.imports, mailboxImports);
({{ instance }} = await WebAssembly.instantiate(parentBytes, imports));
const initialized = instance.exports.fe_actor_initialize_v1();
const initialValue = Array.isArray(initialized) ? initialized[0] : initialized;
if (initialValue !== 7) throw new Error(`bad two-child initial state ${{initialized}}`);
const tasks = createMaterializedTaskRegistry(instance.exports);
const calculated = await broker.run(tasks.calculate, [initialValue]);
if (calculated.length !== 1 || calculated[0] !== 19) {{
  throw new Error(`two-child Fe calculation drifted: ${{calculated}}`);
}}
if (residentValue !== 26) {{
  throw new Error(`two-child resident transition drifted: ${{residentValue}}`);
}}
if (requests.length !== 2 || requests[0][2] !== 7 || requests[1][2] !== 14
    || requests[0][0] !== {scale_index} || requests[1][0] !== {offset_index}) {{
  throw new Error(`typed child mailbox routing drifted: ${{JSON.stringify(requests)}}`);
}}
if (broker.activeCount() !== 0) throw new Error("two-child mailbox leaked completion tokens");

const cancelSupervisor = async (task, index) => {{
  const controller = new AbortController();
  const running = broker.run(task, [], {{ signal: controller.signal }});
  for (let attempt = 0; attempt < 100 && lifecycle[index].length < 2; attempt += 1) {{
    await new Promise(resolve => setTimeout(resolve, 0));
  }}
  if (JSON.stringify(lifecycle[index]) !== JSON.stringify([["spawn", 0], ["failure", 0]])) {{
    throw new Error(`wrong nominal child scope was selected: ${{JSON.stringify(lifecycle)}}`);
  }}
  controller.abort();
  let error;
  try {{ await running; }} catch (caught) {{ error = caught; }}
  if (error?.name !== "AbortError") throw new Error("scope cancellation was not terminal");
  if (JSON.stringify(lifecycle[index]) !== JSON.stringify([
    ["spawn", 0], ["failure", 0], ["close", 0],
  ])) {{
    throw new Error(`typed scope cleanup drifted: ${{JSON.stringify(lifecycle[index])}}`);
  }}
  if (broker.activeCount() !== 0) throw new Error("typed scope cancellation leaked tokens");
}};
await cancelSupervisor(tasks.supervise_scale, {scale_index});
await cancelSupervisor(tasks.supervise_offset, {offset_index});
if (lifecycle[0].length !== 3 || lifecycle[1].length !== 3) {{
  throw new Error(`one nominal scope absorbed another: ${{JSON.stringify(lifecycle)}}`);
}}
"#,
            key0 = child0.scope.key,
            key1 = child1.scope.key,
            parent_wasm = parent_wasm.display().to_string(),
            spawn0 = child0.scope.spawn,
            failure0 = child0.scope.failure,
            close0 = child0.scope.close,
            spawn1 = child1.scope.spawn,
            failure1 = child1.scope.failure,
            close1 = child1.scope.close,
            scale_index = scale_index,
            offset_index = offset_index,
        ),
    )
    .unwrap();
    let output = std::process::Command::new("bun")
        .arg("run")
        .arg(&runner)
        .output()
        .expect("run two-child package under Bun");
    assert!(
        output.status.success(),
        "two-child package failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn resident_actor_rejects_typed_mailbox_without_owning_child_scope() {
    let (db, file) = invalid_mailbox_fixture(false, false);
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unscoped mailbox fixture diagnostics:\n{diagnostics}"
    );
    let error = compile_resident_actor(&db, top_mod)
        .expect_err("a typed mailbox without an owning child scope must fail closed")
        .to_string();
    assert!(
        error.contains("typed child mailbox requires an owning structured-child scope"),
        "unexpected unscoped-mailbox diagnostic: {error}",
    );
}

#[test]
fn resident_actor_rejects_forged_handles_relation_without_child_behavior() {
    let (db, file) = invalid_mailbox_fixture(true, true);
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "forged mailbox fixture diagnostics:\n{diagnostics}"
    );
    let error = compile_resident_actor(&db, top_mod)
        .expect_err("a handwritten Handles impl must not invent a child endpoint")
        .to_string();
    assert!(
        error.contains("selects 0 Worker behaviors on `ArithmeticChild`; exactly one is required"),
        "unexpected forged-mailbox diagnostic: {error}",
    );
}

#[test]
fn resident_actor_rejects_scoped_task_with_partial_actor_state() {
    let (db, file) = partial_state_scoped_task_fixture();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "partial-state scoped-task fixture diagnostics:\n{diagnostics}"
    );

    let error = compile_resident_actor(&db, top_mod)
        .expect_err("a partial actor-state task input must fail closed")
        .to_string();
    assert!(
        error.contains(
            "scoped task `heartbeat` must be self-less or take self as exactly 2 flattened actor-state arguments; found 1"
        ),
        "unexpected partial-state diagnostic: {error}",
    );
}

#[test]
fn scoped_task_sends_typed_event_into_resident_actor_through_generated_continuation() {
    let (db, file) = actor_sink_fixture("Tick");
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "actor-sink fixture diagnostics:\n{diagnostics}"
    );

    let artifact = compile_resident_actor(&db, top_mod)
        .expect("resident actor sink contract")
        .expect("role-selected resident actor");
    assert_eq!(artifact.scoped_tasks.len(), 1);
    let imports = wasmparser::Parser::new(0)
        .parse_all(&artifact.wasm)
        .filter_map(|payload| match payload.expect("valid Wasm") {
            wasmparser::Payload::ImportSection(reader) => Some(
                reader
                    .into_imports()
                    .map(|import| {
                        let import = import.expect("valid import");
                        (import.module.to_owned(), import.name.to_owned())
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    assert!(imports.contains(&("fe:actor".to_owned(), "send_begin".to_owned())));

    if !std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }
    let directory = tempfile::tempdir().expect("actor-sink execution directory");
    let wasm_path = directory.path().join("actor.wasm");
    let adapter_path = directory.path().join("tasks.mjs");
    let task_runtime_path = directory.path().join("materialized-task.js");
    let host_runtime_path = directory.path().join("host-completion.js");
    std::fs::write(&wasm_path, &artifact.wasm).unwrap();
    std::fs::write(
        &adapter_path,
        emit_materialized_task_adapter_js(&artifact.scoped_tasks, "./materialized-task.js")
            .expect("emit actor task adapter")
            .expect("one actor task adapter"),
    )
    .unwrap();
    std::fs::write(&task_runtime_path, fe_codegen::MATERIALIZED_TASK_RUNTIME_JS).unwrap();
    std::fs::write(&host_runtime_path, fe_codegen::HOST_COMPLETION_RUNTIME_JS).unwrap();
    let runner = directory.path().join("run.mjs");
    std::fs::write(
        &runner,
        format!(
            r#"
import {{ createMaterializedTaskRegistry }} from {adapter:?};
import {{ createHostCompletionBroker }} from {host:?};
const accepted = [];
let instance;
const broker = createHostCompletionBroker({{
  actorEvents: {{
    send(event, signal) {{
      if (signal.aborted) throw new DOMException("stale actor scope", "AbortError");
      accepted.push(event);
      instance.exports.fe_actor_transition_v1(...event);
    }},
  }},
}});
const bytes = await Bun.file({wasm:?}).arrayBuffer();
({{ instance }} = await WebAssembly.instantiate(bytes, broker.imports));
const initial = instance.exports.fe_actor_initialize_v1();
if (initial[0] !== 2 || initial[1] !== 0) throw new Error(`bad initial state ${{initial}}`);
const machines = Object.values(createMaterializedTaskRegistry(instance.exports));
if (machines.length !== 1) throw new Error("expected one generated task");
const output = await broker.run(machines[0], []);
const projected = instance.exports.fe_actor_project_v1();
if (output.length !== 1 || output[0] !== 71) throw new Error(`bad task output ${{output}}`);
if (accepted.length !== 1 || accepted[0][0] !== 0 || accepted[0][1] !== 7) {{
  throw new Error(`bad opaque event ${{accepted}}`);
}}
if (projected[0] !== 9 || projected[1] !== 1) {{
  throw new Error(`resident reducer did not receive event: ${{projected}}`);
}}
if (broker.activeCount() !== 0) throw new Error("actor send leaked pending work");
"#,
            adapter = Url::from_file_path(&adapter_path).unwrap().to_string(),
            host = Url::from_file_path(&host_runtime_path).unwrap().to_string(),
            wasm = wasm_path.display().to_string(),
        ),
    )
    .unwrap();
    let output = std::process::Command::new("bun")
        .arg("run")
        .arg(&runner)
        .output()
        .expect("run actor-sink continuation under Bun");
    assert!(
        output.status.success(),
        "actor-sink continuation failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn resident_actor_rejects_scoped_sink_with_different_event_type() {
    let (db, file) = actor_sink_fixture("Other");
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "mismatched sink source diagnostics:\n{diagnostics}"
    );
    let error = compile_resident_actor(&db, top_mod)
        .expect_err("mismatched typed actor sink must fail closed")
        .to_string();
    assert!(
        error.contains("typed sink event differs from its resident transition"),
        "unexpected mismatched-sink diagnostic: {error}",
    );
}
