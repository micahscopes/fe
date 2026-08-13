//! Executable, platform-neutral host-interface state machines.
//!
//! The wire protocol uses table identity, slot, and generation together. A
//! numeric slot is never authority on its own. Owned roots are non-`Copy`;
//! borrowed references are scoped guards; callbacks remain rooted while
//! invoked; and futures have one terminal transition.

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet, VecDeque},
    marker::PhantomData,
    num::NonZeroU64,
    ops::Deref,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use fe_host_abi::HandleOwnership;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TableId(NonZeroU64);

impl TableId {
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawHandle {
    pub table: TableId,
    pub slot: u32,
    pub generation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleDomain {
    Resource,
    Callback,
    Future,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskExecutor {
    Current,
    External,
}

/// Runtime binding for one compiler-derived Fe task body.
///
/// `K` is deliberately supplied by the materializer. The executor neither
/// parses string identities nor knows about synthetic start/resume/poll entry
/// names; a backend may use an enum, a function-table key, or another typed
/// internal identity derived from the Fe program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDescriptor<K> {
    pub body: K,
    pub executor: TaskExecutor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumableTaskState {
    Created,
    Running,
    Suspended,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskOutcome<E, V> {
    Failure(E),
    Success(V),
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskToken(i32);

impl TaskToken {
    pub const fn from_core(value: i32) -> Self {
        Self(value)
    }

    pub const fn to_core(self) -> i32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskRuntimeError {
    InvalidToken(i32),
    StaleToken(i32),
    InvalidTransition {
        state: ResumableTaskState,
        operation: &'static str,
    },
    AlreadyTerminal(ResumableTaskState),
    PlacementMismatch {
        expected: TaskExecutor,
        actual: TaskExecutor,
    },
    TokenSpaceExhausted,
}

struct TaskSlot<K, V, E> {
    generation: u16,
    state: ResumableTaskState,
    descriptor: Option<TaskDescriptor<K>>,
    queue: VecDeque<TaskOutcome<E, V>>,
}

/// Generation-tagged opaque-i32 table for target-neutral resumable tasks.
pub struct ResumableTaskTable<K, V, E> {
    slots: RefCell<Vec<TaskSlot<K, V, E>>>,
    free: RefCell<BTreeSet<u16>>,
}

impl<K, V, E> Default for ResumableTaskTable<K, V, E> {
    fn default() -> Self {
        Self {
            slots: RefCell::new(Vec::new()),
            free: RefCell::new(BTreeSet::new()),
        }
    }
}

impl<K: Clone, V, E> ResumableTaskTable<K, V, E> {
    pub fn register(&self, descriptor: TaskDescriptor<K>) -> Result<TaskToken, TaskRuntimeError> {
        let slot = if let Some(slot) = self.free.borrow_mut().pop_first() {
            slot
        } else {
            let slot = u16::try_from(self.slots.borrow().len())
                .map_err(|_| TaskRuntimeError::TokenSpaceExhausted)?;
            self.slots.borrow_mut().push(TaskSlot {
                generation: 1,
                state: ResumableTaskState::Created,
                descriptor: None,
                queue: VecDeque::new(),
            });
            slot
        };
        let mut slots = self.slots.borrow_mut();
        let entry = &mut slots[usize::from(slot)];
        entry.state = ResumableTaskState::Created;
        entry.descriptor = Some(descriptor);
        entry.queue.clear();
        Ok(pack_task_token(slot, entry.generation))
    }

    pub fn descriptor(&self, token: TaskToken) -> Result<TaskDescriptor<K>, TaskRuntimeError> {
        let slots = self.slots.borrow();
        let (_, entry) = task_entry(&slots, token)?;
        entry
            .descriptor
            .clone()
            .ok_or(TaskRuntimeError::StaleToken(token.to_core()))
    }

    pub fn state(&self, token: TaskToken) -> Result<ResumableTaskState, TaskRuntimeError> {
        let slots = self.slots.borrow();
        Ok(task_entry(&slots, token)?.1.state)
    }

    pub fn start(&self, token: TaskToken, executor: TaskExecutor) -> Result<(), TaskRuntimeError> {
        let mut slots = self.slots.borrow_mut();
        let (_, entry) = task_entry_mut(&mut slots, token)?;
        enforce_task_executor(entry, executor)?;
        transition_task(
            entry,
            ResumableTaskState::Created,
            ResumableTaskState::Running,
            "start",
        )
    }

    pub fn suspend(&self, token: TaskToken) -> Result<(), TaskRuntimeError> {
        let mut slots = self.slots.borrow_mut();
        let (_, entry) = task_entry_mut(&mut slots, token)?;
        transition_task(
            entry,
            ResumableTaskState::Running,
            ResumableTaskState::Suspended,
            "suspend",
        )
    }

    pub fn queue_value(&self, token: TaskToken, value: V) -> Result<(), TaskRuntimeError> {
        self.queue(token, TaskOutcome::Success(value), "resume_value")
    }

    pub fn queue_error(&self, token: TaskToken, error: E) -> Result<(), TaskRuntimeError> {
        self.queue(token, TaskOutcome::Failure(error), "resume_error")
    }

    pub fn queue_cancel(&self, token: TaskToken) -> Result<(), TaskRuntimeError> {
        self.queue(token, TaskOutcome::Cancelled, "resume_cancel")
    }

    /// Cancellation has priority over an already queued value/error which has
    /// not yet been delivered to the task body.
    pub fn prioritize_cancel(&self, token: TaskToken) -> Result<(), TaskRuntimeError> {
        let mut slots = self.slots.borrow_mut();
        let (_, entry) = task_entry_mut(&mut slots, token)?;
        if entry.state != ResumableTaskState::Suspended {
            return Err(TaskRuntimeError::InvalidTransition {
                state: entry.state,
                operation: "resume_cancel",
            });
        }
        entry.queue.clear();
        entry.queue.push_back(TaskOutcome::Cancelled);
        Ok(())
    }

    fn queue(
        &self,
        token: TaskToken,
        delivery: TaskOutcome<E, V>,
        operation: &'static str,
    ) -> Result<(), TaskRuntimeError> {
        let mut slots = self.slots.borrow_mut();
        let (_, entry) = task_entry_mut(&mut slots, token)?;
        if entry.state != ResumableTaskState::Suspended || !entry.queue.is_empty() {
            return Err(TaskRuntimeError::InvalidTransition {
                state: entry.state,
                operation,
            });
        }
        entry.queue.push_back(delivery);
        Ok(())
    }

    pub fn deliver_next(
        &self,
        token: TaskToken,
        executor: TaskExecutor,
    ) -> Result<Option<TaskOutcome<E, V>>, TaskRuntimeError> {
        let mut slots = self.slots.borrow_mut();
        let (_, entry) = task_entry_mut(&mut slots, token)?;
        enforce_task_executor(entry, executor)?;
        let Some(delivery) = entry.queue.pop_front() else {
            return Ok(None);
        };
        entry.state = match delivery {
            TaskOutcome::Success(_) => ResumableTaskState::Running,
            TaskOutcome::Failure(_) => ResumableTaskState::Failed,
            TaskOutcome::Cancelled => ResumableTaskState::Cancelled,
        };
        Ok(Some(delivery))
    }

    pub fn complete(&self, token: TaskToken) -> Result<(), TaskRuntimeError> {
        let mut slots = self.slots.borrow_mut();
        let (_, entry) = task_entry_mut(&mut slots, token)?;
        match entry.state {
            ResumableTaskState::Running | ResumableTaskState::Suspended
                if entry.queue.is_empty() =>
            {
                entry.state = ResumableTaskState::Completed;
                Ok(())
            }
            ResumableTaskState::Completed
            | ResumableTaskState::Failed
            | ResumableTaskState::Cancelled => Err(TaskRuntimeError::AlreadyTerminal(entry.state)),
            state => Err(TaskRuntimeError::InvalidTransition {
                state,
                operation: "complete",
            }),
        }
    }

    pub fn release(&self, token: TaskToken) -> Result<(), TaskRuntimeError> {
        let mut slots = self.slots.borrow_mut();
        let (slot, entry) = task_entry_mut(&mut slots, token)?;
        if !matches!(
            entry.state,
            ResumableTaskState::Completed
                | ResumableTaskState::Failed
                | ResumableTaskState::Cancelled
        ) {
            return Err(TaskRuntimeError::InvalidTransition {
                state: entry.state,
                operation: "release",
            });
        }
        entry.generation = entry
            .generation
            .checked_add(1)
            .ok_or(TaskRuntimeError::TokenSpaceExhausted)?;
        entry.descriptor = None;
        entry.queue.clear();
        self.free.borrow_mut().insert(slot);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskBodyAction {
    Suspend,
    Complete,
    Wake,
}

pub trait ResumableTaskBody<K, V, E> {
    fn start(&mut self, token: TaskToken, descriptor: &TaskDescriptor<K>) -> TaskBodyAction;
    fn poll(&mut self, token: TaskToken, descriptor: &TaskDescriptor<K>) -> TaskBodyAction;
    fn resume(
        &mut self,
        token: TaskToken,
        descriptor: &TaskDescriptor<K>,
        delivery: TaskOutcome<E, V>,
    ) -> TaskBodyAction;
    fn terminal(&mut self, token: TaskToken, state: ResumableTaskState);
    fn route(&mut self, token: TaskToken, executor: TaskExecutor);
    fn yielded(&mut self, remaining_ready: usize);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadyKind {
    Start,
    Poll,
    Resume,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutorDrain {
    pub steps: usize,
    pub remaining_ready: usize,
    pub reentrant: bool,
}

/// Deterministic, non-reentrant FIFO scheduler over [`ResumableTaskTable`].
pub struct ResumableExecutor<K, V, E> {
    placement: TaskExecutor,
    tasks: ResumableTaskTable<K, V, E>,
    ready: RefCell<VecDeque<TaskToken>>,
    kinds: RefCell<BTreeMap<TaskToken, ReadyKind>>,
    draining: Cell<bool>,
    notified: RefCell<BTreeSet<TaskToken>>,
}

impl<K: Clone, V, E> ResumableExecutor<K, V, E> {
    pub fn new(placement: TaskExecutor) -> Self {
        Self {
            placement,
            tasks: ResumableTaskTable::default(),
            ready: RefCell::new(VecDeque::new()),
            kinds: RefCell::new(BTreeMap::new()),
            draining: Cell::new(false),
            notified: RefCell::new(BTreeSet::new()),
        }
    }

    pub fn tasks(&self) -> &ResumableTaskTable<K, V, E> {
        &self.tasks
    }

    pub fn spawn(&self, descriptor: TaskDescriptor<K>) -> Result<TaskToken, TaskRuntimeError> {
        let token = self.tasks.register(descriptor)?;
        self.enqueue(token, ReadyKind::Start);
        Ok(token)
    }

    /// Wakeups deduplicate without changing the token's FIFO position.
    pub fn wake(&self, token: TaskToken) -> Result<bool, TaskRuntimeError> {
        let state = self.tasks.state(token)?;
        if matches!(
            state,
            ResumableTaskState::Completed
                | ResumableTaskState::Failed
                | ResumableTaskState::Cancelled
        ) {
            return Err(TaskRuntimeError::AlreadyTerminal(state));
        }
        Ok(self.enqueue(token, ReadyKind::Poll))
    }

    pub fn resume_value(&self, token: TaskToken, value: V) -> Result<(), TaskRuntimeError> {
        self.tasks.queue_value(token, value)?;
        self.enqueue(token, ReadyKind::Resume);
        Ok(())
    }

    pub fn resume_error(&self, token: TaskToken, error: E) -> Result<(), TaskRuntimeError> {
        self.tasks.queue_error(token, error)?;
        self.enqueue(token, ReadyKind::Resume);
        Ok(())
    }

    pub fn cancel(&self, token: TaskToken) -> Result<(), TaskRuntimeError> {
        self.tasks.prioritize_cancel(token)?;
        // Cancellation replaces any pending work and moves to the priority
        // front while retaining one queue occurrence.
        self.kinds.borrow_mut().insert(token, ReadyKind::Cancel);
        self.ready.borrow_mut().retain(|queued| *queued != token);
        self.ready.borrow_mut().push_front(token);
        Ok(())
    }

    fn enqueue(&self, token: TaskToken, kind: ReadyKind) -> bool {
        let mut kinds = self.kinds.borrow_mut();
        if kinds.contains_key(&token) {
            return false;
        }
        kinds.insert(token, kind);
        self.ready.borrow_mut().push_back(token);
        true
    }

    pub fn drain(
        &self,
        budget: usize,
        body: &mut impl ResumableTaskBody<K, V, E>,
    ) -> Result<ExecutorDrain, TaskRuntimeError> {
        if self.draining.replace(true) {
            return Ok(ExecutorDrain {
                steps: 0,
                remaining_ready: self.ready.borrow().len(),
                reentrant: true,
            });
        }
        struct DrainGuard<'a>(&'a Cell<bool>);
        impl Drop for DrainGuard<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _guard = DrainGuard(&self.draining);
        let mut steps = 0;
        while steps < budget {
            let Some(token) = self.ready.borrow_mut().pop_front() else {
                break;
            };
            let kind = self
                .kinds
                .borrow_mut()
                .remove(&token)
                .expect("ready token has kind");
            let descriptor = self.tasks.descriptor(token)?;
            if descriptor.executor != self.placement {
                body.route(token, descriptor.executor);
                steps += 1;
                continue;
            }
            let state = self.tasks.state(token)?;
            let action = match kind {
                ReadyKind::Start => {
                    self.tasks.start(token, self.placement)?;
                    Some(body.start(token, &descriptor))
                }
                ReadyKind::Poll => Some(body.poll(token, &descriptor)),
                ReadyKind::Resume | ReadyKind::Cancel => {
                    match self.tasks.deliver_next(token, self.placement)? {
                        Some(TaskOutcome::Success(value)) => {
                            Some(body.resume(token, &descriptor, TaskOutcome::Success(value)))
                        }
                        Some(TaskOutcome::Failure(error)) => {
                            body.resume(token, &descriptor, TaskOutcome::Failure(error));
                            None
                        }
                        Some(TaskOutcome::Cancelled) => None,
                        None => Some(body.poll(token, &descriptor)),
                    }
                }
            };
            if let Some(action) = action {
                self.apply_action(token, action)?;
            }
            let terminal = self.tasks.state(token)?;
            if matches!(
                terminal,
                ResumableTaskState::Completed
                    | ResumableTaskState::Failed
                    | ResumableTaskState::Cancelled
            ) && self.notified.borrow_mut().insert(token)
            {
                body.terminal(token, terminal);
            }
            debug_assert!(
                kind == ReadyKind::Start || state != ResumableTaskState::Created,
                "only start may run a created task"
            );
            steps += 1;
        }
        let remaining_ready = self.ready.borrow().len();
        if remaining_ready > 0 {
            body.yielded(remaining_ready);
        }
        Ok(ExecutorDrain {
            steps,
            remaining_ready,
            reentrant: false,
        })
    }

    fn apply_action(
        &self,
        token: TaskToken,
        action: TaskBodyAction,
    ) -> Result<(), TaskRuntimeError> {
        match action {
            TaskBodyAction::Suspend => self.tasks.suspend(token),
            TaskBodyAction::Complete => self.tasks.complete(token),
            TaskBodyAction::Wake => {
                self.enqueue(token, ReadyKind::Poll);
                Ok(())
            }
        }
    }
}

fn pack_task_token(slot: u16, generation: u16) -> TaskToken {
    TaskToken::from_core(i32::from_ne_bytes(
        ((u32::from(generation) << 16) | (u32::from(slot) + 1)).to_ne_bytes(),
    ))
}

fn unpack_task_token(token: TaskToken) -> Result<(u16, u16), TaskRuntimeError> {
    let bits = u32::from_ne_bytes(token.to_core().to_ne_bytes());
    let encoded_slot = (bits & 0xffff) as u16;
    let generation = (bits >> 16) as u16;
    if encoded_slot == 0 || generation == 0 {
        return Err(TaskRuntimeError::InvalidToken(token.to_core()));
    }
    Ok((encoded_slot - 1, generation))
}

fn task_entry<K, V, E>(
    slots: &[TaskSlot<K, V, E>],
    token: TaskToken,
) -> Result<(u16, &TaskSlot<K, V, E>), TaskRuntimeError> {
    let (slot, generation) = unpack_task_token(token)?;
    let entry = slots
        .get(usize::from(slot))
        .ok_or(TaskRuntimeError::InvalidToken(token.to_core()))?;
    if entry.generation != generation || entry.descriptor.is_none() {
        return Err(TaskRuntimeError::StaleToken(token.to_core()));
    }
    Ok((slot, entry))
}

fn task_entry_mut<K, V, E>(
    slots: &mut [TaskSlot<K, V, E>],
    token: TaskToken,
) -> Result<(u16, &mut TaskSlot<K, V, E>), TaskRuntimeError> {
    let (slot, generation) = unpack_task_token(token)?;
    let entry = slots
        .get_mut(usize::from(slot))
        .ok_or(TaskRuntimeError::InvalidToken(token.to_core()))?;
    if entry.generation != generation || entry.descriptor.is_none() {
        return Err(TaskRuntimeError::StaleToken(token.to_core()));
    }
    Ok((slot, entry))
}

fn enforce_task_executor<K, V, E>(
    entry: &TaskSlot<K, V, E>,
    actual: TaskExecutor,
) -> Result<(), TaskRuntimeError> {
    let expected = entry
        .descriptor
        .as_ref()
        .expect("live task has descriptor")
        .executor;
    if expected == actual {
        Ok(())
    } else {
        Err(TaskRuntimeError::PlacementMismatch { expected, actual })
    }
}

fn transition_task<K, V, E>(
    entry: &mut TaskSlot<K, V, E>,
    expected: ResumableTaskState,
    next: ResumableTaskState,
    operation: &'static str,
) -> Result<(), TaskRuntimeError> {
    if entry.state == expected {
        entry.state = next;
        Ok(())
    } else {
        Err(TaskRuntimeError::InvalidTransition {
            state: entry.state,
            operation,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalState {
    Resolved,
    Rejected,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum RuntimeError {
    WrongTable {
        domain: HandleDomain,
        expected: TableId,
        received: TableId,
    },
    SlotOutOfRange {
        domain: HandleDomain,
        slot: u32,
    },
    StaleHandle {
        domain: HandleDomain,
        slot: u32,
        expected_generation: u32,
        received_generation: u32,
    },
    VacantHandle {
        domain: HandleDomain,
        slot: u32,
        generation: u32,
    },
    ResourceBorrowed {
        slot: u32,
        active_borrows: u32,
    },
    CallbackPanicked {
        slot: u32,
    },
    AlreadyCompleted {
        slot: u32,
        state: TerminalState,
    },
    FutureStillPending {
        slot: u32,
    },
    GenerationExhausted {
        domain: HandleDomain,
        slot: u32,
    },
    CallbackTokenExhausted,
    FutureTokenExhausted,
    UnknownToken {
        domain: HandleDomain,
        token: i32,
    },
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for RuntimeError {}

fn validate_raw(
    id: TableId,
    domain: HandleDomain,
    raw: RawHandle,
    len: usize,
) -> Result<usize, RuntimeError> {
    if raw.table != id {
        return Err(RuntimeError::WrongTable {
            domain,
            expected: id,
            received: raw.table,
        });
    }
    let slot = raw.slot as usize;
    if slot >= len {
        return Err(RuntimeError::SlotOutOfRange {
            domain,
            slot: raw.slot,
        });
    }
    Ok(slot)
}

fn next_generation(domain: HandleDomain, slot: u32, generation: u32) -> Result<u32, RuntimeError> {
    generation
        .checked_add(1)
        .ok_or(RuntimeError::GenerationExhausted { domain, slot })
}

#[derive(Debug, PartialEq, Eq)]
pub struct OwnedResource<R> {
    raw: RawHandle,
    marker: PhantomData<fn() -> R>,
}

impl<R> OwnedResource<R> {
    pub const fn raw(&self) -> RawHandle {
        self.raw
    }

    pub const fn ownership(&self) -> HandleOwnership {
        HandleOwnership::Own
    }
}

struct ResourceSlot<R> {
    generation: u32,
    value: Option<Box<R>>,
    active_borrows: u32,
}

struct ResourceState<R> {
    slots: Vec<ResourceSlot<R>>,
    free: BTreeSet<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceInventory {
    pub live: u32,
    pub active_borrows: u32,
    pub vacant: u32,
}

pub struct ResourceTable<R> {
    id: TableId,
    state: RefCell<ResourceState<R>>,
}

impl<R> ResourceTable<R> {
    pub fn new(id: TableId) -> Self {
        Self {
            id,
            state: RefCell::new(ResourceState {
                slots: Vec::new(),
                free: BTreeSet::new(),
            }),
        }
    }

    pub const fn id(&self) -> TableId {
        self.id
    }

    pub fn insert(&self, value: R) -> OwnedResource<R> {
        let mut state = self.state.borrow_mut();
        let slot = state.free.pop_first().unwrap_or_else(|| {
            let slot = u32::try_from(state.slots.len()).expect("resource slot space exhausted");
            state.slots.push(ResourceSlot {
                generation: 1,
                value: None,
                active_borrows: 0,
            });
            slot
        });
        let entry = &mut state.slots[slot as usize];
        debug_assert!(entry.value.is_none());
        debug_assert_eq!(entry.active_borrows, 0);
        entry.value = Some(Box::new(value));
        OwnedResource {
            raw: RawHandle {
                table: self.id,
                slot,
                generation: entry.generation,
            },
            marker: PhantomData,
        }
    }

    pub fn drop_owned(&self, owned: OwnedResource<R>) -> Result<R, RuntimeError> {
        self.drop_raw(owned.raw)
    }

    /// Wire-facing consuming drop. Replaying the same raw handle is rejected.
    pub fn drop_raw(&self, raw: RawHandle) -> Result<R, RuntimeError> {
        let mut state = self.state.borrow_mut();
        let slot = validate_raw(self.id, HandleDomain::Resource, raw, state.slots.len())?;
        let entry = &mut state.slots[slot];
        check_entry(
            HandleDomain::Resource,
            raw,
            entry.generation,
            entry.value.is_some(),
        )?;
        if entry.active_borrows != 0 {
            return Err(RuntimeError::ResourceBorrowed {
                slot: raw.slot,
                active_borrows: entry.active_borrows,
            });
        }
        let generation = next_generation(HandleDomain::Resource, raw.slot, entry.generation)?;
        let value = entry.value.take().expect("checked live resource");
        entry.generation = generation;
        state.free.insert(raw.slot);
        Ok(*value)
    }

    /// Create a scoped borrow. The guard's lifetime is tied to this table and
    /// its destructor ends the host-call borrow. Resource storage is boxed, and
    /// consuming drop is rejected while a guard exists, so the pointer remains
    /// stable even if other table operations re-enter the runtime.
    pub fn borrow(&self, raw: RawHandle) -> Result<BorrowedResource<'_, R>, RuntimeError> {
        let pointer = {
            let mut state = self.state.borrow_mut();
            let slot = validate_raw(self.id, HandleDomain::Resource, raw, state.slots.len())?;
            let entry = &mut state.slots[slot];
            check_entry(
                HandleDomain::Resource,
                raw,
                entry.generation,
                entry.value.is_some(),
            )?;
            entry.active_borrows = entry
                .active_borrows
                .checked_add(1)
                .expect("active resource borrow count overflow");
            entry
                .value
                .as_deref()
                .map(std::ptr::from_ref)
                .expect("checked live resource")
        };
        Ok(BorrowedResource {
            table: self,
            raw,
            pointer,
        })
    }

    pub fn inventory(&self) -> ResourceInventory {
        let state = self.state.borrow();
        ResourceInventory {
            live: state
                .slots
                .iter()
                .filter(|slot| slot.value.is_some())
                .count() as u32,
            active_borrows: state.slots.iter().map(|slot| slot.active_borrows).sum(),
            vacant: state.free.len() as u32,
        }
    }

    fn end_borrow(&self, raw: RawHandle) {
        let mut state = self.state.borrow_mut();
        let entry = &mut state.slots[raw.slot as usize];
        debug_assert_eq!(entry.generation, raw.generation);
        debug_assert!(entry.value.is_some());
        debug_assert!(entry.active_borrows > 0);
        entry.active_borrows -= 1;
    }
}

pub struct BorrowedResource<'table, R> {
    table: &'table ResourceTable<R>,
    raw: RawHandle,
    pointer: *const R,
}

impl<R> std::fmt::Debug for BorrowedResource<'_, R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BorrowedResource")
            .field("raw", &self.raw)
            .finish_non_exhaustive()
    }
}

impl<R> BorrowedResource<'_, R> {
    pub const fn raw(&self) -> RawHandle {
        self.raw
    }

    pub const fn ownership(&self) -> HandleOwnership {
        HandleOwnership::Borrow
    }
}

impl<R> Deref for BorrowedResource<'_, R> {
    type Target = R;

    fn deref(&self) -> &Self::Target {
        // SAFETY: `borrow` obtains this pointer from boxed storage and increments
        // the slot's borrow count. `drop_raw` cannot remove that storage until
        // all guards decrement the count.
        unsafe { &*self.pointer }
    }
}

impl<R> Drop for BorrowedResource<'_, R> {
    fn drop(&mut self) {
        self.table.end_borrow(self.raw);
    }
}

fn check_entry(
    domain: HandleDomain,
    raw: RawHandle,
    generation: u32,
    occupied: bool,
) -> Result<(), RuntimeError> {
    if raw.generation != generation {
        return Err(RuntimeError::StaleHandle {
            domain,
            slot: raw.slot,
            expected_generation: generation,
            received_generation: raw.generation,
        });
    }
    if !occupied {
        return Err(RuntimeError::VacantHandle {
            domain,
            slot: raw.slot,
            generation,
        });
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub struct CallbackRoot<A, O> {
    raw: RawHandle,
    marker: PhantomData<fn(A) -> O>,
}

impl<A, O> CallbackRoot<A, O> {
    pub const fn raw(&self) -> RawHandle {
        self.raw
    }
}

type Callback<A, O> = Box<dyn Fn(A) -> O>;

struct CallbackSlot<A, O> {
    generation: u32,
    callback: Option<Callback<A, O>>,
    active_depth: u32,
    release_pending: bool,
}

struct CallbackState<A, O> {
    slots: Vec<CallbackSlot<A, O>>,
    free: BTreeSet<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallbackInventory {
    pub rooted: u32,
    pub active: u32,
    pub pending_release: u32,
}

pub struct CallbackRegistry<A, O> {
    inner: Rc<CallbackRegistryInner<A, O>>,
}

/// Host-interface terminology alias for [`CallbackRegistry`].
pub type CallbackTable<A, O> = CallbackRegistry<A, O>;

/// Opaque core-Wasm `i32` token naming a rooted callback.
///
/// This intentionally does not pack [`RawHandle`]. Table identity and
/// generation remain host-runtime state and never become forgeable wire bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CallbackToken(i32);

impl CallbackToken {
    pub const fn from_core(value: i32) -> Self {
        Self(value)
    }

    pub const fn to_core(self) -> i32 {
        self.0
    }
}

/// Callback lifetime table for core-Wasm scalar/token boundaries.
///
/// Signature-specific generated adapters may wrap this type, but registration,
/// stale-token rejection, reentrancy, and deferred release stay here.
pub struct CallbackTokenTable<A, O> {
    registry: CallbackRegistry<A, O>,
    tokens: RefCell<BTreeMap<CallbackToken, RawHandle>>,
    next_token: Cell<u32>,
}

impl<A, O> CallbackTokenTable<A, O> {
    pub fn new(id: TableId) -> Self {
        Self {
            registry: CallbackRegistry::new(id),
            tokens: RefCell::new(BTreeMap::new()),
            next_token: Cell::new(1),
        }
    }

    pub fn register(
        &self,
        callback: impl Fn(A) -> O + 'static,
    ) -> Result<CallbackToken, RuntimeError> {
        let token_bits = self.next_token.get();
        let next = token_bits
            .checked_add(1)
            .ok_or(RuntimeError::CallbackTokenExhausted)?;
        let token = CallbackToken::from_core(token_bits as i32);
        if self.tokens.borrow().contains_key(&token) {
            return Err(RuntimeError::CallbackTokenExhausted);
        }
        let root = self.registry.register(callback);
        self.tokens.borrow_mut().insert(token, root.raw());
        self.next_token.set(next);
        Ok(token)
    }

    pub fn invoke(&self, token: CallbackToken, args: A) -> Result<O, RuntimeError> {
        let raw = self
            .tokens
            .borrow()
            .get(&token)
            .copied()
            .ok_or(RuntimeError::VacantHandle {
                domain: HandleDomain::Callback,
                slot: token.to_core() as u32,
                generation: 0,
            })?;
        self.registry.invoke(raw, args)
    }

    pub fn release(&self, token: CallbackToken) -> Result<(), RuntimeError> {
        let raw = self
            .tokens
            .borrow()
            .get(&token)
            .copied()
            .ok_or(RuntimeError::VacantHandle {
                domain: HandleDomain::Callback,
                slot: token.to_core() as u32,
                generation: 0,
            })?;
        self.registry.release_raw(raw)?;
        self.tokens.borrow_mut().remove(&token);
        Ok(())
    }

    pub fn inventory(&self) -> CallbackInventory {
        self.registry.inventory()
    }
}

struct CallbackRegistryInner<A, O> {
    id: TableId,
    state: RefCell<CallbackState<A, O>>,
}

impl<A, O> Clone for CallbackRegistry<A, O> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<A, O> CallbackRegistry<A, O> {
    pub fn new(id: TableId) -> Self {
        Self {
            inner: Rc::new(CallbackRegistryInner {
                id,
                state: RefCell::new(CallbackState {
                    slots: Vec::new(),
                    free: BTreeSet::new(),
                }),
            }),
        }
    }

    pub fn register(&self, callback: impl Fn(A) -> O + 'static) -> CallbackRoot<A, O> {
        let mut state = self.inner.state.borrow_mut();
        let slot = state.free.pop_first().unwrap_or_else(|| {
            let slot = u32::try_from(state.slots.len()).expect("callback slot space exhausted");
            state.slots.push(CallbackSlot {
                generation: 1,
                callback: None,
                active_depth: 0,
                release_pending: false,
            });
            slot
        });
        let entry = &mut state.slots[slot as usize];
        debug_assert!(
            entry.callback.is_none() && entry.active_depth == 0 && !entry.release_pending
        );
        entry.callback = Some(Box::new(callback));
        CallbackRoot {
            raw: RawHandle {
                table: self.inner.id,
                slot,
                generation: entry.generation,
            },
            marker: PhantomData,
        }
    }

    /// Invoke a rooted callback.
    ///
    /// The same handle may be invoked recursively. Each invocation increments
    /// its depth before user code runs; a release requested at any depth is
    /// deferred until the outermost invocation returns.
    pub fn invoke(&self, raw: RawHandle, args: A) -> Result<O, RuntimeError> {
        let callback = {
            let mut state = self.inner.state.borrow_mut();
            let slot = validate_raw(
                self.inner.id,
                HandleDomain::Callback,
                raw,
                state.slots.len(),
            )?;
            let entry = &mut state.slots[slot];
            if raw.generation != entry.generation {
                return Err(RuntimeError::StaleHandle {
                    domain: HandleDomain::Callback,
                    slot: raw.slot,
                    expected_generation: entry.generation,
                    received_generation: raw.generation,
                });
            }
            let callback = entry
                .callback
                .as_deref()
                .ok_or(RuntimeError::VacantHandle {
                    domain: HandleDomain::Callback,
                    slot: raw.slot,
                    generation: raw.generation,
                })?;
            let callback = std::ptr::from_ref(callback);
            entry.active_depth = entry
                .active_depth
                .checked_add(1)
                .expect("callback invocation depth overflow");
            callback
        };

        // SAFETY: the boxed callback allocation is stable while active. Release
        // only sets `release_pending`, and the allocation is removed only after
        // the outermost invocation decrements `active_depth` to zero.
        let outcome = catch_unwind(AssertUnwindSafe(|| unsafe { (&*callback)(args) }));
        let mut state = self.inner.state.borrow_mut();
        let entry = &mut state.slots[raw.slot as usize];
        debug_assert!(entry.active_depth > 0);
        entry.active_depth -= 1;
        if entry.active_depth == 0 && entry.release_pending {
            let generation =
                match next_generation(HandleDomain::Callback, raw.slot, entry.generation) {
                    Ok(generation) => generation,
                    Err(error) => {
                        entry.release_pending = false;
                        return Err(error);
                    }
                };
            entry.release_pending = false;
            entry.callback = None;
            entry.generation = generation;
            state.free.insert(raw.slot);
        }
        outcome.map_err(|_| RuntimeError::CallbackPanicked { slot: raw.slot })
    }

    pub fn release(&self, root: CallbackRoot<A, O>) -> Result<(), RuntimeError> {
        self.release_raw(root.raw)
    }

    /// Release a callback root. During invocation this is deferred until the
    /// callback returns, so its closure remains alive for the whole call.
    pub fn release_raw(&self, raw: RawHandle) -> Result<(), RuntimeError> {
        let mut state = self.inner.state.borrow_mut();
        let slot = validate_raw(
            self.inner.id,
            HandleDomain::Callback,
            raw,
            state.slots.len(),
        )?;
        let entry = &mut state.slots[slot];
        if raw.generation != entry.generation {
            return Err(RuntimeError::StaleHandle {
                domain: HandleDomain::Callback,
                slot: raw.slot,
                expected_generation: entry.generation,
                received_generation: raw.generation,
            });
        }
        if entry.active_depth != 0 {
            entry.release_pending = true;
            return Ok(());
        }
        let generation = next_generation(HandleDomain::Callback, raw.slot, entry.generation)?;
        if entry.callback.take().is_none() {
            return Err(RuntimeError::VacantHandle {
                domain: HandleDomain::Callback,
                slot: raw.slot,
                generation: raw.generation,
            });
        }
        entry.generation = generation;
        state.free.insert(raw.slot);
        Ok(())
    }

    pub fn inventory(&self) -> CallbackInventory {
        let state = self.inner.state.borrow();
        CallbackInventory {
            rooted: state
                .slots
                .iter()
                .filter(|slot| slot.callback.is_some())
                .count() as u32,
            active: state
                .slots
                .iter()
                .filter(|slot| slot.active_depth != 0)
                .count() as u32,
            pending_release: state
                .slots
                .iter()
                .filter(|slot| slot.release_pending)
                .count() as u32,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct FutureRoot<T, E> {
    raw: RawHandle,
    marker: PhantomData<fn() -> (T, E)>,
}

impl<T, E> FutureRoot<T, E> {
    pub const fn raw(&self) -> RawHandle {
        self.raw
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FutureOutcome<T, E> {
    Resolved(T),
    Rejected(E),
    Cancelled,
}

enum FutureState<T, E> {
    Vacant,
    Pending,
    Complete(FutureOutcome<T, E>),
}

struct FutureSlot<T, E> {
    generation: u32,
    state: FutureState<T, E>,
}

struct Futures<T, E> {
    slots: Vec<FutureSlot<T, E>>,
    free: BTreeSet<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FutureInventory {
    pub pending: u32,
    pub completed: u32,
    pub vacant: u32,
}

pub struct FutureRegistry<T, E> {
    id: TableId,
    state: RefCell<Futures<T, E>>,
}

/// Opaque core-Wasm `i32` token for an asynchronous operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FutureToken(i32);

impl FutureToken {
    pub const fn from_core(value: i32) -> Self {
        Self(value)
    }

    pub const fn to_core(self) -> i32 {
        self.0
    }
}

/// Exactly-once future state addressed by opaque core-Wasm tokens.
///
/// The side constructing this table owns each token until it consumes the
/// terminal outcome with [`FutureTokenTable::take`]. Producers may only
/// resolve, reject, or cancel by token; none of those operations transfer
/// ownership.
pub struct FutureTokenTable<T, E> {
    registry: FutureRegistry<T, E>,
    tokens: RefCell<BTreeMap<FutureToken, RawHandle>>,
    next_token: Cell<u32>,
}

impl<T, E> FutureTokenTable<T, E> {
    pub fn new(id: TableId) -> Self {
        Self {
            registry: FutureRegistry::new(id),
            tokens: RefCell::new(BTreeMap::new()),
            next_token: Cell::new(1),
        }
    }

    pub fn register(&self) -> Result<FutureToken, RuntimeError> {
        let token_bits = self.next_token.get();
        let next = token_bits
            .checked_add(1)
            .ok_or(RuntimeError::FutureTokenExhausted)?;
        let token = FutureToken::from_core(token_bits as i32);
        if self.tokens.borrow().contains_key(&token) {
            return Err(RuntimeError::FutureTokenExhausted);
        }
        let root = self.registry.register();
        self.tokens.borrow_mut().insert(token, root.raw());
        self.next_token.set(next);
        Ok(token)
    }

    pub fn resolve(&self, token: FutureToken, value: T) -> Result<(), RuntimeError> {
        self.registry.resolve(self.raw(token)?, value)
    }

    pub fn reject(&self, token: FutureToken, error: E) -> Result<(), RuntimeError> {
        self.registry.reject(self.raw(token)?, error)
    }

    pub fn cancel(&self, token: FutureToken) -> Result<(), RuntimeError> {
        self.registry.cancel(self.raw(token)?)
    }

    pub fn take(&self, token: FutureToken) -> Result<FutureOutcome<T, E>, RuntimeError> {
        let raw = self.raw(token)?;
        let outcome = self.registry.take(FutureRoot {
            raw,
            marker: PhantomData,
        })?;
        self.tokens.borrow_mut().remove(&token);
        Ok(outcome)
    }

    pub fn inventory(&self) -> FutureInventory {
        self.registry.inventory()
    }

    fn raw(&self, token: FutureToken) -> Result<RawHandle, RuntimeError> {
        self.tokens
            .borrow()
            .get(&token)
            .copied()
            .ok_or(RuntimeError::UnknownToken {
                domain: HandleDomain::Future,
                token: token.to_core(),
            })
    }
}

impl<T, E> FutureRegistry<T, E> {
    pub fn new(id: TableId) -> Self {
        Self {
            id,
            state: RefCell::new(Futures {
                slots: Vec::new(),
                free: BTreeSet::new(),
            }),
        }
    }

    pub fn register(&self) -> FutureRoot<T, E> {
        let mut state = self.state.borrow_mut();
        let slot = state.free.pop_first().unwrap_or_else(|| {
            let slot = u32::try_from(state.slots.len()).expect("future slot space exhausted");
            state.slots.push(FutureSlot {
                generation: 1,
                state: FutureState::Vacant,
            });
            slot
        });
        let entry = &mut state.slots[slot as usize];
        debug_assert!(matches!(entry.state, FutureState::Vacant));
        entry.state = FutureState::Pending;
        FutureRoot {
            raw: RawHandle {
                table: self.id,
                slot,
                generation: entry.generation,
            },
            marker: PhantomData,
        }
    }

    pub fn resolve(&self, raw: RawHandle, value: T) -> Result<(), RuntimeError> {
        self.complete(raw, FutureOutcome::Resolved(value))
    }

    pub fn reject(&self, raw: RawHandle, error: E) -> Result<(), RuntimeError> {
        self.complete(raw, FutureOutcome::Rejected(error))
    }

    pub fn cancel(&self, raw: RawHandle) -> Result<(), RuntimeError> {
        self.complete(raw, FutureOutcome::Cancelled)
    }

    fn complete(&self, raw: RawHandle, outcome: FutureOutcome<T, E>) -> Result<(), RuntimeError> {
        let mut state = self.state.borrow_mut();
        let slot = validate_raw(self.id, HandleDomain::Future, raw, state.slots.len())?;
        let entry = &mut state.slots[slot];
        if raw.generation != entry.generation {
            return Err(RuntimeError::StaleHandle {
                domain: HandleDomain::Future,
                slot: raw.slot,
                expected_generation: entry.generation,
                received_generation: raw.generation,
            });
        }
        match &entry.state {
            FutureState::Pending => {
                entry.state = FutureState::Complete(outcome);
                Ok(())
            }
            FutureState::Complete(outcome) => Err(RuntimeError::AlreadyCompleted {
                slot: raw.slot,
                state: terminal_state(outcome),
            }),
            FutureState::Vacant => Err(RuntimeError::VacantHandle {
                domain: HandleDomain::Future,
                slot: raw.slot,
                generation: raw.generation,
            }),
        }
    }

    /// Consume a completed future and free its slot for generation-safe reuse.
    pub fn take(&self, root: FutureRoot<T, E>) -> Result<FutureOutcome<T, E>, RuntimeError> {
        let raw = root.raw;
        let mut state = self.state.borrow_mut();
        let slot = validate_raw(self.id, HandleDomain::Future, raw, state.slots.len())?;
        let entry = &mut state.slots[slot];
        check_future_generation(raw, entry.generation)?;
        match &entry.state {
            FutureState::Pending => {
                return Err(RuntimeError::FutureStillPending { slot: raw.slot });
            }
            FutureState::Vacant => {
                return Err(RuntimeError::VacantHandle {
                    domain: HandleDomain::Future,
                    slot: raw.slot,
                    generation: raw.generation,
                });
            }
            FutureState::Complete(_) => {}
        }
        let generation = next_generation(HandleDomain::Future, raw.slot, entry.generation)?;
        let outcome = match std::mem::replace(&mut entry.state, FutureState::Vacant) {
            FutureState::Complete(outcome) => outcome,
            FutureState::Pending | FutureState::Vacant => unreachable!("state checked above"),
        };
        entry.generation = generation;
        state.free.insert(raw.slot);
        Ok(outcome)
    }

    pub fn inventory(&self) -> FutureInventory {
        let state = self.state.borrow();
        FutureInventory {
            pending: state
                .slots
                .iter()
                .filter(|slot| matches!(slot.state, FutureState::Pending))
                .count() as u32,
            completed: state
                .slots
                .iter()
                .filter(|slot| matches!(slot.state, FutureState::Complete(_)))
                .count() as u32,
            vacant: state.free.len() as u32,
        }
    }
}

fn check_future_generation(raw: RawHandle, generation: u32) -> Result<(), RuntimeError> {
    if raw.generation == generation {
        Ok(())
    } else {
        Err(RuntimeError::StaleHandle {
            domain: HandleDomain::Future,
            slot: raw.slot,
            expected_generation: generation,
            received_generation: raw.generation,
        })
    }
}

fn terminal_state<T, E>(outcome: &FutureOutcome<T, E>) -> TerminalState {
    match outcome {
        FutureOutcome::Resolved(_) => TerminalState::Resolved,
        FutureOutcome::Rejected(_) => TerminalState::Rejected,
        FutureOutcome::Cancelled => TerminalState::Cancelled,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeakInventory {
    pub resources: ResourceInventory,
    pub callbacks: CallbackInventory,
    pub futures: FutureInventory,
}

impl LeakInventory {
    pub const fn is_empty(self) -> bool {
        self.resources.live == 0
            && self.resources.active_borrows == 0
            && self.callbacks.rooted == 0
            && self.callbacks.active == 0
            && self.futures.pending == 0
            && self.futures.completed == 0
    }
}

/// Utility for deterministic tests and single-runtime construction.
#[derive(Debug)]
pub struct TableIdAllocator {
    next: Cell<u64>,
}

impl TableIdAllocator {
    pub const fn new(first: NonZeroU64) -> Self {
        Self {
            next: Cell::new(first.get()),
        }
    }

    pub fn allocate(&self) -> TableId {
        let value = self.next.get();
        self.next
            .set(value.checked_add(1).expect("host table id space exhausted"));
        TableId(NonZeroU64::new(value).expect("allocator starts nonzero"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task_descriptor(body: &'static str, executor: TaskExecutor) -> TaskDescriptor<&'static str> {
        TaskDescriptor { body, executor }
    }

    #[test]
    fn resumable_task_table_runs_suspend_resume_and_completes_exactly_once() {
        let tasks = ResumableTaskTable::<&'static str, u32, &'static str>::default();
        let token = tasks
            .register(task_descriptor("job", TaskExecutor::Current))
            .unwrap();
        assert_eq!(tasks.descriptor(token).unwrap().body, "job");
        tasks.start(token, TaskExecutor::Current).unwrap();
        tasks.suspend(token).unwrap();
        tasks.queue_value(token, 7).unwrap();
        assert!(matches!(
            tasks.deliver_next(token, TaskExecutor::Current).unwrap(),
            Some(TaskOutcome::Success(7))
        ));
        tasks.complete(token).unwrap();
        assert_eq!(
            tasks.complete(token).unwrap_err(),
            TaskRuntimeError::AlreadyTerminal(ResumableTaskState::Completed)
        );
        tasks.release(token).unwrap();
        assert_eq!(
            tasks.state(token).unwrap_err(),
            TaskRuntimeError::StaleToken(token.to_core())
        );
    }

    #[test]
    fn resumable_task_races_reject_late_delivery_and_reuse_generation() {
        let tasks = ResumableTaskTable::<&'static str, u32, &'static str>::default();
        let cancelled = tasks
            .register(task_descriptor("job", TaskExecutor::External))
            .unwrap();
        assert!(matches!(
            tasks.start(cancelled, TaskExecutor::Current),
            Err(TaskRuntimeError::PlacementMismatch { .. })
        ));
        tasks.start(cancelled, TaskExecutor::External).unwrap();
        tasks.suspend(cancelled).unwrap();
        tasks.queue_cancel(cancelled).unwrap();
        assert!(tasks.queue_value(cancelled, 1).is_err());
        assert!(matches!(
            tasks
                .deliver_next(cancelled, TaskExecutor::External)
                .unwrap(),
            Some(TaskOutcome::Cancelled)
        ));
        assert!(tasks.queue_error(cancelled, "late").is_err());
        tasks.release(cancelled).unwrap();

        let reused = tasks
            .register(task_descriptor("job", TaskExecutor::External))
            .unwrap();
        assert_ne!(reused, cancelled);
        assert_eq!(
            tasks.start(cancelled, TaskExecutor::External).unwrap_err(),
            TaskRuntimeError::StaleToken(cancelled.to_core())
        );
        tasks.start(reused, TaskExecutor::External).unwrap();
        tasks.suspend(reused).unwrap();
        tasks.queue_error(reused, "boom").unwrap();
        assert!(matches!(
            tasks.deliver_next(reused, TaskExecutor::External).unwrap(),
            Some(TaskOutcome::Failure("boom"))
        ));
        assert_eq!(tasks.state(reused).unwrap(), ResumableTaskState::Failed);
        tasks.release(reused).unwrap();
    }

    #[derive(Default)]
    struct RecordingBody {
        events: Vec<String>,
        terminals: Vec<(TaskToken, ResumableTaskState)>,
        routes: Vec<(TaskToken, TaskExecutor)>,
        yields: Vec<usize>,
        start_action: Option<TaskBodyAction>,
        poll_action: Option<TaskBodyAction>,
    }

    impl ResumableTaskBody<&'static str, u32, &'static str> for RecordingBody {
        fn start(
            &mut self,
            token: TaskToken,
            descriptor: &TaskDescriptor<&'static str>,
        ) -> TaskBodyAction {
            self.events.push(format!("start:{}", descriptor.body));
            self.start_action.unwrap_or_else(|| {
                let _ = token;
                TaskBodyAction::Suspend
            })
        }

        fn poll(
            &mut self,
            _token: TaskToken,
            descriptor: &TaskDescriptor<&'static str>,
        ) -> TaskBodyAction {
            self.events.push(format!("poll:{}", descriptor.body));
            self.poll_action.unwrap_or(TaskBodyAction::Suspend)
        }

        fn resume(
            &mut self,
            _token: TaskToken,
            descriptor: &TaskDescriptor<&'static str>,
            delivery: TaskOutcome<&'static str, u32>,
        ) -> TaskBodyAction {
            self.events
                .push(format!("resume:{}:{delivery:?}", descriptor.body));
            TaskBodyAction::Complete
        }

        fn terminal(&mut self, token: TaskToken, state: ResumableTaskState) {
            self.terminals.push((token, state));
        }

        fn route(&mut self, token: TaskToken, executor: TaskExecutor) {
            self.routes.push((token, executor));
        }

        fn yielded(&mut self, remaining_ready: usize) {
            self.yields.push(remaining_ready);
        }
    }

    #[test]
    fn resumable_executor_is_fifo_deduplicated_and_budget_fair() {
        let executor =
            ResumableExecutor::<&'static str, u32, &'static str>::new(TaskExecutor::Current);
        let first = task_descriptor("first", TaskExecutor::Current);
        let second = task_descriptor("second", TaskExecutor::Current);
        let first = executor.spawn(first).unwrap();
        let _second = executor.spawn(second).unwrap();
        assert!(
            !executor.wake(first).unwrap(),
            "wake must deduplicate queued start"
        );
        let mut body = RecordingBody::default();
        let first_drain = executor.drain(1, &mut body).unwrap();
        assert_eq!(first_drain.steps, 1);
        assert_eq!(first_drain.remaining_ready, 1);
        assert_eq!(body.yields, [1]);
        executor.drain(8, &mut body).unwrap();
        assert_eq!(body.events, ["start:first", "start:second"]);
    }

    #[test]
    fn resumable_executor_prioritizes_cancel_routes_and_notifies_once() {
        let executor =
            ResumableExecutor::<&'static str, u32, &'static str>::new(TaskExecutor::Current);
        let local = executor
            .spawn(task_descriptor("local", TaskExecutor::Current))
            .unwrap();
        let external = executor
            .spawn(task_descriptor("external", TaskExecutor::External))
            .unwrap();
        let mut body = RecordingBody::default();
        executor.drain(8, &mut body).unwrap();
        assert_eq!(body.routes, [(external, TaskExecutor::External)]);
        executor.resume_value(local, 9).unwrap();
        executor.cancel(local).unwrap();
        executor.drain(8, &mut body).unwrap();
        assert_eq!(
            body.terminals,
            [(local, ResumableTaskState::Cancelled)],
            "cancel must replace queued value and notify once"
        );
        assert_eq!(
            executor.wake(local).unwrap_err(),
            TaskRuntimeError::AlreadyTerminal(ResumableTaskState::Cancelled)
        );
        assert_eq!(body.terminals.len(), 1);
    }

    struct ReentrantBody<'a> {
        executor: &'a ResumableExecutor<&'static str, u32, &'static str>,
        observed: Option<ExecutorDrain>,
    }

    impl ResumableTaskBody<&'static str, u32, &'static str> for ReentrantBody<'_> {
        fn start(&mut self, _: TaskToken, _: &TaskDescriptor<&'static str>) -> TaskBodyAction {
            let mut nested = RecordingBody::default();
            self.observed = Some(self.executor.drain(1, &mut nested).unwrap());
            TaskBodyAction::Complete
        }
        fn poll(&mut self, _: TaskToken, _: &TaskDescriptor<&'static str>) -> TaskBodyAction {
            TaskBodyAction::Complete
        }
        fn resume(
            &mut self,
            _: TaskToken,
            _: &TaskDescriptor<&'static str>,
            _: TaskOutcome<&'static str, u32>,
        ) -> TaskBodyAction {
            TaskBodyAction::Complete
        }
        fn terminal(&mut self, _: TaskToken, _: ResumableTaskState) {}
        fn route(&mut self, _: TaskToken, _: TaskExecutor) {}
        fn yielded(&mut self, _: usize) {}
    }

    #[test]
    fn resumable_executor_drain_is_non_reentrant() {
        let executor =
            ResumableExecutor::<&'static str, u32, &'static str>::new(TaskExecutor::Current);
        executor
            .spawn(task_descriptor("job", TaskExecutor::Current))
            .unwrap();
        let mut body = ReentrantBody {
            executor: &executor,
            observed: None,
        };
        executor.drain(8, &mut body).unwrap();
        assert_eq!(
            body.observed,
            Some(ExecutorDrain {
                steps: 0,
                remaining_ready: 0,
                reentrant: true,
            })
        );
    }

    fn id(value: u64) -> TableId {
        TableId::new(NonZeroU64::new(value).unwrap())
    }

    #[test]
    fn resource_slot_reuse_changes_generation_and_rejects_stale_handles() {
        let table = ResourceTable::new(id(1));
        let first = table.insert("first");
        let stale = first.raw();
        assert_eq!(table.drop_owned(first).unwrap(), "first");
        let second = table.insert("second");
        assert_eq!(second.raw().slot, stale.slot);
        assert_eq!(second.raw().generation, stale.generation + 1);
        assert!(matches!(
            table.borrow(stale).unwrap_err(),
            RuntimeError::StaleHandle { .. }
        ));
    }

    #[test]
    fn resource_double_drop_and_cross_table_forgery_are_rejected() {
        let table = ResourceTable::new(id(1));
        let owned = table.insert(7);
        let raw = owned.raw();
        assert_eq!(table.drop_raw(raw).unwrap(), 7);
        assert!(matches!(
            table.drop_raw(raw).unwrap_err(),
            RuntimeError::StaleHandle { .. }
        ));
        let other = ResourceTable::<u32>::new(id(2));
        assert!(matches!(
            other.drop_raw(raw).unwrap_err(),
            RuntimeError::WrongTable { .. }
        ));
    }

    #[test]
    fn scoped_borrow_blocks_drop_and_inventory_tracks_it() {
        let table = ResourceTable::new(id(1));
        let owned = table.insert(String::from("value"));
        let raw = owned.raw();
        {
            let borrowed = table.borrow(raw).unwrap();
            assert_eq!(&**borrowed, "value");
            assert_eq!(borrowed.ownership(), HandleOwnership::Borrow);
            assert!(matches!(
                table.drop_raw(raw).unwrap_err(),
                RuntimeError::ResourceBorrowed {
                    active_borrows: 1,
                    ..
                }
            ));
            assert_eq!(table.inventory().active_borrows, 1);
        }
        assert_eq!(table.inventory().active_borrows, 0);
        assert_eq!(table.drop_owned(owned).unwrap(), "value");
    }

    #[test]
    fn nested_callbacks_and_same_callback_recursion_are_safe() {
        let registry = CallbackRegistry::<u32, u32>::new(id(3));
        let inner = registry.register(|value| value + 1);
        let inner_raw = inner.raw();
        let nested_registry = registry.clone();
        let outer =
            registry.register(move |value| nested_registry.invoke(inner_raw, value).unwrap() * 2);
        assert_eq!(registry.invoke(outer.raw(), 4).unwrap(), 10);

        let self_raw = Rc::new(Cell::new(None));
        let captured_raw = Rc::clone(&self_raw);
        let reentrant_registry = registry.clone();
        let reentrant = registry.register(move |value| {
            if value == 0 {
                1
            } else {
                reentrant_registry
                    .invoke(captured_raw.get().unwrap(), value - 1)
                    .unwrap()
                    + 1
            }
        });
        self_raw.set(Some(reentrant.raw()));
        assert_eq!(registry.invoke(reentrant.raw(), 4).unwrap(), 5);
        assert_eq!(registry.inventory().active, 0);
    }

    #[test]
    fn release_during_recursive_invocation_waits_for_outermost_return() {
        let registry = CallbackRegistry::<u32, ()>::new(id(4));
        let raw_cell = Rc::new(Cell::new(None));
        let captured = Rc::clone(&raw_cell);
        let releasing_registry = registry.clone();
        let root = registry.register(move |depth| {
            let raw = captured.get().unwrap();
            if depth == 0 {
                releasing_registry.release_raw(raw).unwrap();
            } else {
                releasing_registry.invoke(raw, depth - 1).unwrap();
            }
            assert_eq!(
                releasing_registry.inventory(),
                CallbackInventory {
                    rooted: 1,
                    active: 1,
                    pending_release: 1,
                }
            );
        });
        let raw = root.raw();
        raw_cell.set(Some(raw));
        registry.invoke(raw, 2).unwrap();
        assert_eq!(registry.inventory().rooted, 0);
        assert!(matches!(
            registry.invoke(raw, 0).unwrap_err(),
            RuntimeError::StaleHandle { .. }
        ));
    }

    #[test]
    fn callback_release_is_exact_and_panics_become_errors() {
        let registry = CallbackRegistry::<(), ()>::new(id(5));
        let root = registry.register(|_| panic!("host callback failure"));
        let raw = root.raw();
        assert_eq!(
            registry.invoke(raw, ()).unwrap_err(),
            RuntimeError::CallbackPanicked { slot: raw.slot }
        );
        registry.release(root).unwrap();
        assert!(matches!(
            registry.release_raw(raw).unwrap_err(),
            RuntimeError::StaleHandle { .. }
        ));
    }

    #[test]
    fn callback_tokens_are_opaque_i32_authority_for_scalar_trampolines() {
        let callbacks = CallbackTokenTable::<i32, i32>::new(id(12));
        let token = callbacks.register(|event_token| event_token + 7).unwrap();

        // This is the generic host side of
        // `export trampoline(callback_token, event_token) -> i32`.
        assert_eq!(
            callbacks
                .invoke(CallbackToken::from_core(token.to_core()), 35)
                .unwrap(),
            42
        );
        assert_eq!(callbacks.inventory().rooted, 1);

        callbacks.release(token).unwrap();
        assert!(matches!(
            callbacks
                .invoke(CallbackToken::from_core(token.to_core()), 35)
                .unwrap_err(),
            RuntimeError::VacantHandle {
                domain: HandleDomain::Callback,
                ..
            }
        ));
        assert_eq!(callbacks.inventory().rooted, 0);
    }

    #[test]
    fn futures_resolve_reject_and_cancel_exactly_once() {
        let futures = FutureRegistry::<u32, &'static str>::new(id(6));

        let resolved = futures.register();
        futures.resolve(resolved.raw(), 42).unwrap();
        assert_eq!(
            futures.reject(resolved.raw(), "late").unwrap_err(),
            RuntimeError::AlreadyCompleted {
                slot: resolved.raw().slot,
                state: TerminalState::Resolved,
            }
        );
        assert_eq!(futures.take(resolved).unwrap(), FutureOutcome::Resolved(42));

        let rejected = futures.register();
        futures.reject(rejected.raw(), "no").unwrap();
        assert_eq!(
            futures.take(rejected).unwrap(),
            FutureOutcome::Rejected("no")
        );

        let cancelled = futures.register();
        futures.cancel(cancelled.raw()).unwrap();
        assert_eq!(
            futures.resolve(cancelled.raw(), 9).unwrap_err(),
            RuntimeError::AlreadyCompleted {
                slot: cancelled.raw().slot,
                state: TerminalState::Cancelled,
            }
        );
        assert_eq!(futures.take(cancelled).unwrap(), FutureOutcome::Cancelled);
    }

    #[test]
    fn future_tokens_enforce_completion_cancellation_and_stale_token_protocol() {
        let futures = FutureTokenTable::<i32, i32>::new(id(13));

        // Promise resolution wins; every later terminal signal observes the
        // already-selected state until the owner consumes it.
        let resolved = futures.register().unwrap();
        futures.resolve(resolved, 42).unwrap();
        assert_eq!(
            futures.resolve(resolved, 43).unwrap_err(),
            RuntimeError::AlreadyCompleted {
                slot: 0,
                state: TerminalState::Resolved,
            }
        );
        assert_eq!(
            futures.cancel(resolved).unwrap_err(),
            RuntimeError::AlreadyCompleted {
                slot: 0,
                state: TerminalState::Resolved,
            }
        );
        assert_eq!(futures.take(resolved).unwrap(), FutureOutcome::Resolved(42));

        // The opaque token is retired after consumption. A late Promise
        // handler cannot complete a reused underlying slot.
        assert_eq!(
            futures.reject(resolved, 9).unwrap_err(),
            RuntimeError::UnknownToken {
                domain: HandleDomain::Future,
                token: resolved.to_core(),
            }
        );

        // Cancellation wins the opposite race and preserves exactly-once
        // semantics for late resolution/rejection.
        let cancelled = futures.register().unwrap();
        futures.cancel(cancelled).unwrap();
        assert_eq!(
            futures.resolve(cancelled, 1).unwrap_err(),
            RuntimeError::AlreadyCompleted {
                slot: 0,
                state: TerminalState::Cancelled,
            }
        );
        assert_eq!(
            futures.reject(cancelled, 2).unwrap_err(),
            RuntimeError::AlreadyCompleted {
                slot: 0,
                state: TerminalState::Cancelled,
            }
        );
        assert_eq!(futures.take(cancelled).unwrap(), FutureOutcome::Cancelled);
        assert_eq!(
            futures.take(cancelled).unwrap_err(),
            RuntimeError::UnknownToken {
                domain: HandleDomain::Future,
                token: cancelled.to_core(),
            }
        );

        // Rejection is the third terminal path and owns the supplied error
        // value exactly once.
        let rejected = futures.register().unwrap();
        futures.reject(rejected, 17).unwrap();
        assert_eq!(futures.take(rejected).unwrap(), FutureOutcome::Rejected(17));
        assert_eq!(
            futures.resolve(rejected, 5).unwrap_err(),
            RuntimeError::UnknownToken {
                domain: HandleDomain::Future,
                token: rejected.to_core(),
            }
        );
        assert_eq!(
            futures.inventory(),
            FutureInventory {
                pending: 0,
                completed: 0,
                vacant: 1,
            }
        );
    }

    #[test]
    fn pending_future_cannot_be_taken_and_stale_completion_is_rejected() {
        let futures = FutureRegistry::<(), ()>::new(id(7));
        let pending = futures.register();
        let raw = pending.raw();
        assert_eq!(
            futures.take(pending).unwrap_err(),
            RuntimeError::FutureStillPending { slot: raw.slot }
        );
        futures.cancel(raw).unwrap();
        // Recreate a root exactly as a wire adapter would retain it.
        let root = FutureRoot {
            raw,
            marker: PhantomData,
        };
        assert_eq!(futures.take(root).unwrap(), FutureOutcome::Cancelled);
        let next = futures.register();
        assert_eq!(next.raw().slot, raw.slot);
        assert!(matches!(
            futures.resolve(raw, ()).unwrap_err(),
            RuntimeError::StaleHandle { .. }
        ));
    }

    #[test]
    fn inventories_report_and_clear_all_roots() {
        let resources = ResourceTable::new(id(10));
        let resource = resources.insert(1);
        let callbacks = CallbackRegistry::<(), ()>::new(id(11));
        let callback = callbacks.register(|_| {});
        let futures = FutureRegistry::<(), ()>::new(id(12));
        let future = futures.register();
        futures.resolve(future.raw(), ()).unwrap();

        let leaks = LeakInventory {
            resources: resources.inventory(),
            callbacks: callbacks.inventory(),
            futures: futures.inventory(),
        };
        assert!(!leaks.is_empty());
        assert_eq!((leaks.resources.live, leaks.callbacks.rooted), (1, 1));
        assert_eq!(leaks.futures.completed, 1);

        resources.drop_owned(resource).unwrap();
        callbacks.release(callback).unwrap();
        futures.take(future).unwrap();
        assert!(
            LeakInventory {
                resources: resources.inventory(),
                callbacks: callbacks.inventory(),
                futures: futures.inventory(),
            }
            .is_empty()
        );
    }

    #[test]
    fn raw_handles_and_errors_have_stable_structured_json() {
        let raw = RawHandle {
            table: id(9),
            slot: 2,
            generation: 7,
        };
        let json = serde_json::to_string(&raw).unwrap();
        assert_eq!(json, r#"{"table":9,"slot":2,"generation":7}"#);
        assert_eq!(serde_json::from_str::<RawHandle>(&json).unwrap(), raw);
        let error = RuntimeError::WrongTable {
            domain: HandleDomain::Resource,
            expected: id(1),
            received: id(2),
        };
        assert_eq!(
            serde_json::to_string(&error).unwrap(),
            r#"{"code":"wrong_table","domain":"resource","expected":1,"received":2}"#
        );
    }
}
