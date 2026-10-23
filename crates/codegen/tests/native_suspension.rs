#![cfg(all(
    feature = "cranelift",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use common::{InputDb, stdlib::HasBuiltinCore};
use driver::DriverDataBase;
use fe_codegen::{OptLevel, emit_native_scalar_task};
use hir::analysis::{
    semantic::{SemanticInstance, root_semantic_instance_key},
    ty::ty_check::BodyOwner,
};
use mir::{
    RuntimeInstanceKey,
    instance::{RuntimeInstanceSource, get_or_build_runtime_instance},
};
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
use url::Url;

const PREFIX: &str = r#"
use core::task::{Native, Suspend, TaskOutcome}
extern { unsafe fn native_task_probe() -> u64 }
extern { unsafe fn native_task_seed() -> u64 }
"#;
const SINGLE: &str = r#"
fn entry(_ native: own Native) -> u64 {
    var cap = native
    let _ = unsafe { native_task_probe() }
    let pending = cap.begin()
    let keep: u64 = unsafe { native_task_seed() }
    match cap.suspend(pending) {
        TaskOutcome::Success(value) => keep + value
        TaskOutcome::Failure(error) => error
        TaskOutcome::Cancelled => 0
    }
}
"#;
const REPEATED: &str = r#"
fn entry(_ native: own Native) -> u64 {
    var cap = native
    let first = cap.begin()
    let keep: u64 = 5
    let subtotal = match cap.suspend(first) {
        TaskOutcome::Success(value) => keep + value
        TaskOutcome::Failure(error) => error
        TaskOutcome::Cancelled => 0
    }
    let second = cap.begin()
    match cap.suspend(second) {
        TaskOutcome::Success(value) => subtotal + value
        TaskOutcome::Failure(error) => error
        TaskOutcome::Cancelled => 0
    }
}
"#;
const LOOP: &str = r#"
fn entry(_ native: own Native) -> u64 {
    var cap = native
    var total: u64 = 5
    while total < 23 {
        let pending = cap.begin()
        total = match cap.suspend(pending) {
            TaskOutcome::Success(value) => total + value
            TaskOutcome::Failure(error) => error
            TaskOutcome::Cancelled => 23
        }
    }
    total
}
"#;
const STICKY: &str = r#"
fn entry(_ native: own Native) -> u64 {
    var cap = native
    let first = cap.begin()
    match cap.suspend(first) {
        TaskOutcome::Success(value) => value
        TaskOutcome::Failure(error) => error
        TaskOutcome::Cancelled => {
            let cleanup = cap.begin()
            let marker: u64 = match cap.suspend(cleanup) {
                TaskOutcome::Success(_) => 100
                TaskOutcome::Failure(_) => 100
                TaskOutcome::Cancelled => 0
            }
            marker + 42
        }
    }
}
"#;

