//! A deterministic broker for several native tasks.
//!
//! The broker owns tasks and one first-in first-out queue of tasks that can
//! make progress. The host announces events (a start, a delivery, a
//! cancellation) and the broker runs the task at the front of the queue, one
//! segment per [`Broker::step`]. The sequence of steps depends only on the
//! sequence of announcements: the broker reads no clock, no thread and no
//! randomness, and it iterates no hashed collection.
//!
//! The rule is a property of this test runtime. It is not a promise of the
//! language: Fe code cannot observe which other task ran in between.
//!
//! Single-threaded, like [`Task`]. Like [`Task::poll`], it cannot be entered
//! from inside a running segment.

use std::{collections::VecDeque, rc::Rc};

use super::{Delivery, Error, NativeCode, Progress, Task, Ticket, identity};

/// Names a task of one broker. A value from another broker is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaskId {
    broker: u64,
    index: usize,
}
impl TaskId {
    /// The position of the task in start order, beginning at zero.
    pub fn index(self) -> usize {
        self.index
    }
}

/// One segment executed by [`Broker::step`], or the refusal to execute it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub task: TaskId,
    pub result: Result<Progress, Error>,
}

pub struct Broker<G> {
    id: u64,
    tasks: Vec<Task<G>>,
    /// `queued[i]` is true exactly when task `i` is in `ready`.
    queued: Vec<bool>,
    ready: VecDeque<usize>,
}

