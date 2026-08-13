//! Acceptance for the target-neutral resident actor contract and its first Fe
//! web-component consumer. Correctness comes from executing authored Fe over a
//! lifecycle/input tape and comparing every state to an independent reducer;
//! source or artifact byte equality is not used as behavioral evidence.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    CanonicalType, RESIDENT_ACTOR_INITIALIZE_EXPORT, RESIDENT_ACTOR_PROJECT_EXPORT,
    RESIDENT_ACTOR_STATE_REPLACE_EXPORT, RESIDENT_ACTOR_TRANSITION_EXPORT, compile_resident_actor,
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