fn execute(name: &str, source: &str, checks: &str, invalid_tag: bool) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::path::PathBuf::from(option_env!("CARGO_TARGET_TMPDIR").unwrap_or("/tmp")).join(
        format!(
            "fe-native-task-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ),
    );
    fs::create_dir_all(&dir).unwrap();
    let mut db = DriverDataBase::default();
    db.initialize_builtin_core();
    let file = db.workspace().touch(
        &mut db,
        Url::parse(&format!("file:///{name}.fe")).unwrap(),
        Some(format!("{PREFIX}\n{source}")),
    );
    let module = db.top_mod(file);
    let entry = module
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|func| {
            func.name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "entry")
        })
        .unwrap();
    let semantic = SemanticInstance::new(
        &db,
        root_semantic_instance_key(&db, BodyOwner::Func(entry, None)).unwrap(),
    );
    let instance = get_or_build_runtime_instance(
        &db,
        RuntimeInstanceKey::new(&db, RuntimeInstanceSource::Semantic(semantic), vec![]),
    );
    let artifact = emit_native_scalar_task(&db, module, instance, OptLevel::O0)
        .expect("emit actual native scalar task");
    assert!(
        !artifact.ir.contains("native_suspend"),
        "raw cut must not survive emission"
    );
    fs::write(dir.join("task.o"), &artifact.object).unwrap();
    fs::write(dir.join("task.snt"), &artifact.ir).unwrap();
    let declarations = artifact
        .symbols
        .iter()
        .map(|name| format!("fn {name}(frame: *mut u64, kind:u64,payload:u64)->u64;"))
        .collect::<String>();
    let entries = artifact.symbols.join(",");
    let runtime = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../native-runtime/src/lib.rs")
        .canonicalize()
        .unwrap();
    let driver = format!(
        r#"
#[path = {runtime:?}] mod runtime;
use runtime::{{NativeCode,Task,Delivery,Progress,Ticket,Error}};
use std::{{rc::Rc,cell::Cell}};
unsafe extern "C" {{ {declarations} }}
#[derive(Clone,Copy)] enum Probe {{ Queue(*const Task<()>,Ticket), Cancel(*const Task<()>) }}
thread_local! {{ static PROBE: Cell<Option<Probe>> = const {{ Cell::new(None) }}; }}
thread_local! {{ static SEED: Cell<u64> = const {{ Cell::new(5) }}; }}
#[unsafe(no_mangle)] extern "C" fn native_task_seed()->u64 {{ SEED.get() }}
#[unsafe(no_mangle)] extern "C" fn native_task_probe()->u64 {{
    if let Some(probe)=PROBE.get() {{
        match probe {{
            Probe::Queue(task,ticket)=>{{
                let task=unsafe{{&*task}};
                assert_eq!(task.deliver(ticket,Delivery::Success(9)),Ok(()));
                assert_eq!(task.poll(),Err(Error::Reentrant));
            }}
            Probe::Cancel(task)=>{{ assert!(unsafe{{&*task}}.cancel()); }}
        }}
        PROBE.set(None);
    }}
    0
}}
fn waiting(task:&Task<()>)->Ticket {{ match task.poll().unwrap() {{ Progress::Suspended(ticket)=>ticket, other=>panic!("expected suspension: {{other:?}}") }} }}
fn done(task:&Task<()>,value:u64,cancelled:bool) {{
    assert_eq!(task.poll(),Ok(Progress::Complete{{value,cancelled}}));
    assert!(!task.retains_frame()); assert_eq!(task.registrations(),0);
}}
fn main() {{
    let entries:Vec<runtime::Segment>=vec![{entries}];
    if std::env::args().any(|arg|arg=="invalid") {{
        let mut frame=vec![0u64;{words}];
        unsafe{{entries[1](frame.as_mut_ptr(),99,0);}}
        std::process::exit(0);
    }}
    assert!(matches!(unsafe{{NativeCode::new(vec![],0,())}},Err(Error::InvalidArtifact)));
    assert_eq!(Delivery::from_wire(99,0),Err(Error::InvalidDelivery));
    struct Guard(Rc<Cell<usize>>);
    impl Drop for Guard {{ fn drop(&mut self) {{ self.0.set(self.0.get()+1); }} }}
    let released=Rc::new(Cell::new(0));
    let held_code=Rc::new(unsafe{{NativeCode::new(entries.clone(),{words},Guard(released.clone()))}}.unwrap());
    let held=Task::new(held_code.clone()).unwrap();
    drop(held_code); assert_eq!(released.get(),0);
    assert!(matches!(held.poll(),Ok(Progress::Suspended(_))));
    drop(held); assert_eq!(released.get(),1);
    let code=Rc::new(unsafe{{NativeCode::new(entries,{words},())}}.unwrap());
    {checks}
}}
"#,
        runtime = runtime,
        declarations = declarations,
        entries = entries,
        words = artifact.frame_words,
        checks = checks
    );
    fs::write(dir.join("driver.rs"), driver).unwrap();
    let compiler = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let built = Command::new(compiler)
        .arg("--edition=2024")
        .arg(dir.join("driver.rs"))
        .arg("-C")
        .arg(format!("link-arg={}", dir.join("task.o").display()))
        .arg("-o")
        .arg(dir.join("run"))
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "native driver link failed in {}:\n{}",
        dir.display(),
        String::from_utf8_lossy(&built.stderr)
    );
    let result = Command::new(dir.join("run")).output().unwrap();
    assert!(
        result.status.success(),
        "native execution failed in {}:\n{}",
        dir.display(),
        String::from_utf8_lossy(&result.stderr)
    );
    if invalid_tag {
        assert!(
            !Command::new(dir.join("run"))
                .arg("invalid")
                .status()
                .unwrap()
                .success(),
            "invalid wire tag must trap"
        );
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn emitted_scalar_outcomes_and_delivery_custody() {
    execute(
        "single",
        SINGLE,
        r#"
let task=Task::new(code.clone()).unwrap(); let ticket=waiting(&task);
SEED.set(99);
assert_eq!(task.deliver(ticket,Delivery::Success(9)),Ok(()));
assert_eq!(task.deliver(ticket,Delivery::Success(9)),Err(Error::Duplicate));
done(&task,14,false);
SEED.set(5);
assert_eq!(task.deliver(ticket,Delivery::Success(9)),Err(Error::Duplicate));
let failure=Task::new(code.clone()).unwrap(); let f=waiting(&failure);
assert_eq!(failure.deliver(ticket,Delivery::Success(9)),Err(Error::Foreign));
failure.deliver(f,Delivery::Failure(3)).unwrap(); done(&failure,3,false);
let cancel=Task::new(code.clone()).unwrap(); let c=waiting(&cancel);
cancel.deliver(c,Delivery::Success(9)).unwrap(); assert!(cancel.cancel());
done(&cancel,0,true); assert!(!cancel.cancel());
let target=Task::new(code.clone()).unwrap(); let t=waiting(&target);
PROBE.set(Some(Probe::Queue(&target,t)));
let trigger=Task::new(code.clone()).unwrap(); let _=waiting(&trigger);
assert!(PROBE.get().is_none()); done(&target,14,false);
let running_cancel=Task::new(code.clone()).unwrap();
PROBE.set(Some(Probe::Cancel(&running_cancel)));
assert_eq!(running_cancel.poll(),Ok(Progress::Cancelling));
assert!(running_cancel.retains_frame()); done(&running_cancel,0,true);
"#,
        true,
    );
}

#[test]
fn emitted_sequential_and_loop_cuts_reject_stale_tickets() {
    let checks = r#"
let task=Task::new(code.clone()).unwrap(); let first=waiting(&task);
task.deliver(first,Delivery::Success(9)).unwrap(); let second=waiting(&task);
assert_eq!(task.deliver(first,Delivery::Success(9)),Err(Error::Stale));
task.deliver(second,Delivery::Success(9)).unwrap(); done(&task,23,false);
"#;
    execute("sequential", REPEATED, checks, false);
    execute("loop", LOOP, checks, false);
}

#[test]
fn emitted_sticky_cancellation_runs_code_after_a_second_cut() {
    execute(
        "sticky",
        STICKY,
        r#"
let task=Task::new(code.clone()).unwrap(); let ticket=waiting(&task);
assert!(task.cancel());
assert_eq!(task.poll(),Ok(Progress::Cancelling));
assert!(task.retains_frame()); assert_eq!(task.registrations(),0);
assert_eq!(task.deliver(ticket,Delivery::Success(9)),Err(Error::Stale));
done(&task,42,true);
"#,
        false,
    );
}