impl<G> Broker<G> {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            id: identity()?,
            tasks: Vec::new(),
            queued: Vec::new(),
            ready: VecDeque::new(),
        })
    }

    /// Create a task and queue its first segment behind every task that is
    /// already queued.
    pub fn spawn(&mut self, code: Rc<NativeCode<G>>) -> Result<TaskId, Error> {
        let task = Task::new(code)?;
        let index = self.tasks.len();
        self.tasks.push(task);
        self.queued.push(false);
        self.enqueue(index);
        Ok(TaskId {
            broker: self.id,
            index,
        })
    }

    /// Announce the completion of the operation that `ticket` names. The
    /// ticket is checked now, so a stale, duplicate or foreign ticket is
    /// refused here and leaves the queue unchanged. An accepted delivery
    /// queues the task behind every task that is already queued.
    pub fn deliver(
        &mut self,
        task: TaskId,
        ticket: Ticket,
        delivery: Delivery,
    ) -> Result<(), Error> {
        let index = self.index(task).ok_or(Error::Foreign)?;
        self.tasks[index].deliver(ticket, delivery)?;
        self.enqueue(index);
        Ok(())
    }

    /// Announce cancellation. Returns whether this call latched it. The task
    /// is queued so that its cleanup runs; a task that has not started
    /// completes at once and runs no code.
    pub fn cancel(&mut self, task: TaskId) -> bool {
        let Some(index) = self.index(task) else {
            return false;
        };
        let changed = self.tasks[index].cancel();
        if changed {
            self.enqueue(index);
        }
        changed
    }

    /// Run one segment of the task at the front of the queue. A task whose
    /// cancellation needs another segment goes to the back of the queue, so
    /// other queued tasks run between the segments of its cleanup. Returns
    /// `None` when no task is queued.
    pub fn step(&mut self) -> Option<Step> {
        let index = self.ready.pop_front()?;
        self.queued[index] = false;
        let result = self.tasks[index].poll();
        if result == Ok(Progress::Cancelling) {
            self.enqueue(index);
        }
        Some(Step {
            task: TaskId {
                broker: self.id,
                index,
            },
            result,
        })
    }

    /// Step until no task is queued or `limit` steps have run. The limit
    /// guards tests against cleanup code that never ends, which sticky
    /// cancellation does not rule out.
    pub fn run(&mut self, limit: usize) -> Vec<Step> {
        let mut steps = Vec::new();
        while steps.len() < limit {
            let Some(step) = self.step() else { break };
            steps.push(step);
        }
        steps
    }

    pub fn is_idle(&self) -> bool {
        self.ready.is_empty()
    }

    /// Whether the task still holds its frame. `None` for a foreign id.
    pub fn retains_frame(&self, task: TaskId) -> Option<bool> {
        Some(self.tasks[self.index(task)?].retains_frame())
    }

    /// The number of operations the task has registered. `None` for a
    /// foreign id.
    pub fn registrations(&self, task: TaskId) -> Option<usize> {
        Some(self.tasks[self.index(task)?].registrations())
    }

    fn index(&self, task: TaskId) -> Option<usize> {
        (task.broker == self.id && task.index < self.tasks.len()).then_some(task.index)
    }

    /// A task is in the queue at most once, at the position of its first
    /// announcement that has not yet been served.
    fn enqueue(&mut self, index: usize) {
        if !std::mem::replace(&mut self.queued[index], true) {
            self.ready.push_back(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Segment, native_task_begin};
    use super::*;
    use std::cell::{Cell, RefCell};

    // A task with two cuts written by hand against the segment ABI. Frame
    // word 0 holds the pending identity (then the result), word 1 the sum of
    // the outcomes seen so far, word 2 the tag of the task. Every segment
    // logs (tag, segment, delivery kind, payload), so a test can state the
    // order in which code ran.
    thread_local! {
        static LOG: RefCell<Vec<(u64, u64, u64, u64)>> = const { RefCell::new(Vec::new()) };
        static NEXT_TAG: Cell<u64> = const { Cell::new(0) };
    }
    fn weight(kind: u64, payload: u64) -> u64 {
        match kind {
            1 => payload,
            0 => 100,
            _ => 1000,
        }
    }
    unsafe fn word(frame: *mut u64, index: usize) -> *mut u64 {
        unsafe { frame.add(index) }
    }
    unsafe extern "C" fn start(frame: *mut u64, kind: u64, payload: u64) -> u64 {
        let tag = NEXT_TAG.replace(NEXT_TAG.get() + 1);
        LOG.with_borrow_mut(|log| log.push((tag, 0, kind, payload)));
        unsafe {
            *word(frame, 2) = tag;
            *word(frame, 0) = native_task_begin();
        }
        1
    }
    unsafe extern "C" fn resume_first(frame: *mut u64, kind: u64, payload: u64) -> u64 {
        unsafe {
            let tag = *word(frame, 2);
            LOG.with_borrow_mut(|log| log.push((tag, 1, kind, payload)));
            *word(frame, 1) = weight(kind, payload);
            *word(frame, 0) = native_task_begin();
        }
        2
    }
    unsafe extern "C" fn resume_second(frame: *mut u64, kind: u64, payload: u64) -> u64 {
        unsafe {
            let tag = *word(frame, 2);
            LOG.with_borrow_mut(|log| log.push((tag, 2, kind, payload)));
            *word(frame, 1) += weight(kind, payload);
            *word(frame, 0) = *word(frame, 1);
        }
        0
    }
    fn code() -> Rc<NativeCode<()>> {
        let entries: Vec<Segment> = vec![start, resume_first, resume_second];
        Rc::new(unsafe { NativeCode::new(entries, 3, ()) }.unwrap())
    }
    fn reset() {
        LOG.with_borrow_mut(Vec::clear);
        NEXT_TAG.set(0);
    }
    fn log() -> Vec<(u64, u64, u64, u64)> {
        LOG.with_borrow(Clone::clone)
    }

    /// A step without its ticket. Ticket values come from process-wide
    /// counters and differ between runs; the order of steps does not.
    #[derive(Debug, PartialEq, Eq)]
    enum Brief {
        Suspended,
        Cancelling,
        Complete(u64, bool),
        Refused(Error),
    }
    fn brief(steps: &[Step]) -> Vec<(usize, Brief)> {
        steps
            .iter()
            .map(|step| {
                let brief = match step.result {
                    Ok(Progress::Suspended(_)) => Brief::Suspended,
                    Ok(Progress::Cancelling) => Brief::Cancelling,
                    Ok(Progress::Complete { value, cancelled }) => {
                        Brief::Complete(value, cancelled)
                    }
                    Err(error) => Brief::Refused(error),
                };
                (step.task.index(), brief)
            })
            .collect()
    }
    fn ticket(step: &Step) -> Ticket {
        match step.result {
            Ok(Progress::Suspended(ticket)) => ticket,
            other => panic!("expected a suspension, got {other:?}"),
        }
    }
    /// Start `count` tasks and return their ids and first tickets.
    fn started(count: usize) -> (Broker<()>, Vec<TaskId>, Vec<Ticket>) {
        let code = code();
        let mut broker = Broker::new().unwrap();
        let ids: Vec<_> = (0..count)
            .map(|_| broker.spawn(code.clone()).unwrap())
            .collect();
        let steps = broker.run(100);
        assert_eq!(steps.len(), count);
        let tickets = steps.iter().map(ticket).collect();
        (broker, ids, tickets)
    }

    #[test]
    fn tasks_start_in_spawn_order() {
        reset();
        let code = code();
        let mut broker = Broker::new().unwrap();
        for _ in 0..3 {
            broker.spawn(code.clone()).unwrap();
        }
        assert_eq!(log(), []);
        let steps = broker.run(100);
        assert_eq!(
            brief(&steps),
            [
                (0, Brief::Suspended),
                (1, Brief::Suspended),
                (2, Brief::Suspended)
            ]
        );
        let tags: Vec<_> = log().iter().map(|entry| entry.0).collect();
        assert_eq!(tags, [0, 1, 2]);
        assert!(broker.is_idle());
    }

    #[test]
    fn completions_run_in_announcement_order_one_segment_each() {
        reset();
        let (mut broker, ids, tickets) = started(3);
        let before = log().len();
        for index in [2, 0, 1] {
            broker
                .deliver(ids[index], tickets[index], Delivery::Success(7))
                .unwrap();
        }
        assert_eq!(log().len(), before, "a delivery must not run code");
        let first = broker.step().unwrap();
        assert_eq!(first.task, ids[2]);
        assert_eq!(log().len(), before + 1);
        let rest = broker.run(100);
        assert_eq!(brief(&rest), [(0, Brief::Suspended), (1, Brief::Suspended)]);
        let order: Vec<_> = log()[before..].iter().map(|entry| entry.0).collect();
        assert_eq!(order, [2, 0, 1]);
        assert!(log()[before..].iter().all(|entry| entry.1 == 1));
    }

    #[test]
    fn tasks_interleave_a_segment_at_a_time() {
        reset();
        let (mut broker, ids, tickets) = started(2);
        broker
            .deliver(ids[0], tickets[0], Delivery::Success(1))
            .unwrap();
        broker
            .deliver(ids[1], tickets[1], Delivery::Success(2))
            .unwrap();
        let second: Vec<_> = broker.run(100).iter().map(ticket).collect();
        // Answer in the opposite order to the start order.
        broker
            .deliver(ids[1], second[1], Delivery::Success(20))
            .unwrap();
        broker
            .deliver(ids[0], second[0], Delivery::Success(10))
            .unwrap();
        assert_eq!(
            brief(&broker.run(100)),
            [
                (1, Brief::Complete(22, false)),
                (0, Brief::Complete(11, false))
            ]
        );
        for id in &ids {
            assert_eq!(broker.retains_frame(*id), Some(false));
            assert_eq!(broker.registrations(*id), Some(0));
        }
    }

    #[test]
    fn cancelling_a_task_that_has_not_started_runs_no_code() {
        reset();
        let mut broker = Broker::new().unwrap();
        let id = broker.spawn(code()).unwrap();
        assert!(broker.cancel(id));
        assert!(!broker.cancel(id));
        assert_eq!(brief(&broker.run(100)), [(0, Brief::Complete(0, true))]);
        assert_eq!(log(), []);
        assert_eq!(broker.retains_frame(id), Some(false));
    }

    #[test]
    fn cleanup_after_cancellation_interleaves_with_other_tasks() {
        reset();
        let (mut broker, ids, tickets) = started(2);
        assert!(broker.cancel(ids[0]));
        broker
            .deliver(ids[1], tickets[1], Delivery::Success(5))
            .unwrap();
        let before = log().len();
        // Task 0 needs two more segments. Task 1 runs between them.
        assert_eq!(
            brief(&broker.run(100)),
            [
                (0, Brief::Cancelling),
                (1, Brief::Suspended),
                (0, Brief::Complete(2000, true)),
            ]
        );
        let order: Vec<_> = log()[before..]
            .iter()
            .map(|entry| (entry.0, entry.1, entry.2))
            .collect();
        assert_eq!(order, [(0, 1, 2), (1, 1, 1), (0, 2, 2)]);
    }

    #[test]
    fn refused_announcements_leave_nothing_queued() {
        reset();
        let (mut broker, ids, tickets) = started(2);
        let mut other = Broker::<()>::new().unwrap();
        let foreign_id = other.spawn(code()).unwrap();
        assert_eq!(
            broker.deliver(ids[0], tickets[0], Delivery::Success(1)),
            Ok(())
        );
        assert_eq!(
            broker.deliver(ids[0], tickets[0], Delivery::Success(1)),
            Err(Error::Duplicate)
        );
        assert_eq!(
            broker.deliver(ids[1], tickets[0], Delivery::Success(1)),
            Err(Error::Foreign)
        );
        assert_eq!(
            broker.deliver(foreign_id, tickets[1], Delivery::Success(1)),
            Err(Error::Foreign)
        );
        assert!(!broker.cancel(foreign_id));
        assert_eq!(broker.retains_frame(foreign_id), None);
        assert_eq!(brief(&broker.run(100)), [(0, Brief::Suspended)]);
        assert_eq!(
            broker.deliver(ids[0], tickets[0], Delivery::Success(1)),
            Err(Error::Stale)
        );
        assert!(broker.is_idle());
    }

    #[test]
    fn a_task_is_queued_once_and_cancellation_wins() {
        reset();
        let (mut broker, ids, tickets) = started(1);
        let before = log().len();
        broker
            .deliver(ids[0], tickets[0], Delivery::Success(9))
            .unwrap();
        assert!(broker.cancel(ids[0]));
        assert_eq!(
            brief(&broker.run(100)),
            [(0, Brief::Cancelling), (0, Brief::Complete(2000, true))]
        );
        assert_eq!(log()[before].2, 2, "the queued success must not be seen");
    }

    #[test]
    fn run_stops_at_its_limit() {
        reset();
        let mut broker = Broker::new().unwrap();
        let code = code();
        broker.spawn(code.clone()).unwrap();
        broker.spawn(code).unwrap();
        assert_eq!(broker.run(1).len(), 1);
        assert!(!broker.is_idle());
        assert_eq!(broker.run(0).len(), 0);
        assert_eq!(broker.run(5).len(), 1);
        assert!(broker.is_idle());
    }

    fn script() -> (Vec<(usize, Brief)>, Vec<(u64, u64, u64, u64)>) {
        reset();
        let (mut broker, ids, tickets) = started(4);
        let mut steps = Vec::new();
        broker
            .deliver(ids[3], tickets[3], Delivery::Success(4))
            .unwrap();
        broker
            .deliver(ids[1], tickets[1], Delivery::Failure(1))
            .unwrap();
        broker.cancel(ids[0]);
        steps.extend(broker.run(2));
        broker
            .deliver(ids[2], tickets[2], Delivery::Success(3))
            .unwrap();
        steps.extend(broker.run(100));
        (brief(&steps), log())
    }

    #[test]
    fn the_same_announcements_give_the_same_steps_and_the_same_code_order() {
        let first = script();
        let second = script();
        assert_eq!(first, second);
        assert!(first.0.len() > 4);
    }
}
