//! Single-threaded custody for emitted native scalar task segments.
//! Cancellation is sticky and cooperative. Each poll executes at most one
//! segment; readiness callbacks enqueue delivery and never execute a task.
//! `Broker` owns several tasks, fixes the order in which they run, and owns
//! the virtual clock that completes sleeps.

mod broker;
pub use broker::{Broker, Step, TaskId};

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

pub type Segment = unsafe extern "C" fn(*mut u64, u64, u64) -> u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidArtifact,
    IdentityExhausted,
    Foreign,
    Stale,
    Duplicate,
    Reentrant,
    InvalidPending,
    InvalidStep,
    InvalidDelivery,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Failure(u64),
    Success(u64),
    Cancelled,
}
impl Delivery {
    pub fn from_wire(kind: u64, payload: u64) -> Result<Self, Error> {
        match kind {
            0 => Ok(Self::Failure(payload)),
            1 => Ok(Self::Success(payload)),
            2 => Ok(Self::Cancelled),
            _ => Err(Error::InvalidDelivery),
        }
    }
    fn wire(self) -> (u64, u64) {
        match self {
            Self::Failure(value) => (0, value),
            Self::Success(value) => (1, value),
            Self::Cancelled => (2, 0),
        }
    }
}

/// An unforgeable safe-Rust delivery ticket. Copying a ticket does not copy
/// authority: only its first accepted delivery can claim the suspension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket {
    owner: u64,
    generation: u64,
    pending: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Suspended(Ticket),
    Cancelling,
    Complete { value: u64, cancelled: bool },
}

/// Retains the code loader's lifetime guard beside its function pointers.
pub struct NativeCode<G> {
    entries: Vec<Segment>,
    words: usize,
    _guard: G,
}
impl<G> NativeCode<G> {
    /// # Safety
    /// The entries must be a single emitted scalar task artifact in start/
    /// resume order, with exactly its declared frame capacity. They must obey
    /// the scalar task ABI, access only that buffer, and never unwind across
    /// FFI. `guard` must keep every entry and its imports executable until drop.
    pub unsafe fn new(entries: Vec<Segment>, frame_words: usize, guard: G) -> Result<Self, Error> {
        if entries.is_empty() || frame_words == 0 {
            return Err(Error::InvalidArtifact);
        }
        Ok(Self {
            entries,
            words: frame_words,
            _guard: guard,
        })
    }
}

#[derive(Clone, Copy)]
enum State {
    Ready {
        segment: usize,
    },
    Waiting {
        segment: usize,
        ticket: Ticket,
        queued: Option<Delivery>,
    },
    Running,
    Complete {
        value: u64,
        cancelled: bool,
    },
    Failed(Error),
}

pub struct Task<G> {
    owner: u64,
    code: Rc<NativeCode<G>>,
    frame: RefCell<Option<Box<[u64]>>>,
    state: RefCell<State>,
    cancelled: Cell<bool>,
    generation: Cell<u64>,
    operations: RefCell<HashSet<u64>>,
    /// Durations requested by `native_timer_begin`, as (pending, milliseconds).
    /// The runtime holds no clock; a host reads a request with `take_sleep`.
    sleeps: RefCell<Vec<(u64, u64)>>,
}

static NEXT_IDENTITY: AtomicU64 = AtomicU64::new(1);
fn identity() -> Result<u64, Error> {
    NEXT_IDENTITY
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| Error::IdentityExhausted)
}

struct Invocation {
    operations: *const RefCell<HashSet<u64>>,
    sleeps: *const RefCell<Vec<(u64, u64)>>,
    fault: Cell<Option<Error>>,
}
thread_local! { static CURRENT: Cell<*const Invocation> = const { Cell::new(std::ptr::null()) }; }
struct ContextGuard;
impl Drop for ContextGuard {
    fn drop(&mut self) {
        CURRENT.set(std::ptr::null());
    }
}

/// Mint an identity for an operation begun by the running segment and
/// register it with the task. Calling outside a task is a runtime contract
/// violation; abort rather than unwind across native FFI.
fn begin_operation(milliseconds: Option<u64>) -> u64 {
    let context = CURRENT.get();
    if context.is_null() {
        std::process::abort();
    }
    // The invocation lives on poll's stack and its context guard is removed
    // before that stack frame leaves. Tasks cannot recursively enter poll.
    let context = unsafe { &*context };
    match identity() {
        Ok(id) => {
            unsafe { &*context.operations }.borrow_mut().insert(id);
            if let Some(milliseconds) = milliseconds {
                unsafe { &*context.sleeps }
                    .borrow_mut()
                    .push((id, milliseconds));
            }
            id
        }
        Err(error) => {
            context.fault.set(Some(error));
            0
        }
    }
}

/// Private scalar import of core::task::Native::begin.
#[unsafe(no_mangle)]
extern "C" fn native_task_begin() -> u64 {
    begin_operation(None)
}

/// Private scalar import of core::task::Timer::sleep_begin for Native. The
/// operation is an ordinary pending identity that also records the requested
/// duration for the host, which alone decides when the sleep completes.
#[unsafe(no_mangle)]
extern "C" fn native_timer_begin(milliseconds: u64) -> u64 {
    begin_operation(Some(milliseconds))
}

impl<G> Task<G> {
    pub fn new(code: Rc<NativeCode<G>>) -> Result<Self, Error> {
        Ok(Self {
            owner: identity()?,
            frame: RefCell::new(Some(vec![0; code.words].into_boxed_slice())),
            code,
            state: RefCell::new(State::Ready { segment: 0 }),
            cancelled: Cell::new(false),
            generation: Cell::new(0),
            operations: RefCell::new(HashSet::new()),
            sleeps: RefCell::new(Vec::new()),
        })
    }

