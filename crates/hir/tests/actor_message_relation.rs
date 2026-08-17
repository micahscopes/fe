use fe_hir::test_db::{HirAnalysisTestDb, format_diagnostics};

const ACTOR_WITH_SHARED_REQUEST: &str = r#"
use core::actor::Handles

struct Request { value: u32 }
struct ResponseA { value: u32 }
struct ResponseB { value: u32 }
struct WrongResponse { value: u32 }

actor Child {
    fn first(_ request: Request) -> ResponseA {
        ResponseA { value: request.value }
    }

    fn second(_ request: Request) -> ResponseB {
        ResponseB { value: request.value }
    }
}
"#;

#[test]
fn actor_behaviors_derive_request_response_relations_without_selectors() {
    let mut db = HirAnalysisTestDb::default();
    let source = format!(
        r#"
{ACTOR_WITH_SHARED_REQUEST}

fn accepts_a<T: Handles<Request, ResponseA>>(_ child: T) {{}}
fn accepts_b<T: Handles<Request, ResponseB>>(_ child: T) {{}}

fn proves_a() {{ accepts_a(Child {{}}) }}
fn proves_b() {{ accepts_b(Child {{}}) }}
"#
    );
    let file = db.new_stand_alone("actor_message_relation.fe".into(), &source);
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

#[test]
fn actor_behaviors_do_not_invent_unwritten_response_relations() {
    let mut db = HirAnalysisTestDb::default();
    let source = format!(
        r#"
{ACTOR_WITH_SHARED_REQUEST}

fn accepts_wrong<T: Handles<Request, WrongResponse>>(_ child: T) {{}}
fn rejects_wrong() {{ accepts_wrong(Child {{}}) }}
"#
    );
    let file = db.new_stand_alone("actor_message_relation_negative.fe".into(), &source);
    let (top_mod, _) = db.top_mod(file);
    let rendered = format_diagnostics(&db, &db.run_on_top_mod(top_mod));
    assert!(
        rendered.contains("trait bound is not satisfied")
            && rendered.contains("Handles<Request, WrongResponse>"),
        "expected the unwritten response relation to be rejected:\n{rendered}"
    );
}

#[test]
fn browser_mailbox_provider_preserves_child_request_and_response_types() {
    let mut db = HirAnalysisTestDb::default();
    let source = format!(
        r#"
{ACTOR_WITH_SHARED_REQUEST}

use core::actor::ActorMailbox
use core::pending::Pending
use std::runtime::BrowserActorMailbox
use std::wasm::WasmBackend

fn ask_a(_ request: own Request) -> Pending<WasmBackend, ResponseA>
    uses (mailbox: mut ActorMailbox<WasmBackend, Child>)
{{
    mailbox.ask(request)
}}

fn select_browser_mailbox(_ request: own Request) -> Pending<WasmBackend, ResponseA> {{
    with (
        ActorMailbox<WasmBackend, Child> = BrowserActorMailbox<Child> {{}},
    ) {{
        ask_a(request)
    }}
}}
"#
    );
    let file = db.new_stand_alone("browser_actor_mailbox.fe".into(), &source);
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}
