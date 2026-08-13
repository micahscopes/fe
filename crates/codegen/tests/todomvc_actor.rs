//! Behavioral acceptance for the Fe TodoMVC component. The Wasm actor is
//! executed over lifecycle, UTF-8, editing, filtering, toggle, destroy, and
//! clear tapes. Every resident state and decoded structural projection is
//! compared to an independently written Rust model; artifact byte equality is
//! deliberately not used as a correctness oracle.

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
);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Todo {
    id: u32,
    title: String,
    completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Model {
    todos: Vec<Todo>,
    next_id: u32,
    filter: u32,
    editing: u32,
    draft: String,
    connected: bool,
    prevent_default: bool,
    clear_input: bool,
    focus_edit: bool,
    revision: u32,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            todos: vec![
                Todo {
                    id: 1,
                    title: "strings".to_owned(),
                    completed: false,
                },
                Todo {
                    id: 2,
                    title: "review code".to_owned(),
                    completed: false,
                },
                Todo {
                    id: 3,
                    title: "much much more".to_owned(),
                    completed: false,
                },
            ],
            next_id: 4,
            filter: 0,
            editing: 0,
            draft: String::new(),
            connected: false,
            prevent_default: false,
            clear_input: false,
            focus_edit: false,
            revision: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Event<'a> {
    kind: u32,
    target: u32,
    key: u32,
    detail: u32,
    text: &'a str,
}

fn bounded_utf8(value: &str) -> String {
    let mut end = value.len().min(96);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn reduce(model: &mut Model, event: Event<'_>) {
    model.prevent_default = false;
    model.clear_input = false;
    model.focus_edit = false;
    match event.kind {
        0 => {
            model.connected = true;
            model.revision += 1;
        }
        1 => {
            model.connected = false;
            model.revision += 1;
        }
        3 if model.connected => {
            match event.target {
                3 => model.todos.retain(|todo| !todo.completed),
                4 => model.filter = 0,
                5 => model.filter = 1,
                6 => model.filter = 2,
                9 => {
                    model.todos.retain(|todo| todo.id != event.key);
                    if model.editing == event.key {
                        model.editing = 0;
                    }
                }
                8 | 11 if event.target == 11 || event.detail >= 2 => {
                    if let Some(todo) = model.todos.iter().find(|todo| todo.id == event.key) {
                        model.draft = todo.title.clone();
                        model.editing = event.key;
                        model.focus_edit = true;
                    }
                }
                _ => {}
            }
            model.revision += 1;
        }
        5 if model.connected => {
            if event.target == 2 {
                let make_completed = model.todos.iter().any(|todo| !todo.completed);
                for todo in &mut model.todos {
                    todo.completed = make_completed;
                }
            } else if event.target == 7
                && let Some(todo) = model.todos.iter_mut().find(|todo| todo.id == event.key)
            {
                todo.completed = !todo.completed;
            }
            model.revision += 1;
        }
        4 if model.connected && event.target == 10 && event.key == model.editing => {
            model.draft = bounded_utf8(event.text);
            model.revision += 1;
        }
        7 if model.connected => {
            if event.target == 1 && event.detail == 13 {
                let title = event.text.trim_matches([' ', '\t', '\n', '\r']);
                if !title.is_empty() && model.todos.len() < 32 {
                    model.todos.push(Todo {
                        id: model.next_id,
                        title: bounded_utf8(title),
                        completed: false,
                    });
                    model.next_id += 1;
                    model.clear_input = true;
                }
                model.prevent_default = true;
            } else if event.target == 10 && event.key == model.editing {
                if event.detail == 13 {
                    let title = model.draft.trim_matches([' ', '\t', '\n', '\r']).to_owned();
                    if title.is_empty() {
                        model.todos.retain(|todo| todo.id != event.key);
                    } else if let Some(todo) =
                        model.todos.iter_mut().find(|todo| todo.id == event.key)
                    {
                        todo.title = title;
                    }
                    model.editing = 0;
                    model.prevent_default = true;
                } else if event.detail == 27 {
                    model.editing = 0;
                    model.prevent_default = true;
                }
            }
            model.revision += 1;
        }
        _ => {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Operation {
    RepeatBegin(u32, u32),
    RepeatItem(u32),
    RepeatEnd,
    Text(u32, String),
    Value(u32, String),
    Checked(u32, bool),
    Hidden(u32, bool),
    Class(u32, u32, bool),
    Focus(u32),
    Disabled(u32, bool),
}

fn decode_operations(bytes: &[u8]) -> Vec<Operation> {
    fn byte(bytes: &[u8], cursor: &mut usize) -> u8 {
        let value = *bytes.get(*cursor).expect("command byte");
        *cursor += 1;
        value
    }
    fn word(bytes: &[u8], cursor: &mut usize) -> u32 {
        let end = *cursor + 4;
        let value = u32::from_le_bytes(
            bytes[*cursor..end]
                .try_into()
                .expect("complete command word"),
        );
        *cursor = end;
        value
    }
    fn text(bytes: &[u8], cursor: &mut usize) -> String {
        let len = word(bytes, cursor) as usize;
        let end = *cursor + len;
        let value = std::str::from_utf8(&bytes[*cursor..end])
            .expect("Fe command text is UTF-8")
            .to_owned();
        *cursor = end;
        value
    }
    let mut cursor = 0;
    let mut operations = Vec::new();
    while cursor < bytes.len() {
        operations.push(match byte(bytes, &mut cursor) {
            1 => Operation::RepeatBegin(word(bytes, &mut cursor), word(bytes, &mut cursor)),
            2 => Operation::RepeatItem(word(bytes, &mut cursor)),
            3 => Operation::RepeatEnd,
            4 => {
                let target = word(bytes, &mut cursor);
                Operation::Text(target, text(bytes, &mut cursor))
            }
            5 => {
                let target = word(bytes, &mut cursor);
                Operation::Value(target, text(bytes, &mut cursor))
            }
            6 => Operation::Checked(word(bytes, &mut cursor), byte(bytes, &mut cursor) != 0),
            7 => Operation::Hidden(word(bytes, &mut cursor), byte(bytes, &mut cursor) != 0),
            8 => Operation::Class(
                word(bytes, &mut cursor),
                word(bytes, &mut cursor),
                byte(bytes, &mut cursor) != 0,
            ),
            9 => Operation::Focus(word(bytes, &mut cursor)),
            10 => Operation::Disabled(word(bytes, &mut cursor), byte(bytes, &mut cursor) != 0),
            opcode => panic!("unknown Fe command opcode {opcode}"),
        });
    }
    operations
}

fn expected_operations(model: &Model) -> Vec<Operation> {
    let active = model.todos.iter().filter(|todo| !todo.completed).count() as u32;
    let mut operations = vec![
        Operation::Hidden(1, model.todos.is_empty()),
        Operation::Hidden(2, model.todos.is_empty()),
        Operation::Text(4, active.to_string()),
        Operation::Checked(6, !model.todos.is_empty() && active == 0),
        Operation::Class(7, 13, model.filter == 0),
        Operation::Class(8, 13, model.filter == 1),
        Operation::Class(9, 13, model.filter == 2),
    ];
    if model.clear_input {
        operations.push(Operation::Value(5, String::new()));
    }
    operations.push(Operation::RepeatBegin(3, 3));
    for todo in &model.todos {
        let visible = match model.filter {
            1 => !todo.completed,
            2 => todo.completed,
            _ => true,
        };
        if !visible {
            continue;
        }
        operations.extend([
            Operation::RepeatItem(todo.id),
            Operation::Text(11, todo.title.clone()),
            Operation::Checked(10, todo.completed),
            Operation::Class(0, 14, todo.completed),
            Operation::Class(0, 15, todo.id == model.editing),
        ]);
        if todo.id == model.editing {
            operations.push(Operation::Value(12, model.draft.clone()));
            if model.focus_edit {
                operations.push(Operation::Focus(12));
            }
        }
    }
    operations.push(Operation::RepeatEnd);
    operations
}

fn assert_state(actual: ActorState, pointers: (i32, i32, i32), model: &Model) {
    assert_eq!((actual.0, actual.1, actual.2), pointers);
    assert_eq!(actual.3 as usize, model.todos.len(), "todo count");
    assert_eq!(actual.4 as u32, model.next_id, "next stable key");
    assert_eq!(actual.5 as u32, model.filter, "filter");
    assert_eq!(actual.6 as u32, model.editing, "editing key");
    assert_eq!(actual.7 as usize, model.draft.len(), "draft UTF-8 length");
    assert_eq!(actual.8 != 0, model.connected, "connected lifecycle state");
    assert_eq!(
        actual.9 != 0,
        model.prevent_default,
        "prevent-default effect"
    );
    assert_eq!(actual.10 != 0, model.clear_input, "new-input clear effect");
    assert_eq!(actual.11 != 0, model.focus_edit, "edit focus effect");
    assert_eq!(actual.12 as u32, model.revision, "revision");
}

#[test]
fn todomvc_reducer_utf8_keyed_projection_and_lifecycle_are_fe_owned() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../demos/sketches/todomvc")
        .canonicalize()
        .expect("TodoMVC ingot path");
    let url = Url::from_directory_path(path).expect("TodoMVC URL");
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url), "TodoMVC diagnostics");
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("TodoMVC ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "TodoMVC diagnostics:\n{diagnostics}"
    );
    let artifact = compile_resident_actor(&db, top_mod)
        .expect("TodoMVC resident contract")
        .expect("TodoMVC resident actor");
    assert_eq!(artifact.contract.actor, "TodoApp");
    assert_eq!(artifact.contract.event_leaf_count, 9);
    assert_eq!(artifact.contract.state_leaf_count, 13);
    assert_eq!(artifact.contract.projection_leaf_count, 5);

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &artifact.wasm).expect("TodoMVC Wasm");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("TodoMVC instance");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("TodoMVC memory");
    let initialize = instance
        .get_typed_func::<(), ActorState>(&mut store, RESIDENT_ACTOR_INITIALIZE_EXPORT)
        .expect("TodoMVC initializer");
    let transition = instance
        .get_typed_func::<(i32, i32, i32, i32, i32, f32, f32, i32, i32), ActorState>(
            &mut store,
            RESIDENT_ACTOR_TRANSITION_EXPORT,
        )
        .expect("TodoMVC transition");
    let project = instance
        .get_typed_func::<(), (i32, i32, i32, i32, i32)>(&mut store, RESIDENT_ACTOR_PROJECT_EXPORT)
        .expect("TodoMVC projection");
    let alloc = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .expect("canonical input allocator");

    let initial = initialize.call(&mut store, ()).expect("Fe initialization");
    let pointers = (initial.0, initial.1, initial.2);
    assert!(pointers.0 > 0 && pointers.1 > pointers.0 && pointers.2 > pointers.1);
    let scratch = alloc
        .call(&mut store, (4096, 1))
        .expect("one reusable input scratch");
    let mut model = Model::default();
    assert_state(initial, pointers, &model);

    const UTF8_BOUNDARY_TITLE: &str = concat!(
        "aaaaaaaaaa",
        "aaaaaaaaaa",
        "aaaaaaaaaa",
        "aaaaaaaaaa",
        "aaaaaaaaaa",
        "aaaaaaaaaa",
        "aaaaaaaaaa",
        "aaaaaaaaaa",
        "aaaaaaaaaa",
        "aaaaa",
        "🌍",
    );
    assert_eq!(UTF8_BOUNDARY_TITLE.as_bytes().len(), 99);

    let tape = [
        Event {
            kind: 0,
            target: 0,
            key: 0,
            detail: 0,
            text: "",
        },
        Event {
            kind: 7,
            target: 1,
            key: 0,
            detail: 13,
            text: "  alpha  ",
        },
        Event {
            kind: 7,
            target: 1,
            key: 0,
            detail: 13,
            text: "βeta 🌍",
        },
        Event {
            kind: 7,
            target: 1,
            key: 0,
            detail: 13,
            text: UTF8_BOUNDARY_TITLE,
        },
        Event {
            kind: 5,
            target: 7,
            key: 1,
            detail: 0,
            text: "on",
        },
        Event {
            kind: 3,
            target: 5,
            key: 0,
            detail: 1,
            text: "",
        },
        Event {
            kind: 3,
            target: 4,
            key: 0,
            detail: 1,
            text: "",
        },
        Event {
            kind: 3,
            target: 11,
            key: 2,
            detail: 1,
            text: "",
        },
        Event {
            kind: 4,
            target: 10,
            key: 2,
            detail: 0,
            text: " gamma ",
        },
        Event {
            kind: 7,
            target: 10,
            key: 2,
            detail: 13,
            text: " gamma ",
        },
        Event {
            kind: 5,
            target: 2,
            key: 0,
            detail: 0,
            text: "on",
        },
        Event {
            kind: 5,
            target: 2,
            key: 0,
            detail: 0,
            text: "",
        },
        Event {
            kind: 3,
            target: 6,
            key: 0,
            detail: 1,
            text: "",
        },
        Event {
            kind: 3,
            target: 9,
            key: 1,
            detail: 1,
            text: "",
        },
        Event {
            kind: 3,
            target: 3,
            key: 0,
            detail: 1,
            text: "",
        },
        Event {
            kind: 1,
            target: 0,
            key: 0,
            detail: 0,
            text: "",
        },
        Event {
            kind: 7,
            target: 1,
            key: 0,
            detail: 13,
            text: "ignored",
        },
    ];
    for (index, event) in tape.into_iter().enumerate() {
        let bytes = event.text.as_bytes();
        memory
            .write(&mut store, scratch as usize, bytes)
            .expect("write event UTF-8");
        reduce(&mut model, event);
        let actual = transition
            .call(
                &mut store,
                (
                    event.kind as i32,
                    event.target as i32,
                    0,
                    event.key as i32,
                    event.detail as i32,
                    0.0,
                    index as f32,
                    if bytes.is_empty() { 0 } else { scratch },
                    bytes.len() as i32,
                ),
            )
            .unwrap_or_else(|error| panic!("TodoMVC event {index} trapped: {error}"));
        assert_state(actual, pointers, &model);

        let patch = project
            .call(&mut store, ())
            .unwrap_or_else(|error| panic!("TodoMVC projection {index} trapped: {error}"));
        assert_eq!((patch.0, patch.1), (0, 0));
        assert_eq!(patch.2 != 0, model.prevent_default);
        let mut commands = vec![0; patch.4 as usize];
        memory
            .read(&store, patch.3 as usize, &mut commands)
            .expect("read Fe command stream");
        assert_eq!(
            decode_operations(&commands),
            expected_operations(&model),
            "decoded Fe projection differs after event {index}"
        );
    }

    assert!(
        transition
            .call(&mut store, (99, 0, 0, 0, 0, 0.0, 0.0, 0, 0))
            .is_err(),
        "invalid ComponentEventKind must trap before reaching Fe"
    );
    assert!(
        transition
            .call(&mut store, (3, 99, 0, 0, 0, 0.0, 0.0, 0, 0))
            .is_err(),
        "invalid FCO-derived TodoAction must trap before reaching Fe"
    );
}