    /// Queue one completion. No Fe code runs here, including in callbacks.
    pub fn deliver(&self, ticket: Ticket, delivery: Delivery) -> Result<(), Error> {
        if ticket.owner != self.owner {
            return Err(Error::Foreign);
        }
        if ticket.generation != self.generation.get() {
            return Err(Error::Stale);
        }
        let mut state = self.state.borrow_mut();
        match &mut *state {
            State::Waiting {
                ticket: expected,
                queued,
                ..
            } if *expected == ticket => {
                if queued.is_some() || self.cancelled.get() {
                    return Err(Error::Duplicate);
                }
                if delivery == Delivery::Cancelled {
                    self.cancelled.set(true);
                }
                *queued = Some(delivery);
                Ok(())
            }
            _ => Err(Error::Duplicate),
        }
    }

    /// Latch cancellation. A queued, unclaimed completion cannot override it.
    /// Cancellation before start retires the untouched task without running it.
    pub fn cancel(&self) -> bool {
        let mut state = self.state.borrow_mut();
        if matches!(*state, State::Complete { .. } | State::Failed(_)) {
            return false;
        }
        let changed = !self.cancelled.replace(true);
        if matches!(*state, State::Ready { segment: 0 }) {
            *state = State::Complete {
                value: 0,
                cancelled: true,
            };
            self.release_operations();
        }
        changed
    }

    /// Execute at most one generated segment. Reentrant callers may enqueue
    /// delivery but cannot recursively poll this task or another native task.
    pub fn poll(&self) -> Result<Progress, Error> {
        if !CURRENT.get().is_null() {
            return Err(Error::Reentrant);
        }
        let previous = self.state.replace(State::Running);
        let (segment, delivery) = match previous {
            State::Ready { segment } => (segment, Delivery::Cancelled),
            State::Waiting {
                segment,
                ticket,
                queued,
            } => {
                if self.cancelled.get() {
                    (segment, Delivery::Cancelled)
                } else if let Some(delivery) = queued {
                    (segment, delivery)
                } else {
                    self.state.replace(previous);
                    return Ok(Progress::Suspended(ticket));
                }
            }
            State::Complete { value, cancelled } => {
                self.state.replace(previous);
                return Ok(Progress::Complete { value, cancelled });
            }
            State::Failed(error) => {
                self.state.replace(previous);
                return Err(error);
            }
            State::Running => {
                self.state.replace(previous);
                return Err(Error::Reentrant);
            }
        };
        let Some(entry) = self.code.entries.get(segment) else {
            return self.fail(Error::InvalidStep);
        };
        let invocation = Invocation {
            operations: &self.operations,
            sleeps: &self.sleeps,
            fault: Cell::new(None),
        };
        let (next, value) = {
            let mut frame = self.frame.borrow_mut();
            let Some(frame) = frame.as_mut() else {
                drop(frame);
                return self.fail(Error::InvalidArtifact);
            };
            CURRENT.set(&invocation);
            let guard = ContextGuard;
            let (kind, payload) = delivery.wire();
            let next = unsafe { entry(frame.as_mut_ptr(), kind, payload) };
            drop(guard);
            (next, frame[0])
        };
        if let Some(error) = invocation.fault.get() {
            return self.fail(error);
        }
        if next == 0 {
            let cancelled = self.cancelled.get();
            self.state.replace(State::Complete { value, cancelled });
            self.release_operations();
            return Ok(Progress::Complete { value, cancelled });
        }
        let Ok(segment) = usize::try_from(next) else {
            return self.fail(Error::InvalidStep);
        };
        if segment >= self.code.entries.len() {
            return self.fail(Error::InvalidStep);
        }
        if !self.operations.borrow_mut().remove(&value) {
            return self.fail(Error::InvalidPending);
        }
        let Some(generation) = self.generation.get().checked_add(1) else {
            return self.fail(Error::IdentityExhausted);
        };
        self.generation.set(generation);
        if self.cancelled.get() {
            self.state.replace(State::Ready { segment });
            Ok(Progress::Cancelling)
        } else {
            let ticket = Ticket {
                owner: self.owner,
                generation,
                pending: value,
            };
            self.state.replace(State::Waiting {
                segment,
                ticket,
                queued: None,
            });
            Ok(Progress::Suspended(ticket))
        }
    }

    fn fail(&self, error: Error) -> Result<Progress, Error> {
        self.state.replace(State::Failed(error));
        self.release_operations();
        Err(error)
    }

    /// Drop the frame and every registration of a task that will not run again.
    fn release_operations(&self) {
        self.frame.borrow_mut().take();
        self.operations.borrow_mut().clear();
        self.sleeps.borrow_mut().clear();
    }

    /// The duration, in milliseconds, requested by the sleep that `ticket`
    /// names, or `None` if that operation is not a sleep, the ticket is not
    /// the task's current suspension, or the request was already taken. The
    /// host that completes the sleep reads this once, right after the poll
    /// that returned the ticket.
    pub fn take_sleep(&self, ticket: Ticket) -> Option<u64> {
        if ticket.owner != self.owner || ticket.generation != self.generation.get() {
            return None;
        }
        let mut sleeps = self.sleeps.borrow_mut();
        let position = sleeps.iter().position(|(id, _)| *id == ticket.pending)?;
        Some(sleeps.remove(position).1)
    }

    pub fn retains_frame(&self) -> bool {
        matches!(*self.state.borrow(), State::Running) || self.frame.borrow().is_some()
    }
    pub fn registrations(&self) -> usize {
        self.operations.borrow().len()
            + usize::from(matches!(*self.state.borrow(), State::Waiting { .. }))
    }
}
