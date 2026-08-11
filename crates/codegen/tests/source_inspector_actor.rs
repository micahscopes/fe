//! Independent semantic acceptance for the resident Fe SourceInspector.
//! Wasmtime executes authored Fe over lifecycle, selection, stale completion,
//! text/binary success, error, and cancellation events. A separate Rust model
//! derives state and decoded effects; artifact byte equality is not a behavior
//! oracle.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    RESIDENT_ACTOR_INITIALIZE_EXPORT, RESIDENT_ACTOR_PROJECT_EXPORT,
    RESIDENT_ACTOR_TRANSITION_EXPORT, compile_resident_actor,
};
use hir::hir_def::HirIngot;
use url::Url;

type ActorState = (
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Model {
    url: String,
    selected: u32,
    pending: u32,
    status: u32,
    byte_len: u32,
    content: String,
    connected: bool,
    open: bool,
    issue_load: bool,
    prevent_default: bool,
    focus_close: bool,
    revision: u32,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            url: String::new(),
            selected: 0,
            pending: 0,
            status: 0,
            byte_len: 0,
            content: String::new(),
            connected: false,
            open: false,
            issue_load: false,
            prevent_default: false,
            focus_close: false,
            revision: 0,
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

fn reduce(model: &mut Model, event: Event<'_>) {
    model.issue_load = false;
    model.prevent_default = false;
    model.focus_close = false;
    match event.kind {
        0 => {
            model.connected = true;
            model.revision += 1;
        }
        1 => {
            model.connected = false;
            model.open = false;
            model.revision += 1;
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
            model.issue_load = true;
            model.prevent_default = true;
            model.focus_close = true;
        }
        3 if model.connected && event.target == 5 && model.open => {
            model.open = false;
            model.prevent_default = true;
            model.revision += 1;
        }
        7 if model.connected && model.open && event.detail == 27 => {
            model.open = false;
            model.prevent_default = true;
            model.revision += 1;
        }
        8 if model.connected && model.open && event.request == model.pending => {
            model.byte_len = event.detail;
            model.content = event.text.to_owned();
            model.status = u32::from(!(200..300).contains(&event.key)) + 1;
            model.revision += 1;
        }
        _ => {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Operation {
    Text(u32, String),
    Href(u32, String),
    LoadText(u32, String),
    LoadBytes(u32, String),
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
        let id = word(bytes, &mut cursor);
        let value = text(bytes, &mut cursor);
        operations.push(match opcode {
            4 => Operation::Text(id, value),
            11 => Operation::Href(id, value),
            12 => Operation::LoadText(id, value),
            13 => Operation::LoadBytes(id, value),
            _ => panic!("unexpected SourceInspector opcode {opcode}"),
        });
    }
    operations
}

fn expected_projection(model: &Model) -> (u32, u32, u32, Vec<Operation>) {
    let mut mask = 0;
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
    if model.status == 1 {
        if model.selected == 2 {
            operations.push(Operation::Text(10, model.byte_len.to_string()));
        } else {
            operations.push(Operation::Text(9, model.content.clone()));
        }
    } else if model.status == 2 {
        operations.push(Operation::Text(12, model.content.clone()));
    }
    if model.issue_load {
        operations.push(if model.selected == 2 {
            Operation::LoadBytes(model.pending, model.url.clone())
        } else {
            Operation::LoadText(model.pending, model.url.clone())
        });
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
    assert_eq!(artifact.contract.event_leaf_count, 9);
    assert_eq!(artifact.contract.state_leaf_count, 15);

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &artifact.wasm).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    let initialize = instance
        .get_typed_func::<(), ActorState>(&mut store, RESIDENT_ACTOR_INITIALIZE_EXPORT)
        .unwrap();
    let transition = instance
        .get_typed_func::<(i32, i32, i32, i32, i32, f32, f32, i32, i32), ActorState>(
            &mut store,
            RESIDENT_ACTOR_TRANSITION_EXPORT,
        )
        .unwrap();
    let project = instance
        .get_typed_func::<(), (i32, i32, i32, i32, i32)>(&mut store, RESIDENT_ACTOR_PROJECT_EXPORT)
        .unwrap();
    let alloc = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .unwrap();
    let initial = initialize.call(&mut store, ()).unwrap();
    let pointers = (initial.0, initial.1, initial.7);
    let scratch = alloc.call(&mut store, (4096, 1)).unwrap();
    let mut model = Model::default();

    let tape = [
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
        Event {
            kind: 8,
            target: 0,
            request: 7,
            key: 200,
            detail: 2,
            text: "ok",
        },
    ];
    for (index, event) in tape.into_iter().enumerate() {
        memory
            .write(&mut store, scratch as usize, event.text.as_bytes())
            .unwrap();
        reduce(&mut model, event);
        let actual = transition
            .call(
                &mut store,
                (
                    event.kind as i32,
                    event.target as i32,
                    event.request as i32,
                    event.key as i32,
                    event.detail as i32,
                    0.0,
                    index as f32,
                    if event.text.is_empty() { 0 } else { scratch },
                    event.text.len() as i32,
                ),
            )
            .unwrap_or_else(|error| panic!("SourceInspector event {index}: {error}"));
        assert_eq!(
            (actual.0, actual.1, actual.7),
            pointers,
            "persistent pointers at {index}"
        );
        assert_eq!(
            actual.2 as u32,
            model.url.len() as u32,
            "url length at {index}"
        );
        assert_eq!(actual.3 as u32, model.selected, "selection at {index}");
        assert_eq!(actual.4 as u32, model.pending, "pending request at {index}");
        assert_eq!(actual.5 as u32, model.status, "status at {index}");
        assert_eq!(actual.6 as u32, model.byte_len, "byte length at {index}");
        assert_eq!(
            actual.8 as u32,
            model.content.len() as u32,
            "content length at {index}"
        );
        assert_eq!(actual.9 != 0, model.connected, "connected at {index}");
        assert_eq!(actual.10 != 0, model.open, "open at {index}");
        assert_eq!(actual.11 != 0, model.issue_load, "load effect at {index}");
        assert_eq!(
            actual.12 != 0,
            model.prevent_default,
            "prevent default at {index}"
        );
        assert_eq!(actual.13 != 0, model.focus_close, "focus at {index}");
        assert_eq!(actual.14 as u32, model.revision, "revision at {index}");

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
        transition
            .call(&mut store, (3, 99, 0, 0, 0, 0.0, 0.0, 0, 0))
            .is_err(),
        "invalid FCO-derived InspectorAction must trap before reaching Fe"
    );
}
